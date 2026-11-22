// SPDX-License-Identifier: MIT
// SDK checks and the small C++ handle used by the carrier and the live-hook fixture.
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

using InputObserver = void (*)(DeviceKind) noexcept;

// The DirectInput values input/gate.rs uses.
static_assert(sizeof(DIDEVICEOBJECTDATA) == 24 && offsetof(DIDEVICEOBJECTDATA, dwTimeStamp) == 8);
static_assert(DIMOFS_BUTTON0 == 12 && DIMOFS_BUTTON7 == 19 && DIGDD_PEEK == 1 && DISCL_EXCLUSIVE == 1);
static_assert(DIERR_INPUTLOST == HRESULT(0x8007001EL) && DIERR_NOTACQUIRED == HRESULT(0x8007000CL));
}

extern "C" {
struct rsf_anomaly_input_gate;
rsf_anomaly_input_gate* rsf_anomaly_input_gate_create(uint32_t (*capture)(void*) noexcept, void* user) noexcept;
void rsf_anomaly_input_gate_destroy(rsf_anomaly_input_gate*) noexcept;
bool rsf_anomaly_input_gate_tracked(rsf_anomaly_input_gate*, void* device) noexcept;
}

namespace rsf::anomaly::overlay {
class InputGate {
public:
    using Capture = uint32_t (*)(void* user) noexcept;
    InputGate(Capture capture, void* user) noexcept : gate_(rsf_anomaly_input_gate_create(capture, user)) {}
    ~InputGate() { rsf_anomaly_input_gate_destroy(gate_); }
    InputGate(const InputGate&) = delete;
    InputGate& operator=(const InputGate&) = delete;
    uintptr_t handle() const noexcept { return reinterpret_cast<uintptr_t>(gate_); }

    bool tracked(void* device) const noexcept { return rsf_anomaly_input_gate_tracked(gate_, device); }

private:
    rsf_anomaly_input_gate* gate_;
};
}
