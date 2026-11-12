// SPDX-License-Identifier: GPL-3.0-only
// Hidden-window D3D11 fixture for rsf_get_overlay_api with the real Rust panel
// (RSF_OVERLAY_DLL). Opens and closes the panel over several Presents, runs a game-style
// ShowCursor hide loop while it is open, and exercises resize and device-reset notifications.
// Exit 77 only when neither a hardware nor a WARP D3D11 device can be created.
#include <rescaleframe/overlay_api.h>
#include <windows.h>
#include <d3d11.h>
#include <dxgi.h>
#include <cstdio>
#include <cstring>
#include <string>

#ifdef RSF_RUNTIME_BUILD
#error The fixture must import Runtime, never embed its owner.
#endif

namespace {
int failures = 0;
void check(bool condition, const char* what)
{
    if (!condition) { std::printf("FAIL: %s\n", what); ++failures; }
}
HWND accepted_window{};
int window_calls = 0, report_calls = 0;
void log(void*, const char* message) { std::printf("  log: %s\n", message); }
int accept(void*, void* hwnd) { ++window_calls; return hwnd == accepted_window; }
int report(void*, rsf_overlay_sr_report* out)
{
    ++report_calls;
    out->effective_backend = 1; out->effective_quality = 2;
    out->render_width = 320; out->render_height = 180;
    out->frames_evaluated = static_cast<uint32_t>(report_calls);
    out->inputs = RSF_OVERLAY_SR_INPUT_COLOR | RSF_OVERLAY_SR_INPUT_DEPTH | RSF_OVERLAY_SR_INPUT_MOTION;
    return 1;
}
void pump()
{
    MSG message;
    while (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) { TranslateMessage(&message); DispatchMessageW(&message); }
}
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
// Draw-free frame: clear, let the overlay draw, then present.
bool frame(const rsf_overlay_api& api, Graphics& gpu, uint32_t& drawn)
{
    pump();
    ID3D11Texture2D* back{};
    if (FAILED(gpu.chain->GetBuffer(0, IID_PPV_ARGS(&back)))) return false;
    ID3D11RenderTargetView* view{};
    const HRESULT made = gpu.device->CreateRenderTargetView(back, nullptr, &view);
    back->Release();
    if (FAILED(made)) return false;
    const float colour[4] = {0.1f, 0.2f, 0.3f, 1};
    gpu.context->ClearRenderTargetView(view, colour);
    view->Release();
    const auto result = api.present(gpu.chain, &drawn);
    return result == RSF_OK && SUCCEEDED(gpu.chain->Present(0, 0));
}
bool frames(const rsf_overlay_api& api, Graphics& gpu, int count, uint32_t expected, const char* what)
{
    for (int i = 0; i < count; ++i) {
        uint32_t drawn = 2;
        if (!frame(api, gpu, drawn)) { std::printf("FAIL: %s: frame %d did not present\n", what, i); ++failures; return false; }
        if (drawn != expected) { std::printf("FAIL: %s: frame %d drawn=%u expected %u\n", what, i, drawn, expected); ++failures; return false; }
    }
    return true;
}
bool open_after_debounce(const rsf_overlay_api& api, uint32_t visible)
{
    Sleep(300);
    return api.set_visible(visible) == RSF_OK && api.visible() == visible;
}
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

    WNDCLASSW type{}; type.lpfnWndProc = DefWindowProcW; type.hInstance = GetModuleHandleW(nullptr);
    type.lpszClassName = L"RsfOverlayDllApiFixture";
    RegisterClassW(&type);
    accepted_window = CreateWindowExW(0, type.lpszClassName, L"overlay fixture", WS_OVERLAPPEDWINDOW,
        0, 0, 640, 360, nullptr, nullptr, type.hInstance, nullptr);
    if (!accepted_window) { std::puts("FAIL: hidden window"); return 1; }

