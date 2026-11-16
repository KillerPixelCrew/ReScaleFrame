// SPDX-License-Identifier: MIT
#include "../../common/fg_helpers.h"
#include "../../common/generation_state.h"
#include "../../common/d3d12_helpers.h"
#include "../../common/frame_capture12.h"
#include "ffx_version.h"
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
    ComPtr<ID3D12CommandQueue> queue;
    rsf::FrameCapture12 capture;
    struct CaptureIdentity { uint64_t sequence = 0; rsf_frame_record record{}; };
    std::array<CaptureIdentity, 64> capture_identities;
    std::mutex capture_guard;
    ffxCreateContextDescFrameGeneration description{};
    ffxCreateContextDescFrameGenerationVersion api_version{};
    ffxCreateBackendDX12Desc backend{};
    ffxOverrideVersion override_version{};
    rsf::GenerationState copied;
    rsf_ui_mode ui_mode = RSF_UI_MODE_NONE;
    std::atomic<uint64_t> generated{0}, presented{0};
    std::atomic<uint64_t> dispatched{0};
    std::atomic<int32_t> dispatch_result{0};
    std::atomic<uint32_t> dispatch_logs{0};
    rsf_backend_log_fn log = nullptr;
    void* log_user = nullptr;
    uint64_t sequence = 0;
    uint64_t observed_generated = 0;
    uint32_t counter_reports = 0;
    UINT last_dxgi_present = 0;
    float view_space_to_meters = 1;
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
ffxReturnCode_t generate(ffxDispatchDescFrameGeneration* desc, void* pointer)
{
    auto* self = static_cast<FfxSession*>(pointer);
    const auto result = self->dispatch(&self->generation, &desc->header);
    if (result == FFX_API_RETURN_OK && self->capture.enabled()) {
        rsf_frame_record record{};
        {
            std::lock_guard lock(self->capture_guard);
            const auto& identity = self->capture_identities[desc->frameID % self->capture_identities.size()];
            if (identity.sequence == desc->frameID) record = identity.record;
        }
        if (self->capture.wants(record.frame_id)) {
            auto* list = static_cast<ID3D12GraphicsCommandList*>(desc->commandList);
            D3D12_RESOURCE_STATES state{};
            if (native_state(desc->presentColor.state, state))
                self->capture.record(list, static_cast<ID3D12Resource*>(desc->presentColor.resource), state, record, "fg-generation-input");
            for (uint32_t i = 0; i < desc->numGeneratedFrames && i < 4; ++i)
                if (native_state(desc->outputs[i].state, state))
                    self->capture.record(list, static_cast<ID3D12Resource*>(desc->outputs[i].resource), state, record, "fg-generated-before-ui", 0, i, desc->reset ? 1u : 0u);
        }
    }
    self->dispatch_result = static_cast<int32_t>(result);
    if (result == FFX_API_RETURN_OK) self->dispatched += desc->numGeneratedFrames;
    const auto report = self->dispatch_logs.fetch_add(1);
    if (self->log && (report < 3 || (result != FFX_API_RETURN_OK && report < 12))) {
        char text[176]; std::snprintf(text, sizeof(text), "FSR FG dispatch frame=%llu requested_outputs=%u result=%d reset=%u",
            static_cast<unsigned long long>(desc->frameID), desc->numGeneratedFrames, static_cast<int32_t>(result), desc->reset);
        self->log(self->log_user, text);
    }
    return result;
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
    if (source != output) rsf::copy_transitioned(list, output, output_state, output_state, source, source_state, source_state);
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
    if (self->generation && self->chain) {
        // Detach callbacks and HUD-less resources before releasing either context.
        // Disabling composition drains the SDK's in-flight work.
        ffxConfigureDescFrameGeneration off{};
        off.header.type = FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION;
        off.swapChain = self->chain.Get(); off.frameID = ++self->sequence;
        if (self->configure(&self->generation, &off.header) != FFX_API_RETURN_OK) return;
    }
    if (wait(*self) != 0) return; // Pending SDK callbacks still own this context.
    if (self->generation && self->destroy(&self->generation, nullptr) != FFX_API_RETURN_OK) return;
    self->chain.Reset();
    if (self->swapchain && self->destroy(&self->swapchain, nullptr) != FFX_API_RETURN_OK) return;
    if (self->module) FreeLibrary(self->module);
    delete self;
}
rsf_backend_result create(const rsf_generation_setup* setup, void** out, void** chain)
{
    auto result = rsf::fg_setup(setup, out, chain); if (result != 0) return result;
    // A Streamline SR-only registration can coexist on the native engine device. An
    // actual DLSS-G presentation owner still forbids a second vendor proxy.
    if (GetModuleHandleW(L"sl.dlss_g.dll") && !setup->streamline_host) return RSF_BACKEND_ERROR_NEEDS_RESTART;
    if (setup->feature_major != 3 && setup->feature_major != 4) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    if (setup->chain.ui_mode != RSF_UI_MODE_NONE && setup->chain.ui_mode != RSF_UI_MODE_BACKBUFFER_HUDLESS)
        return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    auto* self = new (std::nothrow) FfxSession; if (!self) return RSF_BACKEND_ERROR_INIT_FAILED;
    rsf_fg_status initial{};
    self->device = static_cast<ID3D12Device*>(setup->chain.d3d12_device); self->ui_mode = setup->chain.ui_mode;
    self->queue = static_cast<ID3D12CommandQueue*>(setup->chain.d3d12_queue);
    self->log = setup->chain.log; self->log_user = setup->chain.log_user;
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
    rsf::select_ffx_version(ids, names, count, setup->feature_major, setup->version_id, initial.version_id,
        initial.version_name, sizeof(initial.version_name));
    if (!initial.version_id) { destroy(self); return RSF_BACKEND_ERROR_NOT_SUPPORTED; }
    self->backend.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_DX12; self->backend.device = self->device.Get();
    self->override_version.header.type = FFX_API_DESC_TYPE_OVERRIDE_VERSION; self->override_version.versionId = initial.version_id;
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
    const auto created_generation = self->create(&self->generation, &desc.header, nullptr);
    if (self->log) {
        char text[1024]{};
        std::snprintf(text, sizeof(text),
            "FSR FG create generation: FFX result=%u requested_major=%u selected=%s id=%llu API=%u.%u.%u root=%s",
            created_generation, setup->feature_major, initial.version_name,
            static_cast<unsigned long long>(initial.version_id), uint32_t(FFX_FRAMEGENERATION_VERSION_MAJOR),
            uint32_t(FFX_FRAMEGENERATION_VERSION_MINOR), uint32_t(FFX_FRAMEGENERATION_VERSION_PATCH), setup->runtime_directory_utf8);
        self->log(self->log_user, text);
    }
    if (created_generation != FFX_API_RETURN_OK) { destroy(self); return RSF_BACKEND_ERROR_INIT_FAILED; }
    ffxQueryGetProviderVersion actual{}; actual.header.type = FFX_API_QUERY_DESC_TYPE_GET_PROVIDER_VERSION;
    if (self->query(&self->generation, &actual.header) != FFX_API_RETURN_OK || actual.versionId != initial.version_id) {
        destroy(self); return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    }
    initial.struct_size = sizeof(initial); initial.supported = 1; initial.max_generated_frames = 1;
    initial.vsync_supported = 1;
    if (setup->chain.hwnd) {
        ComPtr<IDXGIFactory2> factory;
        const auto created_factory = CreateDXGIFactory2(0, IID_PPV_ARGS(&factory));
        if (FAILED(created_factory)) {
            if (self->log) {
                char text[128]{}; std::snprintf(text, sizeof(text), "FSR FG create DXGI factory failed HRESULT=0x%08lx",
                    static_cast<unsigned long>(created_factory)); self->log(self->log_user, text);
            }
            destroy(self); return RSF_BACKEND_ERROR_INIT_FAILED;
        }
        auto chain_desc = rsf::chain_description(setup->chain);
        IDXGISwapChain4* raw = nullptr;
        ffxCreateContextDescFrameGenerationSwapChainVersionDX12 version{};
        version.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_VERSION_DX12;
        version.version = FFX_FRAMEGENERATION_SWAPCHAIN_DX12_VERSION;
        ffxCreateContextDescFrameGenerationSwapChainForHwndDX12 swap{};
        swap.header.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATIONSWAPCHAIN_FOR_HWND_DX12;
        swap.header.pNext = &version.header; swap.swapchain = &raw; swap.hwnd = static_cast<HWND>(setup->chain.hwnd);
        swap.desc = &chain_desc; swap.dxgiFactory = factory.Get(); swap.gameQueue = static_cast<ID3D12CommandQueue*>(setup->chain.d3d12_queue);
        const auto created_swapchain = self->create(&self->swapchain, &swap.header, nullptr);
        if (self->log) {
            char text[256]{};
            std::snprintf(text, sizeof(text),
                "FSR FG create swapchain: FFX result=%u chain=%u API=%u.%u.%u size=%ux%u buffers=%u format=%u",
                created_swapchain, uint32_t(raw != nullptr), uint32_t(FFX_FRAMEGENERATION_SWAPCHAIN_DX12_VERSION_MAJOR),
                uint32_t(FFX_FRAMEGENERATION_SWAPCHAIN_DX12_VERSION_MINOR), uint32_t(FFX_FRAMEGENERATION_SWAPCHAIN_DX12_VERSION_PATCH),
                chain_desc.Width, chain_desc.Height, chain_desc.BufferCount, uint32_t(chain_desc.Format));
            self->log(self->log_user, text);
        }
        if (created_swapchain != FFX_API_RETURN_OK || !raw) { destroy(self); return RSF_BACKEND_ERROR_INIT_FAILED; }
        self->chain.Attach(raw);
    }
    self->copied.initialize(initial);
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
    const auto before=self.copied.read();
    if (!self.chain) return RSF_BACKEND_ERROR_NOT_READY;
    if (options->mode != RSF_FG_OFF && !options->generated_frames) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    // Called at every application Present. Repeating the request must not disable generation
    // or reset temporal history between two consecutive source frames.
    if (before.options.struct_size && before.options.mode == options->mode &&
        before.options.generated_frames == options->generated_frames) return RSF_BACKEND_OK;
    ffxConfigureDescFrameGeneration config{};
    config.header.type = FFX_API_CONFIGURE_DESC_TYPE_FRAMEGENERATION;
    config.swapChain = self.chain.Get(); config.frameGenerationEnabled = false;
    config.frameGenerationCallback = generate; config.frameGenerationCallbackUserContext = &self;
    config.frameID = ++self.sequence;
    if (self.configure(&self.generation, &config.header) != FFX_API_RETURN_OK)
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    self.copied.configured(*options,RSF_FG_OFF,0,RSF_REFLEX_OFF,true);
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
    auto before=self.copied.read();
    const auto& record = *frame->record;
    if (record.frame_id <= before.last_prepare) return RSF_BACKEND_ERROR_NOT_READY;
    const bool enabled = frame->interpolate && before.options.mode != RSF_FG_OFF;
    if (enabled) {
        result = rsf::fg_command(command, self.device.Get()); if (result != 0) return result;
        DXGI_SWAP_CHAIN_DESC1 current{};
        if (FAILED(self.chain->GetDesc1(&current))) return RSF_BACKEND_ERROR_FEATURE_FAILED;
        if (current.Width != self.description.displaySize.width || current.Height != self.description.displaySize.height ||
            ffxApiGetSurfaceFormatDX12(current.Format) != self.description.backBufferFormat) {
            // ResizeBuffers was drained by the presentation owner. The generation context's
            // display extent is fixed at creation, independently of the proxy swapchain.
            if (wait(self) != RSF_BACKEND_OK) return RSF_BACKEND_ERROR_NOT_READY;
            if (self.destroy(&self.generation, nullptr) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_FEATURE_FAILED;
            self.generation = nullptr;
            self.description.displaySize = {current.Width, current.Height};
            self.description.maxRenderSize = self.description.displaySize;
            self.description.backBufferFormat = ffxApiGetSurfaceFormatDX12(current.Format);
            if (self.create(&self.generation, &self.description.header, nullptr) != FFX_API_RETURN_OK)
                return RSF_BACKEND_ERROR_INIT_FAILED;
            if (!self.copied.reset_history(before.revision)) return RSF_BACKEND_ERROR_NOT_READY;
            before.history_valid = false;
        }
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
    if (self.capture.enabled()) {
        std::lock_guard lock(self.capture_guard);
        self.capture_identities[config.frameID % self.capture_identities.size()] = {config.frameID, record};
    }
    if (enabled && self.ui_mode == RSF_UI_MODE_BACKBUFFER_HUDLESS) {
        if (!frame->hudless.resource) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
        config.HUDLessColor = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(frame->hudless.resource));
    }
    if (self.configure(&self.generation, &config.header) != FFX_API_RETURN_OK) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    const bool staged=self.copied.stage_prepare(before.revision,record.frame_id,enabled ? before.options.mode : RSF_FG_OFF,
        enabled ? 1u : 0u);
    if (!enabled) {
        const bool completed=self.copied.complete_prepare(before.revision,record.frame_id,false,RSF_FG_OFF,0);
        return staged && completed ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_NOT_READY;
    }
    if (!staged) return RSF_BACKEND_ERROR_NOT_READY;
    ffxDispatchDescFrameGenerationPrepareV2 data{};
    data.header.type = FFX_API_DISPATCH_DESC_TYPE_FRAMEGENERATION_PREPARE_V2;
    data.frameID = self.sequence; data.commandList = command;
    data.renderSize = {record.render_width, record.render_height};
    data.jitterOffset = {record.camera.jitter_pixels[0], record.camera.jitter_pixels[1]};
    data.motionVectorScale = {frame->motion_scale_x, frame->motion_scale_y};
    data.frameTimeDelta = record.frame_time_ms; data.cameraNear = record.camera.near_plane;
    data.cameraFar = record.camera.far_plane; data.cameraFovAngleVertical = record.camera.vertical_fov_radians;
    data.viewSpaceToMetersFactor = self.view_space_to_meters;
    data.reset = (record.flags & RSF_FRAME_FLAG_RESET) || !before.history_valid || record.frame_id != before.last_prepare + 1;
    data.depth = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(frame->depth.resource));
    data.motionVectors = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(frame->motion.resource));
    // Perspective W follows view Z. Unity's right-handed projection looks down
    // negative Z; UE's positive-Z view basis remains positive.
    const float forward_sign = record.camera.view_to_clip[11] < 0 ? -1.0f : 1.0f;
    for (uint32_t i = 0; i < 3; ++i) {
        data.cameraPosition[i] = record.camera.view_to_world[12 + i];
        data.cameraRight[i] = record.camera.view_to_world[i];
        data.cameraUp[i] = record.camera.view_to_world[4 + i];
        data.cameraForward[i] = forward_sign * record.camera.view_to_world[8 + i];
    }
    const auto dispatched = self.dispatch(&self.generation, &data.header);
    (void)self.capture.event(record, "fsr-prepare", data.frameID, data.reset, static_cast<int32_t>(dispatched));
    if (dispatched != FFX_API_RETURN_OK) {
        (void)self.copied.reset_history(before.revision); return RSF_BACKEND_ERROR_FEATURE_FAILED;
    }
    return self.copied.complete_prepare(before.revision,record.frame_id,true,before.options.mode,1) ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_NOT_READY;
}
rsf_backend_result after(void* pointer)
{
    if (!pointer) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto& self = *static_cast<FfxSession*>(pointer);
    const auto before=self.copied.read();
    if (self.capture.pending() && wait(self) == RSF_BACKEND_OK)
        (void)self.capture.flush_after_queue(self.queue.Get());
    rsf::GenerationState::Observation observation{};
    observation.replace_statistics=observation.replace_callbacks=true;
    observation.generated_callbacks = self.ui_mode == RSF_UI_MODE_NONE ? self.generated.load() : self.dispatched.load();
    observation.present_callbacks = self.presented.load(); observation.vendor_status = self.dispatch_result.load();
    observation.valid_statistics = RSF_FG_STAT_ACTIVITY | (self.ui_mode == RSF_UI_MODE_NONE ? RSF_FG_STAT_PRESENT_CALLBACKS : 0u);
    const auto observed = self.ui_mode == RSF_UI_MODE_NONE ? observation.generated_callbacks : self.dispatched.load();
    observation.active = observed > self.observed_generated;
    self.observed_generated = observed;
    UINT count = 0;
    const auto counter_result = self.chain->GetLastPresentCount(&count);
    if (SUCCEEDED(counter_result)) {
        const UINT delta = count >= self.last_dxgi_present || self.last_dxgi_present > 0xf0000000u ?
            count - self.last_dxgi_present : count;
        observation.total_presented_delta = delta; self.last_dxgi_present = count;
        observation.valid_statistics |= RSF_FG_STAT_TOTAL_PRESENTED;
    }
    (void)self.copied.observed(before.revision,observation);
    if (self.log && self.counter_reports++ < 6) {
        char text[176]; std::snprintf(text, sizeof(text), "FSR FG counters source=%llu generated=%llu DXGI_present_count=%u query=%08lx",
            static_cast<unsigned long long>(before.last_prepare), static_cast<unsigned long long>(self.dispatched.load()),
            count, static_cast<unsigned long>(counter_result)); self.log(self.log_user, text);
    }
    return RSF_BACKEND_OK;
}
rsf_backend_result retirement(void* pointer, rsf_fg_retirement* out)
{
    if (!pointer || !out || out->struct_size < sizeof(*out)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    // This integration does not enable async compute. Interpolation and HUD-less
    // reads run on the game queue, whose completion fence the caller already owns.
    // Waiting for every display Present here serializes subsequent rendering.
    out->fence = nullptr; out->value = 0; return RSF_BACKEND_OK;
}
rsf_backend_result abort_frame(void* pointer, uint64_t id)
{
    if (!pointer || !id) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto& self = *static_cast<FfxSession*>(pointer);
    self.copied.abort();
    return RSF_BACKEND_OK;
}
rsf_backend_result status(void* pointer, rsf_fg_status* out) {
    if (!pointer || !out || out->struct_size<sizeof(*out)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out=static_cast<FfxSession*>(pointer)->copied.read().status; return RSF_BACKEND_OK;
}
const rsf_generation_provider provider{sizeof(provider), create, configure, begin, marker, prepare, after, status, retirement, destroy, abort_frame};
}
extern "C" const rsf_generation_provider* rsf_generation_fsr() { return &provider; }
extern "C" uint32_t rsf_generation_fsr4_supported(void* device, const char* directory)
{
    if (!device || !directory || !*directory) return 0;
    rsf_generation_setup setup{}; setup.runtime_directory_utf8 = directory;
    HMODULE module = rsf::fg_library(setup, L"amd_fidelityfx_framegeneration_dx12.dll");
    if (!module) return 0;
    const auto query = rsf::entry<PfnFfxQuery>(module, "ffxQuery");
    uint64_t count = 32, ids[32]{}, id = 0; const char* names[32]{}; char name[64]{};
    ffxQueryDescGetVersions versions{}; versions.header.type = FFX_API_QUERY_DESC_TYPE_GET_VERSIONS;
    versions.createDescType = FFX_API_CREATE_CONTEXT_DESC_TYPE_FRAMEGENERATION;
    versions.device = device; versions.outputCount = &count; versions.versionIds = ids; versions.versionNames = names;
    const bool supported = query && query(nullptr, &versions.header) == FFX_API_RETURN_OK && count <= 32 &&
        rsf::select_ffx_version(ids, names, count, 4, 0, id, name, sizeof(name));
    FreeLibrary(module);
    return supported ? 1u : 0u;
}
#else
extern "C" const rsf_generation_provider* rsf_generation_fsr() { return rsf::not_compiled_provider(); }
extern "C" uint32_t rsf_generation_fsr4_supported(void*, const char*) { return 0; }
#endif
