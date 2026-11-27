/* SPDX-License-Identifier: MIT */
/* Explicit device regression, never registered in CTest. Scratch SDK root only.
   Hidden-window callbacks are not proof of physical scanout or gameplay quality.
   Exit 77 means prerequisites/hidden presentation prevented a generation receipt;
   exit 1 means a contract failure; exit 0 requires observed FSR generated callbacks.
   C11 launcher; the Rust fixture owns the phases and verdicts. */
#include "d3d11_host_device_launch.h"
#include <rescaleframe/d3d11_host.h>
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/d3d11_present_api.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_window.h>
#include <rescaleframe/overlay_api.h>
#include <d3d11sdklayers.h>
#include <d3d12.h>
#include <dxgidebug.h>
#include <stdlib.h>

/* A forwarding outer wrapper with native canonical QI and real pre/post Present
   work. The engine retains this pointer; no test rewrites it to the inner owner. */
typedef struct OuterChain {
    const IDXGISwapChainVtbl* lpVtbl;
    IDXGISwapChain* inner;
    ULONG refs;
    uint32_t pre, post, dispatched;
    void* host_user;
} OuterChain;

static HRESULT STDMETHODCALLTYPE outer_query_interface(IDXGISwapChain* self, REFIID iid, void** out)
{
    return IDXGISwapChain_QueryInterface(((OuterChain*)self)->inner, iid, out);
}
static ULONG STDMETHODCALLTYPE outer_add_ref(IDXGISwapChain* self) { return ++((OuterChain*)self)->refs; }
static ULONG STDMETHODCALLTYPE outer_release(IDXGISwapChain* self)
{
    OuterChain* outer = (OuterChain*)self;
    const ULONG remaining = --outer->refs;
    if (!remaining) {
        IDXGISwapChain_Release(outer->inner);
        free(outer);
    }
    return remaining;
}
static HRESULT STDMETHODCALLTYPE outer_set_private_data(IDXGISwapChain* self, REFGUID key, UINT size, const void* data)
{
    return IDXGISwapChain_SetPrivateData(((OuterChain*)self)->inner, key, size, data);
}
static HRESULT STDMETHODCALLTYPE outer_set_private_data_interface(IDXGISwapChain* self, REFGUID key, const IUnknown* value)
{
    return IDXGISwapChain_SetPrivateDataInterface(((OuterChain*)self)->inner, key, value);
}
static HRESULT STDMETHODCALLTYPE outer_get_private_data(IDXGISwapChain* self, REFGUID key, UINT* size, void* data)
{
    return IDXGISwapChain_GetPrivateData(((OuterChain*)self)->inner, key, size, data);
}
static HRESULT STDMETHODCALLTYPE outer_get_parent(IDXGISwapChain* self, REFIID iid, void** out)
{
    return IDXGISwapChain_GetParent(((OuterChain*)self)->inner, iid, out);
}
static HRESULT STDMETHODCALLTYPE outer_get_device(IDXGISwapChain* self, REFIID iid, void** out)
{
    return IDXGISwapChain_GetDevice(((OuterChain*)self)->inner, iid, out);
}
static HRESULT STDMETHODCALLTYPE outer_present(IDXGISwapChain* self, UINT sync, UINT flags)
{
    OuterChain* outer = (OuterChain*)self;
    HRESULT result;
    int32_t status;
    /* Model a continuation reached after external pre-work, without reentering
       the facade's virtual slot. Never fall through after a service error. */
    rsf_d3d11_present_dispatch request = {sizeof(request), RSF_D3D11_HOST_ABI_VERSION, outer->inner, sync, flags, 0, E_FAIL};
    ++outer->pre;
    status = rsf_d3d11_host_dispatch_present(outer->host_user, &request);
    result = status != RSF_OK ? E_FAIL : request.handled ? request.native_result : IDXGISwapChain_Present(outer->inner, sync, flags);
    if (status == RSF_OK && request.handled) ++outer->dispatched;
    ++outer->post;
    return result;
}
static HRESULT STDMETHODCALLTYPE outer_get_buffer(IDXGISwapChain* self, UINT index, REFIID iid, void** out)
{
    return IDXGISwapChain_GetBuffer(((OuterChain*)self)->inner, index, iid, out);
}
static HRESULT STDMETHODCALLTYPE outer_set_fullscreen_state(IDXGISwapChain* self, BOOL enabled, IDXGIOutput* output)
{
    return IDXGISwapChain_SetFullscreenState(((OuterChain*)self)->inner, enabled, output);
}
static HRESULT STDMETHODCALLTYPE outer_get_fullscreen_state(IDXGISwapChain* self, BOOL* enabled, IDXGIOutput** output)
{
    return IDXGISwapChain_GetFullscreenState(((OuterChain*)self)->inner, enabled, output);
}
static HRESULT STDMETHODCALLTYPE outer_get_desc(IDXGISwapChain* self, DXGI_SWAP_CHAIN_DESC* out)
{
    return IDXGISwapChain_GetDesc(((OuterChain*)self)->inner, out);
}
static HRESULT STDMETHODCALLTYPE outer_resize_buffers(IDXGISwapChain* self, UINT n, UINT w, UINT h, DXGI_FORMAT format, UINT flags)
{
    return IDXGISwapChain_ResizeBuffers(((OuterChain*)self)->inner, n, w, h, format, flags);
}
static HRESULT STDMETHODCALLTYPE outer_resize_target(IDXGISwapChain* self, const DXGI_MODE_DESC* desc)
{
    return IDXGISwapChain_ResizeTarget(((OuterChain*)self)->inner, desc);
}
static HRESULT STDMETHODCALLTYPE outer_get_containing_output(IDXGISwapChain* self, IDXGIOutput** out)
{
    return IDXGISwapChain_GetContainingOutput(((OuterChain*)self)->inner, out);
}
static HRESULT STDMETHODCALLTYPE outer_get_frame_statistics(IDXGISwapChain* self, DXGI_FRAME_STATISTICS* out)
{
    return IDXGISwapChain_GetFrameStatistics(((OuterChain*)self)->inner, out);
}
static HRESULT STDMETHODCALLTYPE outer_get_last_present_count(IDXGISwapChain* self, UINT* out)
{
    return IDXGISwapChain_GetLastPresentCount(((OuterChain*)self)->inner, out);
}

