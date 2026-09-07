// SPDX-License-Identifier: GPL-3.0-only
// Regression: an observer can select a helper device before the game's device exists.
// Render in the real Present callback with no backend, then read pixels and resize the chain.
#include "../loader/proxy/src/overlay_host.h"
#include <rescaleframe/d3d11_observer.h>

#include <windows.h>
#include <d3d11.h>

#include <cstdio>
#include <cstdlib>

namespace {
ID3D11Device* device = nullptr;
ID3D11DeviceContext* context = nullptr;
unsigned int frames = 0;
unsigned int colored_frames = 0;

void require(bool ok, const char* message)
{
    if (!ok) {
        std::fprintf(stderr, "FAIL: %s\n", message);
        std::exit(1);
    }
}

void log_line(void*, const char* message)
{
    std::fprintf(stderr, "%s\n", message);
    std::fflush(stderr);
}

void present(void*, void* pointer)
{
    auto* chain = static_cast<IDXGISwapChain*>(pointer);
    if (frames == 0) {
        void* observed = nullptr;
        require(rsf_observer_acquire_device(&observed, nullptr) == RSF_OBSERVER_OK,
                "observer must have selected a device");
        require(observed != device, "helper device must win observer selection to exercise the bug");
        static_cast<ID3D11Device*>(observed)->Release();
        require(rsf_overlay_host_start(chain, log_line, nullptr) != 0, "host starts without DLSS");
        rsf_overlay_host_toggle();
    }

    ID3D11Texture2D* buffer = nullptr;
    require(SUCCEEDED(chain->GetBuffer(0, __uuidof(ID3D11Texture2D),
                                        reinterpret_cast<void**>(&buffer))), "get back buffer");
    ID3D11RenderTargetView* clear_view = nullptr;
    require(SUCCEEDED(device->CreateRenderTargetView(buffer, nullptr, &clear_view)), "clear view");
    const float black[4] = {0, 0, 0, 1};
    context->ClearRenderTargetView(clear_view, black);
    clear_view->Release();
    context->OMSetRenderTargets(0, nullptr, nullptr);

    D3D11_TEXTURE2D_DESC desc{};
    buffer->GetDesc(&desc);
    rsf_overlay_stats stats{};
    stats.struct_size = sizeof(stats);
    stats.output_width = desc.Width;
    stats.output_height = desc.Height;
    stats.backend_name = "DLSS";
    stats.refusal_reason = "No backend in this test";
    rsf_overlay_intent intent{};
    require(rsf_overlay_host_present(chain, &stats, &intent) != 0, "draw on presenting device");

    ID3D11RenderTargetView* bound = nullptr;
    context->OMGetRenderTargets(1, &bound, nullptr);
    require(bound == nullptr, "restore the game's unbound render target");

    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.MiscFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    ID3D11Texture2D* staging = nullptr;
    require(SUCCEEDED(device->CreateTexture2D(&desc, nullptr, &staging)), "readback allocation");
    context->CopyResource(staging, buffer);
    D3D11_MAPPED_SUBRESOURCE mapped{};
    require(SUCCEEDED(context->Map(staging, 0, D3D11_MAP_READ, 0, &mapped)), "readback map");
    unsigned int colored = 0;
    for (UINT y = 0; y < desc.Height; ++y) {
        const auto* row = static_cast<const unsigned char*>(mapped.pData) + y * mapped.RowPitch;
        for (UINT x = 0; x < desc.Width; ++x) {
            if (row[x * 4] || row[x * 4 + 1] || row[x * 4 + 2]) {
                ++colored;
            }
        }
    }
    context->Unmap(staging, 0);
    staging->Release();
    buffer->Release();
    if (colored > 20) {
        ++colored_frames;
    }
    std::fprintf(stderr, "frame %u: %u nonblack pixels at %ux%u\n", frames, colored,
                 desc.Width, desc.Height);
    ++frames;
}
} // namespace

