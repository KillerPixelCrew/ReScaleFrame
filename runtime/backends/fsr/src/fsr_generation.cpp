// SPDX-License-Identifier: GPL-3.0-only
#include "../../common/fg_helpers.h"
#include <wrl/client.h>
#include <atomic>
#include <new>
#include <cstring>
#if defined(RSF_HAVE_FFX_FG)
#include <ffx_framegeneration.h>
#include <dx12/ffx_api_framegeneration_dx12.h>
using Microsoft::WRL::ComPtr;
namespace {
struct FfxSession {
    HMODULE module = nullptr;
    ffxContext generation = nullptr, swapchain = nullptr;
    PfnFfxCreateContext create{};
    PfnFfxDestroyContext destroy{};
    PfnFfxConfigure configure{};
    PfnFfxDispatch dispatch{};
    PfnFfxQuery query{};
    ComPtr<ID3D12Device> device;
    ComPtr<IDXGISwapChain4> chain;
    ffxCreateContextDescFrameGeneration description{};
    ffxCreateContextDescFrameGenerationVersion api_version{};
    ffxCreateBackendDX12Desc backend{};
    ffxOverrideVersion override_version{};
    rsf_fg_status state{};
    rsf_fg_options options{};
    rsf_ui_mode ui_mode = RSF_UI_MODE_NONE;
    std::atomic<uint64_t> generated{0}, presented{0};
    uint64_t sequence = 0;
    uint64_t last_prepare = 0, observed_generated = 0;
    float view_space_to_meters = 1;
    bool history_valid = false, prepared_enabled = false;
};
bool native_state(uint32_t state, D3D12_RESOURCE_STATES& out)
{
    out = D3D12_RESOURCE_STATE_COMMON;
    const uint32_t known = FFX_API_RESOURCE_STATE_COMMON | FFX_API_RESOURCE_STATE_PRESENT |
        FFX_API_RESOURCE_STATE_UNORDERED_ACCESS | FFX_API_RESOURCE_STATE_PIXEL_COMPUTE_READ |
        FFX_API_RESOURCE_STATE_COPY_SRC | FFX_API_RESOURCE_STATE_COPY_DEST |
        FFX_API_RESOURCE_STATE_INDIRECT_ARGUMENT | FFX_API_RESOURCE_STATE_RENDER_TARGET |
        FFX_API_RESOURCE_STATE_DEPTH_ATTACHMENT;
    if (state & ~known) return false;
    if (state & FFX_API_RESOURCE_STATE_UNORDERED_ACCESS) out |= D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
    if (state & FFX_API_RESOURCE_STATE_COMPUTE_READ) out |= D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
    if (state & FFX_API_RESOURCE_STATE_PIXEL_READ) out |= D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
    if (state & FFX_API_RESOURCE_STATE_COPY_SRC) out |= D3D12_RESOURCE_STATE_COPY_SOURCE;
    if (state & FFX_API_RESOURCE_STATE_COPY_DEST) out |= D3D12_RESOURCE_STATE_COPY_DEST;
    if (state & FFX_API_RESOURCE_STATE_INDIRECT_ARGUMENT) out |= D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT;
    if (state & FFX_API_RESOURCE_STATE_RENDER_TARGET) out |= D3D12_RESOURCE_STATE_RENDER_TARGET;
    if (state & FFX_API_RESOURCE_STATE_DEPTH_ATTACHMENT) out |= D3D12_RESOURCE_STATE_DEPTH_WRITE;
    return true;
}
void transition(ID3D12GraphicsCommandList* list, ID3D12Resource* resource,
                D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after)
{
    if (before == after) return;
    D3D12_RESOURCE_BARRIER barrier{};
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition = {resource, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, before, after};
    list->ResourceBarrier(1, &barrier);
}
ffxReturnCode_t generate(ffxDispatchDescFrameGeneration* desc, void* pointer)
{
    auto* self = static_cast<FfxSession*>(pointer);
    return self->dispatch(&self->generation, &desc->header);
}
ffxReturnCode_t present(ffxCallbackDescFrameGenerationPresent* desc, void* pointer)
{
    auto* self = static_cast<FfxSession*>(pointer);
    auto* list = static_cast<ID3D12GraphicsCommandList*>(desc->commandList);
    auto* source = static_cast<ID3D12Resource*>(desc->currentBackBuffer.resource);
    auto* output = static_cast<ID3D12Resource*>(desc->outputSwapChainBuffer.resource);
    if (!source || !output || !list) return FFX_API_RETURN_ERROR_PARAMETER;
    D3D12_RESOURCE_STATES source_state{}, output_state{};
    if (!native_state(desc->currentBackBuffer.state, source_state) ||
        !native_state(desc->outputSwapChainBuffer.state, output_state)) return FFX_API_RETURN_ERROR_PARAMETER;
    if (source != output) {
        transition(list, source, source_state, D3D12_RESOURCE_STATE_COPY_SOURCE);
        transition(list, output, output_state, D3D12_RESOURCE_STATE_COPY_DEST);
        list->CopyResource(output, source);
        transition(list, output, D3D12_RESOURCE_STATE_COPY_DEST, output_state);
        transition(list, source, D3D12_RESOURCE_STATE_COPY_SOURCE, source_state);
    }
    ++self->presented;
    if (desc->isGeneratedFrame) ++self->generated;
    return FFX_API_RETURN_OK;
}
rsf_backend_result wait(FfxSession& self)
{
    if (!self.swapchain) return RSF_BACKEND_OK;
    ffxDispatchDescFrameGenerationSwapChainWaitForPresentsDX12 desc{};
    desc.header.type = FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_WAIT_FOR_PRESENTS_DX12;
    return self.dispatch(&self.swapchain, &desc.header) == FFX_API_RETURN_OK ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
void destroy(void* pointer)
{
    auto* self = static_cast<FfxSession*>(pointer); if (!self) return;
    if (wait(*self) != 0) return; // Pending SDK callbacks still own this context.
    self->chain.Reset();
    if (self->swapchain) self->destroy(&self->swapchain, nullptr);
    if (self->generation) self->destroy(&self->generation, nullptr);
    if (self->module) FreeLibrary(self->module);
    delete self;
}
rsf_backend_result create(const rsf_generation_setup* setup, void** out, void** chain)
{
    auto result = rsf::fg_setup(setup, out, chain); if (result != 0) return result;
    // Cross-SDK transitions need common-present instrumentation and pacing feature unload.
    // Until that shared-host transition exists, retain the previous owner rather than stack proxies.
    if (GetModuleHandleW(L"sl.interposer.dll")) return RSF_BACKEND_ERROR_NEEDS_RESTART;
    if (setup->feature_major != 3 && setup->feature_major != 4) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    if (setup->chain.ui_mode != RSF_UI_MODE_NONE && setup->chain.ui_mode != RSF_UI_MODE_BACKBUFFER_HUDLESS)
        return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    auto* self = new (std::nothrow) FfxSession; if (!self) return RSF_BACKEND_ERROR_INIT_FAILED;
    self->device = static_cast<ID3D12Device*>(setup->chain.d3d12_device); self->ui_mode = setup->chain.ui_mode;
    self->view_space_to_meters = setup->view_space_to_meters;
    self->module = rsf::fg_library(*setup, L"amd_fidelityfx_framegeneration_dx12.dll");
    if (!self->module) { destroy(self); return RSF_BACKEND_ERROR_LOAD_FAILED; }
    self->create = rsf::entry<PfnFfxCreateContext>(self->module, "ffxCreateContext");
    self->destroy = rsf::entry<PfnFfxDestroyContext>(self->module, "ffxDestroyContext");
    self->configure = rsf::entry<PfnFfxConfigure>(self->module, "ffxConfigure");
    self->dispatch = rsf::entry<PfnFfxDispatch>(self->module, "ffxDispatch");
    self->query = rsf::entry<PfnFfxQuery>(self->module, "ffxQuery");
    if (!self->create || !self->destroy || !self->configure || !self->dispatch || !self->query) {
        destroy(self); return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT;
    }
    uint64_t count = 32, ids[32]{}; const char* names[32]{};
    ffxQueryDescGetVersions versions{}; versions.header.type = FFX_API_QUERY_DESC_TYPE_GET_VERSIONS;
    versions.createDescType = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION;
    versions.device = self->device.Get(); versions.outputCount = &count; versions.versionIds = ids; versions.versionNames = names;
    if (self->query(nullptr, &versions.header) != FFX_API_RETURN_OK || count > 32) { destroy(self); return RSF_BACKEND_ERROR_NOT_SUPPORTED; }
    unsigned best_minor = 0, best_patch = 0;
    for (uint64_t i = 0; i < count; ++i) {
        unsigned major = 0, minor = 0, patch = 0;
        if (!names[i] || std::sscanf(names[i], "%u.%u.%u", &major, &minor, &patch) != 3 || major != setup->feature_major ||
            (setup->version_id && ids[i] != setup->version_id)) continue;
        if (!self->state.version_id || minor > best_minor || (minor == best_minor && patch > best_patch)) {
            self->state.version_id = ids[i]; best_minor = minor; best_patch = patch;
            std::snprintf(self->state.version_name, sizeof(self->state.version_name), "%s", names[i]);
        }
    }
    if (!self->state.version_id) { destroy(self); return RSF_BACKEND_ERROR_NOT_SUPPORTED; }
    self->backend.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_DX12; self->backend.device = self->device.Get();
    self->override_version.header.type = FFX_API_DESC_TYPE_OVERRIDE_VERSION; self->override_version.versionId = self->state.version_id;
    self->backend.header.pNext = &self->override_version.header;
    self->api_version.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION_VERSION;
    self->api_version.version = FFX_FRAMEGENERATION_VERSION; self->api_version.header.pNext = &self->backend.header;
    auto& desc = self->description;
    desc.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION; desc.header.pNext = &self->api_version.header;
    desc.displaySize = {setup->chain.width, setup->chain.height}; desc.maxRenderSize = desc.displaySize;
    desc.backBufferFormat = ffxApiGetSurfaceFormatDX12(static_cast<DXGI_FORMAT>(setup->chain.format));
    desc.flags = (setup->depth_inverted ? FFX_FRAMEGENERATION_ENABLE_DEPTH_INVERTED : 0u) |
        (setup->depth_infinite ? FFX_FRAMEGENERATION_ENABLE_DEPTH_INFINITE : 0u) |
        (setup->motion_jittered ? FFX_FRAMEGENERATION_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION : 0u) |
        (setup->motion_at_display_resolution ? FFX_FRAMEGENERATION_ENABLE_DISPLAY_RESOLUTION_MOTION_VECTORS : 0u) |
        (setup->hdr ? FFX_FRAMEGENERATION_ENABLE_HIGH_DYNAMIC_RANGE : 0u);
    if (self->create(&self->generation, &desc.header, nullptr) != FFX_API_RETURN_OK) { destroy(self); return RSF_BACKEND_ERROR_INIT_FAILED; }
    ffxQueryGetProviderVersion actual{}; actual.header.type = FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION;
    if (self->query(&self->generation, &actual.header) != FFX_API_RETURN_OK || actual.versionId != self->state.version_id) {
        destroy(self); return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    }
    self->state.struct_size = sizeof(self->state); self->state.supported = 1; self->state.max_generated_frames = 1;
    self->state.vsync_supported = 1;
    if (setup->chain.hwnd) {
        ComPtr<IDXGIFactory2> factory;
        if (FAILED(CreateDXGIFactory2(0, IID_PPV_ARGS(&factory)))) { destroy(self); return RSF_BACKEND_ERROR_INIT_FAILED; }
        auto chain_desc = rsf::chain_description(setup->chain);
        IDXGISwapChain4* raw = nullptr;
        ffxCreateContextDescFrameGenerationSwapChainVersionDX12 version{};
        version.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_VERSION_DX12;
        version.version = FFX_FRAMEGENERATION_SWAPCHAIN_DX12_VERSION;
        ffxCreateContextDescFrameGenerationSwapChainForHwndDX12 swap{};
        swap.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_FOR_HWND_DX12;
        swap.header.pNext = &version.header; swap.swapchain = &raw; swap.hwnd = static_cast<HWND>(setup->chain.hwnd);
        swap.desc = &chain_desc; swap.dxgiFactory = factory.Get(); swap.gameQueue = static_cast<ID3D12CommandQueue*>(setup->chain.d3d12_queue);
        if (self->create(&self->swapchain, &swap.header, nullptr) != FFX_API_RETURN_OK || !raw) {
            destroy(self); return RSF_BACKEND_ERROR_INIT_FAILED;
        }
        self->chain.Attach(raw);
    }
    *out = self; *chain = self->chain.Get(); return RSF_BACKEND_OK;
}
rsf_backend_result configure(void* pointer, const rsf_fg_options* options)
{
    if (!pointer) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    const auto validation = rsf::fg_options_result(options); if (validation != 0) return validation;
    if (options->mode > RSF_FG_FIXED) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    if (options->reflex_mode != RSF_REFLEX_OFF || options->frame_limit_us)
        return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    auto& self = *static_cast<FfxSession*>(pointer);
    if (!self.chain) return RSF_BACKEND_ERROR_NOT_READY;
    if (options->mode != RSF_FG_OFF && !options->generated_frames) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    ffxConfigureDescFrameGeneration config{};
    config.header.type = FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION;
    config.swapChain = self.chain.Get(); config.frameGenerationEnabled = false;
    config.frameGenerationCallback = generate; config.frameGenerationCallbackUserContext = &self;
    config.frameID = ++self.sequence;
    if (self.configure(&self.generation, &config.header) != FFX_API_RETURN_OK)
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    self.options = *options; self.state.configured_mode = options->mode; self.state.effective_mode = RSF_FG_OFF;
    self.state.effective_generated_frames = 0;
    self.history_valid = false; self.state.active = 0;
    return RSF_BACKEND_OK;
}
rsf_backend_result begin(void* pointer, uint64_t id)
{
    return pointer && id ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INVALID_ARGUMENT;
}
rsf_backend_result marker(void* pointer, rsf_latency_marker, uint64_t id, uint32_t)
{
    return pointer && id ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INVALID_ARGUMENT;
}
rsf_backend_result prepare(void* pointer, void* command, const rsf_fg_frame* frame)
{
    if (!pointer) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto& self = *static_cast<FfxSession*>(pointer);
    auto result = rsf::fg_frame(frame, self.device.Get()); if (result != 0) return result;
    if (!self.chain) return RSF_BACKEND_ERROR_NOT_READY;
    const auto& record = *frame->record;
    if (record.frame_id <= self.last_prepare) return RSF_BACKEND_ERROR_NOT_READY;
    const bool enabled = frame->interpolate && self.options.mode != RSF_FG_OFF;
    if (enabled) {
        result = rsf::fg_command(command, self.device.Get()); if (result != 0) return result;
        if (record.output_width != self.description.displaySize.width || record.output_height != self.description.displaySize.height ||
            record.render_width > self.description.maxRenderSize.width || record.render_height > self.description.maxRenderSize.height)
            return RSF_BACKEND_ERROR_STALE_RESOURCES;
        if (bool(record.camera.depth_inverted) != bool(self.description.flags & FFX_FRAMEGENERATION_ENABLE_DEPTH_INVERTED) ||
            bool(record.camera.motion_jittered) != bool(self.description.flags & FFX_FRAMEGENERATION_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION))
            return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
        const rsf_backend_resource* inputs[] = {&frame->depth, &frame->motion, &frame->hudless};
        for (const auto* input : inputs) {
            if (!input->resource) continue;
            const auto desc = static_cast<ID3D12Resource*>(input->resource)->GetDesc();
            if (input->x || input->y || input->width != desc.Width || input->height != desc.Height)
                return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
        }
    }
    if (enabled && (frame->depth.state != D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE ||
        frame->motion.state != D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE ||
        (frame->hudless.resource && frame->hudless.state != D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE)))
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    ffxConfigureDescFrameGeneration config{};
    config.header.type = FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION;
    config.swapChain = self.chain.Get(); config.frameGenerationEnabled = enabled;
    config.frameGenerationCallback = generate; config.frameGenerationCallbackUserContext = &self;
    // The SDK's default callback performs HUDless UI extraction and composition.
    // A copy callback is sufficient only when the caller explicitly declares no UI.
    if (self.ui_mode == RSF_UI_MODE_NONE) {
        config.presentCallback = present; config.presentCallbackUserContext = &self;
    }
    config.generationRect = {0, 0, static_cast<int32_t>(record.output_width), static_cast<int32_t>(record.output_height)};
    config.frameID = ++self.sequence;
    if (enabled && self.ui_mode == RSF_UI_MODE_BACKBUFFER_HUDLESS) {
        if (!frame->hudless.resource) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
        config.HUDLessColor = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(frame->hudless.resource));
    }
    if (self.configure(&self.generation, &config.header) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    self.state.effective_mode = enabled ? self.options.mode : RSF_FG_OFF;
    self.state.effective_generated_frames = enabled ? 1 : 0;
    self.state.active = 0;
    self.prepared_enabled = enabled;
    if (!enabled) { self.history_valid = false; self.last_prepare = record.frame_id; return RSF_BACKEND_OK; }
    ffxDispatchDescFrameGenerationPrepareV2 data{};
    data.header.type = FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE_V2;
    data.frameID = self.sequence; data.commandList = command;
    data.renderSize = {record.render_width, record.render_height};
    data.jitterOffset = {record.camera.jitter_pixels[0], record.camera.jitter_pixels[1]};
    data.motionVectorScale = {frame->motion_scale_x, frame->motion_scale_y};
    data.frameTimeDelta = record.frame_time_ms; data.cameraNear = record.camera.near_plane;
    data.cameraFar = record.camera.far_plane; data.cameraFovAngleVertical = record.camera.vertical_fov_radians;
    data.viewSpaceToMetersFactor = self.view_space_to_meters;
    data.reset = !self.history_valid || record.frame_id != self.last_prepare + 1;
    data.depth = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(frame->depth.resource));
    data.motionVectors = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(frame->motion.resource));
    for (uint32_t i = 0; i < 3; ++i) {
        data.cameraPosition[i] = record.camera.view_to_world[12 + i];
        data.cameraRight[i] = record.camera.view_to_world[i];
        data.cameraUp[i] = record.camera.view_to_world[4 + i];
        data.cameraForward[i] = record.camera.view_to_world[8 + i];
    }
    if (self.dispatch(&self.generation, &data.header) != FFX_API_RETURN_OK) {
        self.history_valid = false; self.prepared_enabled = false; return RSF_BACKEND_ERROR_FEATURE_FAILED;
    }
    self.history_valid = true; self.last_prepare = record.frame_id; return RSF_BACKEND_OK;
}
rsf_backend_result after(void* pointer)
{
    if (!pointer) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto& self = *static_cast<FfxSession*>(pointer);
    self.state.generated_callbacks = self.generated.load(); self.state.present_callbacks = self.presented.load();
    self.state.valid_statistics = self.ui_mode == RSF_UI_MODE_NONE ?
        RSF_FG_STAT_PRESENT_CALLBACKS | RSF_FG_STAT_ACTIVITY : 0;
    self.state.active = self.prepared_enabled && self.state.generated_callbacks > self.observed_generated;
    self.observed_generated = self.state.generated_callbacks;
    return RSF_BACKEND_OK;
}
rsf_backend_result status(void* pointer, rsf_fg_status* out)
{
    if (!pointer || !out || out->struct_size < sizeof(*out)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = static_cast<FfxSession*>(pointer)->state; return RSF_BACKEND_OK;
}
rsf_backend_result retirement(void* pointer, rsf_fg_retirement* out)
{
    if (!pointer || !out || out->struct_size < sizeof(*out)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    const auto result = wait(*static_cast<FfxSession*>(pointer));
    out->fence = nullptr; out->value = 0; return result;
}
rsf_backend_result abort_frame(void* pointer, uint64_t id)
{
    if (!pointer || !id) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto& self = *static_cast<FfxSession*>(pointer);
    self.history_valid = false; self.prepared_enabled = false; self.state.active = 0;
    return RSF_BACKEND_OK;
}
const rsf_generation_provider provider{sizeof(provider), create, configure, begin, marker, prepare, after, status, retirement, destroy, abort_frame};
}
extern "C" const rsf_generation_provider* rsf_generation_fsr() { return &provider; }
#else
namespace {
rsf_backend_result create(const rsf_generation_setup* setup, void** out, void** chain)
{
    const auto result = rsf::fg_setup_header(setup, out, chain);
    return result != 0 ? result : RSF_BACKEND_ERROR_NOT_COMPILED;
}
rsf_backend_result configure(void*, const rsf_fg_options*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
rsf_backend_result begin(void*, uint64_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
rsf_backend_result marker(void*, rsf_latency_marker, uint64_t, uint32_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
rsf_backend_result prepare(void*, void*, const rsf_fg_frame*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
rsf_backend_result after(void*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
rsf_backend_result status(void*, rsf_fg_status*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
rsf_backend_result retirement(void*, rsf_fg_retirement*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
void destroy(void*) {}
rsf_backend_result abort_frame(void*, uint64_t) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
const rsf_generation_provider provider{sizeof(provider), create, configure, begin, marker, prepare, after, status, retirement, destroy, abort_frame};
}
extern "C" const rsf_generation_provider* rsf_generation_fsr() { return &provider; }
#endif
