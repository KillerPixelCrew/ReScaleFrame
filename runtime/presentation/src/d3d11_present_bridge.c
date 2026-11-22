/* SPDX-License-Identifier: MIT */
/* Factory interception, native trampolines and SDK linkage for the Rust presentation owner.
   C11 leaf. Exception containment is a structured-exception filter on the MSVC C++ exception
   code 0xE06D7363, which stops what the former catch-all stopped; other faults propagate. */
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <rescaleframe/d3d11_present_bridge.h>
#include <rescaleframe/shared_surface.h>
#include "d3d11_present_platform.h"
#include <d3d11_4.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <MinHook.h>
#include <stddef.h>

#define RSF_CXX_EXCEPTION_CODE 0xE06D7363UL

#if defined(_MSC_VER)
#define RSF_FOREIGN_TRY __try
#define RSF_FOREIGN_CATCH(on_exception) \
    __except (GetExceptionCode() == RSF_CXX_EXCEPTION_CODE ? EXCEPTION_EXECUTE_HANDLER \
                                                           : EXCEPTION_CONTINUE_SEARCH) { on_exception; }
#else
#error "Foreign C++ exception containment needs MSVC structured exception handling"
#endif

_Static_assert(sizeof(rsf_streamline_graphics) == 48, "Streamline graphics size");
_Static_assert(sizeof(DXGI_SWAP_CHAIN_DESC) == 72 && sizeof(DXGI_SWAP_CHAIN_DESC1) == 48 &&
    sizeof(DXGI_SWAP_CHAIN_FULLSCREEN_DESC) == 20, "DXGI description sizes");
_Static_assert(RSF_D3D11_DISPATCH_RELEASE == 1 && sizeof(rsf_d3d11_chain_dispatch) == 72 &&
    offsetof(rsf_d3d11_chain_dispatch, receiver) == 16 &&
    offsetof(rsf_d3d11_chain_dispatch, result) == 64, "Chain dispatch layout");
_Static_assert(sizeof(rsf_d3d11p_request) == 48 && offsetof(rsf_d3d11p_request, refresh_denominator) == 44,
    "Request layout");
_Static_assert(sizeof(rsf_d3d11p_facts) == 112 && offsetof(rsf_d3d11p_facts, graphics) == 40 &&
    offsetof(rsf_d3d11p_facts, direct_queue) == 88 && offsetof(rsf_d3d11p_facts, device11) == 96 &&
    offsetof(rsf_d3d11p_facts, buffer_usage) == 104, "Facts layout");
_Static_assert(RSF_BACKEND_OK == 0 && RSF_BACKEND_ERROR_FEATURE_FAILED == -9, "Backend results");
_Static_assert(sizeof(rsf_d3d11p_plain_plan) == 24 && offsetof(rsf_d3d11p_plain_plan, swap_effect) == 8,
    "Plain plan layout");
_Static_assert(sizeof(rsf_d3d11p_adapter) == 16 && sizeof(rsf_d3d11p_device_adapter_info) == 12,
    "Adapter layouts");
_Static_assert(sizeof(rsf_d3d11p_platform) == 72 * sizeof(void*), "Platform table size");
_Static_assert(offsetof(rsf_d3d11p_platform, bridge_new) == 15 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, init_source) == 19 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, create_interop_queue) == 29 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, close_event) == 33 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, shared_surface_platform) == 36 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, create_plain) == 38 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, join) == 44 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, acquire_backbuffer) == 47 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, query_buffer_interface) == 56 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, signal_wait) == 59 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, set_description) == 62 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, create_retirement_fence) == 63 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, release_direct_graphics) == 67 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, release_host) == 68 * sizeof(void*) &&
    offsetof(rsf_d3d11p_platform, verify_outer) == 71 * sizeof(void*), "Platform slot offsets");
_Static_assert(sizeof(rsf_d3d11_present_status) == 280 && sizeof(rsf_observer_present_event) == 32,
    "Status and observer layouts");