static const IDXGISwapChainVtbl outer_vtable = {
    outer_query_interface, outer_add_ref, outer_release,
    outer_set_private_data, outer_set_private_data_interface, outer_get_private_data, outer_get_parent,
    outer_get_device,
    outer_present, outer_get_buffer, outer_set_fullscreen_state, outer_get_fullscreen_state, outer_get_desc,
    outer_resize_buffers, outer_resize_target, outer_get_containing_output, outer_get_frame_statistics,
    outer_get_last_present_count,
};

static HWND window;
static const GUID diagnostic_all = {0xe48ae283, 0xda80, 0x490b, {0x87, 0xe6, 0x43, 0xe9, 0xa9, 0xcf, 0xda, 0x08}};

static int accept_window(void* user, void* candidate)
{
    (void)user;
    return candidate == (void*)window;
}
static void close_window(void) { DestroyWindow(window); }
static int fail(const char* label)
{
    printf("FAIL: %s\n", label);
    return 1;
}

static void dxgi_errors(IDXGIInfoQueue* queue)
{
    UINT64 count, i;
    unsigned printed = 0;
    if (!queue) return;
    count = IDXGIInfoQueue_GetNumStoredMessagesAllowedByRetrievalFilters(queue, diagnostic_all);
    printf("DXGI stored messages=%llu\n", (unsigned long long)count);
    for (i = 0; i < count && printed < 16; ++i) {
        SIZE_T size = 0;
        DXGI_INFO_QUEUE_MESSAGE* message;
        if (FAILED(IDXGIInfoQueue_GetMessage(queue, diagnostic_all, i, NULL, &size)) || !size) continue;
        message = (DXGI_INFO_QUEUE_MESSAGE*)malloc(size);
        if (!message) continue;
        if (SUCCEEDED(IDXGIInfoQueue_GetMessage(queue, diagnostic_all, i, message, &size)) &&
            message->Severity <= DXGI_INFO_QUEUE_MESSAGE_SEVERITY_WARNING) {
            printf("DXGI message severity=%u id=%d: %s\n", (unsigned)message->Severity, message->ID, message->pDescription);
            ++printed;
        }
        free(message);
    }
}

