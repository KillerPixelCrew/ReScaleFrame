// SPDX-License-Identifier: MIT
#include <windows.h>

namespace {
constexpr wchar_t class_name[] = L"RsfXeSSInteropFixture";
}

extern "C" int rsf_fixture_xess_window_open(void** output) {
    WNDCLASSW klass{};
    klass.lpfnWndProc = DefWindowProcW;
    klass.hInstance = GetModuleHandleW(nullptr);
    klass.lpszClassName = class_name;
    if (!RegisterClassW(&klass)) return 100;
    HWND window = CreateWindowExW(0, class_name, L"ReScaleFrame XeSS interop fixture",
        WS_OVERLAPPEDWINDOW, CW_USEDEFAULT, CW_USEDEFAULT, 1296, 759, nullptr, nullptr,
        klass.hInstance, nullptr);
    if (!window) {
        UnregisterClassW(class_name, klass.hInstance);
        return 101;
    }
    ShowWindow(window, SW_SHOWNORMAL);
    SetForegroundWindow(window);
    *output = window;
    return 0;
}

extern "C" void rsf_fixture_xess_window_pump() {
    MSG message{};
    while (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) {
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
}

extern "C" void rsf_fixture_xess_window_close(void* window) {
    DestroyWindow(static_cast<HWND>(window));
    UnregisterClassW(class_name, GetModuleHandleW(nullptr));
}