_Static_assert(DXGI_FORMAT_R8G8B8A8_UNORM == 28 && DXGI_FORMAT_B8G8R8A8_UNORM == 87 &&
    DXGI_FORMAT_R10G10B10A2_UNORM == 24 && DXGI_FORMAT_R16G16B16A16_FLOAT == 10, "DXGI formats");
_Static_assert(DXGI_PRESENT_TEST == 1 && DXGI_PRESENT_ALLOW_TEARING == 0x200 &&
    DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT == 64 && DXGI_USAGE_UNORDERED_ACCESS == 0x400,
    "DXGI present flags");
_Static_assert(DXGI_SWAP_EFFECT_FLIP_DISCARD == 4 && DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING == 2048 &&
    DXGI_USAGE_RENDER_TARGET_OUTPUT == 0x20, "DXGI swap constants");
_Static_assert(RSF_FG_BACKEND_DLSS == 1 && RSF_FG_BACKEND_FSR3 == 3 && RSF_FG_BACKEND_FSR4 == 4 &&
    RSF_FG_BACKEND_XESS == 5 && RSF_FG_BACKEND_AUTO == 7 && RSF_PACING_ANTILAG2 == 3 &&
    RSF_LATENCY_INPUT_SAMPLE == 6 && RSF_UI_MODE_NONE == 1, "Generation constants");

uint32_t rsf_d3d11p_native_window_valid(void* window);
uint32_t rsf_d3d11p_native_client_size(void* window, uint32_t* width, uint32_t* height);
void* rsf_d3d11p_native_canonical_identity(void* receiver);
void rsf_d3d11p_native_add_ref(void* object);
void rsf_d3d11p_native_release(void* object);
void* rsf_d3d11p_native_bridge_new(rsf_d3d11p_chain* chain);
void rsf_d3d11p_native_bridge_release_owned(void* facade);
void rsf_d3d11p_native_facts(void* facade, rsf_d3d11p_facts* out);
uint32_t rsf_d3d11p_native_init_source(void* facade, void* factory, void* source, const void* desc);
int32_t rsf_d3d11p_native_queue_device(void* facade);
int32_t rsf_d3d11p_native_owning_adapter(void* facade, void* source, rsf_d3d11p_adapter* out);
uint32_t rsf_d3d11p_native_adopt_host(void* facade, uint32_t identity, int32_t* result);
int32_t rsf_d3d11p_native_d3d11_device(void* facade, void* source);
int32_t rsf_d3d11p_native_create_host(void* facade);
void* rsf_d3d11p_native_host(void* facade);
int32_t rsf_d3d11p_native_resolve_graphics(void* facade);
uint32_t rsf_d3d11p_native_tearing_supported(void* facade);
uint32_t rsf_d3d11p_native_fsr4_supported(void* facade, const char* directory);
int32_t rsf_d3d11p_native_create_upload_fence(void* facade);
int32_t rsf_d3d11p_native_create_slot(void* facade, void** allocator, void** list);
int32_t rsf_d3d11p_native_create_render_buffer(void* facade, uint32_t width, uint32_t height, uint32_t format, void** out);
int32_t rsf_d3d11p_native_create_plain(void* facade, const rsf_d3d11p_plain_plan* plan, void** physical, void** queue);
int32_t rsf_d3d11p_native_resize_physical(void* facade, void* physical, uint32_t count, uint32_t width, uint32_t height, uint32_t format);
int32_t rsf_d3d11p_native_upload_wait(void* facade, void* queue, uint64_t value);
void rsf_d3d11p_native_record_copy(void* facade, void* commands, void* backbuffer, void* source);
void* rsf_d3d11p_native_context11(void* facade);
int32_t rsf_d3d11p_native_upload_return(void* facade, void* queue, uint64_t value);
void rsf_d3d11p_native_report_device_failure(void* facade, int32_t result, uint32_t backend);
void rsf_d3d11p_native_log_present_failure(void* facade, const char* step, int32_t result, uint32_t render_index);
void rsf_d3d11p_native_log_buffer_refusal(void* facade, uint32_t index, const void* requested, int32_t result, uint32_t pending);
void rsf_d3d11p_native_set_description(void* facade, uint32_t count, uint32_t width, uint32_t height, uint32_t format, uint32_t flags);
int32_t rsf_d3d11p_native_create_retirement_fence(void* facade, void** out);
int32_t rsf_d3d11p_native_retire_submit(void* facade, void* queue, void* retirement_fence, void* vendor_fence, uint64_t vendor_value, void* list, uint64_t value);
void rsf_d3d11p_native_retain_all(void* facade);
void rsf_d3d11p_native_release_presentation(void* facade);
void rsf_d3d11p_native_release_direct_graphics(void* facade);
void rsf_d3d11p_native_release_host(void* facade);
void* rsf_d3d11p_native_source_queue(void* facade, void* queue);
void* rsf_d3d11p_native_command_list(void* facade, void* queue, void* list);
rsf_d3d11p_chain* rsf_d3d11p_facade_chain(void*);
int32_t rsf_d3d11p_native_query_interface(void*, const void*, void**);
int32_t rsf_d3d11p_native_query_chain4(void*, void**);
int32_t rsf_d3d11p_native_get_fullscreen(void*, int32_t*);
int32_t rsf_d3d11p_native_set_fullscreen(void*, int32_t, void*);
int32_t rsf_d3d11p_native_present(void*, uint32_t, uint32_t);
int32_t rsf_d3d11p_native_get_buffer(void*, uint32_t, const void*, void**);
int32_t rsf_d3d11p_native_acquire_backbuffer(void*, void**);
int32_t rsf_d3d11p_factory_wrap_native(const rsf_d3d11p_platform*, uint32_t, void*, void*, void**);
void rsf_d3d11p_dispatch_chain(rsf_d3d11_chain_dispatch*);
int32_t rsf_d3d11p_dispatch_present(void*, uint32_t, uint32_t, int32_t*);

