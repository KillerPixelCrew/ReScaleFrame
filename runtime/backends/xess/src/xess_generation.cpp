// SPDX-License-Identifier: GPL-3.0-only
#include "../../common/fg_helpers.h"
#include <wrl/client.h>
#include <new>
#include <mutex>
#include <cstring>
#if defined(RSF_HAVE_XESS_FG)
#include <xess_fg/xefg_swapchain_d3d12.h>
#include <xell/xell_d3d12.h>
using Microsoft::WRL::ComPtr;
namespace {
struct XegSession {
    HMODULE module = nullptr, xell_module = nullptr;
    xefg_swapchain_handle_t context = nullptr;
    xell_context_handle_t xell = nullptr;
    ComPtr<IDXGISwapChain4> chain;
    ComPtr<ID3D12Device> device;
    std::mutex latency_guard;
    decltype(&xefgSwapChainD3D12CreateContext) create{};
    decltype(&xefgSwapChainD3D12InitFromSwapChainDesc) init{};
    decltype(&xefgSwapChainD3D12GetSwapChainPtr) chain_ptr{};
    decltype(&xefgSwapChainD3D12TagFrameResource) tag{};
    decltype(&xefgSwapChainGetProperties) properties{};
    decltype(&xefgSwapChainSetEnabled) enabled{};
    decltype(&xefgSwapChainSetPresentId) present_id{};
    decltype(&xefgSwapChainTagFrameConstants) constants{};
    decltype(&xefgSwapChainGetLastPresentStatus) present_status{};
    decltype(&xefgSwapChainSetNumInterpolatedFrames) count{};
    decltype(&xefgSwapChainSetLatencyReduction) set_xell{};
    decltype(&xefgSwapChainSetUiCompositionState) compose{};
    decltype(&xefgSwapChainDestroy) destroy{};
    decltype(&xefgSwapChainGetVersion) version{};
    decltype(&xellD3D12CreateContext) xell_create{};
    decltype(&xellDestroyContext) xell_destroy{};
    decltype(&xellSetSleepMode) xell_options{};
    decltype(&xellSleep) sleep{};
    decltype(&xellAddMarkerData) marker{};
    rsf_fg_status state{};
    rsf_fg_options options{};
    uint64_t last_begin = 0;
    uint32_t reserved = 0;
    bool history_valid = false;
    uint64_t last_prepare = 0;
    rsf_ui_mode ui_mode = RSF_UI_MODE_NONE;
};
void destroy(void* pointer)
{
    auto* self = static_cast<XegSession*>(pointer); if (!self) return;
    self->chain.Reset();
    if (self->context) self->destroy(self->context);
    if (self->xell) self->xell_destroy(self->xell);
    if (self->module) FreeLibrary(self->module);
    if (self->xell_module) FreeLibrary(self->xell_module);
    delete self;
}
rsf_backend_result create(const rsf_generation_setup* setup, void** out, void** chain)
{
    auto result = rsf::fg_setup(setup, out, chain); if (result != 0) return result;
    if (setup->chain.ui_mode == RSF_UI_MODE_UI_LAYER) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    if (GetModuleHandleW(L"sl.dlss_g.dll") && !setup->streamline_host) return RSF_BACKEND_ERROR_NEEDS_RESTART;
    auto* self = new (std::nothrow) XegSession; if (!self) return RSF_BACKEND_ERROR_INIT_FAILED;
    self->device = static_cast<ID3D12Device*>(setup->chain.d3d12_device);
    self->ui_mode = setup->chain.ui_mode;
    self->module = rsf::fg_library(*setup, L"libxess_fg.dll");
    self->xell_module = rsf::fg_library(*setup, L"libxell.dll");
    if (!self->module || !self->xell_module) { destroy(self); return RSF_BACKEND_ERROR_LOAD_FAILED; }
#define XEG(member, function) self->member = rsf::entry<decltype(&function)>(self->module, #function); if (!self->member) { destroy(self); return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT; }
    XEG(create, xefgSwapChainD3D12CreateContext); XEG(init, xefgSwapChainD3D12InitFromSwapChainDesc);
    XEG(chain_ptr, xefgSwapChainD3D12GetSwapChainPtr); XEG(tag, xefgSwapChainD3D12TagFrameResource);
    XEG(properties, xefgSwapChainGetProperties); XEG(enabled, xefgSwapChainSetEnabled);
    XEG(present_id, xefgSwapChainSetPresentId); XEG(constants, xefgSwapChainTagFrameConstants);
    XEG(present_status, xefgSwapChainGetLastPresentStatus); XEG(count, xefgSwapChainSetNumInterpolatedFrames);
    XEG(set_xell, xefgSwapChainSetLatencyReduction); XEG(destroy, xefgSwapChainDestroy); XEG(version, xefgSwapChainGetVersion);
    XEG(compose, xefgSwapChainSetUiCompositionState);
#undef XEG
#define XELL(member, function) self->member = rsf::entry<decltype(&function)>(self->xell_module, #function); if (!self->member) { destroy(self); return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT; }
    XELL(xell_create, xellD3D12CreateContext); XELL(xell_destroy, xellDestroyContext);
    XELL(xell_options, xellSetSleepMode); XELL(sleep, xellSleep); XELL(marker, xellAddMarkerData);
#undef XELL
    if (self->create(self->device.Get(), &self->context) != XEFG_SWAPCHAIN_RESULT_SUCCESS ||
        self->xell_create(self->device.Get(), &self->xell) != XELL_RESULT_SUCCESS) {
        destroy(self); return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    }
    xefg_swapchain_properties_t properties{};
    if (self->properties(self->context, &properties) != XEFG_SWAPCHAIN_RESULT_SUCCESS ||
        !properties.maxSupportedInterpolations || self->set_xell(self->context, self->xell) != XEFG_SWAPCHAIN_RESULT_SUCCESS) {
        destroy(self); return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    }
    self->reserved = setup->chain.max_generated_frames < properties.maxSupportedInterpolations ?
        setup->chain.max_generated_frames : properties.maxSupportedInterpolations;
    self->state.struct_size = sizeof(self->state); self->state.supported = 1;
    self->state.max_generated_frames = self->reserved; self->state.vsync_supported = 1;
    self->state.pacing_owner = RSF_PACING_XELL;
    xefg_swapchain_version_t version{}; self->version(&version);
    self->state.version_id = (uint64_t(version.major) << 32) | (uint64_t(version.minor) << 16) | version.patch;
    std::snprintf(self->state.version_name, sizeof(self->state.version_name), "XeFG %u.%u.%u", version.major, version.minor, version.patch);
    if (setup->version_id && setup->version_id != self->state.version_id) { destroy(self); return RSF_BACKEND_ERROR_NOT_SUPPORTED; }
    if (setup->chain.hwnd) {
        ComPtr<IDXGIFactory2> factory;
        if (FAILED(CreateDXGIFactory2(0, IID_PPV_ARGS(&factory)))) { destroy(self); return RSF_BACKEND_ERROR_INIT_FAILED; }
        auto desc = rsf::chain_description(setup->chain);
        xefg_swapchain_d3d12_init_params_t init{}; init.maxInterpolatedFrames = self->reserved;
        init.initFlags = (setup->depth_inverted ? XEFG_SWAPCHAIN_INIT_FLAG_INVERTED_DEPTH : 0u) |
            (setup->motion_jittered ? XEFG_SWAPCHAIN_INIT_FLAG_JITTERED_MV : 0u);
        init.uiMode = setup->chain.ui_mode == RSF_UI_MODE_BACKBUFFER_HUDLESS_UI ? XEFG_SWAPCHAIN_UI_MODE_BACKBUFFER_HUDLESS_UITEXTURE :
            setup->chain.ui_mode == RSF_UI_MODE_BACKBUFFER_HUDLESS ? XEFG_SWAPCHAIN_UI_MODE_BACKBUFFER_HUDLESS : XEFG_SWAPCHAIN_UI_MODE_NONE;
        if (self->init(self->context, static_cast<HWND>(setup->chain.hwnd), &desc, nullptr,
            static_cast<ID3D12CommandQueue*>(setup->chain.d3d12_queue), factory.Get(), &init) != XEFG_SWAPCHAIN_RESULT_SUCCESS ||
            self->chain_ptr(self->context, IID_PPV_ARGS(&self->chain)) != XEFG_SWAPCHAIN_RESULT_SUCCESS) {
            destroy(self); return RSF_BACKEND_ERROR_INIT_FAILED;
        }
        if (self->compose(self->context, self->ui_mode == RSF_UI_MODE_NONE ? XEFG_SWAPCHAIN_UI_COMPOSITION_STATE_DISABLED :
            XEFG_SWAPCHAIN_UI_COMPOSITION_STATE_ENABLED) != XEFG_SWAPCHAIN_RESULT_SUCCESS) {
            destroy(self); return RSF_BACKEND_ERROR_FEATURE_FAILED;
        }
    }
    *out = self; *chain = self->chain.Get(); return RSF_BACKEND_OK;
}
rsf_backend_result configure(void* pointer, const rsf_fg_options* options)
{
    if (!pointer) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    const auto validation = rsf::fg_options_result(options); if (validation != 0) return validation;
    auto& self = *static_cast<XegSession*>(pointer);
    if (!self.chain) return RSF_BACKEND_ERROR_NOT_READY;
    if (options->mode > RSF_FG_FIXED) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    if (options->reflex_mode != RSF_REFLEX_OFF) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    const uint32_t count = options->generated_frames > self.reserved ? self.reserved : options->generated_frames;
    if (options->mode != RSF_FG_OFF && !count) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (self.options.struct_size && self.options.mode == options->mode &&
        self.options.generated_frames == options->generated_frames &&
        self.options.frame_limit_us == options->frame_limit_us) return RSF_BACKEND_OK;
    const bool reset_history = !self.options.struct_size || self.options.mode != options->mode ||
        self.options.generated_frames != options->generated_frames;
    if (count && self.count(self.context, count) != XEFG_SWAPCHAIN_RESULT_SUCCESS) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    xell_sleep_params_t sleep{}; sleep.minimumIntervalUs = options->frame_limit_us;
    sleep.bLowLatencyMode = options->mode != RSF_FG_OFF;
    { std::lock_guard<std::mutex> lock(self.latency_guard);
      if (self.xell_options(self.xell, &sleep) != XELL_RESULT_SUCCESS) return RSF_BACKEND_ERROR_FEATURE_FAILED; }
    if (self.enabled(self.context, options->mode != RSF_FG_OFF) != XEFG_SWAPCHAIN_RESULT_SUCCESS)
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    self.options = *options; self.state.effective_mode = options->mode;
    self.state.configured_mode = options->mode;
    self.state.effective_generated_frames = options->mode == RSF_FG_OFF ? 0 : count;
    if (reset_history) { self.state.active = 0; self.history_valid = false; }
    return RSF_BACKEND_OK;
}
rsf_backend_result begin(void* pointer, uint64_t id)
{
    if (!pointer || !id) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (id > UINT32_MAX) return RSF_BACKEND_ERROR_NEEDS_RESTART;
    auto& self = *static_cast<XegSession*>(pointer);
    std::lock_guard<std::mutex> lock(self.latency_guard);
    if (id <= self.last_begin) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    const auto result = self.sleep(self.xell, static_cast<uint32_t>(id));
    if (result == XELL_RESULT_SUCCESS) self.last_begin = id;
    return result == XELL_RESULT_SUCCESS ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
rsf_backend_result marker(void* pointer, rsf_latency_marker marker, uint64_t id, uint32_t)
{
    if (!pointer || !id || id > UINT32_MAX || marker > RSF_LATENCY_INPUT_SAMPLE) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto& self = *static_cast<XegSession*>(pointer);
    std::lock_guard<std::mutex> lock(self.latency_guard);
    return self.marker(self.xell, static_cast<uint32_t>(id), static_cast<xell_latency_marker_type_t>(marker)) == XELL_RESULT_SUCCESS ?
        RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
rsf_backend_result prepare(void* pointer, void* list, const rsf_fg_frame* frame)
{
    if (!pointer) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto& self = *static_cast<XegSession*>(pointer);
    auto result = rsf::fg_frame(frame, self.device.Get()); if (result != 0) return result;
    const uint64_t id = frame->record->frame_id;
    if (id > UINT32_MAX) return RSF_BACKEND_ERROR_NEEDS_RESTART;
    if (!self.chain || id <= self.last_prepare) return RSF_BACKEND_ERROR_NOT_READY;
    if (self.present_id(self.context, static_cast<uint32_t>(id)) != XEFG_SWAPCHAIN_RESULT_SUCCESS)
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    if (!frame->interpolate || self.options.mode == RSF_FG_OFF) {
        self.state.active = 0;
        self.state.effective_mode = RSF_FG_OFF;
        self.state.effective_generated_frames = 0;
        self.history_valid = false; self.last_prepare = id;
        return self.enabled(self.context, 0) == XEFG_SWAPCHAIN_RESULT_SUCCESS ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
    }
    if (!list) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    result = rsf::fg_command(list, self.device.Get()); if (result != 0) return result;
    if ((self.ui_mode != RSF_UI_MODE_NONE && !frame->hudless.resource) ||
        (self.ui_mode == RSF_UI_MODE_BACKBUFFER_HUDLESS_UI && !frame->ui.resource))
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    xefg_swapchain_frame_constant_data_t constants{};
    constants.resetHistory = !self.history_valid || id != self.last_prepare + 1;
    std::memcpy(constants.viewMatrix, frame->record->camera.world_to_view, sizeof(constants.viewMatrix));
    std::memcpy(constants.projectionMatrix, frame->record->camera.view_to_clip, sizeof(constants.projectionMatrix));
    constants.jitterOffsetX = frame->record->camera.jitter_pixels[0]; constants.jitterOffsetY = frame->record->camera.jitter_pixels[1];
    constants.motionVectorScaleX = frame->motion_scale_x; constants.motionVectorScaleY = frame->motion_scale_y;
    constants.frameRenderTime = frame->record->frame_time_ms;
    const rsf_backend_resource* resources[] = {&frame->depth, &frame->motion, &frame->hudless, &frame->ui};
    const xefg_swapchain_resource_type_t types[] = {XEFG_SWAPCHAIN_RES_DEPTH, XEFG_SWAPCHAIN_RES_MOTION_VECTOR, XEFG_SWAPCHAIN_RES_HUDLESS_COLOR, XEFG_SWAPCHAIN_RES_UI};
    for (uint32_t i = 0; i < 4; ++i) {
        if (!resources[i]->resource) continue;
        xefg_swapchain_d3d12_resource_data_t data{};
        data.type = types[i]; data.validity = XEFG_SWAPCHAIN_RV_ONLY_NOW;
        data.resourceBase = {resources[i]->x, resources[i]->y}; data.resourceSize = {resources[i]->width, resources[i]->height};
        data.pResource = static_cast<ID3D12Resource*>(resources[i]->resource);
        data.incomingState = static_cast<D3D12_RESOURCE_STATES>(resources[i]->state);
        if (self.tag(self.context, static_cast<ID3D12CommandList*>(list), static_cast<uint32_t>(id), &data) != XEFG_SWAPCHAIN_RESULT_SUCCESS)
            return RSF_BACKEND_ERROR_FEATURE_FAILED;
    }
    if (self.constants(self.context, static_cast<uint32_t>(id), &constants) != XEFG_SWAPCHAIN_RESULT_SUCCESS ||
        self.enabled(self.context, self.options.mode != RSF_FG_OFF) != XEFG_SWAPCHAIN_RESULT_SUCCESS)
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    self.last_prepare = id; self.history_valid = true; self.state.active = 0;
    self.state.effective_mode = self.options.mode;
    self.state.effective_generated_frames = self.options.generated_frames > self.reserved ? self.reserved : self.options.generated_frames;
    return RSF_BACKEND_OK;
}
rsf_backend_result after(void* pointer)
{
    if (!pointer) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto& self = *static_cast<XegSession*>(pointer);
    xefg_swapchain_present_status_t status{};
    const auto result = self.present_status(self.context, &status);
    self.state.vendor_status = result;
    self.state.active = 0;
    if (result < XEFG_SWAPCHAIN_RESULT_SUCCESS) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    if (result != XEFG_SWAPCHAIN_RESULT_SUCCESS) return RSF_BACKEND_OK;
    self.state.total_presented += status.framesPresented;
    self.state.valid_statistics |= RSF_FG_STAT_TOTAL_PRESENTED | RSF_FG_STAT_ACTIVITY;
    self.state.active = status.framesPresented > 1 && status.isFrameGenEnabled &&
        status.frameGenResult == XEFG_SWAPCHAIN_RESULT_SUCCESS;
    return RSF_BACKEND_OK;
}
rsf_backend_result retirement(void* pointer, rsf_fg_retirement* out)
{
    if (!pointer || !out || out->struct_size < sizeof(*out)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    out->fence = nullptr; out->value = 0; return RSF_BACKEND_OK;
}
rsf_backend_result abort_frame(void* pointer, uint64_t id)
{
    if (!pointer || !id) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    auto& self = *static_cast<XegSession*>(pointer);
    self.history_valid = false; self.state.active = 0; return RSF_BACKEND_OK;
}
const rsf_generation_provider provider{sizeof(provider), create, configure, begin, marker, prepare, after, rsf::session_status<XegSession>, retirement, destroy, abort_frame};
}
extern "C" const rsf_generation_provider* rsf_generation_xess() { return &provider; }
#else
extern "C" const rsf_generation_provider* rsf_generation_xess() { return rsf::not_compiled_provider(); }
#endif
