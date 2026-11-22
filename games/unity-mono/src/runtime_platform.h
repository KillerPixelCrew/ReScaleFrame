/* SPDX-License-Identifier: MIT */
#pragma once
/* The Rust runtime setup owner (runtime_setup.rs) reads the [UnitySR] settings, applies the start
   gates and fills the rsf_fg12_setup and rsf_callback_host_setup descriptors. runtime_host.cpp
   keeps the exported entry points and lends the typed Runtime and SDK calls through this table.
   Those calls bind to the Runtime the loader already loaded; the Rust archive references no
   Runtime export itself, so the Unity package can link it beside the static core. */
#include <rescaleframe/callback_render_host.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <wchar.h>

typedef struct rsf_unity_runtime_platform {
    /* RSF_UNITY_ENGINE_VERSION and RSF_UNITY_PROJECT_ID from unity_runtime_host.h. */
    const char* engine_version_utf8;
    const char* project_id_utf8;
    /* [UnitySR] path key of a frame-generation backend (rsf_fg_backend_info_for), null when the
       id is unknown. */
    const char* (*fg_backend_key)(uint32_t backend);
    uint32_t (*fg_choice_start)(const wchar_t* preferences_path, uint32_t fallback, uint32_t choices);
    /* rsf_fg_choice_get().choices; zero before any choice was started. */
    uint32_t (*fg_choice_choices)(void);
    rsf_backend_result (*fg_install)(const rsf_fg12_setup* setup);
    rsf_backend_result (*fg_capture_outputs)(const char* directory_utf8);
    /* Enables the D3D12 debug layer when it is installed. */
    void (*graphics_debug)(void);
    uint32_t (*dlss_preset_valid)(uint32_t preset);
    /* SHA-256 hex digest of the file and its terminator into digest; zero on failure. */
    uint32_t (*sha256_file)(const wchar_t* path, char* digest);
    uint32_t (*callback_host_start)(const rsf_callback_host_setup* setup);
    uint32_t (*callback_host_stop)(void);
    int (*set_engine_spatial)(uint32_t spatial, uint32_t quality);
    int (*request_policy)(uint32_t spatial, uint32_t quality, uint32_t* generation);
    int (*drained_generation)(uint32_t* generation);
    int (*spatial_extent)(uint32_t quality, uint32_t output_width, uint32_t output_height,
        uint32_t* render_width, uint32_t* render_height);
} rsf_unity_runtime_platform;

#ifdef __cplusplus
extern "C" {
#endif
/* The bodies of the rsf_unity_* exports in unity_runtime_host.h, with the same result codes. */
uint32_t rsf_unity_mono_runtime_start(const wchar_t* ini, const rsf_unity_runtime_platform* platform);
uint32_t rsf_unity_mono_fg_start(const wchar_t* ini, const rsf_unity_runtime_platform* platform);
uint32_t rsf_unity_mono_sr_start(const wchar_t* ini, const rsf_unity_runtime_platform* platform);
uint32_t rsf_unity_mono_sr_stop(const rsf_unity_runtime_platform* platform);
#ifdef __cplusplus
}
#endif