typedef HRESULT(STDMETHODCALLTYPE* rsf_d3d11p_create_fn)(IDXGIFactory*, IUnknown*, DXGI_SWAP_CHAIN_DESC*, IDXGISwapChain**);
typedef HRESULT(STDMETHODCALLTYPE* rsf_d3d11p_create_hwnd_fn)(IDXGIFactory2*, IUnknown*, HWND,
    const DXGI_SWAP_CHAIN_DESC1*, const DXGI_SWAP_CHAIN_FULLSCREEN_DESC*, IDXGIOutput*, IDXGISwapChain1**);
typedef int32_t (*rsf_d3d11p_create_original_fn)(void*, void*, DXGI_SWAP_CHAIN_DESC*, void**);

/* SDK callbacks and the guarded original factory trampoline. */
static const rsf_generation_provider* leaf_provider_table(uint32_t family)
{
    switch (family) {
    case RSF_D3D11P_FAMILY_DLSS: return rsf_generation_dlss();
    case RSF_D3D11P_FAMILY_FIDELITYFX: return rsf_generation_fsr();
    case RSF_D3D11P_FAMILY_XESS: return rsf_generation_xess();
    default: return NULL;
    }
}
static void leaf_observer_present(rsf_observer_present_event* event) { (void)rsf_observer_notify_application_present(event); }
static int32_t leaf_host_sleep(void* host, uint64_t id) { return rsf_streamline_host_sleep((rsf_streamline_host*)host, id); }
static int32_t leaf_host_marker(void* host, uint64_t id, uint32_t marker, uint32_t controller)
{
    return rsf_streamline_host_marker((rsf_streamline_host*)host, id, marker, controller);
}
static int32_t leaf_host_acquire(void* host, uint64_t id) { return rsf_streamline_host_acquire((rsf_streamline_host*)host, id); }
static int32_t leaf_host_input(void* host, uint64_t id, uint32_t kinds, uint32_t message)
{
    return rsf_streamline_host_input((rsf_streamline_host*)host, id, kinds, message);
}
static int32_t leaf_host_abort(void* host, uint64_t id) { return rsf_streamline_host_abort((rsf_streamline_host*)host, id); }
static int32_t leaf_host_reflex_off(void* host) { return rsf_streamline_host_reflex((rsf_streamline_host*)host, RSF_REFLEX_OFF, 0); }
static int32_t leaf_host_generation_load(void* host, uint32_t enabled)
{
    return rsf_streamline_host_generation_load((rsf_streamline_host*)host, enabled);
}
static int32_t leaf_host_present_common(void* host, void* chain, uint32_t sync, uint32_t flags, uint32_t begin)
{
    return rsf_streamline_host_present_common((rsf_streamline_host*)host, chain, sync, flags, begin);
}