    Graphics gpu;
    DXGI_SWAP_CHAIN_DESC desc{};
    desc.BufferDesc.Width = 640; desc.BufferDesc.Height = 360; desc.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    desc.SampleDesc.Count = 1; desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT; desc.BufferCount = 2;
    desc.OutputWindow = accepted_window; desc.Windowed = TRUE; desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    const char* adapter = "hardware";
    HRESULT created = D3D11CreateDeviceAndSwapChain(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        nullptr, 0, D3D11_SDK_VERSION, &desc, &gpu.chain, &gpu.device, nullptr, &gpu.context);
    if (FAILED(created)) {
        adapter = "WARP";
        created = D3D11CreateDeviceAndSwapChain(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            nullptr, 0, D3D11_SDK_VERSION, &desc, &gpu.chain, &gpu.device, nullptr, &gpu.context);
    }
    if (FAILED(created)) {
        std::printf("SKIP: no D3D11 device (hr 0x%08lx)\n", static_cast<unsigned long>(created));
        DestroyWindow(accepted_window);
        return 77;
    }
    std::printf("D3D11 %s device, hidden 640x360 window\n", adapter);

    rsf_overlay_api api{}; api.struct_size = sizeof(api); api.abi_version = RSF_OVERLAY_API_VERSION;
    if (rsf_get_overlay_api(&api) != RSF_OK) { std::puts("FAIL: query"); return 1; }

    wchar_t directory[MAX_PATH]{};
    GetTempPathW(MAX_PATH, directory);
    const std::wstring ini = std::wstring(directory) + L"rsf_overlay_dll_device_" + std::to_wstring(GetCurrentProcessId()) + L".ini";
    DeleteFileW(ini.c_str());
    rsf_overlay_service_setup setup{};
    setup.struct_size = sizeof(setup); setup.abi_version = RSF_OVERLAY_API_VERSION;
    setup.identity_utf8 = "D3D11 overlay fixture";
    setup.configuration_path = ini.c_str(); setup.configuration_section = L"Overlay";
    setup.defaults = {sizeof(rsf_render_settings), RSF_RENDER_FRONTEND_ABI_VERSION, 1, 1, 0, 0, 7, 1, 0, 0, 0, 2};
    setup.sr_backend_choices = 1u << 1; setup.sr_quality_choices = 0x1fu;
    setup.fg_backend_choices = 0; setup.latency_choices = 1u; setup.max_generated_frames = 1;
    setup.log = log; setup.accept_window = accept; setup.sr_report = report;
    check(api.configure(&setup) == RSF_OK, "configure");

    // A chain targeting another window is refused by the adapter's filter and starts nothing.
    {
        HWND other = CreateWindowExW(0, type.lpszClassName, L"other", WS_OVERLAPPEDWINDOW, 0, 0, 64, 64, nullptr, nullptr, type.hInstance, nullptr);
        IDXGIDevice* dxgi{}; IDXGIAdapter* parent{}; IDXGIFactory* factory{}; IDXGISwapChain* foreign{};
        auto second = desc; second.OutputWindow = other; second.BufferDesc.Width = second.BufferDesc.Height = 64;
        if (SUCCEEDED(gpu.device->QueryInterface(IID_PPV_ARGS(&dxgi))) && SUCCEEDED(dxgi->GetAdapter(&parent)) &&
            SUCCEEDED(parent->GetParent(IID_PPV_ARGS(&factory))) && SUCCEEDED(factory->CreateSwapChain(gpu.device, &second, &foreign))) {
            uint32_t drawn = 7;
            check(api.present(foreign, &drawn) == RSF_ERROR_NOT_READY && drawn == 0, "window filter refuses another chain");
            rsf_overlay_capture state{sizeof(state)};
            check(api.capture(&state) == RSF_OK && !state.started, "refused chain did not start the panel");
            foreign->Release();
        } else check(false, "second chain for the window filter");
        if (factory) factory->Release();
        if (parent) parent->Release();
        if (dxgi) dxgi->Release();
        DestroyWindow(other);
    }

