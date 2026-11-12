// SPDX-License-Identifier: GPL-3.0-only
#include "motion_stage.h"
#include "adapter_options.h"
#include "geometry_feed.h"
#include "motion_d3d11.h"
#include "motion_foliage.h"
#include "motion_history.h"
#include "motion_policy.h"
#include "build_profile.h"
#include "motion_target.h"
#include "native_geometry.h"
#include "native_hooks.h"
#include "native_observer.h"
#include "shader_d3d.h"
#include "shader_loader.h"
#include "runtime_shaders.h"
#include "sr_profile.h"
#include "sr_stage.h"
#include <rescaleframe/game_frame.h>
#include <rescaleframe/texture_dump.h>
#include <MinHook.h>
#include <d3d11.h>
#include <intrin.h>
#include <windows.h>
#include <algorithm>
#include <array>
#include <atomic>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <map>
#include <memory>
#include <mutex>
#include <shared_mutex>
#include <string>
#include <tuple>
#include <unordered_map>
#include <vector>

namespace rsf::anomaly::motion {
namespace {
// ---- configuration and logging -----------------------------------------------------------------------------------
MotionOptions configured{};
rsf_game_log_fn log_fn{};
void* log_user{};
void say(const char* message) noexcept {
    if (log_fn) try { log_fn(log_user, message); } catch (...) {}
}
void sayf(const char* format, ...) noexcept {
    char text[768];
    va_list args;
    va_start(args, format);
    std::vsnprintf(text, sizeof(text), format, args);
    va_end(args);
    say(text);
}
std::string utf8(const std::wstring& value) {
    if (value.empty()) return {};
    const int size = WideCharToMultiByte(CP_UTF8, 0, value.c_str(), int(value.size()), nullptr, 0, nullptr, nullptr);
    std::string result(size > 0 ? size : 0, '\0');
    if (size > 0) WideCharToMultiByte(CP_UTF8, 0, value.c_str(), int(value.size()), result.data(), size, nullptr, nullptr);
    return result;
}

// ---- engine memory -------------------------------------------------------------------------------------------
std::byte* image{};
bool guarded_copy(void* output, const void* input, std::size_t size) noexcept {
    if (!output || !input) return false;
    __try { std::memcpy(output, input, size); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}
bool guarded_write(void* output, const void* input, std::size_t size) noexcept {
    if (!output || !input) return false;
    __try { std::memcpy(output, input, size); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}
// The exact build install() was given (build_profile.h), set together with `image`. Until then every read fails on the null
// image and the empty tables are never dereferenced into engine memory.
const BuildProfile* build{};
const MotionProfile& mp() noexcept { static constexpr MotionProfile none{}; return build ? *build->motion : none; }
const sr::SrProfile& srp() noexcept { static constexpr sr::SrProfile none{}; return build ? *build->sr : none; }
const NativeProfile& np() noexcept { static constexpr NativeProfile none{}; return build ? *build->native : none; }
template<class T> bool read_global(uint32_t rva, T& value) noexcept { return image && guarded_copy(&value, image + rva, sizeof(T)); }
bool bytes_match(const std::byte* address, const char* hex) noexcept {
    const auto digit = [](char c) { return c <= '9' ? c - '0' : c - 'a' + 10; };
    for (std::size_t i = 0; hex[2 * i] && hex[2 * i + 1]; ++i) {
        std::byte value{};
        if (!guarded_copy(&value, address + i, 1) || value != static_cast<std::byte>(digit(hex[2 * i]) * 16 + digit(hex[2 * i + 1])))
            return false;
    }
    return true;
}

// ---- core frame dump service (Runtime exports) ----------------------------------------------------------------
struct DumpApi {
    void (*configure)(const char*, const char*, const char*, rsf_dump_log_fn, void*){};
    void (*request)(){};
    void (*poll)(){};
    int (*requested)(){};
    int (*begin_record)(const rsf_frame_record*, uint32_t){};
    int (*texture_record)(void*, const rsf_frame_record*, const char*, const char*, void*, rsf_dump_view, float){};
    void (*complete)(const rsf_frame_record*, int32_t){};
    void (*cancel)(uint64_t){};
    bool bind(HMODULE runtime) noexcept {
        const auto get = [runtime](const char* name) { return reinterpret_cast<void*>(GetProcAddress(runtime, name)); };
        configure = reinterpret_cast<decltype(configure)>(get("rsf_input_dump_configure"));
        request = reinterpret_cast<decltype(request)>(get("rsf_input_dump_request"));
        poll = reinterpret_cast<decltype(poll)>(get("rsf_input_dump_poll"));
        requested = reinterpret_cast<decltype(requested)>(get("rsf_input_dump_requested"));
        begin_record = reinterpret_cast<decltype(begin_record)>(get("rsf_input_dump_begin_record"));
        texture_record = reinterpret_cast<decltype(texture_record)>(get("rsf_input_dump_texture_record"));
        complete = reinterpret_cast<decltype(complete)>(get("rsf_input_dump_complete"));
        cancel = reinterpret_cast<decltype(cancel)>(get("rsf_input_dump_cancel"));
        return configure && request && poll && requested && begin_record && texture_record && complete && cancel;
    }
};
DumpApi dump{};
bool dump_ready{};
struct Capture {
    bool active{};
    rsf_frame_record record{};
    uint32_t remaining{};
    uint64_t next{};
} capture;

// ---- shader variants ---------------------------------------------------------------------------------------------
struct Variant {
    ComPtr<IUnknown> stock;            // pinned so the address cannot be reused by another shader
    ComPtr<ID3D11VertexShader> vs;
    ComPtr<ID3D11PixelShader> ps;
    ComPtr<ID3D11HullShader> hs;       // DX11\tess.hs variant (TessHull)
    ComPtr<ID3D11DomainShader> ds;     // DX11\tess.ds variant (TessDomain)
    ID3D11Device* device{};
    Family family{};
    int skin{-2};
    Reflection reflection;
    ShaderRequest request;
    std::vector<std::byte> original_bytecode;
    FoliageLayout foliage;             // foliage VS only: where the engine stages its constants (from the STOCK reflection)
};
std::shared_mutex registry_guard;
std::unordered_map<const void*, std::shared_ptr<const Variant>> registry;
std::map<std::pair<const void*, const void*>, bool> link_cache;   // render thread only
std::map<std::array<const void*, 4>, bool> tess_link_cache;        // stock VS, HS, DS, PS; render thread only
std::mutex loader_guard, stats_guard;
// Diagnostics only: the shader-cache root of every VS/PS the engine created (allowlisted or not), by stock object.
// An address the engine later releases may name a newer shader; the periodic log is evidence, never a decision input.
std::shared_mutex names_guard;
std::unordered_map<const void*, std::string> shader_names;
std::unordered_map<const void*, ModelElement> shader_elements;   // model VS roots: HQ / LQ / tessellation element
constexpr std::size_t shader_names_limit = 16384;
// Draws without a linkable variant pair since the last report, by (stock VS, stock PS, draw class).
struct UnpairedKey {
    const void* vs{};
    const void* ps{};
    std::size_t draw_class{};
    bool operator<(const UnpairedKey& o) const noexcept {
        return std::tie(vs, ps, draw_class) < std::tie(o.vs, o.ps, o.draw_class);
    }
};
std::mutex unpaired_guard;
std::map<UnpairedKey, std::uint64_t> unpaired_pairs;
constexpr std::size_t unpaired_pairs_limit = 4096;
D3DShaderApi* shader_api{};
std::unique_ptr<ShaderLoader> loader;
std::unique_ptr<RuntimeShaders> runtime_shaders;
std::atomic<bool> pack_enabled{}, reload_requested{};
std::atomic<int32_t> pack_status{};
char pack_label[96] = "Motion shader pack: not loaded";
std::atomic<bool> dump_shaders{};
uint32_t refusal_logs{}, replace_logs{};

// ---- D3D11 and engine state --------------------------------------------------------------------------------------
MotionDevice device_state;
MotionHistory history;
FoliageHistory foliage_history;
geometry::RetirementQueue retirements;
MotionCounters stats{};
MotionCounters reported{};
std::atomic<bool> hooks_active{};
std::atomic<float> jitter[4]{};
HookSet hooks;
std::array<void*, motion_site_count> originals{};
using Unary = void(__fastcall*)(void*);
using CreateShader = long long(__fastcall*)(const char*, const void*, unsigned, const char*, void**, char);

struct Host final : BinderHost {
    ID3D11DeviceContext* context{};
    bool read(const void* address, void* out, std::size_t size) noexcept override { return guarded_copy(out, address, size); }
    bool write(void* address, const void* in, std::size_t size) noexcept override { return guarded_write(address, in, size); }
    void set_targets(void* const (&targets)[4], void* depth) noexcept override {
        if (context) context->OMSetRenderTargets(4, reinterpret_cast<ID3D11RenderTargetView* const*>(targets),
            static_cast<ID3D11DepthStencilView*>(depth));
    }
    void clear_targets() noexcept override { if (context) context->OMSetRenderTargets(0, nullptr, nullptr); }
} binder_host;
MotionBinder binder{binder_host, {sr::backend_rt_offset, sr::backend_zb_offset, sr::backend_rt_changed_offset}};

struct Window {
    bool open{};
    bool target_ok{};
    bool cache_may_hold_motion{};
    bool have_frame{};
    uint32_t frame{};
    temporal::ViewId view{};
    void* position_rtv{};
    void* position_surface{};
    ID3D11DeviceContext* context{};
} window;
bool draw_hooks_ok{}, draw_hooks_refused{};
// CRenderTarget::rt_Position (sr_profile.h target_slots "$user$position"), the RT0 of every G-buffer bind.
constexpr uint32_t position_offset = sr::target_slots[0].offset;
static_assert(position_offset == 0x378);

struct Pending {
    bool active{};
    ID3D11DeviceContext* context{};
    bool replay{};
    bool restore{};
    ID3D11VertexShader* vs{};
    ID3D11PixelShader* ps{};
    bool restore_tess{};               // the hull and domain stages were swapped too
    ID3D11HullShader* hs{};
    ID3D11DomainShader* ds{};
};
thread_local Pending pending;
void restore_stock(ID3D11DeviceContext* context, const Pending& taken) noexcept {
    context->VSSetShader(taken.vs, nullptr, 0);
    context->PSSetShader(taken.ps, nullptr, 0);
    if (taken.restore_tess) {
        context->HSSetShader(taken.hs, nullptr, 0);
        context->DSSetShader(taken.ds, nullptr, 0);
    }
}

// ---- D3D11 immediate-context draw detours ------------------------------------------------------------------------
using DrawIndexedFn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT, INT);
using DrawFn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT);
using DrawIndexedInstancedFn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT, UINT, INT, UINT);
using DrawInstancedFn = void(STDMETHODCALLTYPE*)(ID3D11DeviceContext*, UINT, UINT, UINT, UINT);
DrawIndexedFn original_draw_indexed{};
DrawFn original_draw{};
DrawIndexedInstancedFn original_draw_indexed_instanced{};
DrawInstancedFn original_draw_instanced{};
std::array<void*, 4> draw_targets{};

template<class Redraw> void finish_draw(ID3D11DeviceContext* context, const Pending& taken, Redraw&& redraw) noexcept {
    if (taken.replay) {
        switch (device_state.invalidate(context, redraw)) {
        case ReplayResult::Done: ++stats.replays; break;
        case ReplayResult::SkippedNoDepthWrite: ++stats.replay_skipped_no_depth_write; break;
        default: ++stats.replay_failed; break;
        }
    }
    if (taken.restore) {
        restore_stock(context, taken);
        ++stats.restores;
    }
}
void STDMETHODCALLTYPE draw_indexed_hook(ID3D11DeviceContext* c, UINT count, UINT start, INT base) {
    if (!pending.active || pending.context != c) { original_draw_indexed(c, count, start, base); return; }
    const Pending taken = pending;
    pending = {};
    original_draw_indexed(c, count, start, base);
    finish_draw(c, taken, [&](ID3D11DeviceContext* x) { original_draw_indexed(x, count, start, base); });
}
void STDMETHODCALLTYPE draw_hook(ID3D11DeviceContext* c, UINT count, UINT start) {
    if (!pending.active || pending.context != c) { original_draw(c, count, start); return; }
    const Pending taken = pending;
    pending = {};
    original_draw(c, count, start);
    finish_draw(c, taken, [&](ID3D11DeviceContext* x) { original_draw(x, count, start); });
}
void STDMETHODCALLTYPE draw_indexed_instanced_hook(ID3D11DeviceContext* c, UINT count, UINT instances, UINT start, INT base, UINT first) {
    if (!pending.active || pending.context != c) { original_draw_indexed_instanced(c, count, instances, start, base, first); return; }
    const Pending taken = pending;
    pending = {};
    original_draw_indexed_instanced(c, count, instances, start, base, first);
    finish_draw(c, taken, [&](ID3D11DeviceContext* x) { original_draw_indexed_instanced(x, count, instances, start, base, first); });
}
void STDMETHODCALLTYPE draw_instanced_hook(ID3D11DeviceContext* c, UINT count, UINT instances, UINT start, UINT first) {
    if (!pending.active || pending.context != c) { original_draw_instanced(c, count, instances, start, first); return; }
    const Pending taken = pending;
    pending = {};
    original_draw_instanced(c, count, instances, start, first);
    finish_draw(c, taken, [&](ID3D11DeviceContext* x) { original_draw_instanced(x, count, instances, start, first); });
}
// ID3D11DeviceContext vtable slots (d3d11.h declaration order): DrawIndexed 12, Draw 13, DrawIndexedInstanced 20,
// DrawInstanced 21.
bool install_draw_hooks(ID3D11DeviceContext* context) noexcept {
    if (draw_hooks_ok) return true;
    if (draw_hooks_refused || !context) return false;
    void** vtable{};
    if (!guarded_copy(&vtable, context, sizeof(vtable)) || !vtable) { draw_hooks_refused = true; return false; }
    const std::array<std::size_t, 4> slots{12, 13, 20, 21};
    const std::array<void*, 4> replacements{reinterpret_cast<void*>(&draw_indexed_hook), reinterpret_cast<void*>(&draw_hook),
        reinterpret_cast<void*>(&draw_indexed_instanced_hook), reinterpret_cast<void*>(&draw_instanced_hook)};
    std::array<void**, 4> trampolines{reinterpret_cast<void**>(&original_draw_indexed), reinterpret_cast<void**>(&original_draw),
        reinterpret_cast<void**>(&original_draw_indexed_instanced), reinterpret_cast<void**>(&original_draw_instanced)};
    const auto initialized = MH_Initialize();
    if (initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED) { draw_hooks_refused = true; return false; }
    std::size_t created = 0;
    for (std::size_t i = 0; i < slots.size(); ++i) {
        void* target{};
        if (!guarded_copy(&target, vtable + slots[i], sizeof(target)) || !target ||
            std::find(draw_targets.begin(), draw_targets.begin() + created, target) != draw_targets.begin() + created ||
            MH_CreateHook(target, replacements[i], trampolines[i]) != MH_OK) break;
        draw_targets[created++] = target;
    }
    if (created != slots.size()) {
        for (std::size_t i = 0; i < created; ++i) (void)MH_RemoveHook(draw_targets[i]);
        draw_hooks_refused = true;
        say("Anomaly motion refused: the immediate context's draw entry points could not be hooked; shaders stay stock.");
        return false;
    }
    for (const auto target : draw_targets)
        if (MH_QueueEnableHook(target) != MH_OK) { draw_hooks_refused = true; return false; }
    if (MH_ApplyQueued() != MH_OK) { draw_hooks_refused = true; return false; }
    draw_hooks_ok = true;
    say("Anomaly motion: immediate-context Draw/DrawIndexed/DrawInstanced/DrawIndexedInstanced detours installed.");
    return true;
}

// ---- create_shader detour ----------------------------------------------------------------------------------------
std::size_t family_index(Family family) noexcept { return static_cast<std::size_t>(family) < MotionCounters::families ? static_cast<std::size_t>(family) : 0; }
// "...\shaders_cache\r4\deffer_model_bump_4.vs\2048..." -> "deffer_model_bump_4.vs"; a plain source name stays as is.
std::string shader_root_name(const std::string& file) {
    std::size_t end = file.size();
    while (end) {
        const auto slash = file.find_last_of("\\/", end - 1);
        const auto begin = slash == std::string::npos ? 0 : slash + 1;
        const auto part = file.substr(begin, end - begin);
        if (part.size() > 3 && (part.ends_with(".vs") || part.ends_with(".ps") || part.ends_with(".hs") || part.ends_with(".ds")))
            return part;
        if (slash == std::string::npos) break;
        end = slash;
    }
    return file.size() > 64 ? file.substr(file.size() - 64) : file;
}
void remember_name(void** result, const std::string& file) noexcept try {
    void* engine_object{};
    void* stock{};
    if (!guarded_copy(&engine_object, result, sizeof(engine_object)) || !engine_object ||
        !guarded_copy(&stock, static_cast<std::byte*>(engine_object) + shader_object_offset, sizeof(stock)) || !stock) return;
    const auto root = shader_root_name(file);
    std::unique_lock lock(names_guard);
    if (shader_names.size() < shader_names_limit || shader_names.contains(stock)) {
        shader_names[stock] = root;
        shader_elements[stock] = model_element(root);
    }
} catch (...) {}
std::string name_of(const void* shader) {
    if (!shader) return "none";
    std::shared_lock lock(names_guard);
    const auto found = shader_names.find(shader);
    return found == shader_names.end() ? "unknown" : found->second;
}
ModelElement element_of(const void* shader) noexcept {
    if (!shader) return ModelElement::Unknown;
    std::shared_lock lock(names_guard);
    const auto found = shader_elements.find(shader);
    return found == shader_elements.end() ? ModelElement::Unknown : found->second;
}
void shader_created(const char* target, const void* buffer, unsigned size, const char* file, void** result,
    const void* return_address, const std::byte* caller_rsp) noexcept try {
    char stage{};
    // 'h'/'d': create_shader<SHS>/<SDS> (motion_profile.h tess_create_sites) leave the object at the same offset.
    if (!target || !guarded_copy(&stage, target, 1) || (stage != 'v' && stage != 'p' && stage != 'h' && stage != 'd') || !buffer ||
        !size || !result) return;
    ShaderRequest request;
    for (std::size_t i = 0; i < 16; ++i) {
        char c{};
        if (!guarded_copy(&c, target + i, 1) || !c) break;
        request.target.push_back(c);
    }
    request.bytecode = {static_cast<const std::byte*>(buffer), size};
    if (file) for (std::size_t i = 0; i < 520; ++i) {
        char c{};
        if (!guarded_copy(&c, file + i, 1) || !c) break;
        request.file_name.push_back(c);
    }
    remember_name(result, request.file_name);
    const auto reader = [](const void* address, void* out, std::size_t bytes) { return guarded_copy(out, address, bytes); };
    const auto frame = read_request(mp(), image, return_address, caller_rsp, reader, request);
    std::unique_lock lock(loader_guard);
    if (!loader) return;
    if (frame != FrameStatus::Ok) {
        { std::lock_guard s(stats_guard); ++stats.frame_refusals; }
        if (refusal_logs < 32) { ++refusal_logs; sayf("Anomaly motion shader frame refused (status %d): %s", int(frame), request.file_name.c_str()); }
        return;
    }
    const auto* entry = loader->lookup(request.source);
    if (!entry) { std::lock_guard s(stats_guard); ++stats.shaders[0][static_cast<std::size_t>(Outcome::NotAllowlisted)]; return; }
    auto prepared = loader->prepare(request);
    std::shared_ptr<Variant> variant;
    if (prepared.outcome == Outcome::Replaced || prepared.outcome == Outcome::ReplacedFromCache) {
        void* engine_object{};
        IUnknown* stock{};
        ID3D11Device* device{};
        read_global(srp().hw_device_rva, device);
        if (!guarded_copy(&engine_object, result, sizeof(engine_object)) || !engine_object ||
            !guarded_copy(&stock, static_cast<std::byte*>(engine_object) + shader_object_offset, sizeof(stock)) || !stock || !device) {
            prepared.outcome = Outcome::CreateFailed;
            prepared.detail = "engine shader object or device unreadable";
        } else {
            variant = std::make_shared<Variant>();
            variant->stock = stock;
            variant->device = device;
            variant->family = entry->family;
            variant->skin = prepared.skin;
            variant->request = request;
            variant->original_bytecode.assign(request.bytecode.begin(), request.bytecode.end());
            variant->request.bytecode = variant->original_bytecode;
            void* replacement{};
            const auto attached = runtime_shaders->api().attach_d3d11(device);
            const auto made = attached == 0 ? runtime_shaders->api().create_d3d11(device, request.target.c_str(),
                {prepared.variant.data(), prepared.variant.size()}, &replacement) : attached;
            const HRESULT hr = made == 0 ? S_OK : E_FAIL;
            if (stage == 'v') variant->vs.Attach(static_cast<ID3D11VertexShader*>(replacement));
            else if (stage == 'p') variant->ps.Attach(static_cast<ID3D11PixelShader*>(replacement));
            else if (stage == 'h') variant->hs.Attach(static_cast<ID3D11HullShader*>(replacement));
            else variant->ds.Attach(static_cast<ID3D11DomainShader*>(replacement));
            if (FAILED(hr)) {
                prepared.outcome = Outcome::CreateFailed;
                prepared.detail = attached != 0 ? "Runtime D3D11 interception refused" :
                    "Create*Shader 0x" + std::to_string(static_cast<unsigned long>(hr));
                variant.reset();
            } else {
                variant->reflection = std::move(prepared.reflection);
                if (stage == 'v' && (entry->family == Family::FoliageTree || entry->family == Family::FoliageDetail)) {
                    variant->foliage = make_foliage_layout(entry->family, prepared.stock);
                    if (!variant->foliage.usable && refusal_logs < 32) {
                        ++refusal_logs;
                        sayf("Anomaly motion foliage layout unusable for %s (variables missing from the stock reflection); "
                             "its draws keep the all-invalid foliage buffer.", entry->name.c_str());
                    }
                }
                std::unique_lock r(registry_guard);
                registry[stock] = variant;
            }
        }
    }
    {
        std::lock_guard s(stats_guard);
        ++stats.shaders[family_index(entry->family)][static_cast<std::size_t>(prepared.outcome)];
    }
    if (variant) {
        if (replace_logs < 24) {
            ++replace_logs;
            sayf("Anomaly motion variant %s %s skin=%d (%s) for %s", entry->name.c_str(), family_name(entry->family),
                prepared.skin, outcome_name(prepared.outcome), request.file_name.c_str());
        }
    } else if (refusal_logs < 32) {
        ++refusal_logs;
        sayf("Anomaly motion kept stock %s (%s): %s; %s", entry->name.c_str(), outcome_name(prepared.outcome),
            prepared.detail.substr(0, 300).c_str(), request.file_name.c_str());
    }
} catch (...) {}

long long __fastcall create_shader_hook(const char* target, const void* buffer, unsigned size, const char* file,
    void** result, char disasm) {
    {
        std::lock_guard lock(loader_guard);
        ID3D11Device* device{};
        if (runtime_shaders && hooks_active.load(std::memory_order_acquire) &&
            read_global(srp().hw_device_rva, device) && device)
            runtime_shaders->api().attach_d3d11(device);
    }
    const auto hr = reinterpret_cast<CreateShader>(originals[site_create_shader])(target, buffer, size, file, result, disasm);
    if (static_cast<int32_t>(hr) >= 0 && hooks_active.load(std::memory_order_acquire))
        shader_created(target, buffer, size, file, result, _ReturnAddress(),
            static_cast<const std::byte*>(_AddressOfReturnAddress()) + 8);
    return hr;
}

// ---- capture -------------------------------------------------------------------------------------------------------
void capture_abandon() noexcept {
    if (!capture.active) return;
    dump.complete(&capture.record, E_FAIL);
    capture.active = false;
    ++stats.capture_failed;
}
void capture_arm(uint32_t engine_frame) noexcept {
    if (!dump_ready) return;
    capture_abandon();
    dump.poll();
    const bool scheduled = capture.remaining && stats.frames >= capture.next;
    if (scheduled) dump.request();
    if (!dump.requested()) return;
    auto& r = capture.record;
    r = {};
    r.struct_size = sizeof(r);
    r.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    r.frame_id = static_cast<rsf_frame_id>(engine_frame) + 1;
    r.session_id = 0x4d4f54494f4eull;   // "MOTION"
    r.view_id = 0;
    r.phase = RSF_PHASE_RENDER_SUBMIT;
    r.screen = RSF_SCREEN_UNKNOWN;
    r.render_width = r.output_width = device_state.width();
    r.render_height = r.output_height = device_state.height();
    if (!dump.begin_record(&r, 0)) return;
    capture.active = true;
    ++stats.captures;
    if (scheduled) { --capture.remaining; capture.next = stats.frames + configured.capture_interval; }
}
void capture_take(const char* name, const char* description, void* texture, rsf_dump_view view, float scale) noexcept {
    if (!texture || !window.context) return;
    if (dump.texture_record(window.context, &capture.record, name, description, texture, view, scale)) ++stats.images;
    else ++stats.capture_failed;
}
void capture_finish() noexcept {
    if (!capture.active) return;
    capture_take("anomaly_motion", "Motion RTV2 after the G-buffer pass, RGBA16F (current-minus-previous UV x, UV y down, validity, 0)",
        device_state.texture(), RSF_DUMP_VIEW_RAW, 1.0f);
    capture_take("anomaly_motion_view", "Motion RTV2 as dense motion (UV x20)", device_state.texture(), RSF_DUMP_VIEW_DENSE_MOTION, 20.0f);
    capture_take("anomaly_gbuffer_position", "rt_Position after the G-buffer pass (packed normal, view Z, material)",
        window.position_surface, RSF_DUMP_VIEW_RAW, 1.0f);
    dump.complete(&capture.record, S_OK);
    capture.active = false;
}
void bind_dump() noexcept try {
    if (sr::options().capture) { say("Anomaly motion capture refused: SRStageCapture owns the frame dump service this run."); return; }
    const HMODULE runtime = GetModuleHandleW(L"ReScaleFrame.Runtime.dll");
    if (!runtime || !dump.bind(runtime)) { say("Anomaly motion capture refused: Runtime frame dump exports missing."); return; }
    wchar_t root[32768]{};
    std::wstring directory;
    const DWORD custom = GetEnvironmentVariableW(L"RSF_DUMP_DIR", root, 32768);
    if (custom && custom < 32768) directory = root;
    else {
        const DWORD size = GetEnvironmentVariableW(L"LOCALAPPDATA", root, 32768);
        if (!size || size >= 32768) { say("Anomaly motion capture refused: LOCALAPPDATA unavailable."); return; }
        directory = std::wstring(root) + L"\\ReScaleFrame\\Anomaly";
    }
    dump.configure(utf8(directory).c_str(), "Anomaly", build->sha256, log_fn, log_user);
    dump_ready = true;
    capture.remaining = configured.capture_count;
    capture.next = configured.capture_start;
} catch (...) { dump_ready = false; }

// ---- G-buffer window ---------------------------------------------------------------------------------------------
void reload_pack() {
    if (!reload_requested.exchange(false) || !shader_api || !image || !build) return;
    std::lock_guard lock(loader_guard);
    auto candidate = std::make_unique<RuntimeShaders>();
    const std::filesystem::path root(configured.shader_dir);
    std::string error;
    if (!candidate->open(root / "motion.rsfpatch", root / "cache", image, mp(), error)) {
        pack_status.store(2); sayf("Motion pack reload kept previous generation: %s", error.c_str()); return;
    }
    std::unordered_map<const void*, std::shared_ptr<const Variant>> next;
    std::vector<std::pair<const void*, std::shared_ptr<const Variant>>> previous;
    { std::shared_lock r(registry_guard); for (const auto& item : registry) previous.push_back(item); }
    for (const auto& [key, old] : previous) {
        const auto entry = candidate->manifest.by_original.find(sha256_hex(old->request.source));
        if (entry == candidate->manifest.by_original.end()) continue;
        const auto result = candidate->prepare(old->request, entry->second, *shader_api);
        if (result.outcome != Outcome::Replaced && result.outcome != Outcome::ReplacedFromCache) {
            pack_status.store(2); sayf("Motion pack reload kept previous generation: %s", result.detail.c_str()); return;
        }
        auto updated = std::make_shared<Variant>(*old);
        updated->request.bytecode = updated->original_bytecode;
        void* shader{};
        if (candidate->api().create_d3d11(old->device, old->request.target.c_str(),
            {result.variant.data(), result.variant.size()}, &shader) != 0) {
            pack_status.store(2); say("Motion pack reload kept previous generation: shader creation"); return;
        }
        switch (entry->second.stage) {
        case 'v': updated->vs.Attach(static_cast<ID3D11VertexShader*>(shader)); break;
        case 'p': updated->ps.Attach(static_cast<ID3D11PixelShader*>(shader)); break;
        case 'h': updated->hs.Attach(static_cast<ID3D11HullShader*>(shader)); break;
        case 'd': updated->ds.Attach(static_cast<ID3D11DomainShader*>(shader)); break;
        }
        updated->reflection = result.reflection;
        next.emplace(key, std::move(updated));
    }
    auto replacement_loader = std::make_unique<ShaderLoader>(candidate->manifest, *shader_api, *shader_api,
        [](const ShaderRequest& request, const ManifestEntry& entry) { return runtime_shaders->prepare(request, entry, *shader_api); });
    { std::unique_lock r(registry_guard); registry.swap(next); }
    runtime_shaders = std::move(candidate); loader = std::move(replacement_loader);
    link_cache.clear(); tess_link_cache.clear(); history.reset_all(); foliage_history.reset_all();
    pack_status.store(1); say("Motion shader pack reloaded from runtime source; history reset.");
}
void window_open(void* target) noexcept try {
    if (dump_shaders.exchange(false) && runtime_shaders) {
        const auto path = std::filesystem::path(configured.shader_dir) / "cache" / "shader-dump";
        sayf("Runtime shader dump result=%d", runtime_shaders->api().dump(path.c_str()));
    }
    reload_pack();
    if (!hooks_active.load(std::memory_order_acquire) || !pack_enabled.load()) return;
    ID3D11Device* device{};
    ID3D11DeviceContext* context{};
    std::byte* engine_target{};
    if (!read_global(srp().hw_device_rva, device) || !read_global(srp().hw_context_rva, context) || !device || !context ||
        !read_global(srp().render_implementation_rva + sr::render_target_offset, engine_target) || engine_target != target) return;
    if (!install_draw_hooks(context)) return;
    if (device_state.device() != device) {
        device_state.reset();
        history.reset_all();
        foliage_history.reset_all();
        link_cache.clear();
        tess_link_cache.clear();
        if (!device_state.create(device, log_fn, log_user)) { say("Anomaly motion: device resources could not be created."); return; }
    }
    std::byte* crt{};
    void* rtv{};
    void* surface{};
    if (!guarded_copy(&crt, engine_target + position_offset, sizeof(crt)) || !crt ||
        !guarded_copy(&rtv, crt + sr::crt_rtv_offset, sizeof(rtv)) || !guarded_copy(&surface, crt + sr::crt_surface_offset, sizeof(surface)) ||
        !rtv || !surface) return;
    uint32_t frame{};
    read_global(np().device_frame_rva, frame);
    const auto view = geometry::current_render_view(image, *build);
    if (!view) return;
    window.context = context;
    binder_host.context = context;
    window.position_rtv = rtv;
    window.position_surface = surface;
    const bool new_frame = !window.have_frame || window.frame != frame || window.view != *view;
    if (new_frame) {
        window.have_frame = true;
        window.frame = frame;
        window.view = *view;
        stats.retirements += retirements.drain([](void*, temporal::InstanceKey key) { history.retire(key); }, nullptr);
        NativeObservation observation{sizeof(observation), observation_abi};
        const uint64_t generation = rsf_anomaly_observation(&observation) == RSF_OK ? observation.resets : 0;
        uint32_t options3{};
        const bool msaa = !read_global(srp().render_options_word3_rva, options3) || (options3 & sr::dx10_msaa_bit);
        window.target_ok = false;
        ComPtr<ID3D11Texture2D> texture;
        if (!msaa && SUCCEEDED(static_cast<IUnknown*>(surface)->QueryInterface(IID_PPV_ARGS(&texture)))) {
            D3D11_TEXTURE2D_DESC desc{};
            texture->GetDesc(&desc);
            switch (device_state.ensure_target(desc)) {
            case TargetResult::Created: ++stats.target_created; [[fallthrough]];
            case TargetResult::Ready: window.target_ok = true; break;
            case TargetResult::Multisampled: ++stats.msaa_frames; break;
            default: ++stats.target_failed; break;
            }
        } else if (msaa) ++stats.msaa_frames;
        if (window.target_ok) {
            device_state.clear(context);
            ++stats.clears;
            history.begin_frame(*view, frame, generation, device_state.width(), device_state.height());
            foliage_history.begin_frame(*view, frame, generation, device_state.width(), device_state.height());
            stats.foliage_table_overflows = foliage_history.table_overflows();
        }
        ++stats.frames;
        if (window.target_ok && *view == temporal::ViewId::Main && configured.capture) capture_arm(frame);
    }
    window.open = true;
    ++stats.windows;
} catch (...) {}

void window_close() noexcept {
    if (!window.open) return;
    window.open = false;
    if (device_state.rtv()) {
        const auto action = binder.close(image + srp().backend_rva, device_state.rtv());
        if (action == BindAction::Unbound) ++stats.close_unbinds;
        else if (action == BindAction::Fault) ++stats.binder_faults;
    }
    window.cache_may_hold_motion = false;
    if (capture.active) capture_finish();
}
void __fastcall scene_begin_hook(void* self) {
    reinterpret_cast<Unary>(originals[site_phase_scene_begin])(self);
    window_open(self);
}
void __fastcall scene_end_hook(void* self) {
    window_close();
    reinterpret_cast<Unary>(originals[site_phase_scene_end])(self);
}

// ---- draw-time (after CBackend::constants.flush) ----------------------------------------------------------------
std::shared_ptr<const Variant> find_variant(const void* shader) noexcept {
    if (!shader) return {};
    std::shared_lock lock(registry_guard);
    const auto found = registry.find(shader);
    return found == registry.end() ? nullptr : found->second;
}
bool read_camera(CameraSample& out) noexcept {
    float view[16], projection[16];
    const auto* xforms = image + srp().backend_rva + sr::backend_xforms_offset;
    if (!guarded_copy(view, xforms + sr::xforms_view_offset, sizeof(view)) ||
        !guarded_copy(projection, xforms + sr::xforms_project_offset, sizeof(projection))) return false;
    out.view = from_engine(view);
    out.projection = from_engine(projection);
    return true;
}
void count_reason(HistoryReason reason) noexcept {
    const auto i = static_cast<std::size_t>(reason);
    if (i < MotionCounters::reasons) ++stats.reasons_by[i];
    (reason == HistoryReason::Valid ? stats.valid_draws : stats.invalid_draws) += 1;
}
// What a G-buffer draw is, for the per-class counters (draw_class_name).
std::size_t draw_class(const geometry::DrawTimeFacts& f) noexcept {
    using geometry::DrawKind;
    if (f.detail) return 6;
    if (!f.in_visual) return 7;
    if (f.classification.verdict == geometry::DrawVerdict::IgnoredStaticWorld) return 0;
    switch (f.kind) {
    case DrawKind::Rigid: return 1;
    case DrawKind::HardwareSkinned: return 2;
    case DrawKind::SingleBone: return 3;
    case DrawKind::SoftwareSkinned: return 4;
    default: return 5;
    }
}
constexpr std::size_t tessellated_class = 8;
static_assert(MotionCounters::bands == distance_bands, "character band counters follow motion_policy.h distance_band");
// The hull, domain and geometry shaders bound for this draw. The engine keeps its own references; the raw pointers stay
// valid through the draw.
struct BoundStages {
    ID3D11HullShader* hs{};
    ID3D11DomainShader* ds{};
    ID3D11GeometryShader* gs{};
    bool any() const noexcept { return hs || ds || gs; }
};
BoundStages intermediate_stages(ID3D11DeviceContext* context) noexcept {
    BoundStages s;
    context->HSGetShader(&s.hs, nullptr, nullptr);
    context->DSGetShader(&s.ds, nullptr, nullptr);
    context->GSGetShader(&s.gs, nullptr, nullptr);
    if (s.hs) s.hs->Release();
    if (s.ds) s.ds->Release();
    if (s.gs) s.gs->Release();
    return s;
}
void count_unpaired(const void* vs, const void* ps, std::size_t klass) noexcept try {
    std::lock_guard lock(unpaired_guard);
    const UnpairedKey key{vs, ps, klass};
    if (const auto found = unpaired_pairs.find(key); found != unpaired_pairs.end()) ++found->second;
    else if (unpaired_pairs.size() < unpaired_pairs_limit) unpaired_pairs.emplace(key, 1);
} catch (...) {}
// ---- foliage (trees and grass) -------------------------------------------------------------------------------------
// The VS constants of a foliage draw are read from the engine's staged copy after the flush uploaded them, so they are
// exactly what the draw uses. See motion_foliage.h for identity and the grass row hygiene.
bool read_constant(const ConstantRef& ref, void* out, std::uint32_t max_bytes) noexcept {
    if (!ref.present) return true;   // a variable the compiler removed stays zero in the record and in b13
    return geometry::read_vertex_constants(ref.buffer, ref.buffer_size, ref.offset, out, ref.size < max_bytes ? ref.size : max_bytes);
}
bool read_foliage_constants(const FoliageLayout& layout, FoliageConstants& out) noexcept {
    return read_constant(layout.consts, out.consts, 16) && read_constant(layout.wave, out.wave, 16) &&
        read_constant(layout.wind, out.wind, 16);
}
// Marks every instance row the engine has not filled for the NEXT batch. The batch just read was already uploaded by the
// flush (UpdateSubresource copies at call time), the engine writes groups 0..n-1 again before its next draw and the
// vertices of a draw only address those groups, so the unwritten tail is never read; poisoned it identifies itself.
void poison_batch(const FoliageLayout& layout) noexcept {
    DetailRows rows;
    poison_detail_rows(rows);
    if (geometry::write_vertex_constants(layout.array.buffer, layout.array.buffer_size, layout.array.offset, rows.data(),
            static_cast<std::uint32_t>(sizeof(float) * foliage_detail_floats))) ++stats.foliage_poisoned;
    else ++stats.foliage_poison_failed;
}
// A grass flush whose rows are not otherwise consumed (shadow passes, draws outside the motion pass): keep the engine's
// staged batch marked so the first G-buffer batch of the next frame does not inherit stale groups.
void poison_only(ID3D11DeviceContext* context) noexcept {
    if (!configured.foliage || !context) return;
    ID3D11VertexShader* vs{};
    context->VSGetShader(&vs, nullptr, nullptr);
    if (vs) vs->Release();
    const auto variant = find_variant(vs);
    if (variant && variant->family == Family::FoliageDetail && variant->foliage.usable) poison_batch(variant->foliage);
}
bool is_identity(const float (&values)[16]) noexcept {
    static constexpr float identity[16]{1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1};
    return std::memcmp(values, identity, sizeof(identity)) == 0;
}
// Everything a foliage draw's history needs. Records the draw for the next frame whether or not the pair gets swapped.
HistoryReason prepare_foliage(const geometry::DrawTimeFacts& facts, const Variant& vertex, const void* vs_key,
    bool context_known, CameraStatus camera, FoliageVertexBytes& bytes) noexcept {
    pack_foliage_invalid(bytes);
    if (!configured.foliage || !vertex.foliage.usable) return HistoryReason::Foliage;
    const auto& layout = vertex.foliage;
    const auto previous_jitter = sr::current_jitter_uv();
    const CameraSample* previous_camera = context_known ? history.previous_camera(*facts.view, *facts.domain) : nullptr;
    const bool active = context_known && foliage_history.active(*facts.view);
    if (vertex.family == Family::FoliageTree) {
        TreeDrawInput in;
        in.family = vertex.family;
        in.in_visual = facts.in_visual;
        in.kind = facts.kind;
        in.verdict = facts.classification.verdict;
        in.context = context_known;
        in.camera_conflict = camera == CameraStatus::Conflict;
        in.previous_camera = previous_camera;
        in.previous_jitter_u = previous_jitter.previous_u;
        in.previous_jitter_v = previous_jitter.previous_v;
        in.current.lod = facts.tree_lod;
        const bool tree = facts.in_visual && facts.kind == geometry::DrawKind::Tree && facts.visual;
        in.constants_ok = tree && read_constant(layout.xform, in.current.xform, sizeof(in.current.xform)) &&
            read_foliage_constants(layout, in.current.constants);
        if (in.constants_ok && active) in.previous = foliage_history.record_tree(*facts.view, facts.visual, in.current);
        const auto decision = decide_tree(in);
        if (decision.reason != HistoryReason::Valid) return decision.reason;
        if (!pack_foliage_tree(decision.previous_clip, in.previous->xform, in.previous->constants, bytes))
            return HistoryReason::FoliageNoConstants;
        return HistoryReason::Valid;
    }
    DetailDrawInput in;
    in.family = vertex.family;
    in.detail_window = facts.detail;
    in.context = context_known;
    in.camera_conflict = camera == CameraStatus::Conflict;
    in.previous_camera = previous_camera;
    in.previous_jitter_u = previous_jitter.previous_u;
    in.previous_jitter_v = previous_jitter.previous_v;
    float world[16];
    in.world_readable = geometry::read_world_matrix(world);
    in.world_identity = in.world_readable && is_identity(world);
    DetailRows rows;
    FoliageConstants current;
    in.constants_ok = facts.detail && read_foliage_constants(layout, current) &&
        geometry::read_vertex_constants(layout.array.buffer, layout.array.buffer_size, layout.array.offset, rows.data(),
            static_cast<std::uint32_t>(sizeof(float) * foliage_detail_floats));
    DetailRemap remap;
    if (in.constants_ok) {
        // Route: the shader permutation and its wave set (the wave 0 and wave 1 batches share one permutation).
        std::uint64_t route = reinterpret_cast<std::uintptr_t>(vs_key);
        for (const float w : {current.wave[0], current.wave[1], current.wave[2]}) {
            std::uint32_t bits;
            std::memcpy(&bits, &w, sizeof(bits));
            route = (route ^ bits) * 1099511628211ull;
        }
        if (active) remap = foliage_history.record_detail(*facts.view, route, current, rows);
        else remap.live = count_live_groups(rows);
        poison_batch(layout);
        stats.detail_items += remap.live;
        stats.detail_items_matched += remap.matched;
        stats.detail_items_unmatched += remap.unmatched;
        stats.detail_conflicts += remap.conflicts;
    }
    in.live_groups = remap.live;
    in.route_found = remap.route_found;
    const auto decision = decide_detail(in);
    if (decision.reason != HistoryReason::Valid) return decision.reason;
    // The route existed last frame but none of this batch's instances did: nothing to bind.
    if (remap.matched == 0) return HistoryReason::FoliageNoPrevious;
    if (!pack_foliage_detail(decision.previous_clip, remap.previous, remap.items, remap.live, bytes))
        return HistoryReason::FoliageNoConstants;
    return HistoryReason::Valid;
}

void on_flush(const geometry::DrawTimeFacts& facts) noexcept try {
    if (pending.active) {
        // A flush without the draw call it precedes (a compute dispatch): put the stock shaders back now.
        if (pending.restore && pending.context) restore_stock(pending.context, pending);
        pending = {};
        ++stats.stale_restores;
    }
    if (!hooks_active.load(std::memory_order_relaxed) || !image) return;
    auto* backend = image + srp().backend_rva;
    if (!window.open) {
        if (window.cache_may_hold_motion && device_state.rtv()) {
            const auto action = binder.on_draw(backend, false, nullptr, device_state.rtv());
            if (action == BindAction::Unbound) ++stats.unbinds;
            window.cache_may_hold_motion = false;
        }
        if (facts.detail) {
            ID3D11DeviceContext* engine_context{};
            if (read_global(srp().hw_context_rva, engine_context)) poison_only(engine_context);
        }
        return;
    }
    ++stats.window_draws;
    auto* context = window.context;
    const bool main = facts.pass == geometry::DrawPass::Main;
    const auto action = binder.on_draw(backend, window.target_ok && main, window.position_rtv, device_state.rtv());
    switch (action) {
    case BindAction::Bound: ++stats.rebinds; break;
    case BindAction::AlreadyBound: ++stats.already_bound; break;
    case BindAction::Unbound: ++stats.unbinds; break;
    case BindAction::Conflict: ++stats.conflicts; break;
    case BindAction::Fault: ++stats.binder_faults; break;
    default: break;
    }
    window.cache_may_hold_motion = binder.motion_bound();
    // History for the next frame: every main-pass draw's camera, every identified draw's snapshot.
    CameraStatus camera = CameraStatus::NoFrame;
    const temporal::InstanceSnapshot* previous{};
    const bool context_known = facts.view && facts.domain;
    bool stale = false;
    if (main && context_known && history.active(*facts.view)) {
        CameraSample sample;
        if (read_camera(sample)) camera = history.observe_camera(*facts.view, *facts.domain, sample);
        if (camera == CameraStatus::Conflict) ++stats.camera_conflicts;
        if (facts.capture) {
            geometry::FeedStatus why{};
            if (const auto snapshot = geometry::to_snapshot(*facts.capture, why)) {
                previous = history.record(*facts.view, facts.capture->key, *snapshot);
                stale = previous && stale_palette(*previous, *snapshot);
                ++stats.snapshots;
            }
        }
    }
    if (!binder.motion_bound() || !context) {
        if (facts.detail) poison_only(context);
        return;
    }
    // Character draws by camera distance (diagnostics only; MotionCounters::band_*). `band` stays out of range for
    // everything that is not a skinned visual in the main pass.
    std::size_t band = MotionCounters::bands;
    if (main && facts.in_visual && (facts.kind == geometry::DrawKind::HardwareSkinned ||
            facts.kind == geometry::DrawKind::SingleBone || facts.kind == geometry::DrawKind::SoftwareSkinned)) {
        band = distance_bands - 1;
        float world[16];
        CameraSample sample;
        if (geometry::read_world_matrix(world) && read_camera(sample))
            band = distance_band(camera_distance(sample.view, from_engine(world)));
        if (stale) ++stats.band_stale_palette[band];
    }
    const auto count_band = [band](std::uint64_t (&field)[MotionCounters::bands]) noexcept {
        if (band < MotionCounters::bands) ++field[band];
    };
    ID3D11VertexShader* vs{};
    ID3D11PixelShader* ps{};
    context->VSGetShader(&vs, nullptr, nullptr);
    context->PSGetShader(&ps, nullptr, nullptr);
    // The engine keeps its own references; these raw pointers stay valid through the draw.
    if (vs) vs->Release();
    if (ps) ps->Release();
    if (band < MotionCounters::bands) {
        switch (element_of(vs)) {
        case ModelElement::High: ++stats.band_hq[band]; break;
        case ModelElement::Low: ++stats.band_lq[band]; break;
        case ModelElement::Tessellated: ++stats.band_tess[band]; break;
        default: break;
        }
    }
    const auto vertex = find_variant(vs);
    const auto pixel = find_variant(ps);
    const bool tree_draw = facts.in_visual && facts.kind == geometry::DrawKind::Tree;
    const bool detail_draw = facts.detail;
    // A foliage VS variant's history is prepared (and recorded for the next frame) whether or not the pixel shader has a
    // variant to pair with; the swap below only decides what this draw writes.
    FoliageVertexBytes foliage_bytes;
    HistoryReason foliage_reason = HistoryReason::Foliage;
    const bool foliage_vertex = vertex && (vertex->family == Family::FoliageTree || vertex->family == Family::FoliageDetail);
    if (foliage_vertex) foliage_reason = prepare_foliage(facts, *vertex, vs, context_known, camera, foliage_bytes);
    else pack_foliage_invalid(foliage_bytes);
    if (detail_draw && !foliage_vertex) poison_only(context);
    // The variants extend the VS->PS interface (TEXCOORD12/13). With a hull and domain shader bound (X-Ray's DX11\tess
    // pass for PN/displacement materials when r4_enable_tessellation is on, used by near skinned character models), the
    // PS reads its input from the domain stage: the whole chain VS -> HS -> DS -> PS is swapped, and only when every
    // stage has a verified variant and every interface links (tess_chain_linkable). Otherwise the stock VS/PS cannot
    // write defined motion through the stock HS/DS, so the draw keeps its stock shaders and gets the invalidation replay.
    // A geometry shader is never swapped.
    const auto stages = intermediate_stages(context);
    const bool staged = stages.any();
    const auto device = device_state.device();
    std::shared_ptr<const Variant> hull, domain;
    bool paired = vertex && pixel && vertex->vs && pixel->ps && vertex->device == device && pixel->device == device;
    if (!staged && paired) {
        const std::pair<const void*, const void*> key{vs, ps};
        auto found = link_cache.find(key);
        if (found == link_cache.end()) found = link_cache.emplace(key, linkable(vertex->reflection, pixel->reflection)).first;
        paired = found->second;
    } else if (staged) {
        if (!stages.gs) {
            hull = find_variant(stages.hs);
            domain = find_variant(stages.ds);
        }
        const bool core_vertex = paired && (vertex->family == Family::CoreStatic || vertex->family == Family::CoreModel);
        paired = core_vertex && !stages.gs && hull && domain && hull->hs && domain->ds && hull->device == device &&
            domain->device == device;
        if (stages.gs) ++stats.tess_geometry;
        else if (!paired) ++stats.tess_no_variant;
        else {
            const std::array<const void*, 4> key{vs, stages.hs, stages.ds, ps};
            auto found = tess_link_cache.find(key);
            if (found == tess_link_cache.end())
                found = tess_link_cache.emplace(key, tess_chain_linkable(vertex->reflection, hull->reflection, domain->reflection,
                    pixel->reflection)).first;
            paired = found->second;
            if (!paired) ++stats.tess_unlinked;
        }
    }
    const std::size_t klass = staged ? tessellated_class : draw_class(facts);
    if (!paired) {
        if (tree_draw) ++stats.tree_stock;
        if (detail_draw) ++stats.detail_stock;
        ++stats.unpaired_draws;
        ++stats.class_unpaired[klass];
        count_band(stats.band_unpaired);
        count_unpaired(vs, ps, klass);
        pending = {true, context, configured.invalidate, false, nullptr, nullptr};
        return;
    }
    const bool foliage = vertex->family == Family::FoliageTree || vertex->family == Family::FoliageDetail;
    bool uploaded = false;
    if (foliage) {
        uploaded = device_state.set_foliage_vertex(context, foliage_bytes);
        count_reason(foliage_reason);
        const bool valid = foliage_reason == HistoryReason::Valid;
        ++(valid ? stats.class_valid : stats.class_invalid)[klass];
        if (vertex->family == Family::FoliageTree) ++(valid ? stats.tree_valid : stats.tree_invalid);
        else ++(valid ? stats.detail_valid : stats.detail_invalid);
    } else {
        CoreDrawInput input;
        input.family = vertex->family;
        input.skin = vertex->skin;
        input.in_visual = facts.in_visual;
        input.verdict = facts.classification.verdict;
        input.kind = facts.kind;
        input.context = context_known;
        input.camera_conflict = camera == CameraStatus::Conflict;
        input.previous_camera = context_known ? history.previous_camera(*facts.view, *facts.domain) : nullptr;
        input.previous = previous;
        const auto decision = decide_core(input);
        CoreVertexBytes bytes;
        bool valid = decision.reason == HistoryReason::Valid;
        if (valid) {
            valid = pack_core_vertex(decision.previous_wvp, decision.bones, decision.flags, bytes);
            count_reason(valid ? HistoryReason::Valid : HistoryReason::TooManyBones);
        } else {
            pack_core_invalid(bytes);
            count_reason(decision.reason);
            if (decision.reason == HistoryReason::NoPreviousInstance) count_band(stats.band_no_previous);
        }
        ++(valid ? stats.class_valid : stats.class_invalid)[klass];
        count_band(valid ? stats.band_valid : stats.band_invalid);
        uploaded = device_state.set_core_vertex(context, bytes);
        if (staged) {
            // The domain stage rebuilds the previous patch from the VS's previous object-space control points.
            // A refused pack (no history, or non-finite split matrices) writes history x = 0, and the domain stage then
            // writes validity 0 for the whole draw whatever the VS says.
            TessDomainBytes tess;
            if (!pack_tess_domain(decision.previous_wv, decision.previous_projection, valid, tess) && valid) ++stats.tess_pack_refused;
            uploaded = uploaded && device_state.set_tess_domain(context, tess);
        }
    }
    D3D11_VIEWPORT viewport{};
    UINT viewports = 1;
    context->RSGetViewports(&viewports, &viewport);
    // SR Stage 3 publishes the jitter of this frame's world and first-person projections (and of the previous main-view
    // frame, whose jittered camera the history recorded) at phase_scene_prepare; zero below Stage 3 and on scope frames.
    const auto sr_jitter = sr::current_jitter_uv();
    PixelInputs inputs{viewport.TopLeftX, viewport.TopLeftY, viewport.Width, viewport.Height,
        sr_jitter.current_u, sr_jitter.current_v, foliage ? 0.0f : sr_jitter.previous_u, foliage ? 0.0f : sr_jitter.previous_v};
    PixelBytes pixel_bytes;
    pack_pixel(inputs, pixel_bytes);   // an unusable viewport writes control 0: invalid motion, native shading intact
    uploaded = uploaded && device_state.set_pixel(context, pixel_bytes);
    if (!uploaded) {
        if (foliage) ++(vertex->family == Family::FoliageTree ? stats.tree_stock : stats.detail_stock);
        ++stats.upload_failures;
        pending = {true, context, configured.invalidate, false, nullptr, nullptr};
        return;
    }
    ++stats.motion_draws;
    if (foliage) ++(vertex->family == Family::FoliageTree ? stats.tree_replaced : stats.detail_replaced);
    context->VSSetShader(vertex->vs.Get(), nullptr, 0);
    context->PSSetShader(pixel->ps.Get(), nullptr, 0);
    if (staged) {
        context->HSSetShader(hull->hs.Get(), nullptr, 0);
        context->DSSetShader(domain->ds.Get(), nullptr, 0);
        ++stats.tess_swapped;
    }
    pending = {true, context, false, true, vs, ps, staged, stages.hs, stages.ds};
} catch (...) {}

bool guards_match(std::byte* module, uint32_t image_size, const MotionProfile& profile, const char** failed) noexcept {
    for (const auto& site : profile.motion_sites) {
        std::array<std::byte, 16> bytes{};
        if (site.rva > image_size - bytes.size() || !guarded_copy(bytes.data(), module + site.rva, bytes.size()) || bytes != site.expected) {
            *failed = site.name; return false;
        }
        for (const auto& guard : profile.motion_unique_guards)
            if (guard.rva == site.rva && !bytes_match(module + guard.rva, guard.expected_hex)) { *failed = site.name; return false; }
    }
    for (const auto& site : profile.create_shader_calls) {
        std::array<std::byte, 16> bytes{};
        if (!guarded_copy(bytes.data(), module + site.rva, bytes.size()) || bytes != site.expected) { *failed = site.name; return false; }
    }
    if (!bytes_match(module + profile.shader_compile_rva, profile.shader_compile_prologue)) { *failed = "CRender::shader_compile prologue"; return false; }
    if (!bytes_match(module + profile.d3dcompile_setup_rva, profile.d3dcompile_setup)) { *failed = "CRender::shader_compile D3DCompile setup"; return false; }
    if (!bytes_match(module + profile.tess_dispatch_rva, profile.tess_dispatch)) { *failed = "create_shader hull/domain dispatch"; return false; }
    for (const auto& site : profile.tess_create_sites) {
        if (!bytes_match(module + site.rva, site.prologue)) { *failed = site.name; return false; }
        if (!bytes_match(module + site.wrapper_rva, site.wrapper)) { *failed = "CreateHWShader (hull/domain)"; return false; }
    }
    char name{1};
    if (!guarded_copy(&name, module + profile.source_name_rva, 1) || name != 0) { *failed = "D3DCompile pSourceName"; return false; }
    return true;
}
}

