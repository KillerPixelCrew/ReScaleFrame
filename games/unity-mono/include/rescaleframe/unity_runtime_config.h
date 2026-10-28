/* SPDX-License-Identifier: GPL-3.0-only */
#pragma once
/* The [UnitySR] settings both Unity entry points read (the SR host and the shared generation
   install), so a relative path and the frame-generation choice resolve the same way in both. */
#include <cstdint>
#include <string>

namespace rsf::unity_config {
/* [UnitySR] <name> from `ini`. A relative value is resolved against the ini's directory and
   canonicalised with GetFullPathNameW. Empty when unset or when the ini path has no directory. */
std::wstring path_setting(const wchar_t* ini, const wchar_t* name);
/* <ini directory>\ReScaleFrame\preferences.ini, where the overlay saves the next-start choice. */
std::wstring preferences_path(const wchar_t* ini);
/* Backend ID for the next start: the saved preference, else [UnitySR] FrameGeneration. The mask
   offers the four implemented providers. 0 is Off. */
uint32_t fg_choice_start(const wchar_t* ini);
/* UTF-16 to UTF-8, empty on failure. */
std::string utf8(const std::wstring& value);
}
