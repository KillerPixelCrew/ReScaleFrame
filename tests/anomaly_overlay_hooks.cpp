// SPDX-License-Identifier: GPL-3.0-only
// Real-object contract for the Anomaly overlay hooks. A fake overlay API table is started through the
// production start() with hooks enabled, then a WARP swap chain and the system DirectInput mouse and
// keyboard prove the engine boundaries reach it. The engine itself is not involved. Exits 77 (skip)
// where no WARP device or DirectInput device exists.
#include "overlay_service.h"
#include <d3d11.h>
#include <dxgi.h>
#include <cstdio>
#include <cstring>
#include <vector>

using namespace rsf::anomaly::overlay;

namespace {
int failures = 0;
void check(bool condition, const char* what) {
    if (!condition) { std::printf("FAIL: %s\n", what); ++failures; }
}

std::vector<void*> presented;
std::vector<uint32_t> notified;
int toggles = 0, capture_queries = 0;
uint32_t capture_bits = 0;

int32_t fake_configure(const rsf_overlay_service_setup*) { return RSF_OK; }
int32_t fake_present(void* chain, uint32_t* drawn) { presented.push_back(chain); if (drawn) *drawn = 0; return RSF_OK; }
int32_t fake_notify(void*, uint32_t reason) { notified.push_back(reason); return RSF_OK; }
uint32_t fake_visible() { return 0; }
int32_t fake_set_visible(uint32_t) { return RSF_OK; }
int32_t fake_toggle() { ++toggles; return RSF_OK; }
int32_t fake_capture(rsf_overlay_capture* out) { ++capture_queries; out->captured = capture_bits; return RSF_OK; }
int32_t fake_status(rsf_render_status*) { return RSF_OK; }
int32_t fake_settings_get(rsf_render_settings*) { return RSF_OK; }
int32_t fake_settings_apply(const rsf_render_settings*) { return RSF_OK; }

HWND window_with_class(const wchar_t* name) {
    WNDCLASSW cls{};
    cls.lpfnWndProc = DefWindowProcW; cls.hInstance = GetModuleHandleW(nullptr); cls.lpszClassName = name;
    RegisterClassW(&cls);
    return CreateWindowExW(0, name, L"t", WS_POPUP, 0, 0, 64, 64, nullptr, nullptr, GetModuleHandleW(nullptr), nullptr);
}

struct Chain {
    ID3D11Device* device{};
    ID3D11DeviceContext* context{};
    IDXGISwapChain* chain{};
    ~Chain() { if (chain) chain->Release(); if (context) context->Release(); if (device) device->Release(); }
    bool create(HWND window) {
        DXGI_SWAP_CHAIN_DESC desc{};
        desc.BufferCount = 1; desc.BufferDesc.Width = 64; desc.BufferDesc.Height = 64;
        desc.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM; desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
        desc.OutputWindow = window; desc.SampleDesc.Count = 1; desc.Windowed = TRUE; desc.SwapEffect = DXGI_SWAP_EFFECT_DISCARD;
        return SUCCEEDED(D3D11CreateDeviceAndSwapChain(nullptr, D3D_DRIVER_TYPE_WARP, nullptr, 0, nullptr, 0,
            D3D11_SDK_VERSION, &desc, &chain, &device, nullptr, &context));
    }
};

bool dxgi_boundaries() {
    HWND engine = window_with_class(L"_XRAY_1.5");
    HWND other = window_with_class(L"RSF_NOT_THE_ENGINE");
    Chain game, side;
    if (!engine || !other || !game.create(engine) || !side.create(other)) return false;

    presented.clear(); notified.clear();
    const HRESULT first_present = game.chain->Present(0, 0);
    check(first_present == S_OK || first_present == DXGI_STATUS_OCCLUDED, "engine chain presents");
    check(!presented.empty() && presented.front() == game.chain, "api.present ran for the engine chain, before the real Present");
    const size_t after_first = presented.size();
    game.chain->Present(0, DXGI_PRESENT_TEST);
    check(presented.size() == after_first, "DXGI_PRESENT_TEST is not an overlay frame");
    side.chain->Present(0, 0);
    check(presented.size() == after_first, "a chain on another window never reaches the overlay");
    check(notified.empty(), "no reset notification for the first chain");
    check(toggles == 0, "no Insert press, no fallback toggle");

    check(SUCCEEDED(game.chain->ResizeBuffers(1, 32, 32, DXGI_FORMAT_UNKNOWN, 0)), "ResizeBuffers still works through the hook");
    check(notified.size() == 1 && notified[0] == RSF_OVERLAY_NOTIFY_RESIZE, "ResizeBuffers notified RESIZE before forwarding");

    // The engine recreating its device and chain: the panel must release the old one first.
    Chain second;
    if (!second.create(engine)) return false;
    notified.clear();
    second.chain->Present(0, 0);
    check(notified.size() == 1 && notified[0] == RSF_OVERLAY_NOTIFY_DEVICE_RESET, "a new chain/device notified DEVICE_RESET");
    check(presented.back() == second.chain, "and then drew on the new chain");
    DestroyWindow(engine); DestroyWindow(other);
    return true;
}

constexpr GUID guid_mouse{0x6F1D2B60, 0xD5A0, 0x11CF, {0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00}};
constexpr GUID guid_keyboard{0x6F1D2B61, 0xD5A0, 0x11CF, {0xBF, 0xC7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00}};
constexpr GUID iid_input_a{0xBF798030, 0x483A, 0x4DA2, {0xAA, 0x99, 0x5D, 0x64, 0xED, 0x36, 0x97, 0x00}};

// The engine's path: DirectInput8Create, CreateDevice on the system mouse, then polling. Returns false
// to skip when the machine has no such device.
bool directinput_boundaries() {
    wchar_t path[MAX_PATH]{};
    GetSystemDirectoryW(path, MAX_PATH);
    wcscat_s(path, L"\\dinput8.dll");
    HMODULE module = LoadLibraryW(path);
    if (!module) return false;
    using Create = HRESULT (WINAPI*)(HINSTANCE, DWORD, REFIID, LPVOID*, LPUNKNOWN);
    const auto create = reinterpret_cast<Create>(reinterpret_cast<void*>(GetProcAddress(module, "DirectInput8Create")));
    IDirectInput8A* input = nullptr;
    if (!create || FAILED(create(GetModuleHandleW(nullptr), 0x0800, iid_input_a, reinterpret_cast<void**>(&input), nullptr)) || !input)
        return false;
    IDirectInputDevice8A* mouse = nullptr;
    IDirectInputDevice8A* keyboard = nullptr;
    if (FAILED(input->CreateDevice(guid_mouse, &mouse, nullptr)) || FAILED(input->CreateDevice(guid_keyboard, &keyboard, nullptr))) {
        if (mouse) mouse->Release();
        input->Release();
        return false;
    }
    check(input_gate().tracked(mouse), "a mouse created after start is registered with the gate");
    check(input_gate().tracked(keyboard), "a keyboard created after start is registered with the gate");

    HWND window = window_with_class(L"RSF_DINPUT_TEST");
    const bool configured = SUCCEEDED(mouse->SetDataFormat(&c_dfDIMouse)) &&
        SUCCEEDED(mouse->SetCooperativeLevel(window, DISCL_NONEXCLUSIVE | DISCL_BACKGROUND)) && SUCCEEDED(mouse->Acquire());
    if (!configured) { mouse->Release(); keyboard->Release(); input->Release(); return false; }

    DIMOUSESTATE state{};
    capture_bits = 0; capture_queries = 0;
    check(SUCCEEDED(mouse->GetDeviceState(sizeof(state), &state)), "closed panel: the real mouse state is returned");
    check(capture_queries > 0, "the engine's call went through the gate (capture was consulted)");

    // Captured: whatever the real mouse is doing, the engine sees an idle one.
    capture_bits = RSF_OVERLAY_CAPTURE_MOUSE;
    std::memset(&state, 0xab, sizeof(state));
    check(mouse->GetDeviceState(sizeof(state), &state) == DI_OK, "captured: the call succeeds");
    check(state.lX == 0 && state.lY == 0 && state.lZ == 0 && !(state.rgbButtons[0] | state.rgbButtons[1] | state.rgbButtons[2]),
        "captured: the mouse state the engine reads is idle");
    DIDEVICEOBJECTDATA events[16]{};
    DWORD count = 16;
    check(mouse->GetDeviceData(sizeof(DIDEVICEOBJECTDATA), events, &count, 0) == DI_OK && count == 0, "captured: no buffered events reach the engine");
    capture_bits = 0;

    mouse->Unacquire();
    mouse->Release(); keyboard->Release(); input->Release();
    DestroyWindow(window);
    return true;
}
}