int main(int argc, char* argv[])
{
    // Optional manual checks use the shipped egui DLL and/or the installed RenderDoc DLL.
    if (const char* capture = std::getenv("RSF_TEST_RENDERDOC_DLL")) {
        require(LoadLibraryA(capture) != nullptr, "load optional RenderDoc");
    }
    require(argc == 2, "pass the panel DLL filename or Windows path");
    char path[MAX_PATH * 2]{};
    const DWORD length = GetFullPathNameA(argv[1], sizeof(path), path, nullptr);
    require(length > 0 && length < sizeof(path), "panel path");
    require(SetEnvironmentVariableA("RSF_OVERLAY_DLL", path) != 0, "set panel path");

    rsf_observer_options options{};
    options.struct_size = sizeof(options);
    options.abi_version = RSF_OBSERVER_ABI_VERSION;
    options.capacity = 8;
    options.constant_buffer_min_bytes = 1024;
    options.constant_buffer_max_bytes = 8192;
    options.on_present = present;
    const auto installed = rsf_observer_install(&options);
    if (installed == RSF_OBSERVER_ERROR_NO_DEVICE) {
        std::fprintf(stderr, "No D3D11 device, skipping\n");
        return 77;
    }
    require(installed == RSF_OBSERVER_OK, "install observer");

    ID3D11Device* helper = nullptr;
    require(SUCCEEDED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, 0, nullptr,
                                        0, D3D11_SDK_VERSION, &helper, nullptr, nullptr)),
            "create helper device");
    D3D11_BUFFER_DESC cb{};
    cb.ByteWidth = 16;
    cb.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    ID3D11Buffer* constant = nullptr;
    require(SUCCEEDED(helper->CreateBuffer(&cb, nullptr, &constant)), "helper creates first buffer");
    constant->Release();
    helper->Release();

    WNDCLASSW wc{};
    wc.lpfnWndProc = DefWindowProcW;
    wc.hInstance = GetModuleHandleW(nullptr);
    wc.lpszClassName = L"RsfOverlayHostTest";
    require(RegisterClassW(&wc) != 0, "register window class");
    HWND window = CreateWindowW(wc.lpszClassName, L"", WS_OVERLAPPEDWINDOW, 0, 0, 800, 600,
                                nullptr, nullptr, wc.hInstance, nullptr);
    require(window != nullptr, "create window");
    DXGI_SWAP_CHAIN_DESC swap{};
    swap.BufferCount = 2;
    swap.BufferDesc.Width = 800;
    swap.BufferDesc.Height = 600;
    swap.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    swap.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    swap.OutputWindow = window;
    swap.SampleDesc.Count = 1;
    swap.Windowed = TRUE;
    swap.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    IDXGISwapChain* chain = nullptr;
    require(SUCCEEDED(D3D11CreateDeviceAndSwapChain(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, 0,
                    nullptr, 0, D3D11_SDK_VERSION, &swap, &chain, &device, nullptr, &context)),
            "create presenting device and flip chain");
    for (unsigned int size = 0; size < 2; ++size) {
        const unsigned int previous_colored = colored_frames;
        for (unsigned int frame = 0; frame < 8; ++frame) {
            require(SUCCEEDED(chain->Present(0, 0)), "forward real Present");
        }
        require(colored_frames >= previous_colored + 6, "panel must alter actual back-buffer pixels");
        require(SUCCEEDED(chain->ResizeBuffers(2, 960, 720, DXGI_FORMAT_UNKNOWN, 0)),
                "no overlay back-buffer references may prevent resize");
    }
    require(frames == 16, "all callbacks reached");
    rsf_overlay_host_stop();
    require(rsf_observer_uninstall() == RSF_OBSERVER_OK, "uninstall observer");
    context->Release();
    device->Release();
    chain->Release();
    DestroyWindow(window);
    UnregisterClassW(wc.lpszClassName, wc.hInstance);
    return 0;
}
