/* SPDX-License-Identifier: MIT */
#pragma once
/* The [UnitySR] settings both Unity routes read: the game plugin's runtime setup and the Unity
   package. Both link the rsf-unity-mono archive, whose runtime_config.rs owns these, so a relative
   path resolves the same way in both. */
#include <stddef.h>
#include <stdint.h>
#include <wchar.h>
#ifdef __cplusplus
#include <string>
extern "C" {
#endif
/* [UnitySR] <name> from `ini`, with its terminator, into `output`. A relative value is resolved
   against the ini's directory and canonicalised with GetFullPathNameW. Returns the length without
   the terminator; 0 when unset, when the ini path has no directory, or when it does not fit. */
uint32_t rsf_unity_config_path_setting(const wchar_t* ini, const wchar_t* name, wchar_t* output,
    uint32_t capacity);
/* UTF-16 to UTF-8 without a terminator. Returns the byte count; 0 for invalid UTF-16 or when it
   does not fit. */
size_t rsf_unity_config_utf8(const wchar_t* text, size_t length, char* output, size_t capacity);
#ifdef __cplusplus
}

namespace rsf::unity_config {
/* Empty when unset or unresolved. */
inline std::wstring path_setting(const wchar_t* ini, const wchar_t* name)
{
    std::wstring value(32768, L'\0');
    value.resize(rsf_unity_config_path_setting(ini, name, value.data(), static_cast<uint32_t>(value.size())));
    return value;
}
/* Empty on failure. A UTF-16 unit never needs more than three UTF-8 bytes. */
inline std::string utf8(const std::wstring& value)
{
    if (value.empty()) return {};
    std::string result(value.size() * 3, '\0');
    result.resize(rsf_unity_config_utf8(value.data(), value.size(), result.data(), result.size()));
    return result;
}
}
#endif