MotionOptions read_options(const wchar_t* ini) noexcept try {
    MotionOptions result{};
    if (!ini) return result;
    result.enabled = GetPrivateProfileIntW(L"Anomaly", L"Motion", 0, ini) == 1;
    result.shader_dir = (std::filesystem::path(ini).parent_path() / L"ReScaleFrame" / L"patches").wstring();
    result.invalidate = GetPrivateProfileIntW(L"Anomaly", L"MotionInvalidate", 1, ini) != 0;
    result.foliage = GetPrivateProfileIntW(L"Anomaly", L"MotionFoliage", 1, ini) != 0;
    result.capture = GetPrivateProfileIntW(L"Anomaly", L"MotionCapture", 0, ini) != 0;
    result.capture_start = GetPrivateProfileIntW(L"Anomaly", L"MotionCaptureStart", 600, ini);
    result.capture_count = GetPrivateProfileIntW(L"Anomaly", L"MotionCaptureCount", 3, ini);
    result.capture_interval = GetPrivateProfileIntW(L"Anomaly", L"MotionCaptureInterval", 300, ini);
    if (result.capture_count > 16) result.capture_count = 16;
    if (!result.capture_interval) result.capture_interval = 1;
    return result;
} catch (...) { return {}; }

void configure(const MotionOptions& value, rsf_game_log_fn log, void* user) noexcept try {
    if (hooks_active.load()) return;
    configured = value;
    pack_enabled.store(value.enabled);
    options::add({pack_label, true, 0, 1, 1, 1, false,
        []() noexcept {
            const auto status = pack_status.load();
            rsf_patch_counters core{sizeof(core), RSF_SHADER_PATCH_VERSION};
            if (status == 1 && runtime_shaders) runtime_shaders->api().counters(&core);
            if (status == 1 && core.refused)
                std::snprintf(pack_label, sizeof(pack_label), "Motion shader pack: loaded (%llu refusals)",
                    static_cast<unsigned long long>(core.refused));
            else std::snprintf(pack_label, sizeof(pack_label), "Motion shader pack: %s",
                status == 1 ? "loaded" : status == 2 ? "reload refused; previous pack active" : "not loaded");
            return pack_enabled.load() ? 1 : 0;
        },
        [](int32_t v) noexcept { pack_enabled.store(v != 0); options::write_int(L"Motion", v ? 1 : 0); }});
    options::add({"Reload motion shader pack", true, 0, 1, 1, 1, false,
        []() noexcept { return 0; }, [](int32_t v) noexcept { if (v) reload_requested.store(true); }});
    options::add({"Dump intercepted shaders", true, 0, 1, 1, 1, false,
        []() noexcept { return 0; }, [](int32_t v) noexcept { if (v) dump_shaders.store(true); }});
    options::add({"Tree and grass motion", true, 0, 1, 1, 1, false,
        []() noexcept { return configured.foliage ? 1 : 0; },
        [](int32_t v) noexcept {
            configured.foliage = v != 0;
            geometry::geometry_enable_foliage_identity(configured.foliage);
            options::write_int(L"MotionFoliage", v ? 1 : 0);
        }});
    options::add({"Invalidate motion under stock draws", true, 0, 1, 1, 1, false,
        []() noexcept { return configured.invalidate ? 1 : 0; },
        [](int32_t v) noexcept { configured.invalidate = v != 0; options::write_int(L"MotionInvalidate", v ? 1 : 0); }});
    log_fn = log;
    log_user = user;
} catch (...) {}
const MotionOptions& options() noexcept { return configured; }
const char* draw_class_name(std::size_t c) noexcept {
    static constexpr const char* names[MotionCounters::classes] = {"static_world", "rigid", "skinned", "single_bone",
        "soft_skinned", "tree", "detail", "no_visual", "tessellated"};
    return c < MotionCounters::classes ? names[c] : "unknown";
}
bool active() noexcept { return hooks_active.load() && pack_enabled.load(); }
void set_jitter_uv(float current_u, float current_v, float previous_u, float previous_v) noexcept {
    jitter[0] = current_u; jitter[1] = current_v; jitter[2] = previous_u; jitter[3] = previous_v;
}

