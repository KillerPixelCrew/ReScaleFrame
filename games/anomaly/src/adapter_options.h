// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// Every adapter option, as the shared overlay shows it (AGENTS.md Rule 2). Stages add the options they can change
// while the game runs; register_restart_options() adds the ones read only at startup, so they can still be set and
// saved from the panel. The overlay service (overlay_service.cpp) hands the list to the Runtime's overlay API.
//
// Threading: the list is built at cold start, before the overlay starts. get/set run on the Present-owner thread,
// which is the engine's render thread, the same thread the stages read these values on.
#include <windows.h>
#include <cstdint>
#include <cstdio>
#include <cwchar>
#include <string>
#include <vector>

namespace rsf::anomaly::options {
struct Option {
    const char* label{};
    bool toggle{true};
    int32_t minimum{}, maximum{1}, step{1};
    uint32_t divisor{1};          // a number is shown as value / divisor
    bool restart{};               // a change applies at the next start
    int32_t (*get)() noexcept {};
    void (*set)(int32_t) noexcept {};   // applies the value when live, and saves it
};
inline std::vector<Option>& registry() { static std::vector<Option> list; return list; }
inline std::wstring& ini_path() { static std::wstring path; return path; }
inline void add(const Option& option) noexcept try { registry().push_back(option); } catch (...) {}

inline int32_t read_int(const wchar_t* key, int32_t fallback) noexcept {
    return ini_path().empty() ? fallback : static_cast<int32_t>(GetPrivateProfileIntW(L"Anomaly", key, fallback, ini_path().c_str()));
}
inline void write_text(const wchar_t* key, const wchar_t* value) noexcept {
    if (!ini_path().empty()) WritePrivateProfileStringW(L"Anomaly", key, value, ini_path().c_str());
}
inline void write_int(const wchar_t* key, int32_t value) noexcept {
    wchar_t text[16]{};
    std::swprintf(text, 16, L"%d", value);
    write_text(key, text);
}

// Options that are only read at startup: the panel saves them for the next start.
inline void register_restart_options(const wchar_t* ini) noexcept {
    if (ini) try { ini_path() = ini; } catch (...) {}
    add({"SR stage (0 off .. 5 temporal)", false, 0, 5, 1, 1, true,
        []() noexcept { return read_int(L"SRStage", 0); }, [](int32_t v) noexcept { write_int(L"SRStage", v); }});
    add({"Frame generation bridge", true, 0, 1, 1, 1, true,
        []() noexcept { return read_int(L"FGBridge", 1); }, [](int32_t v) noexcept { write_int(L"FGBridge", v); }});
    add({"F10 frame capture", true, 0, 1, 1, 1, true,
        []() noexcept { return read_int(L"SRStageCapture", 0); },
        [](int32_t v) noexcept { write_int(L"SRStageCapture", v); if (v) write_int(L"SRStageCaptureCount", 0); }});
}
}
