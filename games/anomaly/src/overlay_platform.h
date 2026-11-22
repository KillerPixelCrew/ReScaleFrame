// SPDX-License-Identifier: MIT
#pragma once
// Private layouts between the Anomaly overlay service (overlay.rs) and its native bootstrap caller.
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
namespace rsf::anomaly { struct HookOps; }
extern "C" {
uint32_t rsf_anomaly_overlay_start(const rsf_anomaly_overlay_options*, const rsf::anomaly::HookOps*,
    const char** reason) noexcept;
bool rsf_anomaly_overlay_install_default_input(const rsf::anomaly::HookOps*, const char** reason) noexcept;
void rsf_anomaly_overlay_present(void* user, void* chain) noexcept;
}
#endif
