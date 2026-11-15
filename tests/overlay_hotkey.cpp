// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/overlay_input.h>
#include <MinHook.h>
#include <windows.h>
#include <cstdio>

namespace {
bool down{};
HWND foreground{};
int failures{};
SHORT WINAPI key_state(int key) { return key == VK_INSERT && down ? SHORT(-32768) : SHORT(0); }
HWND WINAPI foreground_window() { return foreground; }
void check(bool ok, const char* message) { if (!ok) { ++failures; std::fprintf(stderr, "FAIL %s\n", message); } }
}
int main() {
    WNDCLASSW wc{}; wc.lpfnWndProc = DefWindowProcW; wc.hInstance = GetModuleHandleW(nullptr); wc.lpszClassName = L"RsfHotkeyFixture";
    if (!RegisterClassW(&wc)) return 1;
    HWND window = CreateWindowW(wc.lpszClassName, L"", WS_OVERLAPPEDWINDOW, 0, 0, 64, 64, nullptr, nullptr, wc.hInstance, nullptr);
    if (!window) return 1;
    // Mock only this process's key/focus queries. No SendInput, real key presses or focus changes.
    if (MH_Initialize() != MH_OK) return 1;
    auto* keys = reinterpret_cast<void*>(GetProcAddress(GetModuleHandleW(L"user32.dll"), "GetAsyncKeyState"));
    auto* focus = reinterpret_cast<void*>(GetProcAddress(GetModuleHandleW(L"user32.dll"), "GetForegroundWindow"));
    if (MH_CreateHook(keys, reinterpret_cast<void*>(&key_state), nullptr) != MH_OK ||
        MH_CreateHook(focus, reinterpret_cast<void*>(&foreground_window), nullptr) != MH_OK ||
        MH_EnableHook(keys) != MH_OK || MH_EnableHook(focus) != MH_OK) return 1;
    rsf_overlay_input_options options{sizeof(options), RSF_OVERLAY_INPUT_ABI_VERSION, VK_INSERT, nullptr, nullptr};
    check(rsf_overlay_input_install(window, &options) == 0, "install on hidden fixture window");
    foreground = window; rsf_overlay_input_poll_toggle();
    check(!rsf_overlay_input_visible(), "poll starts closed");
    down = true; rsf_overlay_input_poll_toggle();
    check(rsf_overlay_input_visible() != 0, "Insert opens without a window message");
    Sleep(280); rsf_overlay_input_poll_toggle();
    SendMessageW(window, WM_KEYDOWN, VK_INSERT, 1);
    check(rsf_overlay_input_visible() != 0, "held key and delayed message do not toggle twice");
    down = false; rsf_overlay_input_poll_toggle(); SendMessageW(window, WM_KEYUP, VK_INSERT, 0xc0000001);
    down = true; rsf_overlay_input_poll_toggle();
    check(!rsf_overlay_input_visible(), "second physical edge closes without messages");
    down = false; rsf_overlay_input_poll_toggle();
    foreground = nullptr; down = true; rsf_overlay_input_poll_toggle(); Sleep(260);
    check(!rsf_overlay_input_visible(), "background presses do not open panel");
    foreground = window; rsf_overlay_input_poll_toggle();
    check(!rsf_overlay_input_visible(), "regaining focus with Insert held is not another press");
    down = false; rsf_overlay_input_poll_toggle(); down = true; rsf_overlay_input_poll_toggle();
    check(rsf_overlay_input_visible() != 0, "fresh foreground press opens");
    down = false; rsf_overlay_input_poll_toggle(); Sleep(260);
    down = true; SendMessageW(window, WM_KEYDOWN, VK_INSERT, 1); rsf_overlay_input_poll_toggle();
    check(!rsf_overlay_input_visible(), "message-first route and polling share the edge");
    SendMessageW(window, WM_KEYDOWN, VK_INSERT, 0x40000001);
    check(!rsf_overlay_input_visible(), "auto repeat stays closed");
    down = false; SendMessageW(window, WM_KEYUP, VK_INSERT, 0xc0000001); rsf_overlay_input_poll_toggle();
    check(rsf_overlay_input_uninstall() == 0, "restore fixture window");
    MH_DisableHook(keys); MH_DisableHook(focus); MH_RemoveHook(keys); MH_RemoveHook(focus);
    DestroyWindow(window); UnregisterClassW(wc.lpszClassName, wc.hInstance);
    return failures ? 1 : 0;
}
