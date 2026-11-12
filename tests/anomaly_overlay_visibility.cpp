// SPDX-License-Identifier: GPL-3.0-only
// Hidden-window fixture for the Anomaly overlay's evidence path. It starts the production adapter service
// (install_hooks=false) against the REAL Runtime and the REAL Rust panel (RSF_OVERLAY_DLL), opens the
// panel over a hidden D3D11 swap chain, and checks what a run without desktop capture relies on:
//   - OverlayCapture=1 writes PNGs of the backbuffer after the panel drew, through the Runtime's frame
//     dump service, and an open capture contains panel pixels while a closed one is only background;
//   - the adapter log reports drawn state, target size, pointer events received and consumed, and
//     every settings_apply attempt with its result.
// A window message path stands in for the mouse; this does not prove a physical click in the game.
// Exits 77 only when neither a hardware nor a WARP D3D11 device can be created.
#include "overlay_service.h"
#include <rescaleframe/overlay_api.h>
#include <windows.h>
#include <d3d11.h>
#include <dxgi.h>
#include <wincodec.h>
#include <wrl/client.h>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <string>
#include <vector>

using Microsoft::WRL::ComPtr;
using namespace rsf::anomaly::overlay;

namespace {
int failures = 0;
void check(bool condition, const char* what) {
    if (!condition) { std::printf("FAIL: %s\n", what); ++failures; }
}
std::vector<std::string> log_lines;
void record_log(void*, const char* message) { log_lines.emplace_back(message); std::printf("  log: %s\n", message); }
bool logged(const char* needle) {
    for (const auto& line : log_lines) if (line.find(needle) != std::string::npos) return true;
    return false;
}

void pump() {
    MSG message;
    while (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) { TranslateMessage(&message); DispatchMessageW(&message); }
}

constexpr uint8_t background[3] = {26, 51, 77};   // clear colour 0.1, 0.2, 0.3

// Pixels that differ from the clear colour by more than rounding, in a PNG written by the dump service.
long differing_pixels(const std::filesystem::path& file, uint32_t* width, uint32_t* height) {
    ComPtr<IWICImagingFactory> factory;
    ComPtr<IWICBitmapDecoder> decoder;
    ComPtr<IWICBitmapFrameDecode> frame;
    ComPtr<IWICFormatConverter> converter;
    if (FAILED(CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&factory))) ||
        FAILED(factory->CreateDecoderFromFilename(file.c_str(), nullptr, GENERIC_READ, WICDecodeMetadataCacheOnDemand, &decoder)) ||
        FAILED(decoder->GetFrame(0, &frame)) || FAILED(factory->CreateFormatConverter(&converter)) ||
        FAILED(converter->Initialize(frame.Get(), GUID_WICPixelFormat32bppBGRA, WICBitmapDitherTypeNone, nullptr, 0,
            WICBitmapPaletteTypeCustom)))
        return -1;
    UINT w = 0, h = 0;
    converter->GetSize(&w, &h);
    *width = w; *height = h;
    std::vector<uint8_t> pixels(size_t(w) * h * 4);
    if (FAILED(converter->CopyPixels(nullptr, w * 4, static_cast<UINT>(pixels.size()), pixels.data()))) return -1;
    long count = 0;
    for (size_t i = 0; i < pixels.size(); i += 4) {
        const int b = pixels[i], g = pixels[i + 1], r = pixels[i + 2];
        if (std::abs(r - background[0]) > 3 || std::abs(g - background[1]) > 3 || std::abs(b - background[2]) > 3) ++count;
    }
    return count;
}

struct Graphics {
    ID3D11Device* device{};
    ID3D11DeviceContext* context{};
    IDXGISwapChain* chain{};
    ~Graphics() { if (chain) chain->Release(); if (context) context->Release(); if (device) device->Release(); }
};

PresentSink sink;
void frame(Graphics& gpu) {
    pump();
    ID3D11Texture2D* back{};
    if (FAILED(gpu.chain->GetBuffer(0, IID_PPV_ARGS(&back)))) return;
    ID3D11RenderTargetView* view{};
    if (SUCCEEDED(gpu.device->CreateRenderTargetView(back, nullptr, &view))) {
        const float colour[4] = {0.1f, 0.2f, 0.3f, 1};
        gpu.context->ClearRenderTargetView(view, colour);
        view->Release();
    }
    back->Release();
    sink.before_present(sink.user, gpu.chain);
    (void)gpu.chain->Present(0, 0);
}
void frames(Graphics& gpu, int count) { for (int i = 0; i < count; ++i) frame(gpu); }
}

