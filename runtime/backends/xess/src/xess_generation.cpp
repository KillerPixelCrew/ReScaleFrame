// SPDX-License-Identifier: MIT
#include "../../common/generation12_checks.h"
#if defined(RSF_HAVE_XESS_FG)
#include "../../common/d3d12_helpers.h"
#include <xess_fg/xefg_swapchain_d3d12.h>
#include <xell/xell_d3d12.h>
#include <wrl/client.h>
#include <cstring>
#include <memory>
#include <new>

using Microsoft::WRL::ComPtr;
namespace {
static_assert(RSF_LATENCY_INPUT_SAMPLE == 6);
static_assert(RSF_REFLEX_ON == 1);
static_assert(XEFG_SWAPCHAIN_RESULT_SUCCESS == 0 && XELL_RESULT_SUCCESS == 0);

struct Bindings {
    HMODULE module{};
    HMODULE xell_module{};
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

    ~Bindings() {
        if (module) FreeLibrary(module);
        if (xell_module) FreeLibrary(xell_module);
    }
};

rsf_backend_result open(const rsf_generation_setup* setup, void** out)
{
    auto api = std::unique_ptr<Bindings>(new (std::nothrow) Bindings{});
    if (!api) return RSF_BACKEND_ERROR_INIT_FAILED;
    api->module = rsf::fg_library(*setup, L"libxess_fg.dll");
    api->xell_module = rsf::fg_library(*setup, L"libxell.dll");
    if (!api->module || !api->xell_module) return RSF_BACKEND_ERROR_LOAD_FAILED;
#define ENTRY(module, member, function) \
    api->member = rsf::entry<decltype(&function)>(api->module, #function); \
    if (!api->member) return RSF_BACKEND_ERROR_MISSING_ENTRY_POINT
    ENTRY(module, create, xefgSwapChainD3D12CreateContext);
    ENTRY(module, init, xefgSwapChainD3D12InitFromSwapChainDesc);
    ENTRY(module, chain_ptr, xefgSwapChainD3D12GetSwapChainPtr);
    ENTRY(module, tag, xefgSwapChainD3D12TagFrameResource);
    ENTRY(module, properties, xefgSwapChainGetProperties);
    ENTRY(module, enabled, xefgSwapChainSetEnabled);
    ENTRY(module, present_id, xefgSwapChainSetPresentId);
    ENTRY(module, constants, xefgSwapChainTagFrameConstants);
    ENTRY(module, present_status, xefgSwapChainGetLastPresentStatus);
    ENTRY(module, count, xefgSwapChainSetNumInterpolatedFrames);
    ENTRY(module, set_xell, xefgSwapChainSetLatencyReduction);
    ENTRY(module, destroy, xefgSwapChainDestroy);
    ENTRY(module, version, xefgSwapChainGetVersion);
    ENTRY(module, compose, xefgSwapChainSetUiCompositionState);
    ENTRY(xell_module, xell_create, xellD3D12CreateContext);
    ENTRY(xell_module, xell_destroy, xellDestroyContext);
    ENTRY(xell_module, xell_options, xellSetSleepMode);
    ENTRY(xell_module, sleep, xellSleep);
    ENTRY(xell_module, marker, xellAddMarkerData);
#undef ENTRY
    *out = api.release();
    return RSF_BACKEND_OK;
}

void close(void* bindings)
{
    delete static_cast<Bindings*>(bindings);
}

int32_t create_fg(void* bindings, void* device, void** out)
{
    xefg_swapchain_handle_t handle{};
    const auto result = static_cast<Bindings*>(bindings)->create(static_cast<ID3D12Device*>(device), &handle);
    *out = handle;
    return result;
}

int32_t create_xell(void* bindings, void* device, void** out)
{
    xell_context_handle_t handle{};
    const auto result = static_cast<Bindings*>(bindings)->xell_create(static_cast<ID3D12Device*>(device), &handle);
    *out = handle;
    return result;
}

int32_t destroy_fg(void* bindings, void* context)
{
    return static_cast<Bindings*>(bindings)->destroy(static_cast<xefg_swapchain_handle_t>(context));
}

int32_t destroy_xell(void* bindings, void* context)
{
    return static_cast<Bindings*>(bindings)->xell_destroy(static_cast<xell_context_handle_t>(context));
}

int32_t max_count(void* bindings, void* context, uint32_t* out)
{
    xefg_swapchain_properties_t properties{};
    const auto result = static_cast<Bindings*>(bindings)->properties(static_cast<xefg_swapchain_handle_t>(context), &properties);
    *out = properties.maxSupportedInterpolations;
    return result;
}

int32_t bind_xell(void* bindings, void* context, void* xell)
{
    return static_cast<Bindings*>(bindings)->set_xell(static_cast<xefg_swapchain_handle_t>(context), xell);
}

void version(void* bindings, rsf_xeg12_version* out)
{
    xefg_swapchain_version_t value{};
    static_cast<Bindings*>(bindings)->version(&value);
    *out = {value.major, value.minor, value.patch};
}

rsf_backend_result create_chain(void* bindings, void* raw, const rsf_generation_setup* setup,
    uint32_t reserved, void** out)
{
    const auto& api = *static_cast<Bindings*>(bindings);
    const auto context = static_cast<xefg_swapchain_handle_t>(raw);
    ComPtr<IDXGIFactory2> factory;
    if (FAILED(CreateDXGIFactory2(0, IID_PPV_ARGS(&factory)))) return RSF_BACKEND_ERROR_INIT_FAILED;
    auto desc = rsf::chain_description(setup->chain);
    xefg_swapchain_d3d12_init_params_t init{};
    init.maxInterpolatedFrames = reserved;
    init.initFlags = (setup->depth_inverted ? XEFG_SWAPCHAIN_INIT_FLAG_INVERTED_DEPTH : 0u) |
        (setup->motion_jittered ? XEFG_SWAPCHAIN_INIT_FLAG_JITTERED_MV : 0u);
    init.uiMode = setup->chain.ui_mode == RSF_UI_MODE_BACKBUFFER_HUDLESS_UI ? XEFG_SWAPCHAIN_UI_MODE_BACKBUFFER_HUDLESS_UITEXTURE :
        setup->chain.ui_mode == RSF_UI_MODE_BACKBUFFER_HUDLESS ? XEFG_SWAPCHAIN_UI_MODE_BACKBUFFER_HUDLESS : XEFG_SWAPCHAIN_UI_MODE_NONE;
    if (api.init(context, static_cast<HWND>(setup->chain.hwnd), &desc, nullptr,
        static_cast<ID3D12CommandQueue*>(setup->chain.d3d12_queue), factory.Get(), &init) != XEFG_SWAPCHAIN_RESULT_SUCCESS)
        return RSF_BACKEND_ERROR_INIT_FAILED;
    ComPtr<IDXGISwapChain4> chain;
    if (api.chain_ptr(context, IID_PPV_ARGS(&chain)) != XEFG_SWAPCHAIN_RESULT_SUCCESS)
        return RSF_BACKEND_ERROR_INIT_FAILED;
    *out = chain.Detach();
    return RSF_BACKEND_OK;
}

int32_t compose(void* bindings, void* context, uint32_t enabled)
{
    return static_cast<Bindings*>(bindings)->compose(static_cast<xefg_swapchain_handle_t>(context),
        enabled ? XEFG_SWAPCHAIN_UI_COMPOSITION_STATE_ENABLED : XEFG_SWAPCHAIN_UI_COMPOSITION_STATE_DISABLED);
}

int32_t count(void* bindings, void* context, uint32_t value)
{
    return static_cast<Bindings*>(bindings)->count(static_cast<xefg_swapchain_handle_t>(context), value);
}

int32_t enabled(void* bindings, void* context, uint32_t value)
{
    return static_cast<Bindings*>(bindings)->enabled(static_cast<xefg_swapchain_handle_t>(context), value);
}

int32_t sleep_mode(void* bindings, void* context, uint32_t interval, uint32_t low_latency)
{
    xell_sleep_params_t options{};
    options.minimumIntervalUs = interval;
    options.bLowLatencyMode = low_latency != 0;
    return static_cast<Bindings*>(bindings)->xell_options(static_cast<xell_context_handle_t>(context), &options);
}

int32_t sleep(void* bindings, void* context, uint32_t sdk_id)
{
    return static_cast<Bindings*>(bindings)->sleep(static_cast<xell_context_handle_t>(context), sdk_id);
}

int32_t marker(void* bindings, void* context, uint32_t sdk_id, rsf_latency_marker kind)
{
    return static_cast<Bindings*>(bindings)->marker(static_cast<xell_context_handle_t>(context),
        sdk_id, static_cast<xell_latency_marker_type_t>(kind));
}

int32_t present_id(void* bindings, void* context, uint32_t sdk_id)
{
    return static_cast<Bindings*>(bindings)->present_id(static_cast<xefg_swapchain_handle_t>(context), sdk_id);
}

int32_t tag(void* bindings, void* context, void* list, uint32_t sdk_id, uint32_t kind,
    const rsf_backend_resource* resource)
{
    constexpr xefg_swapchain_resource_type_t types[]{XEFG_SWAPCHAIN_RES_DEPTH, XEFG_SWAPCHAIN_RES_MOTION_VECTOR,
        XEFG_SWAPCHAIN_RES_HUDLESS_COLOR, XEFG_SWAPCHAIN_RES_UI};
    xefg_swapchain_d3d12_resource_data_t data{};
    data.type = types[kind];
    data.validity = XEFG_SWAPCHAIN_RV_ONLY_NOW;
    data.resourceBase = {resource->x, resource->y};
    data.resourceSize = {resource->width, resource->height};
    data.pResource = static_cast<ID3D12Resource*>(resource->resource);
    data.incomingState = static_cast<D3D12_RESOURCE_STATES>(resource->state);
    return static_cast<Bindings*>(bindings)->tag(static_cast<xefg_swapchain_handle_t>(context),
        static_cast<ID3D12CommandList*>(list), sdk_id, &data);
}

int32_t constants(void* bindings, void* context, uint32_t sdk_id, const rsf_fg_frame* frame, uint32_t reset)
{
    const auto& camera = frame->record->camera;
    xefg_swapchain_frame_constant_data_t data{};
    data.resetHistory = reset;
    std::memcpy(data.viewMatrix, camera.world_to_view, sizeof(data.viewMatrix));
    std::memcpy(data.projectionMatrix, camera.view_to_clip, sizeof(data.projectionMatrix));
    data.jitterOffsetX = camera.jitter_pixels[0];
    data.jitterOffsetY = camera.jitter_pixels[1];
    data.motionVectorScaleX = frame->motion_scale_x;
    data.motionVectorScaleY = frame->motion_scale_y;
    // Simulation delta does not measure Present cadence. Zero is the SDK's
    // unspecified value for this deprecated field.
    data.frameRenderTime = 0;
    return static_cast<Bindings*>(bindings)->constants(static_cast<xefg_swapchain_handle_t>(context), sdk_id, &data);
}

int32_t present_status(void* bindings, void* context, rsf_xeg12_present* out)
{
    xefg_swapchain_present_status_t status{};
    const auto result = static_cast<Bindings*>(bindings)->present_status(static_cast<xefg_swapchain_handle_t>(context), &status);
    *out = {status.frameGenResult, status.isFrameGenEnabled, status.framesPresented};
    return result;
}

void* create_fence(void* device)
{
    ComPtr<ID3D12Fence> fence;
    if (FAILED(static_cast<ID3D12Device*>(device)->CreateFence(0, D3D12_FENCE_FLAG_NONE,
        IID_PPV_ARGS(&fence)))) return nullptr;
    return fence.Detach();
}

void* create_event()
{
    return CreateEventW(nullptr, FALSE, FALSE, nullptr);
}

int32_t drain(void* queue, void* fence, void* event, uint64_t serial)
{
    auto* native_fence = static_cast<ID3D12Fence*>(fence);
    const auto signal = static_cast<ID3D12CommandQueue*>(queue)->Signal(native_fence, serial);
    if (FAILED(signal)) return signal;
    const auto waited = rsf::wait_fence(native_fence, serial, static_cast<HANDLE>(event));
    if (FAILED(waited)) return waited;
    // A previous timed-out registration can still signal the reused event.
    const auto completed = native_fence->GetCompletedValue();
    if (completed == UINT64_MAX) return DXGI_ERROR_DEVICE_REMOVED;
    return completed >= serial ? S_OK : DXGI_ERROR_DEVICE_HUNG;
}

const rsf_xeg12_ops& native_ops()
{
    static const rsf_xeg12_ops ops{&rsf::generation12_checks, open, close, create_fg, create_xell,
        destroy_fg, destroy_xell, max_count, bind_xell, version, create_chain, compose, count, enabled,
        sleep_mode, sleep, marker, present_id, tag, constants, present_status, create_fence, create_event, drain};
    return ops;
}

rsf_backend_result create(const rsf_generation_setup* setup, void** out, void** chain)
{
    return rsf_core_xeg12_create(&native_ops(), setup, out, chain);
}
}

extern "C" const rsf_generation_provider* rsf_generation_xess()
{
    static const auto provider = [] {
        auto value = *rsf_core_xeg12_provider();
        value.create = create;
        return value;
    }();
    return &provider;
}
#else
extern "C" const rsf_generation_provider* rsf_generation_xess() { return rsf::not_compiled_provider(); }
#endif
