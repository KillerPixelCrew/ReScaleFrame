/* SPDX-License-Identifier: GPL-3.0-only */
#include <rescaleframe/unity_config.h>
#include <rescaleframe/fg_choice.h>
#include <windows.h>

namespace rsf::unity_config {
std::wstring path_setting(const wchar_t* ini, const wchar_t* name)
{
    wchar_t text[32768]{};
    GetPrivateProfileStringW(L"UnitySR", name, L"", text, 32768, ini);
    std::wstring value(text);
    if (value.empty() || (value.size() >= 2 && (value[1] == L':' || value[0] == L'\\'))) return value;
    std::wstring containing(ini);
    const auto separator = containing.find_last_of(L"/\\");
    if (separator == std::wstring::npos) return {};
    containing.resize(separator + 1); containing += value;
    wchar_t absolute[32768]{};
    const DWORD count = GetFullPathNameW(containing.c_str(), 32768, absolute, nullptr);
    return count && count < 32768 ? std::wstring(absolute, count) : std::wstring{};
}
std::wstring preferences_path(const wchar_t* ini)
{
    std::wstring preferences(ini);
    preferences.resize(preferences.find_last_of(L"/\\") + 1);
    preferences += L"ReScaleFrame\\preferences.ini";
    return preferences;
}
uint32_t fg_choice_start(const wchar_t* ini)
{
    const std::wstring preferences = preferences_path(ini);
    return rsf_fg_choice_start(preferences.c_str(), GetPrivateProfileIntW(L"UnitySR", L"FrameGeneration", 0, ini),
        RSF_FG_BACKEND_ALL);
}
std::string utf8(const std::wstring& value)
{
    if (value.empty()) return {};
    const int bytes = WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, value.data(), static_cast<int>(value.size()),
                                          nullptr, 0, nullptr, nullptr);
    std::string result(bytes > 0 ? static_cast<size_t>(bytes) : 0, '\0');
    if (bytes > 0) WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, value.data(), static_cast<int>(value.size()),
                                       result.data(), bytes, nullptr, nullptr);
    return result;
}
}
