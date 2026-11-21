// SPDX-License-Identifier: MIT
// Anomaly input policy for the shared overlay. The engine polls buffered DirectInput mouse and
// keyboard devices, so a window subclass alone cannot keep the game from acting on input while the
// panel is open. This gate sits between the engine and the real device methods. It owns only the
// decision (discard, flush, release, unacquire); the hooks that route calls here are in
// overlay_hooks.cpp and the capture query is supplied by the service. The gate state and decisions
// live in the adapter's Rust library (input.rs); this class owns one gate and forwards to it.
#pragma once
#include <windows.h>
#ifndef DIRECTINPUT_VERSION
#define DIRECTINPUT_VERSION 0x0800
#endif
#include <dinput.h>
#include <cstddef>
#include <cstdint>

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

struct BufferedRead {
    const void* data;
    const DWORD* count;
    HRESULT result;
    DWORD flags;
    bool hardware;
};
using InputObserver = void (*)(DeviceKind) noexcept;
using ObserverQuery = InputObserver (*)() noexcept;

// The DirectInput values input.rs uses.
static_assert(sizeof(DIDEVICEOBJECTDATA) == 24 && offsetof(DIDEVICEOBJECTDATA, dwTimeStamp) == 8);
static_assert(DIMOFS_BUTTON0 == 12 && DIMOFS_BUTTON7 == 19 && DIGDD_PEEK == 1 && DISCL_EXCLUSIVE == 1);
static_assert(DIERR_INPUTLOST == HRESULT(0x8007001EL) && DIERR_NOTACQUIRED == HRESULT(0x8007000CL));
static_assert(sizeof(BufferedRead) == 32 && offsetof(BufferedRead, hardware) == 24);
}

extern "C" {
struct rsf_anomaly_input_gate;
rsf_anomaly_input_gate* rsf_anomaly_input_gate_create(uint32_t (*capture)(void*) noexcept, void* user) noexcept;
void rsf_anomaly_input_gate_destroy(rsf_anomaly_input_gate*) noexcept;
void rsf_anomaly_input_gate_add(rsf_anomaly_input_gate*, void* device, uint8_t kind) noexcept;
void rsf_anomaly_input_gate_remove(rsf_anomaly_input_gate*, void* device) noexcept;
void rsf_anomaly_input_gate_cooperative_level(rsf_anomaly_input_gate*, void* device, DWORD flags) noexcept;
bool rsf_anomaly_input_gate_tracked(rsf_anomaly_input_gate*, void* device) noexcept;
HRESULT rsf_anomaly_input_gate_acquire(rsf_anomaly_input_gate*, void* device,
    const rsf::anomaly::overlay::DeviceCalls*) noexcept;
HRESULT rsf_anomaly_input_gate_unacquire(rsf_anomaly_input_gate*, void* device,
    const rsf::anomaly::overlay::DeviceCalls*) noexcept;
HRESULT rsf_anomaly_input_gate_get_state(rsf_anomaly_input_gate*, void* device,
    const rsf::anomaly::overlay::DeviceCalls*, DWORD size, void* data) noexcept;
HRESULT rsf_anomaly_input_gate_get_data(rsf_anomaly_input_gate*, void* device,
    const rsf::anomaly::overlay::DeviceCalls*, DWORD object_size, DIDEVICEOBJECTDATA* data, DWORD* count, DWORD flags,
    bool* hardware_data) noexcept;
void rsf_anomaly_input_gate_buffered_read(rsf_anomaly_input_gate*, void* device,
    const rsf::anomaly::overlay::BufferedRead*, rsf::anomaly::overlay::ObserverQuery) noexcept;
}

namespace rsf::anomaly::overlay {
// While the panel captures a device kind:
//  - GetDeviceState reports an idle device; GetDeviceData reports no events. Real input is drained
//    so nothing accumulates, and genuine device loss still reaches the engine.
//  - The first call after the capture state changes flushes the device buffer (both edges).
//  - Keys and buttons the engine saw pressed get synthetic release events when capture begins, so
//    its own key table cannot stay stuck down after a release it never receives.
//  - An exclusive mouse is unacquired for the duration (and reacquired on the falling edge) so the
//    window receives WM_MOUSE*. The engine's own Acquire calls are answered without acquiring.
// Closed panel: every call goes straight to the real method. Untracked devices are never touched.
// `hardware_data` reports only records the device delivered and the engine received, never the
// synthetic releases, peeks or captured reads.
class InputGate {
public:
    using Capture = uint32_t (*)(void* user) noexcept;
    InputGate(Capture capture, void* user) noexcept : gate_(rsf_anomaly_input_gate_create(capture, user)) {}
    ~InputGate() { rsf_anomaly_input_gate_destroy(gate_); }
    InputGate(const InputGate&) = delete;
    InputGate& operator=(const InputGate&) = delete;

    // The same address may be reused by a later device; adding it again replaces the old record.
    void add(void* device, DeviceKind kind) noexcept { rsf_anomaly_input_gate_add(gate_, device, uint8_t(kind)); }
    void remove(void* device) noexcept { rsf_anomaly_input_gate_remove(gate_, device); }
    void cooperative_level(void* device, DWORD flags) noexcept {
        rsf_anomaly_input_gate_cooperative_level(gate_, device, flags);
    }
    bool tracked(void* device) noexcept { return rsf_anomaly_input_gate_tracked(gate_, device); }

    HRESULT acquire(void* device, const DeviceCalls& calls) noexcept {
        return rsf_anomaly_input_gate_acquire(gate_, device, &calls);
    }
    HRESULT unacquire(void* device, const DeviceCalls& calls) noexcept {
        return rsf_anomaly_input_gate_unacquire(gate_, device, &calls);
    }
    HRESULT get_state(void* device, const DeviceCalls& calls, DWORD size, void* data) noexcept {
        return rsf_anomaly_input_gate_get_state(gate_, device, &calls, size, data);
    }
    HRESULT get_data(void* device, const DeviceCalls& calls, DWORD object_size, DIDEVICEOBJECTDATA* data,
        DWORD* count, DWORD flags, bool* hardware_data = nullptr) noexcept {
        return rsf_anomaly_input_gate_get_data(gate_, device, &calls, object_size, data, count, flags, hardware_data);
    }
    void buffered_read(void* device, const BufferedRead& read, ObserverQuery query) noexcept {
        rsf_anomaly_input_gate_buffered_read(gate_, device, &read, query);
    }

private:
    rsf_anomaly_input_gate* gate_;
};
}