static int32_t leaf_create_original(void* factory, void* queue, DXGI_SWAP_CHAIN_DESC* desc, void** out)
{
    RSF_FOREIGN_TRY {
        const rsf_d3d11p_create_fn original = (rsf_d3d11p_create_fn)rsf_d3d11p_factory_legacy_original();
        return original ? original((IDXGIFactory*)factory, (IUnknown*)queue, desc, (IDXGISwapChain**)out) : E_FAIL;
    } RSF_FOREIGN_CATCH(return E_FAIL)
}

static const rsf_d3d11p_platform platform = {
    .provider_table = leaf_provider_table, .observer_present = leaf_observer_present,
    .window_valid = rsf_d3d11p_native_window_valid, .client_size = rsf_d3d11p_native_client_size, .canonical_identity = rsf_d3d11p_native_canonical_identity,
    .add_ref = rsf_d3d11p_native_add_ref, .release = rsf_d3d11p_native_release,
    .host_sleep = leaf_host_sleep, .host_marker = leaf_host_marker, .host_acquire = leaf_host_acquire,
    .host_input = leaf_host_input, .host_abort = leaf_host_abort, .host_reflex_off = leaf_host_reflex_off,
    .host_generation_load = leaf_host_generation_load, .host_present_common = leaf_host_present_common,
    .bridge_new = rsf_d3d11p_native_bridge_new, .bridge_release_owned = rsf_d3d11p_native_bridge_release_owned,
    .bridge_chain = rsf_d3d11p_facade_chain, .facts = rsf_d3d11p_native_facts,
    .init_source = rsf_d3d11p_native_init_source, .owning_adapter = rsf_d3d11p_native_owning_adapter, .native_device = rsf_d3d11p_native_queue_device,
    .adopt_host = rsf_d3d11p_native_adopt_host, .d3d11_device = rsf_d3d11p_native_d3d11_device, .create_host = rsf_d3d11p_native_create_host,
    .host = rsf_d3d11p_native_host, .resolve_graphics = rsf_d3d11p_native_resolve_graphics, .tearing_supported = rsf_d3d11p_native_tearing_supported,
    .fsr4_supported = rsf_d3d11p_native_fsr4_supported, .create_interop_queue = rsf_d3d11p_gpu_queue,
    .create_completion_fence = rsf_d3d11p_gpu_fence, .create_event = rsf_d3d11p_gpu_create_event,
    .duplicate_event = rsf_d3d11p_gpu_duplicate_event, .close_event = rsf_d3d11p_gpu_close_event,
    .create_upload_fence = rsf_d3d11p_native_create_upload_fence, .create_slot = rsf_d3d11p_native_create_slot,
    .shared_surface_platform = rsf_shared_surface_native_platform, .create_render_buffer = rsf_d3d11p_native_create_render_buffer,
    .create_plain = rsf_d3d11p_native_create_plain, .query_chain4 = rsf_d3d11p_native_query_chain4, .get_fullscreen = rsf_d3d11p_native_get_fullscreen,
    .set_fullscreen = rsf_d3d11p_native_set_fullscreen,
    .physical_present = rsf_d3d11p_native_present, .resize_physical = rsf_d3d11p_native_resize_physical,
    .join = rsf_d3d11p_gpu_join,
    .slot_reset = rsf_d3d11p_gpu_reset_slot, .upload_wait = rsf_d3d11p_native_upload_wait, .acquire_backbuffer = rsf_d3d11p_native_acquire_backbuffer,
    .record_copy = rsf_d3d11p_native_record_copy, .context11 = rsf_d3d11p_native_context11,
    .close_execute = rsf_d3d11p_gpu_close_execute, .upload_return = rsf_d3d11p_native_upload_return, .signal_slot = rsf_d3d11p_gpu_signal,
    .application_ready = rsf_d3d11p_gpu_application_ready, .report_device_failure = rsf_d3d11p_native_report_device_failure,
    .log_present_failure = rsf_d3d11p_native_log_present_failure,
    .query_buffer_interface = rsf_d3d11p_native_query_interface, .get_physical_buffer = rsf_d3d11p_native_get_buffer,
    .log_buffer_refusal = rsf_d3d11p_native_log_buffer_refusal,
    .signal_wait = rsf_d3d11p_gpu_signal_wait, .wait_fence = rsf_d3d11p_gpu_wait,
    .buffer_referenced = rsf_d3d11p_gpu_buffer_referenced, .set_description = rsf_d3d11p_native_set_description,
    .create_retirement_fence = rsf_d3d11p_native_create_retirement_fence, .retire_submit = rsf_d3d11p_native_retire_submit, .retain_all = rsf_d3d11p_native_retain_all,
    .release_presentation = rsf_d3d11p_native_release_presentation,
    .release_direct_graphics = rsf_d3d11p_native_release_direct_graphics,
    .release_host = rsf_d3d11p_native_release_host,
    .source_queue = rsf_d3d11p_native_source_queue,
    .command_list = rsf_d3d11p_native_command_list, .verify_outer = rsf_d3d11p_device_verify_outer,
};