int main() {
    auto api = rsf_overlay_api{};
    api.struct_size = sizeof(api); api.abi_version = RSF_OVERLAY_API_VERSION;
    api.configure = fake_configure; api.present = fake_present; api.notify = fake_notify; api.visible = fake_visible;
    api.set_visible = fake_set_visible; api.toggle = fake_toggle; api.capture = fake_capture; api.status = fake_status;
    api.settings_get = fake_settings_get; api.settings_apply = fake_settings_apply;

    StartOptions options{};
    options.configuration = L"C:\\rsf-test\\ReScaleFrame.ini";
    options.api_override = &api;
    const char* reason = nullptr;
    const auto started = start(options, &reason);
    if (started == StartStatus::hooks_refused && reason && std::strstr(reason, "dummy swap chain")) {
        std::puts("SKIP: no WARP device"); return 77;
    }
    check(started == StartStatus::started, "start() installs the input and Present hooks");
    if (started != StartStatus::started) { std::printf("reason: %s\n", reason ? reason : "(none)"); return 1; }
    check(start(options, &reason) == StartStatus::already_started, "hooks are installed once per process");

    if (!dxgi_boundaries()) { std::puts("SKIP: WARP chain unavailable"); return 77; }
    const bool input_ran = directinput_boundaries();
    if (failures) { std::printf("%d failure(s)\n", failures); return 1; }
    if (!input_ran) { std::puts("Anomaly overlay hooks: DXGI PASS; DirectInput SKIPPED (no mouse/keyboard device)"); return 0; }
    std::puts("Anomaly overlay hooks: PASS");
    return 0;
}
