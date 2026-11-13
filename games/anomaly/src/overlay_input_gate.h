// SPDX-License-Identifier: GPL-3.0-only
// Anomaly input policy for the shared overlay. The engine polls buffered DirectInput mouse and
// keyboard devices, so a window subclass alone cannot keep the game from acting on input while the
// panel is open. This gate sits between the engine and the real device methods. It owns only the
// decision (discard, flush, release, unacquire); the hooks that route calls here are in
// overlay_hooks.cpp and the capture query is supplied by the service.
#pragma once
#include <windows.h>
#ifndef DIRECTINPUT_VERSION
#define DIRECTINPUT_VERSION 0x0800
#endif
#include <dinput.h>
#include <bitset>
#include <cstdint>
#include <mutex>
#include <unordered_map>

namespace rsf::anomaly::overlay {
enum class DeviceKind : uint8_t { Mouse, Keyboard };
// Bits returned by the capture query. They equal RSF_OVERLAY_CAPTURE_MOUSE / _KEYBOARD
// (asserted in overlay_service.cpp), kept here so the gate does not depend on the Runtime header.
inline constexpr uint32_t capture_mouse = 0x1u;
inline constexpr uint32_t capture_keyboard = 0x2u;

// The real device methods. In production these are MinHook trampolines of the device's own class;
// the first argument is the IDirectInputDevice8 `this`.
struct DeviceCalls {
    HRESULT (STDMETHODCALLTYPE* acquire)(void*);
    HRESULT (STDMETHODCALLTYPE* unacquire)(void*);
    HRESULT (STDMETHODCALLTYPE* get_state)(void*, DWORD, void*);
    HRESULT (STDMETHODCALLTYPE* get_data)(void*, DWORD, DIDEVICEOBJECTDATA*, DWORD*, DWORD);
};

// While the panel captures a device kind:
//  - GetDeviceState reports an idle device; GetDeviceData reports no events. Real input is drained
//    so nothing accumulates, and genuine device loss still reaches the engine.
//  - The first call after the capture state changes flushes the device buffer (both edges).
//  - Keys and buttons the engine saw pressed get synthetic release events when capture begins, so
//    its own key table cannot stay stuck down after a release it never receives.
//  - An exclusive mouse is unacquired for the duration (and reacquired on the falling edge) so the
//    window receives WM_MOUSE*. The engine's own Acquire calls are answered without acquiring.
// Closed panel: every call goes straight to the real method. Untracked devices are never touched.
class InputGate {
public:
    using Capture = uint32_t (*)(void* user) noexcept;
    InputGate(Capture capture, void* user) noexcept : capture_(capture), user_(user) {}

    // The same address may be reused by a later device; adding it again replaces the old record.
    void add(void* device, DeviceKind kind);
    void remove(void* device);
    void cooperative_level(void* device, DWORD flags);
    bool tracked(void* device);
    bool device_kind(void* device, DeviceKind& kind);

    HRESULT acquire(void* device, const DeviceCalls&);
    HRESULT unacquire(void* device, const DeviceCalls&);
    HRESULT get_state(void* device, const DeviceCalls&, DWORD size, void* data);
    HRESULT get_data(void* device, const DeviceCalls&, DWORD object_size, DIDEVICEOBJECTDATA* data,
        DWORD* count, DWORD flags, bool* hardware_data = nullptr);

private:
    struct Entry {
        DeviceKind kind{};
        bool captured{}, exclusive{}, game_acquired{}, gate_unacquired{};
        // Indexed by dwOfs: keyboard DIK_* codes, mouse DIMOFS_BUTTON0..7.
        std::bitset<256> down, release_pending;
    };
    bool update(Entry&, void* device, const DeviceCalls&);
    static uint32_t emit_pending(Entry&, DIDEVICEOBJECTDATA*, DWORD capacity, DWORD stride, bool consume);
    static void track(Entry&, const DIDEVICEOBJECTDATA*, DWORD count, DWORD stride);

    Capture capture_;
    void* user_;
    std::mutex guard_;
    std::unordered_map<void*, Entry> devices_;
};
}
