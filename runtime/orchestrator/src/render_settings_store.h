// SPDX-License-Identifier: MIT
#pragma once
// Shared persistence for rsf_render_settings in an adapter-supplied INI section. The keys are
// the render frontend's; the file and section always come from the adapter. settings_store.rs
// owns the keys and file format; this facade maps them onto the hosts' C++ types and applies
// the public sharpening range checks.
#include <rescaleframe/game_api.h>
#include <rescaleframe/render_frontend.h>
#include <string>

extern "C" {
// Internal to the runtime and its embedding carriers; none is exported.
rsf_result rsf_settings_store_read(const wchar_t* ini, const wchar_t* section,
    const rsf_render_settings* defaults, const rsf_sharpening_options* sharpening_defaults,
    rsf_render_settings* output);
rsf_result rsf_settings_store_write(const wchar_t* ini, const wchar_t* section,
    const rsf_render_settings* request);
uint32_t rsf_settings_store_read_layout(const wchar_t* ini, const wchar_t* section);
rsf_result rsf_settings_store_write_layout(const wchar_t* ini, const wchar_t* section, uint32_t value);
rsf_result rsf_settings_store_write_sharpening(const wchar_t* ini, const wchar_t* section,
    const rsf_sharpening_options* options);
}

namespace rsf::render_settings {
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

// Missing frontend keys take the host defaults; missing post-pass keys take rsf_sharpening_defaults.
inline rsf_render_settings read(const std::wstring& ini, const std::wstring& section, const rsf_render_settings& defaults)
{
    rsf_render_settings out{sizeof(out), RSF_RENDER_FRONTEND_ABI_VERSION};
    const auto sharpening_defaults = rsf_sharpening_defaults();
    (void)rsf_settings_store_read(ini.c_str(), section.c_str(), &defaults, &sharpening_defaults, &out);
    return out;
}

// Rewrites the owned keys in one section write and keeps every other entry of the section.
inline rsf_result write(const std::wstring& ini, const std::wstring& section, const rsf_render_settings& request)
{
    return rsf_settings_store_write(ini.c_str(), section.c_str(), &request);
}

// The overlay's opaque panel layout word, kept under its own key that settings writes preserve.
inline uint32_t read_overlay_layout(const std::wstring& ini, const std::wstring& section)
{
    return rsf_settings_store_read_layout(ini.c_str(), section.c_str());
}
inline bool write_overlay_layout(const std::wstring& ini, const std::wstring& section, uint32_t value)
{
    return rsf_settings_store_write_layout(ini.c_str(), section.c_str(), value) == RSF_OK;
}

// Hosts with their own base SR settings persist only these shared post-pass keys. A failed write
// restores the keys it had already changed.
inline rsf_sharpening_options read_sharpening(const std::wstring& ini,const std::wstring& section) {
    rsf_render_settings defaults{sizeof(defaults),RSF_RENDER_FRONTEND_ABI_VERSION};
    return rsf::sharpening_settings::sanitize(sharpening(read(ini,section,defaults)));
}
inline rsf_result write_sharpening(const std::wstring& ini,const std::wstring& section,const rsf_sharpening_options& o) {
    if (!rsf::sharpening_settings::valid(o)) return RSF_ERROR_INVALID_ARGUMENT;
    return rsf_settings_store_write_sharpening(ini.c_str(), section.c_str(), &o);
}

}
