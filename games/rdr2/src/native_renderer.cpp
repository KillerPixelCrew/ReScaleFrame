// SPDX-License-Identifier: GPL-3.0-only
#include "native_renderer.h"
#include "render_camera.h"
#include "scene_boundary.h"
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <MinHook.h>
#include <intrin.h>
#include <array>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include <mutex>
#include <unordered_map>
#include <utility>

namespace {
using Microsoft::WRL::ComPtr;
// Direct-resource descriptor constructed by +26f3f10, consumed by +26f3a3c. Keep its full
// native size so a fallback can reset history without changing the caller's stack descriptor.
struct Evaluation {
    ID3D12Resource* color;
    ID3D12Resource* output;
    float sharpness;
    uint32_t padding14;
    ID3D12Resource* depth;
    ID3D12Resource* motion;
    float jitter[2];
    uint32_t width, height, reset;
    float motion_scale[2];
    uint32_t padding44;
    ID3D12Resource* transparency;
    ID3D12Resource* exposure;
    ID3D12Resource* bias;
    uint32_t origins[12];
    float pre_exposure;
    uint32_t invert_x, invert_y;
    unsigned char tail[0x168 - 0x9c];
};
static_assert(sizeof(Evaluation) == 0x168);
static_assert(offsetof(Evaluation, depth) == 0x18 && offsetof(Evaluation, origins) == 0x60);
using Evaluate = uint32_t (*)(ID3D12GraphicsCommandList*, void*, void*, const Evaluation*);
using Pop = void* (*)(void*, void*);
using Execute = void (STDMETHODCALLTYPE *)(ID3D12CommandQueue*, UINT, ID3D12CommandList* const*);
using Present = HRESULT (STDMETHODCALLTYPE *)(IDXGISwapChain*, UINT, UINT);
using FsrDispatch = int32_t (*)(void*, const void*);
using FsrCreate = int32_t (*)(void*, const void*);
Evaluate native_evaluate{};
Pop native_pop{};
FsrDispatch native_fsr{};
FsrCreate native_fsr_create{};
// ffxFsr2ContextDispatch inside the game's own ffx_fsr2_api_x64.dll. Detouring the export, as
// OptiScaler does, reaches every caller; the earlier import-slot patch at +6032a98 saw no call.
unsigned char* fsr_export{};
unsigned char* fsr_create_export{};
std::atomic<uintptr_t> fsr_caller{};
rsf_game_host_services host{};
const unsigned char* image{};
constexpr uintptr_t image_size = 0x7397600;
std::mutex guard;
std::atomic<bool> active{};
std::atomic<uint32_t> callbacks{};
std::atomic<uint64_t> next_submission{};
bool prepared{};
struct History { uint64_t frame{}; float jitter[2]{}; bool native_gap{}; uint32_t native_result{UINT32_MAX}; };
std::unordered_map<void*, History> histories;
ComPtr<ID3D12CommandQueue> queue;
void** queue_original_table{};
std::array<void*, 19> queue_table{};
Execute native_execute{};
ComPtr<IDXGISwapChain4> swapchain;
std::array<void*, 41> swapchain_table{};
Present native_present{};
std::mutex evaluations;
struct Callback { Callback() { ++callbacks; } ~Callback() { --callbacks; } };
bool read(const void* source, void* destination, size_t size)
{
    SIZE_T count{};
    return source && ReadProcessMemory(GetCurrentProcess(), source, destination, size, &count) && count == size;
}
template<class T> bool read_at(uintptr_t address, T& value)
{
    return read(reinterpret_cast<const void*>(address), &value, sizeof(value));
}
void log(const char* message) { if (host.log) host.log(host.user, message); }
HRESULT STDMETHODCALLTYPE present(IDXGISwapChain* owner, UINT interval, UINT flags)
{
    Callback callback;
    if (host.present) host.present(host.user, owner, interval, flags, 1);
    const auto result = native_present(owner, interval, flags);
    if (host.present) host.present(host.user, owner, interval, flags, 0);
    return result;
}
void attach_present()
{
    std::lock_guard lock(guard);
    if (swapchain) return;
    IDXGISwapChain* candidate{};
    if (!read(image + 0x571c7e8, &candidate, sizeof(candidate)) || !candidate) return;
    ComPtr<IDXGISwapChain4> latest;
    if (FAILED(candidate->QueryInterface(IID_PPV_ARGS(&latest))) || latest.Get() != candidate) return;
    auto** table = *reinterpret_cast<void***>(candidate);
    if (!read(table, swapchain_table.data(), sizeof(swapchain_table))) return;
    native_present = reinterpret_cast<Present>(swapchain_table[8]);
    swapchain_table[8] = reinterpret_cast<void*>(&present);
    DWORD protection{};
    if (!VirtualProtect(candidate, sizeof(void*), PAGE_READWRITE, &protection)) return;
    const auto replaced = InterlockedCompareExchangePointer(reinterpret_cast<void* volatile*>(candidate),
        swapchain_table.data(), table);
    DWORD ignored{};
    VirtualProtect(candidate, sizeof(void*), protection, &ignored);
    if (replaced != table) return;
    swapchain = latest;
    rsf_rdr2_scene_swapchain(candidate);
    log("RDR2 actual swapchain Present observer attached; existing Steam/RTSS Present chain preserved.");
}
void STDMETHODCALLTYPE execute(ID3D12CommandQueue* owner, UINT count, ID3D12CommandList* const* lists)
{
    Callback callback;
    const uint64_t id = ++next_submission;
    auto* pointers = reinterpret_cast<void* const*>(lists);
    if (host.submit) host.submit(host.user, owner, id, count, pointers, 1);
    native_execute(owner, count, lists);
    if (host.submit) host.submit(host.user, owner, id, count, pointers, 0);
}
void attach_queue(ID3D12CommandQueue* candidate)
{
    std::lock_guard lock(guard);
    if (!candidate || queue || candidate->GetDesc().Type != D3D12_COMMAND_LIST_TYPE_DIRECT) return;
    // Clone this actual queue's existing table. Its original Execute entry includes any existing
    // wrapper/RTSS chain; no module-wide D3D12 or overlay function is patched.
    auto** table = *reinterpret_cast<void***>(candidate);
    if (!read(table, queue_table.data(), sizeof(queue_table))) return;
    native_execute = reinterpret_cast<Execute>(queue_table[10]);
    queue_table[10] = reinterpret_cast<void*>(&execute);
    DWORD protection{};
    if (!VirtualProtect(candidate, sizeof(void*), PAGE_READWRITE, &protection)) return;
    const auto replaced = InterlockedCompareExchangePointer(reinterpret_cast<void* volatile*>(candidate),
        queue_table.data(), table);
    DWORD ignored{};
    VirtualProtect(candidate, sizeof(void*), protection, &ignored);
    if (replaced != table) return;
    queue_original_table = table;
    queue = candidate;
    log("RDR2 native DIRECT queue submission observer attached; existing Execute chain preserved.");
}
void* pop(void* ring, void* record)
{
    Callback callback;
    const bool game_submit = _ReturnAddress() == image + 0x26df79b;
    void* result = native_pop(ring, record);
    if (game_submit && active.load()) {
        ID3D12CommandQueue* candidate{};
        if (read(record, &candidate, sizeof(candidate))) attach_queue(candidate);
        attach_present();
    }
    return result;
}
bool worker(uintptr_t& view, uint64_t& frame)
{
    uint32_t index{};
    uintptr_t block{};
    if (!read(image + 0x5b403e0, &index, sizeof(index)) || index >= 4096 ||
        !read_at(__readgsqword(0x58) + index * sizeof(void*), block) ||
        !read_at(block + 0x14d8, view)) return false;
    uint32_t flags{}, value{};
    if (!read_at(block + 0x61c, flags)) return false;
    if (flags & 6) {
        uintptr_t pointer{};
        if (!read(image + 0x5429058, &pointer, sizeof(pointer)) || !read_at(pointer, value)) return false;
    } else if (!read(image + 0x542904c, &value, sizeof(value))) return false;
    frame = value;
    return view && frame;
}
bool feature_flags(void* parameters, uint32_t& flags)
{
    // The installed NGX parameter interface, also used by the bounded probe. GetI is slot 11.
    uintptr_t table{}, entry{};
    if (!read(parameters, &table, sizeof(table)) || !read_at(table + 11 * sizeof(void*), entry)) return false;
    using Get = uint32_t (*)(void*, const char*, int32_t*);
    int32_t value{};
    const auto result = reinterpret_cast<Get>(entry)(parameters, "DLSS.Feature.Create.Flags", &value);
    if (result != 1) return false;
    flags = static_cast<uint32_t>(value);
    return true;
}
bool refusal(uint32_t reason, const char* message)
{
    if (host.render_feedback) {
        const rsf_game_render_feedback feedback{sizeof(feedback), reason, host.session_id, 0};
        host.render_feedback(host.user, &feedback);
    }
    static std::atomic<uint32_t> previous{UINT32_MAX};
    if (previous.exchange(reason) != reason) log(message);
    return false;
}
bool process(ID3D12GraphicsCommandList* list, void* feature, uint32_t flags, const Evaluation& d, History& history,
    const D3D12_RESOURCE_STATES* engine_states = nullptr)
{
    if (!active.load() || !host.process_upscale || !host.submit || !list || !feature ||
        !d.color || !d.output || !d.depth || !d.motion || !d.width || !d.height ||
        d.invert_x || d.invert_y || (d.pre_exposure != 0 && d.pre_exposure != 1))
        return refusal(1, "RDR2 SR native fallback: unsupported evaluation descriptor or inactive host.");
    for (auto origin : d.origins) if (origin)
        return refusal(2, "RDR2 SR native fallback: nonzero input/output subrect origin.");
    {
        std::lock_guard lock(guard);
        if (!queue || !swapchain) return refusal(6, "RDR2 SR native fallback: game queue/Present observers not attached yet.");
    }
    uintptr_t view{};
    uint64_t frame{};
    std::array<unsigned char, 0x68c> camera{};
    float dt{};
    if (!worker(view, frame) || !read(reinterpret_cast<void*>(view), camera.data(), camera.size()) ||
        !read(image + 0x59b393c, &dt, sizeof(dt)) || !std::isfinite(dt) || dt <= 0 || dt > 1 ||
        !(flags & 8u))
        return refusal(3, "RDR2 SR native fallback: worker camera, frame time or NGX creation flags unavailable.");
    const auto output = d.output->GetDesc();
    if (output.Width > 16384 || output.Height > 16384 || output.Format != DXGI_FORMAT_R16G16B16A16_FLOAT)
        return refusal(4, "RDR2 SR native fallback: output format/extent unsupported.");
    rsf_rdr2_camera_inputs input{};
    input.render_width = d.width; input.render_height = d.height;
    input.output_width = static_cast<uint32_t>(output.Width); input.output_height = output.Height;
    input.frame_time_seconds = dt; input.reset = d.reset || history.frame + 1 != frame;
    input.motion_jittered = (flags & 4u) != 0;
    std::memcpy(input.jitter, d.jitter, sizeof(input.jitter));
    std::memcpy(input.previous_jitter, history.jitter, sizeof(input.previous_jitter));
    rsf_rdr2_camera_result decoded{};
    if (!rsf_rdr2_decode_render_camera(camera.data(), camera.size(), input, decoded) ||
        !decoded.projection_jitter_matches)
        return refusal(5, "RDR2 SR native fallback: render projection is invalid or its jitter is frozen.");
    rsf_game_render_pass pass{};
    pass.struct_size = sizeof(pass); pass.role = RSF_GAME_RENDER_SR;
    pass.session_id = host.session_id; pass.family_key = pass.view_key = pass.history_key = reinterpret_cast<uint64_t>(feature);
    pass.pass_key = frame; pass.native_frame = frame; pass.scope_id = view;
    pass.resource_generation = 1; pass.flags = RSF_GAME_RENDER_PRIMARY | (input.reset ? RSF_GAME_RENDER_RESET : 0u);
    pass.render_rect[2] = static_cast<int32_t>(d.width); pass.render_rect[3] = static_cast<int32_t>(d.height);
    pass.output_rect[2] = static_cast<int32_t>(output.Width); pass.output_rect[3] = static_cast<int32_t>(output.Height);
    pass.color_input = d.color; pass.color_output = d.output; pass.depth = d.depth; pass.motion = d.motion; pass.exposure = d.exposure;
    pass.camera = decoded.camera; pass.camera_valid = 1;
    pass.source_frame_id = rsf_rdr2_scene_source(frame);
    pass.screen = pass.source_frame_id ? RSF_SCREEN_FLIGHT : RSF_SCREEN_UNKNOWN;
    if (!pass.source_frame_id) pass.flags |= RSF_GAME_RENDER_NO_FG;
    std::memcpy(pass.previous_clip_to_clip, decoded.previous_clip_to_clip, sizeof(pass.previous_clip_to_clip));
    pass.motion_to_uv[0] = -d.motion_scale[0] / d.width;
    pass.motion_to_uv[1] = -d.motion_scale[1] / d.height;
    pass.motion_camera_included = 1;
    // Source identity comes from observed native input/simulation/render markers. Actual
    // Present still requires the later scene/resource match on the submission thread.
    ID3D12Resource* resources[] = {d.color, d.depth, d.motion, d.output, d.exposure};
    const D3D12_RESOURCE_STATES before[] = {
        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        D3D12_RESOURCE_STATE_DEPTH_READ,
        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_UNORDERED_ACCESS};
    D3D12_RESOURCE_BARRIER barriers[5]{};
    for (size_t i = 0; i < 5; ++i) {
        barriers[i].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        barriers[i].Transition = {resources[i], D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, engine_states ? engine_states[i] : before[i],
            i == 3 ? D3D12_RESOURCE_STATE_COPY_DEST : D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE};
    }
    for (auto& barrier : barriers) if (barrier.Transition.pResource && barrier.Transition.StateBefore != barrier.Transition.StateAfter)
        list->ResourceBarrier(1, &barrier);
    const auto accepted = host.process_upscale(host.user, list, &pass) == 0;
    for (auto& barrier : barriers) std::swap(barrier.Transition.StateBefore, barrier.Transition.StateAfter);
    for (auto& barrier : barriers) if (barrier.Transition.pResource && barrier.Transition.StateBefore != barrier.Transition.StateAfter)
        list->ResourceBarrier(1, &barrier);
    history.frame = frame;
    std::memcpy(history.jitter, d.jitter, sizeof(history.jitter));
    if (host.render_feedback) {
        const rsf_game_render_feedback feedback{sizeof(feedback), accepted ? RSF_GAME_RENDER_FEEDBACK_ACCEPTED :
            RSF_GAME_RENDER_FEEDBACK_HOST_REFUSED, host.session_id, frame};
        host.render_feedback(host.user, &feedback);
    }
    if (accepted) rsf_rdr2_scene_upscale(list, pass);
    return accepted;
}
uint32_t evaluate(ID3D12GraphicsCommandList* list, void* feature, void* parameters, const Evaluation* pointer)
{
    Callback callback;
    Evaluation descriptor{};
    if (!read(pointer, &descriptor, sizeof(descriptor))) return native_evaluate(list, feature, parameters, pointer);
    // Upscale workers change each frame. Serialize a feature's history and shared host callback;
    // this lock never surrounds queue submission or a GPU wait on unsubmitted work.
    std::lock_guard lock(evaluations);
    auto& history = histories[feature];
    uint32_t flags{};
    if (feature_flags(parameters, flags) && process(list, feature, flags, descriptor, history)) {
        history.native_gap = true;
        return 1; // NVSDK_NGX_Result_Success; outer wrapper still restores RAGE's heap cache.
    }
    if (history.native_gap) descriptor.reset = 1;
    const auto result = native_evaluate(list, feature, parameters, &descriptor);
    if (history.native_gap || history.native_result != result) {
        char message[144]{};
        std::snprintf(message, sizeof(message), "RDR2 native DLSS fallback result=0x%08x feature=%p reset=%u.",
            result, feature, descriptor.reset);
        log(message);
        history.native_result = result;
    }
    if ((result & 0xfff00000u) != 0xbad00000u) history.native_gap = false;
    history.frame = 0; // Reset RSF history after every native fallback, too.
    return result;
}
template<class T> T field(const std::array<unsigned char, 1352>& description, size_t offset)
{
    T value{}; std::memcpy(&value, description.data() + offset, sizeof(value)); return value;
}
int32_t fsr_dispatch(void* context, const void* pointer)
{
    Callback callback;
    // This private FSR 2.1.2 DLL serves only the game, but its wrapper call (+26f050a) need not be
    // the immediate caller: another hook on the import slot forwards from its own module. Route
    // every call and record each distinct caller module once.
    const auto address = _ReturnAddress();
    uintptr_t first{};
    const auto caller = reinterpret_cast<uintptr_t>(address);
    if (fsr_caller.compare_exchange_strong(first, caller) || (first != caller && fsr_caller.exchange(UINTPTR_MAX) != UINTPTR_MAX)) {
        HMODULE owner{};
        wchar_t path[MAX_PATH]{};
        GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            static_cast<LPCWSTR>(address), &owner);
        if (owner) GetModuleFileNameW(owner, path, MAX_PATH);
        const wchar_t* name = std::wcsrchr(path, L'\\') ? std::wcsrchr(path, L'\\') + 1 : path;
        char message[256]{};
        std::snprintf(message, sizeof(message), "RDR2 native FSR2 dispatch observed from %ls+%llx%s.",
            *name ? name : L"<unknown>", static_cast<unsigned long long>(caller - reinterpret_cast<uintptr_t>(owner)),
            owner == reinterpret_cast<HMODULE>(const_cast<unsigned char*>(image)) && caller - reinterpret_cast<uintptr_t>(image) == 0x26f050a ?
                " (measured wrapper)" : "");
        log(message);
    }
    // FSR 2.1.2 prefix of the game's FSR 2.2 description, checked against the pinned 2.1.2 headers by vendor_census_layouts.
    // Normalize to the very same direct-resource DLSS descriptor, then use the common adapter.
    std::array<unsigned char, 1352> bytes{};
    if (!read(pointer, bytes.data(), bytes.size())) return native_fsr(context, pointer);
    Evaluation descriptor{};
    descriptor.color = field<ID3D12Resource*>(bytes, 8);
    descriptor.depth = field<ID3D12Resource*>(bytes, 192);
    descriptor.motion = field<ID3D12Resource*>(bytes, 376);
    descriptor.exposure = field<ID3D12Resource*>(bytes, 560);
    descriptor.output = field<ID3D12Resource*>(bytes, 1112);
    descriptor.jitter[0] = field<float>(bytes, 1296); descriptor.jitter[1] = field<float>(bytes, 1300);
    descriptor.motion_scale[0] = field<float>(bytes, 1304); descriptor.motion_scale[1] = field<float>(bytes, 1308);
    descriptor.width = field<uint32_t>(bytes, 1312); descriptor.height = field<uint32_t>(bytes, 1316);
    descriptor.pre_exposure = field<float>(bytes, 1332); descriptor.reset = bytes[1336] ? 1u : 0u;
    D3D12_RESOURCE_STATES states[5]{};
    const size_t offsets[] = {8, 192, 376, 1112, 560};
    bool valid = true;
    for (size_t i = 0; i < 5; ++i) {
        if (!field<ID3D12Resource*>(bytes, offsets[i])) continue;
        const auto state = field<uint32_t>(bytes, offsets[i] + 164);
        if (state == 1) states[i] = D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
        else if (state == 2) states[i] = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
        else valid = false;
    }
    std::lock_guard lock(evaluations);
    auto& history = histories[context];
    if (valid && process(field<ID3D12GraphicsCommandList*>(bytes, 0), context, 8u, descriptor, history, states)) {
        history.native_gap = true;
        return 0; // FFX_OK; the outer game wrapper performs its normal cache restoration.
    }
    // The game's DLL is FSR 2.2 (r_input_opaque_only/rw_output_autoreactive, ffxFsr2ResourceIsNull):
    // its description extends past the 1,352-byte 2.1.2 prefix read above with enableAutoReactive
    // and a colorOpaqueOnly resource. Native FSR2 must receive the game's own description; a
    // truncated copy let it register stack garbage and crash in GetDesc after a preset change.
    // Only the shared-prefix reset flag is changed, and only after RSF skipped native frames.
    auto* original = static_cast<unsigned char*>(const_cast<void*>(pointer));
    if (history.native_gap) original[1336] = 1;
    const auto result = native_fsr(context, pointer);
    if (history.native_gap || history.native_result != static_cast<uint32_t>(result)) {
        char message[128]{};
        std::snprintf(message, sizeof(message), "RDR2 native FSR fallback result=%d context=%p reset=%u.",
            result, context, original[1336] ? 1u : 0u);
        log(message);
        history.native_result = static_cast<uint32_t>(result);
    }
    if (!result) history.native_gap = false;
    history.frame = 0;
    return result;
}
int32_t fsr_create(void* context, const void* description)
{
    Callback callback;
    const auto result = native_fsr_create(context, description);
    // FfxFsr2ContextDescription 2.1.2: flags, maxRenderSize, displaySize. Evidence only.
    uint32_t fields[5]{};
    static std::atomic<uint32_t> reports{};
    if (reports++ < 4 && read(description, fields, sizeof(fields))) {
        char message[192]{};
        std::snprintf(message, sizeof(message), "RDR2 native FSR2 context create result=%d flags=0x%x max_render=%ux%u display=%ux%u.",
            result, fields[0], fields[1], fields[2], fields[3], fields[4]);
        log(message);
    }
    return result;
}
unsigned char* find_fsr_export(const char*& refusal)
{
    // The game's signed FSR 2.1.2 API, statically imported and resolved before the ASI loads.
    auto* module = reinterpret_cast<unsigned char*>(GetModuleHandleW(L"ffx_fsr2_api_x64.dll"));
    IMAGE_DOS_HEADER dos{};
    IMAGE_NT_HEADERS64 nt{};
    refusal = "ffx_fsr2_api_x64.dll is not loaded";
    if (!module) return nullptr;
    refusal = "ffx_fsr2_api_x64.dll is not the 1491.50 build (timestamp 64907ede, image 0x10000)";
    if (!read(module, &dos, sizeof(dos)) || dos.e_magic != IMAGE_DOS_SIGNATURE ||
        !read(module + dos.e_lfanew, &nt, sizeof(nt)) || nt.Signature != IMAGE_NT_SIGNATURE ||
        nt.FileHeader.TimeDateStamp != 0x64907ede || nt.OptionalHeader.SizeOfImage != 0x10000) return nullptr;
    auto* entry = reinterpret_cast<unsigned char*>(GetProcAddress(reinterpret_cast<HMODULE>(module), "ffxFsr2ContextDispatch"));
    // Null checks, then renderSize.width at description+0x520 against the context's maximum.
    constexpr unsigned char expected[] = {0x48,0x85,0xc9,0x74,0x36,0x48,0x85,0xd2,0x74,0x31,0x8b,0x41,0x04,0x39,0x82,0x20};
    refusal = "ffxFsr2ContextDispatch is not at +3c70 with the expected entry bytes";
    if (entry != module + 0x3c70 || std::memcmp(entry, expected, sizeof(expected))) return nullptr;
    auto* create = reinterpret_cast<unsigned char*>(GetProcAddress(reinterpret_cast<HMODULE>(module), "ffxFsr2ContextCreate"));
    constexpr unsigned char create_bytes[] = {0x48,0x89,0x5c,0x24,0x08,0x57,0x48,0x83,0xec,0x20,0x48,0x8b,0xda,0x41,0xb8,0x60};
    if (create == module + 0x3ab0 && !std::memcmp(create, create_bytes, sizeof(create_bytes)) &&
        MH_CreateHook(create, reinterpret_cast<void*>(&fsr_create), reinterpret_cast<void**>(&native_fsr_create)) == MH_OK)
        fsr_create_export = create;
    void* imported{};
    char message[192]{};
    std::snprintf(message, sizeof(message), "RDR2 FSR2 import slot +6032a98 %s ffx_fsr2_api_x64.dll!ffxFsr2ContextDispatch.",
        read(image + 0x6032a98, &imported, sizeof(imported)) && imported == entry ? "resolves to" : "does NOT resolve to");
    log(message);
    return entry;
}
}