static HRESULT STDMETHODCALLTYPE create_hook(IDXGIFactory* factory, IUnknown* device, DXGI_SWAP_CHAIN_DESC* desc, IDXGISwapChain** out)
{
    if (rsf_d3d11p_factory_hooks_ready() &&
        rsf_d3d11p_factory_intercept(&platform, factory, device, desc, (void**)out)) return S_OK;
    const rsf_d3d11p_create_fn original = (rsf_d3d11p_create_fn)rsf_d3d11p_factory_legacy_original();
    return original ? original(factory, device, desc, out) : E_FAIL;
}
static HRESULT STDMETHODCALLTYPE hwnd_hook(IDXGIFactory2* factory, IUnknown* device, HWND hwnd,
    const DXGI_SWAP_CHAIN_DESC1* desc, const DXGI_SWAP_CHAIN_FULLSCREEN_DESC* fullscreen, IDXGIOutput* output, IDXGISwapChain1** out)
{
    int32_t result = E_FAIL;
    if (rsf_d3d11p_factory_hooks_ready() &&
        rsf_d3d11p_factory_intercept_hwnd(&platform, factory, device, hwnd, desc, fullscreen, output,
            (void**)out, &result)) return result;
    const rsf_d3d11p_create_hwnd_fn original = (rsf_d3d11p_create_hwnd_fn)rsf_d3d11p_factory_hwnd_original();
    return original ? original(factory, device, hwnd, desc, fullscreen, output, out) : E_FAIL;
}

/* IID_IDXGIFactory2, spelled here so the C leaf needs no dxguid import. */
static const GUID factory2_iid = {0x50c83a1c, 0xe072, 0x4c48, {0x87, 0xb0, 0x36, 0x30, 0xfa, 0x36, 0xa6, 0xd0}};

