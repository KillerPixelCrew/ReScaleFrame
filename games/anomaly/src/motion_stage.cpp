// SPDX-License-Identifier: GPL-3.0-only
#include "motion_stage.h"
#include "geometry_feed.h"
#include "motion_d3d11.h"
#include "motion_history.h"
#include "motion_policy.h"
#include "motion_profile.h"
#include "motion_target.h"
#include "native_geometry.h"
#include "native_hooks.h"
#include "native_observer.h"
#include "shader_d3d.h"
#include "shader_loader.h"
#include "sr_profile.h"
#include "sr_stage.h"
#include <rescaleframe/game_frame.h>
#include <rescaleframe/texture_dump.h>
#include <MinHook.h>
#include <d3d11.h>
#include <intrin.h>
#include <windows.h>
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
#include <unordered_map>

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
    ID3D11Device* device{};
    Family family{};
    int skin{-2};
    Reflection reflection;
};
std::shared_mutex registry_guard;
std::unordered_map<const void*, std::shared_ptr<const Variant>> registry;
std::map<std::pair<const void*, const void*>, bool> link_cache;   // render thread only
std::mutex loader_guard, stats_guard;
D3DShaderApi* shader_api{};
std::unique_ptr<ShaderLoader> loader;
uint32_t refusal_logs{}, replace_logs{};

