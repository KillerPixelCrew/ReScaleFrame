/* SPDX-License-Identifier: MIT */
/* Explicit device regression: <scratch runtime root with streamline/>. Hidden window,
   real shared DLSS evaluation, FG Off. Never uses or changes a game installation.
   C11 launcher; the Rust fixture owns the phases and verdicts. */
#include "d3d11_host_device_launch.h"
#include <rescaleframe/d3d11_host.h>
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/d3d11_present_api.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/input_dump.h>
#include <rescaleframe/overlay_api.h>
#include <d3d11sdklayers.h>

static HWND window;

static void host_log(void* user, const char* message)
{
    (void)user;
    printf("%s\n", message);
}
static int accept_window(void* user, void* candidate)
{
    (void)user;
    return candidate == (void*)window;
}
static void close_window(void) { DestroyWindow(window); }

typedef struct NativeCalls {
    const rsf_overlay_api* overlay;
    const rsf_d3d11_present_api* presentation;
} NativeCalls;

static int32_t has_token(void* user, uint64_t source)
{
    RSF_FOREIGN_TRY {
        const rsf_d3d11_present_api* api = ((const NativeCalls*)user)->presentation;
        return rsf_streamline_host_token(api->host(), source) != NULL;
    } RSF_FOREIGN_CATCH { return -1; }
}

static int32_t set_quality(void* user, uint32_t quality)
{
    RSF_FOREIGN_TRY {
        const rsf_overlay_api* api = ((const NativeCalls*)user)->overlay;
        rsf_render_settings settings = {sizeof(settings), RSF_RENDER_FRONTEND_ABI_VERSION};
        const int32_t result = api->settings_get(&settings);
        if (result != RSF_OK) return result;
        settings.sr_quality = quality;
        return api->settings_apply(&settings);
    } RSF_FOREIGN_CATCH { return RSF_ERROR_NOT_READY; }
}

static int32_t sr_status(void* user, int32_t* result, uint32_t* backend)
{
    RSF_FOREIGN_TRY {
        const rsf_overlay_api* api = ((const NativeCalls*)user)->overlay;
        rsf_render_status published = {sizeof(published), RSF_RENDER_FRONTEND_ABI_VERSION};
        const int32_t status = api->status(&published);
        *result = published.sr_result;
        *backend = published.sr_effective_backend;
        return status;
    } RSF_FOREIGN_CATCH { return RSF_ERROR_NOT_READY; }
}

static int32_t cancel_dump(uint64_t session)
{
    RSF_FOREIGN_TRY {
        rsf_input_dump_cancel(session);
        return RSF_OK;
    } RSF_FOREIGN_CATCH { return RSF_ERROR_NOT_READY; }
}

static int32_t present_chain(void* receiver)
{
    RSF_FOREIGN_TRY {
        return IDXGISwapChain_Present((IDXGISwapChain*)receiver, 0, 0);
    } RSF_FOREIGN_CATCH { return E_FAIL; }
}

static int32_t disable_sr(void* user)
{
    RSF_FOREIGN_TRY {
        const rsf_overlay_api* api = ((const NativeCalls*)user)->overlay;
        rsf_render_settings changed = {sizeof(changed), RSF_RENDER_FRONTEND_ABI_VERSION};
        const int32_t read = api->settings_get(&changed);
        if (read != RSF_OK) return read;
        changed.sr_enabled = 0;
        return api->settings_apply(&changed);
    } RSF_FOREIGN_CATCH { return RSF_ERROR_NOT_READY; }
}

/* IEEE binary16 to binary32, with subnormals, infinities and NaN, as XMConvertHalfToFloat. */
static float decode_half(uint16_t channel)
{
    const uint32_t sign = (uint32_t)(channel & 0x8000u) << 16;
    uint32_t exponent = (channel >> 10) & 0x1fu;
    uint32_t mantissa = channel & 0x3ffu;
    uint32_t bits;
    float value;
    if (exponent == 0x1f) {
        bits = sign | 0x7f800000u | (mantissa << 13);
    } else if (exponent) {
        bits = sign | ((exponent + 112u) << 23) | (mantissa << 13);
    } else if (mantissa) {
        exponent = 113u;
        while (!(mantissa & 0x400u)) { mantissa <<= 1; --exponent; }
        bits = sign | (exponent << 23) | ((mantissa & 0x3ffu) << 13);
    } else {
        bits = sign;
    }
    memcpy(&value, &bits, sizeof(value));
    return value;
}