rsf_result rsf_rdr2_renderer_prepare(const rsf_game_host_services& services) noexcept
{
    if (prepared) return RSF_ERROR_BUSY;
    if (!services.process_upscale || !services.submit || !services.present) return RSF_ERROR_NOT_READY;
    image = reinterpret_cast<const unsigned char*>(GetModuleHandleW(nullptr));
    IMAGE_DOS_HEADER dos{};
    IMAGE_NT_HEADERS64 nt{};
    if (!read(image, &dos, sizeof(dos)) || dos.e_magic != IMAGE_DOS_SIGNATURE ||
        !read(image + dos.e_lfanew, &nt, sizeof(nt)) || nt.Signature != IMAGE_NT_SIGNATURE ||
        nt.FileHeader.TimeDateStamp != 0x65eec92e || nt.OptionalHeader.SizeOfImage != image_size) return RSF_ERROR_NOT_READY;
    constexpr unsigned char eval_bytes[] = {0x48,0x89,0x5c,0x24,0x08,0x48,0x89,0x6c,0x24,0x10,0x48,0x89,0x74,0x24,0x18,0x57};
    constexpr unsigned char pop_bytes[] = {0x48,0x89,0x5c,0x24,0x08,0x48,0x89,0x74,0x24,0x10,0x57,0x48,0x83,0xec,0x20,0x48};
    if (std::memcmp(image + 0x26f3a3c, eval_bytes, sizeof(eval_bytes)) ||
        std::memcmp(image + 0x26c97d4, pop_bytes, sizeof(pop_bytes))) return RSF_ERROR_NATIVE_REFUSED;
    const auto initialized = MH_Initialize();
    if (initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED) return RSF_ERROR_NATIVE_REFUSED;
    host = services;
    auto result = MH_CreateHook(const_cast<unsigned char*>(image + 0x26f3a3c), reinterpret_cast<void*>(&evaluate),
        reinterpret_cast<void**>(&native_evaluate));
    if (result != MH_OK) return RSF_ERROR_NATIVE_REFUSED;
    result = MH_CreateHook(const_cast<unsigned char*>(image + 0x26c97d4), reinterpret_cast<void*>(&pop),
        reinterpret_cast<void**>(&native_pop));
    if (result != MH_OK) {
        MH_RemoveHook(const_cast<unsigned char*>(image + 0x26f3a3c));
        return RSF_ERROR_NATIVE_REFUSED;
    }
    prepared = true;
    if (!rsf_rdr2_scene_prepare(image, host)) log("RDR2 completed-scene producer unavailable; SR remains available.");
    // The FSR route is independent of the DLSS route. A refusal leaves native FSR2 untouched.
    const char* refusal{};
    if (auto* entry = find_fsr_export(refusal)) {
        if (MH_CreateHook(entry, reinterpret_cast<void*>(&fsr_dispatch), reinterpret_cast<void**>(&native_fsr)) == MH_OK)
            fsr_export = entry;
        else refusal = "MinHook could not create the export detour";
    }
    if (!fsr_export) {
        char message[192]{};
        std::snprintf(message, sizeof(message), "RDR2 FSR2 input route refused: %s. Native FSR2 is retained.", refusal);
        log(message);
    }
    log("RDR2 SR hooks prepared: expected bytes match +26f3a3c and +26c97d4.");
    return RSF_OK;
}
rsf_result rsf_rdr2_renderer_start() noexcept
{
    if (!prepared) return RSF_ERROR_NOT_READY;
    if (MH_EnableHook(const_cast<unsigned char*>(image + 0x26c97d4)) != MH_OK) return RSF_ERROR_NATIVE_REFUSED;
    active.store(true);
    if (MH_EnableHook(const_cast<unsigned char*>(image + 0x26f3a3c)) != MH_OK) {
        active.store(false);
        MH_DisableHook(const_cast<unsigned char*>(image + 0x26c97d4));
        return RSF_ERROR_NATIVE_REFUSED;
    }
    const bool fsr = fsr_export && MH_EnableHook(fsr_export) == MH_OK;
    if (fsr_create_export) MH_EnableHook(fsr_create_export);
    if (fsr_export && !fsr) log("RDR2 FSR2 input route refused: export detour could not be enabled. Native FSR2 is retained.");
    if (!rsf_rdr2_scene_start()) log("RDR2 completed-scene producer did not activate.");
    log(fsr ? "RDR2 DLSS and FSR2 (ffxFsr2ContextDispatch export) selections route to the common SR adapter." :
        "RDR2 DLSS selection routes to the common SR adapter; native FSR2 is not intercepted.");
    return RSF_OK;
}
rsf_result rsf_rdr2_renderer_quiesce() noexcept
{
    active.store(false);
    return callbacks.load() || !rsf_rdr2_scene_quiesce() ? RSF_ERROR_BUSY : RSF_OK;
}
rsf_result rsf_rdr2_renderer_stop() noexcept
{
    if (active.load() || callbacks.load()) return RSF_ERROR_BUSY;
    if (!prepared) return RSF_OK;
    // The queue table retains callbacks into this DLL. A live queue therefore pins the module;
    // process teardown owns it until a later explicit queue-drain/unhook protocol is provided.
    if (queue || swapchain) return RSF_ERROR_BUSY;
    rsf_rdr2_scene_stop();
    if (fsr_export) MH_DisableHook(fsr_export);
    if (fsr_create_export) MH_DisableHook(fsr_create_export);
    MH_DisableHook(const_cast<unsigned char*>(image + 0x26f3a3c));
    MH_DisableHook(const_cast<unsigned char*>(image + 0x26c97d4));
    if (callbacks.load()) return RSF_ERROR_BUSY;
    if (fsr_export) MH_RemoveHook(fsr_export);
    if (fsr_create_export) MH_RemoveHook(fsr_create_export);
    fsr_export = fsr_create_export = nullptr;
    MH_RemoveHook(const_cast<unsigned char*>(image + 0x26f3a3c));
    MH_RemoveHook(const_cast<unsigned char*>(image + 0x26c97d4));
    prepared = false;
    host = {};
    return RSF_OK;
}
void rsf_rdr2_renderer_status(rsf_game_renderer_status& status) noexcept
{
    status.prepared = prepared ? 1u : 0u;
    status.active = active.load() ? 1u : 0u;
    status.rendering_ready = 0;
    status.reason = "Native DLSS SR replacement implemented; FSR2 export routing, live acceptance and FG remain pending.";
}
