// SPDX-License-Identifier: MIT
#pragma once
#include "build_profile.h"
namespace rsf::anomaly::overlay { enum class DeviceKind : uint8_t; }  // overlay_input_gate.h
namespace rsf::anomaly::input {
bool install(std::byte* image, uint32_t size, const BuildProfile&, const char** reason) noexcept;
bool enable() noexcept;
bool disable() noexcept;
bool idle() noexcept;
// DirectInput hook reports a device kind whose hardware records the engine received. It becomes an input
// fact only while this thread is executing the verified CInput callback.
void received(overlay::DeviceKind kind) noexcept;
}