typedef struct Fixture {
    IDXGIFactory1* factory;
    IDXGIAdapter1* adapter;
    ID3D11Device* device;
    ID3D11DeviceContext* context;
    IDXGISwapChain* chain;
    ID3D11InfoQueue* diagnostics;
    rsf_fixture_targets targets;
} Fixture;

static void fixture_release(Fixture* f)
{
    RSF_FIXTURE_RELEASE(ID3D11InfoQueue, f->diagnostics);
    rsf_fixture_targets_release(&f->targets);
    RSF_FIXTURE_RELEASE(IDXGISwapChain, f->chain);
    RSF_FIXTURE_RELEASE(ID3D11DeviceContext, f->context);
    RSF_FIXTURE_RELEASE(ID3D11Device, f->device);
    RSF_FIXTURE_RELEASE(IDXGIAdapter1, f->adapter);
    RSF_FIXTURE_RELEASE(IDXGIFactory1, f->factory);
}

#define FAILED_AT() (fprintf(stderr, "FAIL shared-SR fixture startup line=%d\n", __LINE__), 1)

static int body(int argc, char** argv, Fixture* f, wchar_t* ini)
{
    wchar_t root[MAX_PATH], interposer[MAX_PATH], capture[MAX_PATH], capture_root[MAX_PATH], temporary[MAX_PATH];
    WNDCLASSW wc;
    HMODULE module;
    rsf_d3d11_host_setup setup;
    rsf_game_host_services host;
    rsf_d3d11_present_api presentation = {sizeof(presentation), RSF_D3D11_PRESENT_API_VERSION};
    rsf_overlay_api overlay = {sizeof(overlay), RSF_OVERLAY_API_VERSION};
    DXGI_SWAP_CHAIN_DESC chain_desc;
    HRESULT created, first_present;
    NativeCalls native_calls;
    rsf_d3d11_fixture_sr_calls calls;
    rsf_d3d11_fixture_resources resources;

    if (argc != 2) { puts("Usage: rsf_d3d11_host_shared_sr_device <scratch runtime root>"); return 77; }
    if (!rsf_fixture_wide_root(argv[1], root, MAX_PATH)) return 77;
    if (!rsf_fixture_join(interposer, MAX_PATH, root, L"streamline\\sl.interposer.dll") ||
        !rsf_fixture_exists(interposer)) return 77;
    if (FAILED(CreateDXGIFactory1(&IID_IDXGIFactory1, (void**)&f->factory))) return 77;
    f->adapter = rsf_fixture_nvidia_adapter(f->factory);
    if (!f->adapter) return 77;

    module = GetModuleHandleW(NULL);
    memset(&wc, 0, sizeof(wc));
    wc.lpfnWndProc = DefWindowProcW;
    wc.hInstance = module;
    wc.lpszClassName = L"RSFSharedSRDevice";
    if (!RegisterClassW(&wc)) return FAILED_AT();
    window = CreateWindowW(wc.lpszClassName, L"RSF shared SR device check", WS_OVERLAPPED,
        0, 0, (int)RSF_FIXTURE_WIDTH, (int)RSF_FIXTURE_HEIGHT, NULL, NULL, module, NULL);
    if (!window) return FAILED_AT(); /* Deliberately never shown or made foreground. */
    if (!GetTempPathW(MAX_PATH, temporary) || !GetTempFileNameW(temporary, L"rsf", 0, ini)) return FAILED_AT();
    if (!WritePrivateProfileSectionW(L"Device", L"SR=1\0SRBackend=1\0SRQuality=1\0FG=0\0Latency=0\0StatusOverlay=0\0", ini))
        return FAILED_AT();

    memset(&setup, 0, sizeof(setup));
    setup.struct_size = sizeof(setup);
    setup.abi_version = RSF_D3D11_HOST_ABI_VERSION;
    setup.configuration_path = ini;
    setup.configuration_section = L"Device";
    setup.runtime_directory = root;
    setup.identity_utf8 = "shared-sr-device";
    setup.executable_sha256 = "synthetic";
    setup.engine_version_utf8 = "1";
    setup.project_id_utf8 = "668392a0-ecfe-4d0c-9f58-c2838c5ca308";
    setup.units_to_meters = 1;
    setup.quality_choices = 0x1f;
    setup.depth_inverted = 1;
    setup.depth_far_plane = RSF_D3D11_HOST_DEPTH_INFINITE;
    setup.motion.struct_size = sizeof(setup.motion);
    setup.motion.scale_x = setup.motion.scale_y = 1;
    setup.motion.output_scale_x = setup.motion.output_scale_y = 1;
    setup.motion.invalid_value = -1000;
    setup.presentation_bridge = 1;
    setup.accept_window = accept_window;
    memset(&host, 0, sizeof(host));
    host.struct_size = sizeof(host);
    host.abi_version = RSF_GAME_ABI_VERSION;
    host.session_id = 91;
    host.log = host_log;
    if (rsf_d3d11_host_create(&setup, &host) != RSF_OK || rsf_d3d11_host_set_active(host.user, 1) != RSF_OK)
        return FAILED_AT();
    if (rsf_get_d3d11_present_api(&presentation) != RSF_OK) return FAILED_AT();

    memset(&chain_desc, 0, sizeof(chain_desc));
    chain_desc.BufferDesc.Width = RSF_FIXTURE_WIDTH;
    chain_desc.BufferDesc.Height = RSF_FIXTURE_HEIGHT;
    chain_desc.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    chain_desc.SampleDesc.Count = 1;
    chain_desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    chain_desc.BufferCount = 2;
    chain_desc.OutputWindow = window;
    chain_desc.Windowed = TRUE;
    created = rsf_fixture_create_device(f->adapter, &chain_desc, &f->chain, &f->device, &f->context);
    if (FAILED(created)) return FAILED_AT();

    /* Native startup presents before the first scene. The bridge adopts its chain there. */
    first_present = IDXGISwapChain_Present(f->chain, 0, 0);
    if (FAILED(first_present) || !presentation.is_owner(f->chain) || !presentation.host()) {
        printf("Shared presentation not established: 0x%08lx owner=%u host=%p\n",
            (unsigned long)first_present, presentation.is_owner(f->chain), (void*)presentation.host());
        return FAILED_AT();
    }
    if (!rsf_fixture_allocate_textures(&f->targets, f->device)) return FAILED_AT();
    if (!rsf_fixture_create_views(&f->targets, f->device)) return FAILED_AT();
    (void)ID3D11Device_QueryInterface(f->device, &IID_ID3D11InfoQueue, (void**)&f->diagnostics);
    printf("D3D11 debug queue available=%u\n", f->diagnostics ? 1u : 0u);
    if (f->diagnostics) ID3D11InfoQueue_ClearStoredMessages(f->diagnostics);
    if (!rsf_fixture_join(capture, MAX_PATH, root, L"captures") ||
        !rsf_fixture_join(capture_root, MAX_PATH, capture, L"website")) return FAILED_AT();
    if (rsf_get_overlay_api(&overlay) != RSF_OK) return FAILED_AT();

    native_calls.overlay = &overlay;
    native_calls.presentation = &presentation;
    memset(&calls, 0, sizeof(calls));
    calls.user = &native_calls;
    calls.has_token = has_token;
    calls.set_quality = set_quality;
    calls.sr_status = sr_status;
    calls.cancel_dump = cancel_dump;
    calls.present = present_chain;
    calls.disable_sr = disable_sr;
    calls.decode_half = decode_half;
    memset(&resources, 0, sizeof(resources));
    resources.host = &host;
    resources.device = f->device;
    resources.context = f->context;
    resources.chain = f->chain;
    resources.colour = f->targets.color;
    resources.motion = f->targets.motion;
    resources.depth = f->targets.depth;
    resources.output = f->targets.output;
    resources.colour_view = f->targets.color_view;
    resources.motion_view = f->targets.motion_view;
    resources.depth_view = f->targets.depth_view;
    resources.output_view = f->targets.output_view;
    resources.width = RSF_FIXTURE_WIDTH;
    resources.height = RSF_FIXTURE_HEIGHT;
    resources.diagnostics = f->diagnostics;
    resources.close_window = close_window;
    return rsf_d3d11_host_fixture_shared_sr(&resources, &calls, capture_root);
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
