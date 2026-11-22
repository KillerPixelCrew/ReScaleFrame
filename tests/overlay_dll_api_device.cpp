// SPDX-License-Identifier: MIT
// Windows and D3D11 leaves for the DLL import fixture. The Rust scenario owns
// the adapter callbacks, frame expectations and reset sequence.
#include <rescaleframe/overlay_api.h>
#include <windows.h>
#include <d3d11.h>
#include <dxgi.h>
#include <cstdio>

#ifdef RSF_RUNTIME_BUILD
#error The fixture must import Runtime.
#endif

extern "C" {
typedef void (*foreign_fn)(void* user, void* chain);
struct rsf_overlay_device_fixture {
    uint32_t (*clear)(void* gpu);
    uint32_t (*submit)(void* gpu);
    uint32_t (*resize)(void* gpu, uint32_t width, uint32_t height);
    uint32_t (*foreign)(void* gpu, foreign_fn, void* user);
    int32_t (*hide_cursor)();
};
int rsf_test_overlay_device(rsf_get_overlay_api_fn, const rsf_overlay_device_fixture*,
    void* gpu, void* chain, void* window);
}
static_assert(sizeof(rsf_overlay_device_fixture) == 5 * sizeof(void*));

namespace {
constexpr wchar_t window_class[] = L"RsfOverlayDllApiFixture";
struct Graphics {
    ID3D11Device* device{};
    ID3D11DeviceContext* context{};
    IDXGISwapChain* chain{};
    ~Graphics()
    {
        if (chain) chain->Release();
        if (context) context->Release();
        if (device) device->Release();
    }
};

DXGI_SWAP_CHAIN_DESC description(HWND window, UINT width, UINT height)
{
    DXGI_SWAP_CHAIN_DESC desc{};
    desc.BufferDesc.Width = width;
    desc.BufferDesc.Height = height;
    desc.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    desc.SampleDesc.Count = 1;
    desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    desc.BufferCount = 2;
    desc.OutputWindow = window;
    desc.Windowed = TRUE;
    desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    return desc;
}

uint32_t clear(void* owner)
{
    auto& gpu = *static_cast<Graphics*>(owner);
    MSG message;
    while (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) {
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
    ID3D11Texture2D* back{};
    if (FAILED(gpu.chain->GetBuffer(0, IID_PPV_ARGS(&back)))) return 0;
    ID3D11RenderTargetView* view{};
    const HRESULT made = gpu.device->CreateRenderTargetView(back, nullptr, &view);
    back->Release();
    if (FAILED(made)) return 0;
    const float colour[4] = {0.1f, 0.2f, 0.3f, 1};
    gpu.context->ClearRenderTargetView(view, colour);
    view->Release();
    return 1;
}

uint32_t submit(void* owner)
{
    return SUCCEEDED(static_cast<Graphics*>(owner)->chain->Present(0, 0));
}

uint32_t resize(void* owner, uint32_t width, uint32_t height)
{
    return SUCCEEDED(static_cast<Graphics*>(owner)->chain->ResizeBuffers(
        0, width, height, DXGI_FORMAT_UNKNOWN, 0));
}

uint32_t with_foreign_chain(void* owner, foreign_fn run, void* user)
{
    auto& gpu = *static_cast<Graphics*>(owner);
    HWND window = CreateWindowExW(0, window_class, L"other", WS_OVERLAPPEDWINDOW,
        0, 0, 64, 64, nullptr, nullptr, GetModuleHandleW(nullptr), nullptr);
    auto desc = description(window, 64, 64);
    IDXGIDevice* dxgi{};
    IDXGIAdapter* parent{};
    IDXGIFactory* factory{};
    IDXGISwapChain* foreign{};
    const bool created = window &&
        SUCCEEDED(gpu.device->QueryInterface(IID_PPV_ARGS(&dxgi))) &&
        SUCCEEDED(dxgi->GetAdapter(&parent)) &&
        SUCCEEDED(parent->GetParent(IID_PPV_ARGS(&factory))) &&
        SUCCEEDED(factory->CreateSwapChain(gpu.device, &desc, &foreign));
    if (created) run(user, foreign);
    if (foreign) foreign->Release();
    if (factory) factory->Release();
    if (parent) parent->Release();
    if (dxgi) dxgi->Release();
    if (window) DestroyWindow(window);
    return created;
}

int32_t hide_cursor() { return ShowCursor(FALSE); }
constexpr rsf_overlay_device_fixture leaves{clear, submit, resize, with_foreign_chain, hide_cursor};
}

int main()
{
    wchar_t panel[MAX_PATH * 2]{};
    const DWORD length = GetEnvironmentVariableW(L"RSF_OVERLAY_DLL", panel, MAX_PATH * 2);
    if (!length || length >= MAX_PATH * 2 || GetFileAttributesW(panel) == INVALID_FILE_ATTRIBUTES) {
        std::puts("FAIL: RSF_OVERLAY_DLL must name the built rescaleframe_overlay.dll "
                  "(cargo build --release --locked -p rescaleframe-overlay)");
        return 1;
    }
    WNDCLASSW type{};
    type.lpfnWndProc = DefWindowProcW;
    type.hInstance = GetModuleHandleW(nullptr);
    type.lpszClassName = window_class;
    RegisterClassW(&type);
    const HWND window = CreateWindowExW(0, window_class, L"overlay fixture", WS_OVERLAPPEDWINDOW,
        0, 0, 640, 360, nullptr, nullptr, type.hInstance, nullptr);
    if (!window) { std::puts("FAIL: hidden window"); return 1; }

    int result = 77;
    {
        Graphics gpu;
        auto desc = description(window, 640, 360);
        const char* adapter = "hardware";
        const auto create = [&](D3D_DRIVER_TYPE driver) {
            return D3D11CreateDeviceAndSwapChain(nullptr, driver, nullptr, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                nullptr, 0, D3D11_SDK_VERSION, &desc, &gpu.chain, &gpu.device, nullptr, &gpu.context);
        };
        HRESULT status = create(D3D_DRIVER_TYPE_HARDWARE);
        if (FAILED(status)) {
            adapter = "WARP";
            status = create(D3D_DRIVER_TYPE_WARP);
        }
        if (SUCCEEDED(status)) {
            std::printf("D3D11 %s device, hidden 640x360 window\n", adapter);
            result = rsf_test_overlay_device(rsf_get_overlay_api, &leaves, &gpu, gpu.chain, window);
        } else {
            std::printf("SKIP: no D3D11 device (hr 0x%08lx)\n", static_cast<unsigned long>(status));
        }
    }
    DestroyWindow(window);
    return result;
}