bool install(std::byte* module, uint32_t image_size, const BuildProfile& selected) noexcept try {
    if (hooks_active.load()) return true;
    if (!module || image_size != selected.image_size()) { say("Anomaly motion refused: image size differs from the verified build."); return false; }
    // Trampolines are process-lifetime: a re-start must be the build the first install verified.
    if (hooks.created() && build != &selected) { say("Anomaly motion refused: a different build than the installed hooks."); return false; }
    const char* failed = "unknown";
    if (!hooks.created() && !guards_match(module, image_size, *selected.motion, &failed)) {
        sayf("Anomaly motion refused, shaders stay stock: guard mismatch at %s", failed);
        return false;
    }
    if (!geometry::geometry_hooks_active()) { say("Anomaly motion refused: geometry hooks (draw-time site) are not active."); return false; }
    build = &selected;
    image = module;
    {
        std::lock_guard lock(loader_guard);
        if (!loader) {
            const std::filesystem::path root(configured.shader_dir);
            static D3DShaderApi api(engine_compiler_module);
            runtime_shaders = std::make_unique<RuntimeShaders>();
            std::string error;
            if (!api.ready() || !runtime_shaders->open(root / "motion.rsfpatch", root / "cache", module, *selected.motion, error)) {
                sayf("Anomaly motion pack refused: %s", error.c_str()); return false;
            }
            shader_api = &api;
            set_shader_runtime(runtime_shaders->api());
            ID3D11Device* device{};
            if (read_global(selected.sr->hw_device_rva, device) && device) runtime_shaders->api().attach_d3d11(device);
            const auto roots = runtime_shaders->manifest.by_original.size();
            loader = std::make_unique<ShaderLoader>(runtime_shaders->manifest, api, api,
                [](const ShaderRequest& request, const ManifestEntry& entry) { return runtime_shaders->prepare(request, entry, api); });
            sayf("Anomaly runtime shader pack: %zu allowlisted roots, namespace %s", roots,
                loader->manifest().name_space.substr(0, 16).c_str());
            pack_status.store(1);
        }
    }
    const auto& motion_sites = mp().motion_sites;
    const std::array<HookSite, motion_site_count> sites{{
        {motion_sites[site_create_shader].name, motion_sites[site_create_shader].rva, motion_sites[site_create_shader].expected,
            reinterpret_cast<void*>(&create_shader_hook), &originals[site_create_shader]},
        {motion_sites[site_phase_scene_begin].name, motion_sites[site_phase_scene_begin].rva, motion_sites[site_phase_scene_begin].expected,
            reinterpret_cast<void*>(&scene_begin_hook), &originals[site_phase_scene_begin]},
        {motion_sites[site_phase_scene_end].name, motion_sites[site_phase_scene_end].rva, motion_sites[site_phase_scene_end].expected,
            reinterpret_cast<void*>(&scene_end_hook), &originals[site_phase_scene_end]}}};
    geometry::geometry_set_sink({&retirements, &geometry::RetirementQueue::sink});
    geometry::geometry_set_flush_callback(&on_flush);
    geometry::geometry_enable_foliage_identity(configured.foliage);
    hooks_active = true;
    const bool installed = hooks.created() ? hooks.enable() : hooks.install(module, image_size, sites);
    if (!installed) {
        hooks_active = false;
        geometry::geometry_set_flush_callback(nullptr);
        geometry::geometry_enable_foliage_identity(false);
        say("Anomaly motion refused: guarded detour installation failed; shaders stay stock.");
        return false;
    }
    if (configured.capture) bind_dump();
    sayf("Anomaly motion hooks installed: create_shader, phase_scene_begin/end, constants.flush slot; MotionInvalidate=%d "
         "MotionCapture=%d (start=%u count=%u interval=%u) MotionFoliage=%d. No SR/FG consumer reads the target.",
        configured.invalidate ? 1 : 0, configured.capture ? 1 : 0, configured.capture_start, configured.capture_count,
        configured.capture_interval, configured.foliage ? 1 : 0);
    return true;
} catch (...) { return false; }