    // Closed: the startup hint may draw; nothing is captured.
    for (int i = 0; i < 3; ++i) { uint32_t drawn{}; check(frame(api, gpu, drawn), "closed frame presents"); }
    rsf_overlay_capture capture{sizeof(capture)};
    check(api.capture(&capture) == RSF_OK && capture.started && !capture.visible && !capture.captured, "started closed, input passes through");

    check(open_after_debounce(api, 1), "open panel");
    check(api.capture(&capture) == RSF_OK && capture.visible &&
        capture.captured == (RSF_OVERLAY_CAPTURE_MOUSE | RSF_OVERLAY_CAPTURE_KEYBOARD), "open panel captures mouse and keyboard");
    frames(api, gpu, 6, 1, "open panel draws every frame");

    // RAGE-style: hide until ShowCursor reports a negative count. Must terminate while open.
    int count = 0; unsigned attempts = 0;
    do { count = ShowCursor(FALSE); } while (count >= 0 && ++attempts < 512);
    check(count < 0, "game-style ShowCursor hide loop terminated while the panel is open");
    std::printf("hide loop: count=%d after %u extra call(s)\n", count, attempts);

    rsf_render_status status{}; status.struct_size = sizeof(status); status.abi_version = RSF_RENDER_FRONTEND_ABI_VERSION;
    check(api.status(&status) == RSF_OK, "status while open");
    check(status.sr_requested_backend == 1 && status.sr_effective_backend == 1 && status.render_width == 320 &&
        status.output_width == 640 && status.output_height == 360 && status.fg_requested == 0 && status.fg_effective_backend == 0 &&
        status.application_presents >= 9, "status reports adapter SR truth and chain extent");
    std::printf("status: presents=%llu reason=\"%s\"\n", static_cast<unsigned long long>(status.application_presents), status.reason);
    check(report_calls >= 9 && window_calls >= 10, "adapter callbacks consulted per Present");

    check(open_after_debounce(api, 0), "close panel");
    check(api.capture(&capture) == RSF_OK && capture.started && !capture.visible && !capture.captured, "closed panel releases input");
    frames(api, gpu, 4, 0, "closed panel without HUD draws nothing");

    // Resize: notify before ResizeBuffers; the next open frame lays out at the new extent.
    check(api.notify(gpu.chain, RSF_OVERLAY_NOTIFY_RESIZE) == RSF_OK, "resize notification");
    check(SUCCEEDED(gpu.chain->ResizeBuffers(0, 800, 450, DXGI_FORMAT_UNKNOWN, 0)), "ResizeBuffers");
    check(open_after_debounce(api, 1), "reopen after resize");
    frames(api, gpu, 3, 1, "open panel draws after resize");
    check(api.status(&status) == RSF_OK && status.output_width == 800 && status.output_height == 450, "status follows resized chain");

    // Device reset: releases panel, renderer and window subclass; the next Present restarts it.
    check(api.notify(gpu.chain, RSF_OVERLAY_NOTIFY_DEVICE_RESET) == RSF_OK, "device reset notification");
    check(api.capture(&capture) == RSF_OK && !capture.started && !capture.captured && api.visible() == 0, "reset released the panel");
    for (int i = 0; i < 2; ++i) { uint32_t drawn{}; check(frame(api, gpu, drawn), "frame after reset"); }
    check(api.capture(&capture) == RSF_OK && capture.started, "panel restarted on the next Present");
    check(open_after_debounce(api, 1), "open after restart");
    frames(api, gpu, 3, 1, "restarted panel draws");
    check(open_after_debounce(api, 0), "close after restart");

    check(api.notify(gpu.chain, RSF_OVERLAY_NOTIFY_DEVICE_RESET) == RSF_OK, "final release");
    DeleteFileW(ini.c_str());
    DestroyWindow(accepted_window);
    if (failures) return 1;
    std::puts("PASS: shared Rust overlay opened, drew, captured and released input through the Runtime DLL API");
    return 0;
}
