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
#include <cstdlib>
#include <cmath>

namespace rsf::render_settings {
inline constexpr const wchar_t* keys[] = {L"SR", L"SRBackend", L"DLSSPreset", L"FG", L"FGBackend",
    L"FGGeneratedFrames", L"Latency", L"FrameLimitUs", L"StatusOverlay", L"SRQuality", L"SharpeningMode", L"Sharpness", L"MAS", L"MotionSharpness", L"MotionThreshold", L"MotionScaleLimit", L"ContrastEnabled", L"Contrast", L"DADepthScale", L"DADepthBias", L"DAClampOutput", L"SharpeningDebug"};

inline rsf_sharpening_options sharpening(const rsf_render_settings& s) {
    auto o = rsf_sharpening_defaults();
    o.mode = s.sharpening_mode;
    o.sharpness = s.sharpening_sharpness;
    o.mas = s.sharpening_mas;
    o.motion_sharpness = s.sharpening_motion_sharpness;
    o.motion_threshold = s.sharpening_motion_threshold;
    o.motion_scale_limit = s.sharpening_motion_scale_limit;
    o.contrast_enabled = s.sharpening_contrast_enabled;
    o.contrast = s.sharpening_contrast;
    o.depth_scale = s.sharpening_depth_scale;
    o.depth_bias = s.sharpening_depth_bias;
    o.clamp_output = s.sharpening_clamp_output;
    o.debug = s.sharpening_debug;
    return o;
}
inline void sharpening(rsf_render_settings& s, const rsf_sharpening_options& o) {
    s.sharpening_mode = o.mode;
    s.sharpening_sharpness = o.sharpness;
    s.sharpening_mas = o.mas;
    s.sharpening_motion_sharpness = o.motion_sharpness;
    s.sharpening_motion_threshold = o.motion_threshold;
    s.sharpening_motion_scale_limit = o.motion_scale_limit;
    s.sharpening_contrast_enabled = o.contrast_enabled;
    s.sharpening_contrast = o.contrast;
    s.sharpening_depth_scale = o.depth_scale;
    s.sharpening_depth_bias = o.depth_bias;
    s.sharpening_clamp_output = o.clamp_output;
    s.sharpening_debug = o.debug;
}

// Existing host aggregate initializers leave the appended options empty.
inline void initialize_sharpening(rsf_render_settings& s) {
    if (s.sharpening_mode == 0 && s.sharpening_sharpness == 0 && s.sharpening_mas == 0 && s.sharpening_motion_sharpness == 0 && s.sharpening_motion_threshold == 0 && s.sharpening_motion_scale_limit == 0 && s.sharpening_contrast_enabled == 0 && s.sharpening_contrast == 0 && s.sharpening_depth_scale == 0 && s.sharpening_depth_bias == 0 && s.sharpening_clamp_output == 0 && s.sharpening_debug == 0)
        sharpening(s, rsf_sharpening_defaults());
}

inline std::wstring decimal(float value) {
    wchar_t text[64]{};
    std::swprintf(text, std::size(text), L"%.9g", static_cast<double>(value));
    return text;
}

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
    auto number = [&](const wchar_t* key, float fallback) {
        wchar_t buffer[80]{};
        const auto text = decimal(fallback);
        GetPrivateProfileStringW(section.c_str(),key,text.c_str(),buffer,80,ini.c_str());
        wchar_t* end = nullptr; const float result = std::wcstof(buffer,&end);
        return end != buffer && !*end ? result : fallback;
    };
    out.sharpening_mode = value(L"SharpeningMode", 0);
    out.sharpening_sharpness = number(L"Sharpness", 0.4f);
    out.sharpening_mas = value(L"MAS", 0);
    out.sharpening_motion_sharpness = number(L"MotionSharpness", 0.2f);
    out.sharpening_motion_threshold = number(L"MotionThreshold", 0.0f);
    out.sharpening_motion_scale_limit = number(L"MotionScaleLimit", 10.0f);
    out.sharpening_contrast_enabled = value(L"ContrastEnabled", 0);
    out.sharpening_contrast = number(L"Contrast", 0.0f);
    out.sharpening_depth_scale = number(L"DADepthScale", 35.0f);
    out.sharpening_depth_bias = number(L"DADepthBias", 0.001f);
    out.sharpening_clamp_output = value(L"DAClampOutput", 0);
    out.sharpening_debug = value(L"SharpeningDebug", 0);
    return out;
}