int main() {
    wchar_t panel[MAX_PATH * 2]{};
    const DWORD length = GetEnvironmentVariableW(L"RSF_OVERLAY_DLL", panel, MAX_PATH * 2);
    if (!length || length >= MAX_PATH * 2 || GetFileAttributesW(panel) == INVALID_FILE_ATTRIBUTES) {
        std::puts("FAIL: RSF_OVERLAY_DLL must name the built rescaleframe_overlay.dll "
                  "(cargo build --release --locked -p rescaleframe-overlay)");
        return 1;
    }
    if (FAILED(CoInitializeEx(nullptr, COINIT_MULTITHREADED))) { std::puts("FAIL: COM"); return 1; }

    // The Runtime this process imported, not a second copy: the adapter queries the loaded module.
    HMODULE runtime = GetModuleHandleW(L"ReScaleFrame.Runtime.dll");
    check(runtime != nullptr, "the Runtime is loaded in this process");

    WNDCLASSW type{}; type.lpfnWndProc = DefWindowProcW; type.hInstance = GetModuleHandleW(nullptr);
    type.lpszClassName = L"_XRAY_1.5";   // the engine window class the adapter accepts
    RegisterClassW(&type);
    HWND window = CreateWindowExW(0, type.lpszClassName, L"overlay visibility", WS_OVERLAPPEDWINDOW, 0, 0, 640, 360,
        nullptr, nullptr, type.hInstance, nullptr);
    if (!window) { std::puts("FAIL: hidden window"); return 1; }

    Graphics gpu;
    DXGI_SWAP_CHAIN_DESC desc{};
    desc.BufferDesc.Width = 640; desc.BufferDesc.Height = 360; desc.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    desc.SampleDesc.Count = 1; desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT; desc.BufferCount = 2;
    desc.OutputWindow = window; desc.Windowed = TRUE; desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    HRESULT created = D3D11CreateDeviceAndSwapChain(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        nullptr, 0, D3D11_SDK_VERSION, &desc, &gpu.chain, &gpu.device, nullptr, &gpu.context);
    if (FAILED(created))
        created = D3D11CreateDeviceAndSwapChain(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            nullptr, 0, D3D11_SDK_VERSION, &desc, &gpu.chain, &gpu.device, nullptr, &gpu.context);
    if (FAILED(created)) { std::puts("SKIP: no D3D11 device"); DestroyWindow(window); return 77; }

    wchar_t temp[MAX_PATH]{};
    GetTempPathW(MAX_PATH, temp);
    const std::filesystem::path root = std::filesystem::path(temp) / (L"rsf_anomaly_overlay_visibility_" + std::to_wstring(GetCurrentProcessId()));
    std::error_code ignored;
    std::filesystem::remove_all(root, ignored);
    std::filesystem::create_directories(root);
    const auto ini = root / L"ReScaleFrame.ini";
    const auto captures = root / L"captures";
    {
        // The temp path is ASCII on the machines that run this; the capture directory is read back verbatim.
        const std::string text = "[Anomaly]\r\nOverlay=1\r\nOverlayKeyPoll=0\r\nOverlayCapture=1\r\nOverlayCaptureFrames=3\r\n"
            "OverlayCaptureStride=4\r\nOverlayCaptureCycles=1\r\nOverlayCaptureDir=" + captures.string() + "\r\n";
        FILE* file = nullptr;
        _wfopen_s(&file, ini.c_str(), L"wb");
        if (!file) { std::puts("FAIL: ini"); return 1; }
        fwrite(text.data(), 1, text.size(), file);
        fclose(file);
    }

    StartOptions options;
    options.runtime = runtime;
    options.configuration = ini.c_str();
    options.log = record_log;
    options.install_hooks = false;
    const char* reason = nullptr;
    check(start(options, &reason) == StartStatus::started, "the adapter starts against the real Runtime");
    sink = present_sink();
    check(logged("OverlayCapture") || logged("capture armed"), "capture arming is logged");

    rsf_overlay_api api{}; api.struct_size = sizeof(api); api.abi_version = RSF_OVERLAY_API_VERSION;
    check(rsf_get_overlay_api(&api) == RSF_OK, "query the same Runtime service");

    // Closed: the startup hint may draw; nothing is captured.
    frames(gpu, 3);
    check(!std::filesystem::exists(captures / L"open01_f00.png"), "no capture before the panel opens");

    Sleep(300);   // the Runtime debounces visibility changes
    POINT saved_cursor{};
    const bool restore_cursor = GetCursorPos(&saved_cursor) != FALSE;
    POINT click{120, 80};
    // The runtime samples the real cursor as well as window messages. Keep the
    // posted inside click consistent with that position, independent of desktop input.
    check(ClientToScreen(window, &click) != FALSE && SetCursorPos(click.x, click.y) != FALSE,
          "position the fixture cursor before opening the panel");
    check(api.set_visible(1) == RSF_OK && api.visible() == 1, "open the panel");
    // Pointer events through the window procedure the overlay installed: they must be counted and consumed.
    SendMessageW(window, WM_MOUSEMOVE, 0, MAKELPARAM(120, 80));
    SendMessageW(window, WM_LBUTTONDOWN, MK_LBUTTON, MAKELPARAM(120, 80));
    frames(gpu, 2);
    SendMessageW(window, WM_LBUTTONUP, 0, MAKELPARAM(120, 80));
    frames(gpu, 10);

    // A settings request from the API path is logged with its result, accepted or refused.
    rsf_render_settings settings{sizeof(settings), RSF_RENDER_FRONTEND_ABI_VERSION};
    check(api.settings_get(&settings) == RSF_OK, "settings_get");
    rsf_render_settings refused = settings;
    refused.sr_enabled = 1;   // this adapter offers no SR
    check(api.settings_apply(&refused) == RSF_ERROR_INVALID_ARGUMENT, "enabling SR is refused");
    settings.status_overlay = 1;
    check(api.settings_apply(&settings) == RSF_OK, "a valid request is applied");
    char refusal[64];
    std::snprintf(refusal, sizeof(refusal), "refused (%d)", int(RSF_ERROR_INVALID_ARGUMENT));
    check(logged("from api: SR=1") && logged(refusal), "the refused apply is logged with its result");
    check(logged("from api: SR=0") && logged("applied and saved (0)"), "the accepted apply is logged with its result");
    settings.status_overlay = 0;
    check(api.settings_apply(&settings) == RSF_OK, "restore");

    Sleep(300);
    check(api.set_visible(0) == RSF_OK && api.visible() == 0, "close the panel");
    frames(gpu, 10);

    check(logged("panel #1 opened: target 640x360"), "panel open is logged with the target size");
    check(logged("panel #1 frame 1: api.present result 0, drawn 1"), "api.present drawn state is logged");
    check(logged("panel #1 closed: present drawn"), "the close summary is logged");
    bool counted = false;
    for (const auto& line : log_lines) {
        const auto at = line.find("pointer events received ");
        unsigned long long received = 0, consumed = 0;
        if (line.find("panel #1 closed") != std::string::npos && at != std::string::npos &&
            std::sscanf(line.c_str() + at, "pointer events received %llu consumed %llu", &received, &consumed) == 2 &&
            received >= 3 && consumed >= 2) counted = true;
    }
    check(counted, "the close summary counts pointer events received and consumed");

    // The capture files: panel pixels while open, background only once closed.
    long best_open = 0;
    for (const wchar_t* name : {L"open01_f00.png", L"open01_f01.png", L"open01_f02.png"}) {
        uint32_t w = 0, h = 0;
        const long pixels = differing_pixels(captures / name, &w, &h);
        std::printf("%ls: %ldx%ld size, %ld non-background pixels\n", name, long(w), long(h), pixels);
        check(pixels >= 0 && w == 640 && h == 360, "open capture exists at the backbuffer size");
        if (pixels > best_open) best_open = pixels;
    }
    check(best_open > 3000, "an open capture contains panel pixels");
    for (const wchar_t* name : {L"close01_f00.png", L"close01_f01.png", L"close01_f02.png"}) {
        uint32_t w = 0, h = 0;
        const long pixels = differing_pixels(captures / name, &w, &h);
        std::printf("%ls: %ld non-background pixels\n", name, pixels);
        check(pixels == 0, "a closed capture is only background");
    }
    check(!std::filesystem::exists(captures / L"open02_f00.png"), "capture stops after OverlayCaptureCycles");

    api.notify(gpu.chain, RSF_OVERLAY_NOTIFY_DEVICE_RESET);
    if (restore_cursor) SetCursorPos(saved_cursor.x, saved_cursor.y);
    std::filesystem::remove_all(root, ignored);
    DestroyWindow(window);
    if (failures) { std::printf("%d failure(s)\n", failures); return 1; }
    std::puts("Anomaly overlay visibility: PASS");
    return 0;
}
