// SPDX-License-Identifier: MIT
#pragma once
// Private layouts between the Anomaly overlay service (overlay.rs) and its native leaves (overlay_service.cpp).
// tools/generate-rust-bindings.py generates overlay_abi.rs from this header and the Runtime's overlay API.
#include <rescaleframe/named_frame_dump.h>
#include <rescaleframe/overlay_api.h>
#include <stdbool.h>
#include <stdint.h>

// overlay::StartStatus.
enum {
    RSF_ANOMALY_OVERLAY_STARTED, RSF_ANOMALY_OVERLAY_ALREADY_STARTED, RSF_ANOMALY_OVERLAY_API_MISSING,
    RSF_ANOMALY_OVERLAY_API_REFUSED, RSF_ANOMALY_OVERLAY_CONFIGURE_REFUSED, RSF_ANOMALY_OVERLAY_HOOKS_REFUSED,
};

// One adapter option as the panel shows it. get and set run on the Present-owner thread.
typedef struct rsf_anomaly_overlay_option {
    const char* label;
    uint32_t toggle, restart;
    int32_t minimum, maximum, step;
    uint32_t divisor;
    int32_t (*get)(void);
    bool (*set)(int32_t);
} rsf_anomaly_overlay_option;

// A presented chain: its DXGI output window and size, and its D3D11 device, compared only.
typedef struct rsf_anomaly_overlay_chain {
    void* window;
    const void* device;
    uint32_t width, height;
} rsf_anomaly_overlay_chain;

// Platform calls for the service: COM access and guarded hook installation.
typedef struct rsf_anomaly_overlay_platform {
    // Zero when the chain's description cannot be read.
    uint32_t (*chain)(void* chain, rsf_anomaly_overlay_chain*);
    // Writes the chain's backbuffer through `dump`; -1 without a backbuffer or immediate context.
    int32_t (*capture_backbuffer)(void* chain, rsf_input_dump_named_texture_fn dump, const rsf_named_frame_dump*,
        rsf_named_frame_dump_report*);
    uint32_t (*install_input)(const char** reason);
    uint32_t (*install_present)(const char** reason);
} rsf_anomaly_overlay_platform;

typedef struct rsf_anomaly_overlay_options {
    void* runtime;
    const wchar_t* configuration;
    rsf_game_log_fn log;
    void* log_user;
    const rsf_overlay_api* api_override;
    rsf_get_overlay_diagnostics_fn diagnostics_override;
    rsf_input_dump_named_texture_fn capture_override;
    uint32_t install_hooks;
} rsf_anomaly_overlay_options;

#ifdef __cplusplus
extern "C" {
#endif
uint32_t rsf_anomaly_overlay_start(const rsf_anomaly_overlay_options*, const rsf_anomaly_overlay_platform*,
    const char** reason);
uint32_t rsf_anomaly_overlay_describe_chain(void*, rsf_anomaly_overlay_chain*);
int32_t rsf_anomaly_overlay_capture_backbuffer(void*, rsf_input_dump_named_texture_fn, const rsf_named_frame_dump*,
    rsf_named_frame_dump_report*);
uint32_t rsf_anomaly_overlay_capture_bits(void* user);
void rsf_anomaly_overlay_present(void* user, void* chain);
void rsf_anomaly_overlay_resize(void* user, void* chain);
void rsf_anomaly_overlay_option_add(const rsf_anomaly_overlay_option*);
#ifdef __cplusplus
}
#endif
