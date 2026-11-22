// SPDX-License-Identifier: MIT
#pragma once
// Private startup boundary between the Anomaly overlay service and its native bootstrap caller.
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
