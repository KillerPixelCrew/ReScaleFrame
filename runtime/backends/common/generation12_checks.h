/* SPDX-License-Identifier: MIT */
#pragma once
#include "fg_helpers.h"
#include "generation12_platform.h"

namespace rsf {
inline const rsf_gen12_checks generation12_checks{
    fg_setup, fg_frame, fg_command, []() -> uint32_t { return GetModuleHandleW(L"sl.dlss_g.dll") != nullptr; },
};
}
