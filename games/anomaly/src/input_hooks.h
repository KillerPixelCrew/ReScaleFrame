// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "build_profile.h"
namespace rsf::anomaly::input {
bool install(std::byte* image, uint32_t size, const BuildProfile&, const char** reason) noexcept;
bool enable() noexcept;
bool disable() noexcept;
bool idle() noexcept;
// DirectInput hook reports kind bits only while this thread is executing the verified CInput callback.
void received(uint32_t kinds) noexcept;
}