// ---- D3D11 and engine state --------------------------------------------------------------------------------------
MotionDevice device_state;
MotionHistory history;
geometry::RetirementQueue retirements;
MotionCounters stats{};
MotionCounters reported{};
std::atomic<bool> hooks_active{};
std::atomic<float> jitter[4]{};
HookSet hooks;
std::array<void*, motion_sites.size()> originals{};
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
};
thread_local Pending pending;

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
        context->VSSetShader(taken.vs, nullptr, 0);
        context->PSSetShader(taken.ps, nullptr, 0);
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
void shader_created(const char* target, const void* buffer, unsigned size, const char* file, void** result,
    const void* return_address, const std::byte* caller_rsp) noexcept try {
    char stage{};
    if (!target || !guarded_copy(&stage, target, 1) || (stage != 'v' && stage != 'p') || !buffer || !size || !result) return;
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
    const auto reader = [](const void* address, void* out, std::size_t bytes) { return guarded_copy(out, address, bytes); };
    const auto frame = read_request(image, return_address, caller_rsp, reader, request);
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
        read_global(sr::hw_device_rva, device);
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
            HRESULT hr = E_FAIL;
            if (stage == 'v') hr = device->CreateVertexShader(prepared.variant.data(), prepared.variant.size(), nullptr, &variant->vs);
            else hr = device->CreatePixelShader(prepared.variant.data(), prepared.variant.size(), nullptr, &variant->ps);
            if (FAILED(hr)) {
                prepared.outcome = Outcome::CreateFailed;
                prepared.detail = "Create*Shader 0x" + std::to_string(static_cast<unsigned long>(hr));
                variant.reset();
            } else {
                variant->reflection = std::move(prepared.reflection);
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
    dump.configure(utf8(directory).c_str(), "Anomaly", dx11_hash, log_fn, log_user);
    dump_ready = true;
    capture.remaining = configured.capture_count;
    capture.next = configured.capture_start;
} catch (...) { dump_ready = false; }

// ---- G-buffer window ---------------------------------------------------------------------------------------------
void window_open(void* target) noexcept try {
    if (!hooks_active.load(std::memory_order_acquire)) return;
    ID3D11Device* device{};
    ID3D11DeviceContext* context{};
    std::byte* engine_target{};
    if (!read_global(sr::hw_device_rva, device) || !read_global(sr::hw_context_rva, context) || !device || !context ||
        !read_global(sr::render_implementation_rva + sr::render_target_offset, engine_target) || engine_target != target) return;
    if (!install_draw_hooks(context)) return;
    if (device_state.device() != device) {
        device_state.reset();
        history.reset_all();
        link_cache.clear();
        if (!device_state.create(device, log_fn, log_user)) { say("Anomaly motion: device resources could not be created."); return; }
    }
    std::byte* crt{};
    void* rtv{};
    void* surface{};
    if (!guarded_copy(&crt, engine_target + position_offset, sizeof(crt)) || !crt ||
        !guarded_copy(&rtv, crt + sr::crt_rtv_offset, sizeof(rtv)) || !guarded_copy(&surface, crt + sr::crt_surface_offset, sizeof(surface)) ||
        !rtv || !surface) return;
    uint32_t frame{};
    read_global(device_frame_rva, frame);
    const auto view = geometry::current_render_view(image);
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
        const bool msaa = !read_global(sr::render_options_word3_rva, options3) || (options3 & sr::dx10_msaa_bit);
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
        const auto action = binder.close(image + sr::backend_rva, device_state.rtv());
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
    const auto* xforms = image + sr::backend_rva + sr::backend_xforms_offset;
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
void on_flush(const geometry::DrawTimeFacts& facts) noexcept try {
    if (pending.active) {
        // A flush without the draw call it precedes (a compute dispatch): put the stock shaders back now.
        if (pending.restore && pending.context) {
            pending.context->VSSetShader(pending.vs, nullptr, 0);
            pending.context->PSSetShader(pending.ps, nullptr, 0);
        }
        pending = {};
        ++stats.stale_restores;
    }
    if (!hooks_active.load(std::memory_order_relaxed) || !image) return;
    auto* backend = image + sr::backend_rva;
    if (!window.open) {
        if (window.cache_may_hold_motion && device_state.rtv()) {
            const auto action = binder.on_draw(backend, false, nullptr, device_state.rtv());
            if (action == BindAction::Unbound) ++stats.unbinds;
            window.cache_may_hold_motion = false;
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
    if (main && context_known && history.active(*facts.view)) {
        CameraSample sample;
        if (read_camera(sample)) camera = history.observe_camera(*facts.view, *facts.domain, sample);
        if (camera == CameraStatus::Conflict) ++stats.camera_conflicts;
        if (facts.capture) {
            geometry::FeedStatus why{};
            if (const auto snapshot = geometry::to_snapshot(*facts.capture, why)) {
                previous = history.record(*facts.view, facts.capture->key, *snapshot);
                ++stats.snapshots;
            }
        }
    }
    if (!binder.motion_bound() || !context) return;
    ID3D11VertexShader* vs{};
    ID3D11PixelShader* ps{};
    context->VSGetShader(&vs, nullptr, nullptr);
    context->PSGetShader(&ps, nullptr, nullptr);
    // The engine keeps its own references; these raw pointers stay valid through the draw.
    if (vs) vs->Release();
    if (ps) ps->Release();
    const auto vertex = find_variant(vs);
    const auto pixel = find_variant(ps);
    bool paired = vertex && pixel && vertex->vs && pixel->ps && vertex->device == device_state.device() &&
        pixel->device == device_state.device();
    if (paired) {
        const std::pair<const void*, const void*> key{vs, ps};
        auto found = link_cache.find(key);
        if (found == link_cache.end()) found = link_cache.emplace(key, linkable(vertex->reflection, pixel->reflection)).first;
        paired = found->second;
    }
    if (!paired) {
        ++stats.unpaired_draws;
        pending = {true, context, configured.invalidate, false, nullptr, nullptr};
        return;
    }
    const bool foliage = vertex->family == Family::FoliageTree || vertex->family == Family::FoliageDetail;
    bool uploaded = false;
    if (foliage) {
        FoliageVertexBytes bytes;
        pack_foliage_invalid(bytes);
        uploaded = device_state.set_foliage_vertex(context, bytes);
        count_reason(HistoryReason::Foliage);
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
        if (decision.reason == HistoryReason::Valid) {
            if (!pack_core_vertex(decision.previous_wvp, decision.bones, decision.flags, bytes)) count_reason(HistoryReason::TooManyBones);
            else count_reason(HistoryReason::Valid);
        } else {
            pack_core_invalid(bytes);
            count_reason(decision.reason);
        }
        uploaded = device_state.set_core_vertex(context, bytes);
    }
    D3D11_VIEWPORT viewport{};
    UINT viewports = 1;
    context->RSGetViewports(&viewports, &viewport);
    PixelInputs inputs{viewport.TopLeftX, viewport.TopLeftY, viewport.Width, viewport.Height,
        jitter[0].load(std::memory_order_relaxed), jitter[1].load(std::memory_order_relaxed),
        foliage ? 0.0f : jitter[2].load(std::memory_order_relaxed), foliage ? 0.0f : jitter[3].load(std::memory_order_relaxed)};
    PixelBytes pixel_bytes;
    pack_pixel(inputs, pixel_bytes);   // an unusable viewport writes control 0: invalid motion, native shading intact
    uploaded = uploaded && device_state.set_pixel(context, pixel_bytes);
    if (!uploaded) {
        ++stats.upload_failures;
        pending = {true, context, configured.invalidate, false, nullptr, nullptr};
        return;
    }
    ++stats.motion_draws;
    context->VSSetShader(vertex->vs.Get(), nullptr, 0);
    context->PSSetShader(pixel->ps.Get(), nullptr, 0);
    pending = {true, context, false, true, vs, ps};
} catch (...) {}

bool guards_match(std::byte* module, uint32_t image_size, const char** failed) noexcept {
    for (const auto& site : motion_sites) {
        std::array<std::byte, 16> bytes{};
        if (site.rva > image_size - bytes.size() || !guarded_copy(bytes.data(), module + site.rva, bytes.size()) || bytes != site.expected) {
            *failed = site.name; return false;
        }
    }
    for (const auto& site : create_shader_calls) {
        std::array<std::byte, 16> bytes{};
        if (!guarded_copy(bytes.data(), module + site.rva, bytes.size()) || bytes != site.expected) { *failed = site.name; return false; }
    }
    if (!bytes_match(module + shader_compile_rva, shader_compile_prologue)) { *failed = "CRender::shader_compile prologue"; return false; }
    if (!bytes_match(module + d3dcompile_setup_rva, d3dcompile_setup)) { *failed = "CRender::shader_compile D3DCompile setup"; return false; }
    char name{1};
    if (!guarded_copy(&name, module + source_name_rva, 1) || name != 0) { *failed = "D3DCompile pSourceName"; return false; }
    return true;
}
}

MotionOptions read_options(const wchar_t* ini) noexcept try {
    MotionOptions result{};
    if (!ini) return result;
    result.enabled = GetPrivateProfileIntW(L"Anomaly", L"Motion", 0, ini) == 1;
    wchar_t path[32768]{};
    GetPrivateProfileStringW(L"Anomaly", L"MotionShaderDir", L"", path, 32768, ini);
    if (path[0]) result.shader_dir = path;
    else result.shader_dir = (std::filesystem::path(ini).parent_path() / L"ReScaleFrame" / L"anomaly-motion").wstring();
    result.invalidate = GetPrivateProfileIntW(L"Anomaly", L"MotionInvalidate", 1, ini) != 0;
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
    log_fn = log;
    log_user = user;
} catch (...) {}
const MotionOptions& options() noexcept { return configured; }
bool active() noexcept { return hooks_active.load(); }
void set_jitter_uv(float current_u, float current_v, float previous_u, float previous_v) noexcept {
    jitter[0] = current_u; jitter[1] = current_v; jitter[2] = previous_u; jitter[3] = previous_v;
}

bool install(std::byte* module, uint32_t image_size) noexcept try {
    if (!configured.enabled) return true;
    if (hooks_active.load()) return true;
    if (!module || image_size != native_image_size) { say("Anomaly motion refused: image size differs from the verified build."); return false; }
    const char* failed = "unknown";
    if (!hooks.created() && !guards_match(module, image_size, &failed)) {
        sayf("Anomaly motion refused, shaders stay stock: guard mismatch at %s", failed);
        return false;
    }
    if (!geometry::geometry_hooks_active()) { say("Anomaly motion refused: geometry hooks (draw-time site) are not active."); return false; }
    image = module;
    {
        std::lock_guard lock(loader_guard);
        if (!loader) {
            const std::filesystem::path root(configured.shader_dir);
            std::ifstream file(root / "manifest.txt", std::ios::binary);
            if (!file) { sayf("Anomaly motion refused: no manifest.txt in %s", utf8(configured.shader_dir).c_str()); return false; }
            const std::string text((std::istreambuf_iterator<char>(file)), std::istreambuf_iterator<char>());
            std::string error;
            auto manifest = Manifest::parse(text, error);
            if (!manifest) { sayf("Anomaly motion refused: manifest invalid (%s).", error.c_str()); return false; }
            static D3DShaderApi api(engine_compiler_module);
            if (!api.ready()) { say("Anomaly motion refused: the engine's d3dcompiler_43 D3DCompile/D3DReflect is unavailable."); return false; }
            shader_api = &api;
            const auto cache = root / "cache" / manifest->name_space.substr(0, 16);
            const auto roots = manifest->by_original.size();
            loader = std::make_unique<ShaderLoader>(std::move(*manifest), root, cache, api, api);
            sayf("Anomaly motion manifest: %zu allowlisted roots, namespace %s, cache %s", roots,
                loader->manifest().name_space.substr(0, 16).c_str(), cache.string().c_str());
        }
    }
    const std::array<HookSite, motion_sites.size()> sites{{
        {motion_sites[site_create_shader].name, motion_sites[site_create_shader].rva, motion_sites[site_create_shader].expected,
            reinterpret_cast<void*>(&create_shader_hook), &originals[site_create_shader]},
        {motion_sites[site_phase_scene_begin].name, motion_sites[site_phase_scene_begin].rva, motion_sites[site_phase_scene_begin].expected,
            reinterpret_cast<void*>(&scene_begin_hook), &originals[site_phase_scene_begin]},
        {motion_sites[site_phase_scene_end].name, motion_sites[site_phase_scene_end].rva, motion_sites[site_phase_scene_end].expected,
            reinterpret_cast<void*>(&scene_end_hook), &originals[site_phase_scene_end]}}};
    geometry::geometry_set_sink({&retirements, &geometry::RetirementQueue::sink});
    geometry::geometry_set_flush_callback(&on_flush);
    hooks_active = true;
    const bool installed = hooks.created() ? hooks.enable() : hooks.install(module, image_size, sites);
    if (!installed) {
        hooks_active = false;
        geometry::geometry_set_flush_callback(nullptr);
        say("Anomaly motion refused: guarded detour installation failed; shaders stay stock.");
        return false;
    }
    if (configured.capture) bind_dump();
    sayf("Anomaly motion hooks installed: create_shader, phase_scene_begin/end, constants.flush slot; MotionInvalidate=%d "
         "MotionCapture=%d (start=%u count=%u interval=%u). No SR/FG consumer reads the target.",
        configured.invalidate ? 1 : 0, configured.capture ? 1 : 0, configured.capture_start, configured.capture_count,
        configured.capture_interval);
    return true;
} catch (...) { return false; }

bool quiesce() noexcept {
    if (!hooks.created()) return true;
    hooks_active = false;
    geometry::geometry_set_flush_callback(nullptr);
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

void report(void (*emit)(void*, const char*), void* user) noexcept try {
    if (!emit || !hooks_active.load()) return;
    MotionCounters now;
    { std::lock_guard lock(stats_guard); now = stats; }
    char text[900];
    using ull = unsigned long long;
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