static void* bootstrap_factory(void)
{
    RSF_FOREIGN_TRY {
        IDXGIFactory2* factory = NULL;
        if (FAILED(CreateDXGIFactory1(&factory2_iid, (void**)&factory))) return NULL;
        return factory;
    } RSF_FOREIGN_CATCH(return NULL)
}
static uint32_t initialize_hooks(void)
{
    const MH_STATUS initialized = MH_Initialize();
    return initialized == MH_OK || initialized == MH_ERROR_ALREADY_INITIALIZED;
}
static uint32_t create_factory_hook(void* target, void* detour, void** original)
{
    return MH_CreateHook(target, detour, original) == MH_OK;
}
static uint32_t enable_factory_hook(void* target) { return MH_EnableHook(target) == MH_OK; }
static void disable_factory_hook(void* target) { (void)MH_DisableHook(target); }
static uint32_t remove_factory_hook(void* target)
{
    const MH_STATUS removed = MH_RemoveHook(target);
    return removed == MH_OK || removed == MH_ERROR_NOT_CREATED || removed == MH_ERROR_NOT_INITIALIZED;
}
static const rsf_d3d11p_hook_calls hook_calls = {
    bootstrap_factory, initialize_hooks, create_factory_hook,
    enable_factory_hook, disable_factory_hook, remove_factory_hook,
};
_Static_assert(sizeof(rsf_d3d11p_hook_calls) == 6 * sizeof(void*), "Hook call table size");

uint32_t rsf_d3d11p_default_backend(uint32_t vendor, uint32_t software)
{
    return rsf_fg_default_backend(vendor, software);
}

rsf_d3d11p_create_original_fn rsf_d3d11p_original_create(void)
{
    return rsf_d3d11p_factory_legacy_original() ? leaf_create_original : NULL;
}

void* rsf_d3d11p_unwrap_guarded(void* host, void* proxy)
{
    RSF_FOREIGN_TRY {
        return rsf_streamline_host_native((rsf_streamline_host*)host, proxy);
    } RSF_FOREIGN_CATCH(return NULL)
}

int32_t rsf_d3d11p_streamline_graphics(void* host, rsf_streamline_graphics* out)
{
    return rsf_streamline_host_graphics((rsf_streamline_host*)host, out);
}

