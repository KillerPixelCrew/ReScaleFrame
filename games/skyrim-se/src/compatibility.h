// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <cstdint>
namespace rsf::skyrim {
// Arms the process-lived initializer boundary before CS is present. Already
// initialized or unknown profiles refuse; desired settings are never overwritten.
bool compatibility_checkpoint(const char*& refusal, void (*withdraw)() noexcept = nullptr) noexcept;
bool compatibility_render_allowed() noexcept;
const char* compatibility_status() noexcept;
// Either the exact native caller or the guarded CS HDR wrapper's original-call
// scope. All five arguments remain native, including the fifth stack byte.
bool compatibility_postprocess_call(uintptr_t caller, void*, uint32_t, uint32_t, uintptr_t, uint8_t) noexcept;
}