// Rewrites the owned keys in one section write and keeps every other entry of the section.
inline rsf_result write(const std::wstring& ini, const std::wstring& section, const rsf_render_settings& request)
{
    const std::wstring values[] = {
        std::to_wstring(request.sr_enabled),
        std::to_wstring(request.sr_backend),
        std::to_wstring(request.dlss_preset),
        std::to_wstring(request.fg_enabled),
        std::to_wstring(request.fg_backend),
        std::to_wstring(request.generated_frames),
        std::to_wstring(request.latency),
        std::to_wstring(request.frame_limit_us),
        std::to_wstring(request.status_overlay),
        std::to_wstring(request.sr_quality),
        std::to_wstring(request.sharpening_mode),
        decimal(request.sharpening_sharpness),
        std::to_wstring(request.sharpening_mas),
        decimal(request.sharpening_motion_sharpness),
        decimal(request.sharpening_motion_threshold),
        decimal(request.sharpening_motion_scale_limit),
        std::to_wstring(request.sharpening_contrast_enabled),
        decimal(request.sharpening_contrast),
        decimal(request.sharpening_depth_scale),
        decimal(request.sharpening_depth_bias),
        std::to_wstring(request.sharpening_clamp_output),
        std::to_wstring(request.sharpening_debug)};
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
        updated += keys[i]; updated += L'='; updated += values[i]; updated += L'\0';
    }
    updated += L'\0';
    return WritePrivateProfileSectionW(section.c_str(), updated.c_str(), ini.c_str()) ? RSF_OK : RSF_ERROR_NOT_READY;
}
// The overlay's panel layout word (rsf_overlay_stats::ui_preferences). Opaque here: only the
// overlay interprets it. A separate key rather than a field of rsf_render_settings, which other
// ABIs embed; write() keeps it because it is not one of the owned keys.
inline constexpr const wchar_t* overlay_layout_key = L"OverlayLayout";
inline uint32_t read_overlay_layout(const std::wstring& ini, const std::wstring& section)
{
    return static_cast<uint32_t>(GetPrivateProfileIntW(section.c_str(), overlay_layout_key, 0, ini.c_str()));
}
inline bool write_overlay_layout(const std::wstring& ini, const std::wstring& section, uint32_t value)
{
    return WritePrivateProfileStringW(section.c_str(), overlay_layout_key, std::to_wstring(value).c_str(), ini.c_str()) != FALSE;
}

// Hosts with their own base SR settings persist only these shared post-pass keys.
inline rsf_sharpening_options read_sharpening(const std::wstring& ini,const std::wstring& section) {
    rsf_render_settings defaults{sizeof(defaults),RSF_RENDER_FRONTEND_ABI_VERSION};
    return rsf::sharpening_settings::sanitize(sharpening(read(ini,section,defaults)));
}
inline rsf_result write_sharpening(const std::wstring& ini,const std::wstring& section,const rsf_sharpening_options& o) {
    if (!rsf::sharpening_settings::valid(o)) return RSF_ERROR_INVALID_ARGUMENT;
    if (!WritePrivateProfileStringW(section.c_str(),L"SharpeningMode",std::to_wstring(o.mode).c_str(),ini.c_str())) return RSF_ERROR_NOT_READY;
    if (!WritePrivateProfileStringW(section.c_str(),L"Sharpness",decimal(o.sharpness).c_str(),ini.c_str())) return RSF_ERROR_NOT_READY;
    if (!WritePrivateProfileStringW(section.c_str(),L"MAS",std::to_wstring(o.mas).c_str(),ini.c_str())) return RSF_ERROR_NOT_READY;
    if (!WritePrivateProfileStringW(section.c_str(),L"MotionSharpness",decimal(o.motion_sharpness).c_str(),ini.c_str())) return RSF_ERROR_NOT_READY;
    if (!WritePrivateProfileStringW(section.c_str(),L"MotionThreshold",decimal(o.motion_threshold).c_str(),ini.c_str())) return RSF_ERROR_NOT_READY;
    if (!WritePrivateProfileStringW(section.c_str(),L"MotionScaleLimit",decimal(o.motion_scale_limit).c_str(),ini.c_str())) return RSF_ERROR_NOT_READY;
    if (!WritePrivateProfileStringW(section.c_str(),L"ContrastEnabled",std::to_wstring(o.contrast_enabled).c_str(),ini.c_str())) return RSF_ERROR_NOT_READY;
    if (!WritePrivateProfileStringW(section.c_str(),L"Contrast",decimal(o.contrast).c_str(),ini.c_str())) return RSF_ERROR_NOT_READY;
    if (!WritePrivateProfileStringW(section.c_str(),L"DADepthScale",decimal(o.depth_scale).c_str(),ini.c_str())) return RSF_ERROR_NOT_READY;
    if (!WritePrivateProfileStringW(section.c_str(),L"DADepthBias",decimal(o.depth_bias).c_str(),ini.c_str())) return RSF_ERROR_NOT_READY;
    if (!WritePrivateProfileStringW(section.c_str(),L"DAClampOutput",std::to_wstring(o.clamp_output).c_str(),ini.c_str())) return RSF_ERROR_NOT_READY;
    if (!WritePrivateProfileStringW(section.c_str(),L"SharpeningDebug",std::to_wstring(o.debug).c_str(),ini.c_str())) return RSF_ERROR_NOT_READY;
    return RSF_OK;
}

}