const rsf_generation_provider* rsf_d3d11p_provider_for(uint32_t backend)
{
    return leaf_provider_table(rsf_core_fg_provider_family(backend));
}
int rsf_d3d11_present_prepare(const rsf_d3d11_present_setup* setup)
{
    return rsf_core_d3d11p_prepare(setup, rsf_d3d11p_factory_hooks_block_explicit());
}
int32_t rsf_d3d11_present_create_for_hwnd(void* factory, void* queue, void* hwnd,
    const void* description, void** out)
{
    RSF_FOREIGN_TRY {
        return rsf_d3d11p_factory_create_hwnd(&platform, factory, queue, hwnd,
            (const DXGI_SWAP_CHAIN_DESC1*)description, out);
    } RSF_FOREIGN_CATCH(if (out) *out = NULL; return E_FAIL)
}
int32_t rsf_d3d11_present_wrap_native_chain(void* raw_device, void* raw_chain, void** out)
{
    RSF_FOREIGN_TRY {
        return rsf_d3d11p_factory_wrap_native(&platform, rsf_d3d11p_factory_hooks_block_explicit(), raw_device, raw_chain, out);
    } RSF_FOREIGN_CATCH(return E_FAIL)
}
int rsf_d3d11_present_install(const rsf_d3d11_present_setup* setup)
{
    return rsf_d3d11p_factory_install(setup, 0, &hook_calls, (void*)&create_hook, (void*)&hwnd_hook);
}
int rsf_d3d11_present_install_scoped(const rsf_d3d11_present_setup* setup)
{
    return rsf_d3d11p_factory_install(setup, 1, &hook_calls, (void*)&create_hook, (void*)&hwnd_hook);
}
int32_t rsf_d3d11_present_creation_begin(void* window, uint64_t* token)
{
    return rsf_core_d3d11p_creation_begin(&platform, window, token, rsf_d3d11p_factory_hooks_ready());
}
int32_t rsf_d3d11_present_creation_end(void* window, uint64_t token, void* device, void* context, void* outer)
{
    return rsf_core_d3d11p_creation_end(&platform, window, token, device, context, outer);
}
rsf_streamline_host* rsf_d3d11_present_host(void) { return (rsf_streamline_host*)rsf_core_d3d11p_host(); }
int rsf_d3d11_present_graphics(rsf_streamline_graphics* graphics) { return rsf_core_d3d11p_graphics(graphics); }
void* rsf_d3d11_present_queue(void) { return rsf_core_d3d11p_queue(&platform); }
void* rsf_d3d11_present_source_queue(void) { return rsf_core_d3d11p_source_queue(&platform); }
int rsf_d3d11_present_is_owner(void* swapchain) { return rsf_core_d3d11p_is_owner(&platform, swapchain); }
void rsf_d3d11_chain_dispatch_owned(rsf_d3d11_chain_dispatch* request)
{
    rsf_d3d11p_dispatch_chain(request);
}
int rsf_d3d11_present_dispatch_owned(void* receiver, uint32_t sync, uint32_t flags, int32_t* result)
{
    return rsf_d3d11p_dispatch_present(receiver, sync, flags, result);
}
int rsf_d3d11_present_has_owner(void) { return rsf_core_d3d11p_has_owner(); }
const rsf_generation_provider* rsf_d3d11_present_provider(void) { return rsf_core_d3d11p_provider(&platform); }
int32_t rsf_d3d11_present_status_get(rsf_d3d11_present_status* status) { return rsf_core_d3d11p_status_get(status); }
void* rsf_d3d11_present_session(void) { return rsf_core_d3d11p_session(); }
uint32_t rsf_d3d11_present_backend(void) { return rsf_core_d3d11p_backend(); }
uint32_t rsf_d3d11_present_backend_choices(void) { return rsf_core_d3d11p_backend_choices(); }
int32_t rsf_d3d11_present_request(uint32_t backend) { return rsf_core_d3d11p_request(backend); }
int32_t rsf_d3d11_present_switch_result(void) { return rsf_core_d3d11p_switch_result(); }
uint64_t rsf_d3d11_present_generation(void) { return rsf_core_d3d11p_generation(); }
int32_t rsf_d3d11_present_retire(void* session, void* list, void** fence, uint64_t* value)
{
    return rsf_core_d3d11p_retire(&platform, session, list, fence, value);
}
void* rsf_d3d11_present_command_list(void* list) { return rsf_core_d3d11p_command_list(&platform, list); }
int32_t rsf_d3d11_present_begin(uint64_t id, uint64_t* generation) { return rsf_core_d3d11p_begin(&platform, id, generation); }
int32_t rsf_d3d11_present_pace(uint64_t id, rsf_pacing_owner owner, uint64_t* generation)
{
    return rsf_core_d3d11p_pace(&platform, id, owner, NULL, generation);
}
int32_t rsf_d3d11_present_pace_for_generation(uint64_t id, rsf_pacing_owner owner, uint64_t generation)
{
    return rsf_core_d3d11p_pace(&platform, id, owner, &generation, NULL);
}
int32_t rsf_d3d11_present_marker(uint64_t generation, uint64_t id, rsf_latency_marker marker, uint32_t controller)
{
    return rsf_core_d3d11p_marker(&platform, generation, id, marker, controller);
}
int32_t rsf_d3d11_present_acquire(uint64_t id) { return rsf_core_d3d11p_acquire(&platform, id); }
int32_t rsf_d3d11_present_receipt(uint64_t generation, uint64_t id, uint32_t kinds, uint32_t message)
{
    return rsf_core_d3d11p_receipt(&platform, generation, id, kinds, message);
}
int32_t rsf_d3d11_present_input(uint64_t id, uint32_t kinds, uint32_t message)
{
    return rsf_core_d3d11p_input(&platform, id, kinds, message);
}
int32_t rsf_d3d11_present_abort(uint64_t id) { return rsf_core_d3d11p_abort(&platform, id); }
