// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// Shared persistence for rsf_render_settings in an adapter-supplied INI section. The keys are
// the render frontend's; the file and section always come from the adapter.
#include <rescaleframe/game_api.h>
#include <rescaleframe/render_frontend.h>
#include <windows.h>
#include <cwchar>
#include <iterator>
#include <string>
#include <vector>

namespace rsf::render_settings {
inline constexpr const wchar_t* keys[] = {L"SR", L"SRBackend", L"DLSSPreset", L"FG", L"FGBackend",
    L"FGGeneratedFrames", L"Latency", L"FrameLimitUs", L"StatusOverlay", L"SRQuality"};

inline rsf_render_settings read(const std::wstring& ini, const std::wstring& section, const rsf_render_settings& defaults)
{
    auto value = [&](const wchar_t* key, uint32_t fallback) {
        return static_cast<uint32_t>(GetPrivateProfileIntW(section.c_str(), key, static_cast<INT>(fallback), ini.c_str()));
    };
    rsf_render_settings out{sizeof(out), RSF_RENDER_FRONTEND_ABI_VERSION};
    out.sr_enabled = value(L"SR", defaults.sr_enabled) != 0;
    out.sr_backend = value(L"SRBackend", defaults.sr_backend);
    out.dlss_preset = value(L"DLSSPreset", defaults.dlss_preset);
    out.fg_enabled = value(L"FG", defaults.fg_enabled) != 0;
    out.fg_backend = value(L"FGBackend", defaults.fg_backend);
    out.generated_frames = value(L"FGGeneratedFrames", defaults.generated_frames);
    out.latency = value(L"Latency", defaults.latency);
    out.frame_limit_us = value(L"FrameLimitUs", defaults.frame_limit_us);
    out.status_overlay = value(L"StatusOverlay", defaults.status_overlay) != 0;
    out.sr_quality = value(L"SRQuality", defaults.sr_quality);
    return out;
}

// Rewrites the owned keys in one section write and keeps every other entry of the section.
inline rsf_result write(const std::wstring& ini, const std::wstring& section, const rsf_render_settings& request)
{
    const uint32_t values[] = {request.sr_enabled, request.sr_backend, request.dlss_preset, request.fg_enabled,
        request.fg_backend, request.generated_frames, request.latency, request.frame_limit_us, request.status_overlay,
        request.sr_quality};
    std::vector<wchar_t> buffer(32768);
    const auto length = GetPrivateProfileSectionW(section.c_str(), buffer.data(), static_cast<DWORD>(buffer.size()), ini.c_str());
    if (length >= buffer.size() - 2) return RSF_ERROR_INVALID_ARGUMENT;
    std::wstring updated;
    for (const wchar_t* entry = buffer.data(); *entry; entry += std::wcslen(entry) + 1) {
        bool owned = false;
        for (const auto* key : keys) {
            const auto size = std::wcslen(key);
            if (!_wcsnicmp(entry, key, size) && entry[size] == L'=') owned = true;
        }
        if (!owned) { updated += entry; updated += L'\0'; }
    }
    for (size_t i = 0; i < std::size(keys); ++i) {
        updated += keys[i]; updated += L'='; updated += std::to_wstring(values[i]); updated += L'\0';
    }
    updated += L'\0';
    return WritePrivateProfileSectionW(section.c_str(), updated.c_str(), ini.c_str()) ? RSF_OK : RSF_ERROR_NOT_READY;
}
}