bool quiesce() noexcept {
    if (!hooks.created()) return true;
    hooks_active = false;
    geometry::geometry_set_flush_callback(nullptr);
    geometry::geometry_enable_foliage_identity(false);
    const bool disabled = hooks.disable();
    window.open = false;
    if (dump_ready) { capture_abandon(); dump.cancel(capture.record.session_id); }
    // The draw detours stay installed (another thread may be inside one) but are inert without pending work.
    return disabled;
}

MotionCounters counters() noexcept {
    std::lock_guard lock(stats_guard);
    return stats;
}

FrameTarget frame_target() noexcept {
    if (!hooks_active.load(std::memory_order_acquire) || !pack_enabled.load() || !window.have_frame) return {};
    return {device_state.texture(), device_state.width(), device_state.height(), window.frame,
        window.view == temporal::ViewId::Main, window.target_ok && device_state.texture() != nullptr};
}

void report(void (*emit)(void*, const char*), void* user) noexcept try {
    if (!emit || !hooks_active.load()) return;
    MotionCounters now;
    { std::lock_guard lock(stats_guard); now = stats; }
    char text[900];
    using ull = unsigned long long;
    if (runtime_shaders) {
        rsf_patch_counters core{sizeof(core),RSF_SHADER_PATCH_VERSION}; runtime_shaders->api().counters(&core);
        std::snprintf(text, sizeof(text),
            "Runtime shaders intercepted=%llu runtime_built=%llu cache_hits=%llu refused=%llu "
            "source=%llu hash=%llu anchor=%llu compile=%llu original=%llu reflection=%llu format=%llu internal=%llu",
            static_cast<ull>(core.intercepted), static_cast<ull>(core.built), static_cast<ull>(core.cache_hits),
            static_cast<ull>(core.refused), static_cast<ull>(core.refusals[3]), static_cast<ull>(core.refusals[4]),
            static_cast<ull>(core.refusals[5]), static_cast<ull>(core.refusals[6]), static_cast<ull>(core.refusals[7]),
            static_cast<ull>(core.refusals[8]), static_cast<ull>(core.refusals[9]), static_cast<ull>(core.refusals[10]));
        emit(user, text);
    }
    const auto d = [&](uint64_t MotionCounters::*field) { return static_cast<ull>(now.*field - reported.*field); };
    std::snprintf(text, sizeof(text),
        "Anomaly motion window frames=%llu windows=%llu clears=%llu draws=%llu motion=%llu valid=%llu invalid=%llu unpaired=%llu "
        "replays=%llu replay_no_depth_write=%llu replay_failed=%llu | rtv2 rebinds=%llu already=%llu unbinds=%llu close_unbinds=%llu "
        "conflicts=%llu faults=%llu | restores=%llu stale=%llu snapshots=%llu camera_conflicts=%llu retired=%llu upload_failures=%llu msaa=%llu",
        d(&MotionCounters::frames), d(&MotionCounters::windows), d(&MotionCounters::clears), d(&MotionCounters::window_draws),
        d(&MotionCounters::motion_draws), d(&MotionCounters::valid_draws), d(&MotionCounters::invalid_draws),
        d(&MotionCounters::unpaired_draws), d(&MotionCounters::replays), d(&MotionCounters::replay_skipped_no_depth_write),
        d(&MotionCounters::replay_failed), d(&MotionCounters::rebinds), d(&MotionCounters::already_bound), d(&MotionCounters::unbinds),
        d(&MotionCounters::close_unbinds), d(&MotionCounters::conflicts), d(&MotionCounters::binder_faults), d(&MotionCounters::restores),
        d(&MotionCounters::stale_restores), d(&MotionCounters::snapshots), d(&MotionCounters::camera_conflicts),
        d(&MotionCounters::retirements), d(&MotionCounters::upload_failures), d(&MotionCounters::msaa_frames));
    emit(user, text);
    std::string reasons = "Anomaly motion invalid reasons";
    for (std::size_t i = 1; i < MotionCounters::reasons; ++i) {
        char item[64];
        std::snprintf(item, sizeof(item), " %s=%llu", history_reason_name(static_cast<HistoryReason>(i)),
            static_cast<ull>(now.reasons_by[i] - reported.reasons_by[i]));
        reasons += item;
    }
    emit(user, reasons.c_str());
    std::snprintf(text, sizeof(text),
        "Anomaly motion foliage tree replaced=%llu stock=%llu valid=%llu invalid=%llu | detail replaced=%llu stock=%llu valid=%llu "
        "invalid=%llu | detail items=%llu matched=%llu unmatched=%llu conflicts=%llu | rows poisoned=%llu poison_failed=%llu "
        "table_overflows=%llu",
        d(&MotionCounters::tree_replaced), d(&MotionCounters::tree_stock), d(&MotionCounters::tree_valid),
        d(&MotionCounters::tree_invalid), d(&MotionCounters::detail_replaced), d(&MotionCounters::detail_stock),
        d(&MotionCounters::detail_valid), d(&MotionCounters::detail_invalid), d(&MotionCounters::detail_items),
        d(&MotionCounters::detail_items_matched), d(&MotionCounters::detail_items_unmatched),
        d(&MotionCounters::detail_conflicts), d(&MotionCounters::foliage_poisoned), d(&MotionCounters::foliage_poison_failed),
        d(&MotionCounters::foliage_table_overflows));
    emit(user, text);
    std::snprintf(text, sizeof(text),
        "Anomaly motion tessellation chains swapped=%llu no_variant=%llu unlinked=%llu geometry_shader=%llu pack_refused=%llu",
        d(&MotionCounters::tess_swapped), d(&MotionCounters::tess_no_variant), d(&MotionCounters::tess_unlinked),
        d(&MotionCounters::tess_geometry), d(&MotionCounters::tess_pack_refused));
    emit(user, text);
    {
        std::string classes = "Anomaly motion classes";
        for (const auto& [label, field] : {std::pair{"valid", &MotionCounters::class_valid},
                 std::pair{"invalid", &MotionCounters::class_invalid}, std::pair{"unpaired", &MotionCounters::class_unpaired}}) {
            classes += std::string(" | ") + label;
            for (std::size_t c = 0; c < MotionCounters::classes; ++c) {
                char item[48];
                std::snprintf(item, sizeof(item), " %s=%llu", draw_class_name(c), static_cast<ull>((now.*field)[c] - (reported.*field)[c]));
                classes += item;
            }
        }
        emit(user, classes.c_str());
        // Character draws by camera distance: what a far NPC actually gets (history decision, unpaired, element).
        std::string bands = "Anomaly motion character bands";
        for (std::size_t b = 0; b < MotionCounters::bands; ++b) {
            const auto v = [&](std::uint64_t (MotionCounters::*field)[MotionCounters::bands]) {
                return static_cast<ull>((now.*field)[b] - (reported.*field)[b]);
            };
            char item[200];
            std::snprintf(item, sizeof(item), " | %s valid=%llu invalid=%llu unpaired=%llu no_previous=%llu stale_palette=%llu "
                "hq=%llu lq=%llu tess=%llu", distance_band_name(b), v(&MotionCounters::band_valid), v(&MotionCounters::band_invalid),
                v(&MotionCounters::band_unpaired), v(&MotionCounters::band_no_previous), v(&MotionCounters::band_stale_palette),
                v(&MotionCounters::band_hq), v(&MotionCounters::band_lq), v(&MotionCounters::band_tess));
            bands += item;
        }
        emit(user, bands.c_str());
        std::map<UnpairedKey, std::uint64_t> pairs;
        { std::lock_guard lock(unpaired_guard); pairs.swap(unpaired_pairs); }
        std::vector<std::pair<std::uint64_t, UnpairedKey>> top;
        for (const auto& [key, count] : pairs) top.emplace_back(count, key);
        std::sort(top.begin(), top.end(), [](const auto& a, const auto& b) { return a.first > b.first; });
        if (!top.empty()) {
            std::string line = "Anomaly motion unpaired (draws without a linkable variant pair, invalid motion):";
            for (std::size_t i = 0; i < top.size() && i < 10; ++i) {
                const auto& key = top[i].second;
                line += " " + name_of(key.vs) + "+" + name_of(key.ps) + "[" + draw_class_name(key.draw_class) + "]=" +
                    std::to_string(top[i].first);
            }
            if (top.size() > 10) line += " (+" + std::to_string(top.size() - 10) + " more pairs)";
            emit(user, line.c_str());
        }
    }
    std::string shaders = "Anomaly motion shaders (totals)";
    for (std::size_t f = 0; f < MotionCounters::families; ++f)
        for (std::size_t o = 0; o < MotionCounters::outcomes; ++o) {
            if (!now.shaders[f][o]) continue;
            char item[96];
            std::snprintf(item, sizeof(item), " %s.%s=%llu", f ? family_name(static_cast<Family>(f)) : "other",
                outcome_name(static_cast<Outcome>(o)), static_cast<ull>(now.shaders[f][o]));
            shaders += item;
        }
    char tail[200];
    std::snprintf(tail, sizeof(tail), " frame_refusals=%llu | target created=%llu failed=%llu | captures=%llu images=%llu capture_failed=%llu",
        static_cast<ull>(now.frame_refusals), static_cast<ull>(now.target_created), static_cast<ull>(now.target_failed),
        static_cast<ull>(now.captures), static_cast<ull>(now.images), static_cast<ull>(now.capture_failed));
    shaders += tail;
    emit(user, shaders.c_str());
    reported = now;
} catch (...) {}
}
