// SPDX-License-Identifier: MIT
#pragma once
// Shared persistence for rsf_render_settings in an adapter-supplied INI section. The keys are
// the render frontend's; the file and section always come from the adapter. settings_store.rs
// owns the keys, file format and sharpening policy. The hosts and embedded carrier
// share this C interface.
#include <rescaleframe/game_api.h>
#include <rescaleframe/render_frontend.h>
#include <wchar.h>

#ifdef __cplusplus
extern "C" {
#endif
// Internal to the runtime and its embedding carriers; none is exported.
rsf_result rsf_settings_store_read(const wchar_t* ini, const wchar_t* section,
    const rsf_render_settings* defaults, const rsf_sharpening_options* sharpening_defaults,
    rsf_render_settings* output);
rsf_result rsf_settings_store_write(const wchar_t* ini, const wchar_t* section,
    const rsf_render_settings* request);
/* Adapter-owned numeric keys. Section and path are UTF-16, and key is UTF-8. */
uint32_t rsf_settings_store_read_integer(const wchar_t* ini, const wchar_t* section,
    const char* key, uint32_t fallback);
rsf_result rsf_settings_store_write_integer(const wchar_t* ini, const wchar_t* section,
    const char* key, uint32_t value);
uint32_t rsf_settings_store_read_layout(const wchar_t* ini, const wchar_t* section);
uint32_t rsf_settings_store_read_dlss_preset(const wchar_t* ini, const wchar_t* section, uint32_t fallback);
rsf_result rsf_settings_store_write_dlss_preset(const wchar_t* ini, const wchar_t* section, uint32_t preset);
rsf_result rsf_settings_store_write_sharpening(const wchar_t* ini, const wchar_t* section,
    const rsf_sharpening_options* options);
// The shared post-pass keys, missing ones at rsf_sharpening_defaults, sanitized by the public
// sharpening policy.
void rsf_settings_store_read_sharpening(const wchar_t* ini, const wchar_t* section,
    rsf_sharpening_options* output);
#ifdef __cplusplus
}
#endif