typedef struct NativeCalls {
    const rsf_d3d11_present_api* presentation;
    const OuterChain* outer;
    const rsf_overlay_api* overlay;
} NativeCalls;

static int32_t fg_get_buffer(void* receiver, void** out)
{
    RSF_FOREIGN_TRY {
        ID3D11Texture2D* texture = NULL;
        const HRESULT result = IDXGISwapChain_GetBuffer((IDXGISwapChain*)receiver, 0, &IID_ID3D11Texture2D, (void**)&texture);
        if (SUCCEEDED(result)) *out = texture;
        return result;
    } RSF_FOREIGN_CATCH { return E_FAIL; }
}
static int32_t fg_present(void* receiver)
{
    RSF_FOREIGN_TRY {
        return IDXGISwapChain_Present((IDXGISwapChain*)receiver, 0, 0);
    } RSF_FOREIGN_CATCH { return E_FAIL; }
}
static int32_t fg_has_provider_session(void* user)
{
    RSF_FOREIGN_TRY {
        const rsf_d3d11_present_api* api = ((const NativeCalls*)user)->presentation;
        return api->provider() && api->session();
    } RSF_FOREIGN_CATCH { return -1; }
}
static uint32_t fg_backend(void* user)
{
    RSF_FOREIGN_TRY {
        return ((const NativeCalls*)user)->presentation->backend();
    } RSF_FOREIGN_CATCH { return UINT32_MAX; }
}
static void fg_outer_counts(void* user, uint32_t* pre, uint32_t* post, uint32_t* dispatched)
{
    const OuterChain* chain = ((const NativeCalls*)user)->outer;
    *pre = chain->pre;
    *post = chain->post;
    *dispatched = chain->dispatched;
}
static int32_t fg_pump_messages(void)
{
    RSF_FOREIGN_TRY {
        MSG message;
        while (PeekMessageW(&message, NULL, 0, 0, PM_REMOVE)) {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        return RSF_OK;
    } RSF_FOREIGN_CATCH { return RSF_ERROR_NOT_READY; }
}
static int32_t fg_settings_get(void* user, rsf_render_settings* settings)
{
    RSF_FOREIGN_TRY {
        return ((const NativeCalls*)user)->overlay->settings_get(settings);
    } RSF_FOREIGN_CATCH { return RSF_ERROR_NOT_READY; }
}
static int32_t fg_settings_apply(void* user, const rsf_render_settings* settings)
{
    RSF_FOREIGN_TRY {
        return ((const NativeCalls*)user)->overlay->settings_apply(settings);
    } RSF_FOREIGN_CATCH { return RSF_ERROR_NOT_READY; }
}
static int32_t fg_switch_result(void* user, int32_t* result)
{
    RSF_FOREIGN_TRY {
        *result = ((const NativeCalls*)user)->presentation->switch_result();
        return RSF_OK;
    } RSF_FOREIGN_CATCH { return RSF_ERROR_NOT_READY; }
}
static int32_t fg_generation(void* user, uint64_t* generation)
{
    RSF_FOREIGN_TRY {
        *generation = ((const NativeCalls*)user)->presentation->generation();
        return RSF_OK;
    } RSF_FOREIGN_CATCH { return RSF_ERROR_NOT_READY; }
}

/* R11G11B10_FLOAT channels for the two fixture pixels: unsigned floats with a 5-bit exponent
   (bias 15) and a 6-bit mantissa for red and green, 5 bits for blue, as XMFLOAT3PK packs them. */
#define PK11(exponent, mantissa) (((uint32_t)(exponent) << 6) | (uint32_t)(mantissa))
#define PK10(exponent, mantissa) (((uint32_t)(exponent) << 5) | (uint32_t)(mantissa))
#define PK_R11G11B10(r, g, b) ((r) | ((g) << 11) | ((b) << 22))
/* (0.25, 0.5, 0.75): 2^-2, 2^-1, 1.5 * 2^-1 */
#define NATIVE_PIXEL PK_R11G11B10(PK11(13, 0), PK11(14, 0), PK10(14, 16))
/* (3, 0, 3): 1.5 * 2^1, zero, 1.5 * 2^1 */
#define OUTSIDE_MARKER PK_R11G11B10(PK11(16, 32), PK11(0, 0), PK10(16, 16))

typedef struct Fixture {
    IDXGIInfoQueue* dxgi_diagnostics;
    IDXGIFactory1* factory;
    IDXGIAdapter1* adapter;
    ID3D12Device* native_device;
    ID3D12CommandQueue* native_queue;
    ID3D11Device* device;
    ID3D11DeviceContext* context;
    IDXGISwapChain* chain;
    ID3D11Texture2D* buffer;
    ID3D11UnorderedAccessView* unordered;
    ID3D11InfoQueue* diagnostics;
    rsf_fixture_targets targets;
} Fixture;

static void fixture_release(Fixture* f)
{
    RSF_FIXTURE_RELEASE(ID3D11InfoQueue, f->diagnostics);
    rsf_fixture_targets_release(&f->targets);
    RSF_FIXTURE_RELEASE(ID3D11UnorderedAccessView, f->unordered);
    RSF_FIXTURE_RELEASE(ID3D11Texture2D, f->buffer);
    RSF_FIXTURE_RELEASE(IDXGISwapChain, f->chain);
    RSF_FIXTURE_RELEASE(ID3D11DeviceContext, f->context);
    RSF_FIXTURE_RELEASE(ID3D11Device, f->device);
    RSF_FIXTURE_RELEASE(ID3D12CommandQueue, f->native_queue);
    RSF_FIXTURE_RELEASE(ID3D12Device, f->native_device);
    RSF_FIXTURE_RELEASE(IDXGIAdapter1, f->adapter);
    RSF_FIXTURE_RELEASE(IDXGIFactory1, f->factory);
    RSF_FIXTURE_RELEASE(IDXGIInfoQueue, f->dxgi_diagnostics);
}

/* Before installing RSF interception, isolate the usage variable on the
   same native adapter/queue. Separate HWNDs avoid deferred chain lifetime
   becoming an alternative explanation for the second creation result. */
static HRESULT probe_usage(Fixture* f, const WNDCLASSW* wc, HMODULE module, UINT usage)
{
    DXGI_SWAP_CHAIN_DESC descriptor;
    IDXGISwapChain* physical = NULL;
    HRESULT result;
    const HWND probe = CreateWindowW(wc->lpszClassName, L"RSF private usage probe", WS_OVERLAPPED,
        0, 0, (int)RSF_FIXTURE_WIDTH, (int)RSF_FIXTURE_HEIGHT, NULL, NULL, module, NULL);
    if (!probe) return E_FAIL;
    memset(&descriptor, 0, sizeof(descriptor));
    descriptor.BufferDesc.Width = RSF_FIXTURE_WIDTH;
    descriptor.BufferDesc.Height = RSF_FIXTURE_HEIGHT;
    descriptor.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    descriptor.SampleDesc.Count = 1;
    descriptor.BufferUsage = usage;
    descriptor.BufferCount = 3;
    descriptor.OutputWindow = probe;
    descriptor.Windowed = TRUE;
    descriptor.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    result = IDXGIFactory1_CreateSwapChain(f->factory, (IUnknown*)f->native_queue, &descriptor, &physical);
    printf("Native D3D12 paired creation usage=0x%x HRESULT=0x%08lx\n", usage, (unsigned long)result);
    if (physical) IDXGISwapChain_Release(physical);
    DestroyWindow(probe);
    return result;
}

static int body(int argc, char** argv, Fixture* f, wchar_t* ini)
{
    const int scoped = argc == 3 && strcmp(argv[2], "--scoped") == 0;
    wchar_t root[MAX_PATH], interposer[MAX_PATH], fidelityfx[MAX_PATH], temporary[MAX_PATH];
    HMODULE debug_module, module;
    WNDCLASSW wc;
    rsf_d3d11_host_setup setup;
    rsf_game_host_services host;
    rsf_d3d11_present_api presentation = {sizeof(presentation), RSF_D3D11_PRESENT_API_VERSION};
    rsf_overlay_api overlay = {sizeof(overlay), RSF_OVERLAY_API_VERSION};
    DXGI_SWAP_CHAIN_DESC chain_desc;
    rsf_d3d11_creation_scope scope = {sizeof(scope), RSF_D3D11_HOST_ABI_VERSION, NULL, 0};
    HRESULT created, first_present, application_uav = S_OK;
    OuterChain* outer = NULL;
    rsf_d3d11_presentation_binding binding;
    NativeCalls native_calls;
    rsf_d3d11_fixture_fg_calls calls;
    rsf_d3d11_fixture_resources resources;

    if (argc != 2 && !scoped) {
        puts("Usage: rsf_d3d11_host_rendered_fg_device <scratch runtime root> [--scoped]");
        return 77;
    }
    if (!rsf_fixture_wide_root(argv[1], root, MAX_PATH)) return 77;
    if (!rsf_fixture_join(interposer, MAX_PATH, root, L"streamline\\sl.interposer.dll") ||
        !rsf_fixture_join(fidelityfx, MAX_PATH, root, L"fidelityfx") ||
        !rsf_fixture_exists(interposer) || !rsf_fixture_is_directory(fidelityfx)) return 77;

    /* Optional system diagnostic queue, before the exact production creation path.
       Do not change factory creation flags or substitute a different API for it. */
    debug_module = LoadLibraryExW(L"dxgidebug.dll", NULL, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (debug_module) {
        typedef HRESULT(WINAPI * Query)(UINT, REFIID, void**);
        const Query query = (Query)(void*)GetProcAddress(debug_module, "DXGIGetDebugInterface1");
        if (query) (void)query(0, &IID_IDXGIInfoQueue, (void**)&f->dxgi_diagnostics);
    }
    printf("DXGI info queue available=%u\n", f->dxgi_diagnostics ? 1u : 0u);
    if (f->dxgi_diagnostics) IDXGIInfoQueue_ClearStoredMessages(f->dxgi_diagnostics, diagnostic_all);
    if (FAILED(CreateDXGIFactory1(&IID_IDXGIFactory1, (void**)&f->factory))) return 77;
    f->adapter = rsf_fixture_nvidia_adapter(f->factory);
    if (!f->adapter) return 77;

    module = GetModuleHandleW(NULL);
    memset(&wc, 0, sizeof(wc));
    wc.lpfnWndProc = DefWindowProcW;
    wc.hInstance = module;
    wc.lpszClassName = L"RSFRenderedFGDevice";
    if (!RegisterClassW(&wc)) return fail("register hidden fixture window class");
    window = CreateWindowW(wc.lpszClassName, L"RSF rendered FG device check", WS_OVERLAPPED,
        0, 0, (int)RSF_FIXTURE_WIDTH, (int)RSF_FIXTURE_HEIGHT, NULL, NULL, module, NULL);
    if (!window) return fail("create hidden fixture window"); /* Deliberately never shown or made foreground. */
    if (scoped) {
        D3D12_COMMAND_QUEUE_DESC queue_description;
        HRESULT unordered, control;
        memset(&queue_description, 0, sizeof(queue_description));
        queue_description.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
        if (FAILED(D3D12CreateDevice((IUnknown*)f->adapter, D3D_FEATURE_LEVEL_11_0, &IID_ID3D12Device, (void**)&f->native_device)) ||
            FAILED(ID3D12Device_CreateCommandQueue(f->native_device, &queue_description, &IID_ID3D12CommandQueue,
                (void**)&f->native_queue)))
            return fail("native paired-usage device/queue");
        unordered = probe_usage(f, &wc, module,
            DXGI_USAGE_RENDER_TARGET_OUTPUT | DXGI_USAGE_SHADER_INPUT | DXGI_USAGE_UNORDERED_ACCESS);
        control = probe_usage(f, &wc, module, DXGI_USAGE_RENDER_TARGET_OUTPUT);
        dxgi_errors(f->dxgi_diagnostics);
        if (FAILED(control)) return fail("native paired usage render-target control");
        /* Some runtimes may support the larger usage set. Preserve that result;
           attribution of an invalid call requires the actual paired receipt. */
        if (FAILED(unordered) && unordered != DXGI_ERROR_INVALID_CALL) return fail("unexpected native unordered-usage refusal");
        if (f->dxgi_diagnostics) IDXGIInfoQueue_ClearStoredMessages(f->dxgi_diagnostics, diagnostic_all);
    }
    if (!GetTempPathW(MAX_PATH, temporary) || !GetTempFileNameW(temporary, L"rsf", 0, ini)) return fail("create scratch INI path");
    if (!WritePrivateProfileSectionW(L"Device", L"SR=0\0SRBackend=1\0SRQuality=0\0FG=0\0FGBackend=3\0FGGeneratedFrames=1\0Latency=0\0FrameLimitUs=0\0StatusOverlay=0\0", ini))
        return fail("write scratch settings");

    rsf_fixture_host_setup(&setup, &host, ini, root);
    setup.presentation_bridge = scoped ? 3u : 1u;
    setup.accept_window = accept_window;
    setup.capabilities = RSF_D3D11_HOST_CAP_FG_INPUTS | RSF_D3D11_HOST_CAP_RENDERED_SOURCE;
    if (rsf_d3d11_host_create(&setup, &host) != RSF_OK || rsf_d3d11_host_set_active(host.user, 1) != RSF_OK)
        return fail("create/activate production D3D11 host");
    if (rsf_get_d3d11_present_api(&presentation) != RSF_OK) return fail("query production presentation API");

    rsf_fixture_chain_desc(&chain_desc, window);
    if (scoped) chain_desc.BufferUsage |= DXGI_USAGE_SHADER_INPUT | DXGI_USAGE_UNORDERED_ACCESS;
    scope.window = window;
    if (scoped) {
        rsf_d3d11_creation_scope nested = {sizeof(nested), RSF_D3D11_HOST_ABI_VERSION, window, 0};
        rsf_d3d11_creation_scope wrong_token;
        rsf_d3d11_native_chain malformed;
        if (rsf_d3d11_host_creation_begin(host.user, &scope) != RSF_OK || !scope.token) return fail("begin cancelled scope");
        if (rsf_d3d11_host_creation_begin(host.user, &nested) == RSF_OK || nested.token) return fail("nested scope refused without changing output");
        wrong_token = scope;
        ++wrong_token.token;
        if (rsf_d3d11_host_creation_end(host.user, &wrong_token, NULL) == RSF_OK)
            return fail("wrong-token End cannot close the real owner scope");
        if (rsf_d3d11_host_creation_end(host.user, &scope, NULL) == RSF_OK || presentation.is_owner(NULL))
            return fail("empty scope cancels without admitting an owner");
        scope.token = 0;
        if (rsf_d3d11_host_creation_begin(host.user, &scope) != RSF_OK) return fail("begin malformed-result scope");
        memset(&malformed, 0, sizeof(malformed));
        malformed.struct_size = sizeof(malformed) - 1;
        malformed.abi_version = RSF_D3D11_HOST_ABI_VERSION;
        if (rsf_d3d11_host_creation_end(host.user, &scope, &malformed) != RSF_ERROR_INVALID_ARGUMENT)
            return fail("malformed result closes a valid scope before pointer access");
        scope.token = 0;
        if (rsf_d3d11_host_creation_begin(host.user, &scope) != RSF_OK) return fail("begin native scope after cancellation");
    }
    created = rsf_fixture_create_device(f->adapter, &chain_desc, &f->chain, &f->device, &f->context);
    if (FAILED(created)) return fail("create hardware D3D11 device/swapchain");
    if (scoped) {
        /* Check emitted code, not just C++ call spelling: a hook installer that
           follows the first entry jump must not land on the private continuation. */
        void** table = *(void***)f->chain;
        static const unsigned slots[] = {2u, 8u, 10u, 13u, 22u, 38u, 39u};
        size_t index;
        D3D11_TEXTURE2D_DESC allocation;
        DXGI_SWAP_CHAIN_DESC reported;
        IDXGISwapChain* inner;
        void* original_output;
        rsf_d3d11_native_chain outputs;
        unsigned long held_references;
        static const GUID annotation = {0xb6420ea1, 0x5912, 0x4d17, {0xb1, 0x65, 0x0b, 0x5a, 0x28, 0x68, 0x73, 0x64}};
        static const unsigned char metadata[48] = {0};
        for (index = 0; index < sizeof(slots) / sizeof(slots[0]); ++index) {
            const unsigned slot = slots[index];
            unsigned char entry[2] = {0};
            SIZE_T copied = 0;
            if (!ReadProcessMemory(GetCurrentProcess(), table[slot], entry, sizeof(entry), &copied) || copied != sizeof(entry) ||
                entry[0] == 0xe9 || entry[0] == 0xeb || (entry[0] == 0xff && entry[1] == 0x25)) {
                printf("FAIL: public slot%u exposes an entry jump\n", slot);
                return fail("all external methods retain a call/return boundary");
            }
        }
        /* Constructor-time queries must not create a real HWND chain or need Present. */
        if (FAILED(IDXGISwapChain_GetBuffer(f->chain, 0, &IID_ID3D11Texture2D, (void**)&f->buffer)) ||
            FAILED(ID3D11Texture2D_SetPrivateData(f->buffer, &annotation, sizeof(metadata), metadata)))
            return fail("creation-time virtual buffer contract");
        memset(&allocation, 0, sizeof(allocation));
        ID3D11Texture2D_GetDesc(f->buffer, &allocation);
        application_uav = ID3D11Device_CreateUnorderedAccessView(f->device, (ID3D11Resource*)f->buffer, NULL, &f->unordered);
        printf("Application usage=0x%x GetBuffer bind=0x%x UAV HRESULT=0x%08lx\n",
            chain_desc.BufferUsage, allocation.BindFlags, (unsigned long)application_uav);
        memset(&reported, 0, sizeof(reported));
        if (FAILED(IDXGISwapChain_GetDesc(f->chain, &reported)) || reported.BufferUsage != chain_desc.BufferUsage)
            return fail("virtual descriptor preserves requested application usage");
        /* Retain the failed contract result, but still exercise physical creation
           so the same run discriminates application binds from native DXGI usage. */
        if (presentation.is_owner(f->chain)) return fail("inner owner cannot be admitted before End");
        held_references = IDXGISwapChain_AddRef(f->chain);
        if (rsf_d3d11_host_fixture_check_unadmitted(host.user, f->chain, (uint32_t)held_references) != 0) return 1;
        inner = f->chain;
        f->chain = NULL;
        outer = (OuterChain*)calloc(1, sizeof(*outer));
        if (!outer) { f->chain = inner; return fail("allocate outer chain"); }
        outer->lpVtbl = &outer_vtable;
        outer->inner = inner;
        outer->refs = 1;
        outer->host_user = host.user;
        f->chain = (IDXGISwapChain*)outer;
        original_output = f->chain;
        memset(&outputs, 0, sizeof(outputs));
        outputs.struct_size = sizeof(outputs);
        outputs.abi_version = RSF_D3D11_HOST_ABI_VERSION;
        outputs.device = f->device;
        outputs.immediate_context = f->context;
        outputs.swapchain = &original_output;
        if (rsf_d3d11_host_creation_end(host.user, &scope, &outputs) != RSF_OK || original_output != (void*)f->chain ||
            !presentation.is_owner(f->chain) || !presentation.is_owner(outer->inner))
            return fail("canonical outer admission preserves output");
        if (rsf_d3d11_host_fixture_check_outer(host.user, outer, &outer->refs) != 0) return 1;
        /* The virtual buffer and its view end with the creation-time block, as before. */
        RSF_FIXTURE_RELEASE(ID3D11UnorderedAccessView, f->unordered);
        RSF_FIXTURE_RELEASE(ID3D11Texture2D, f->buffer);
    }
    /* Native startup presents before the first scene. The bridge adopts its chain there. */
    first_present = IDXGISwapChain_Present(f->chain, 0, 0);
    printf("Initial production Present HRESULT=0x%08lx\n", (unsigned long)first_present);
    dxgi_errors(f->dxgi_diagnostics);
    if (scoped && FAILED(first_present)) return fail("scoped physical creation/presentation");
    if (scoped && FAILED(application_uav)) return fail("requested application buffer UAV contract");
    if (outer && (outer->pre != 1 || outer->post != 1 || outer->dispatched != 1))
        return fail("outer Present work and exact dispatch remain active");
    if (FAILED(first_present) || !presentation.is_owner(f->chain) || !presentation.host()) {
        printf("Shared presentation not established: 0x%08lx owner=%u host=%p\n",
            (unsigned long)first_present, presentation.is_owner(f->chain), (void*)presentation.host());
        return fail("establish bridge-owned startup Present");
    }
    /* Match the adapter's explicit native presentation binding before publishing
       source CPU/FRAME facts. Scope admission alone does not bind this host. */
    binding.struct_size = sizeof(binding);
    binding.abi_version = RSF_D3D11_HOST_ABI_VERSION;
    binding.swapchain = f->chain;
    if (rsf_d3d11_host_bind_presentation(host.user, &binding) != RSF_OK)
        return fail("bind verified application presentation before source facts");
    if (rsf_d3d11_host_fixture_check_empty_aux(&host, f->context) != 0) return 1;

    if (outer && rsf_d3d11_host_fixture_check_inner(host.user, outer->inner) != 0) return 1;
    if (!rsf_fixture_allocate_textures(&f->targets, f->device)) return fail("allocate native-format fixture textures");
    if (!rsf_fixture_create_views(&f->targets, f->device)) return fail("create native-format fixture resource views");
    (void)ID3D11Device_QueryInterface(f->device, &IID_ID3D11InfoQueue, (void**)&f->diagnostics);
    printf("D3D11 debug queue available=%u\n", f->diagnostics ? 1u : 0u);
    if (f->diagnostics) ID3D11InfoQueue_ClearStoredMessages(f->diagnostics);
    if (rsf_get_overlay_api(&overlay) != RSF_OK) return fail("query production overlay API");

    rsf_fixture_resources(&resources, &host, f->device, f->context, f->chain, &f->targets, f->diagnostics,
        close_window);
    native_calls.presentation = &presentation;
    native_calls.outer = outer;
    native_calls.overlay = &overlay;
    memset(&calls, 0, sizeof(calls));
    calls.user = &native_calls;
    calls.get_buffer = fg_get_buffer;
    calls.present = fg_present;
    calls.has_provider_session = fg_has_provider_session;
    calls.backend = fg_backend;
    calls.outer_counts = outer ? fg_outer_counts : NULL;
    calls.pump_messages = fg_pump_messages;
    calls.settings_get = fg_settings_get;
    calls.settings_apply = fg_settings_apply;
    calls.switch_result = fg_switch_result;
    calls.generation = fg_generation;
    return rsf_d3d11_host_fixture_rendered_fg(&resources, &calls, (uint32_t)scoped, first_present,
        NATIVE_PIXEL, OUTSIDE_MARKER);
}

int main(int argc, char** argv)
{
    Fixture f;
    wchar_t ini[MAX_PATH] = {0};
    int result;
    memset(&f, 0, sizeof(f));
    result = body(argc, argv, &f, ini);
    fixture_release(&f);
    if (ini[0]) DeleteFileW(ini);
    return result;
}
