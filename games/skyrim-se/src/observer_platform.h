// SPDX-License-Identifier: MIT
#pragma once
#include <cstdint>

// The typed native observer continues through the trampoline published by observers.rs.
extern "C" void* rsf_skyrim_observer_original(uint32_t) noexcept;
