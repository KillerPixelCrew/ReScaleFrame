// SPDX-License-Identifier: MIT
#pragma once
// Private startup boundary between the Anomaly overlay service and its native bootstrap caller.
// build.rs binds this header with the Runtime's overlay API.
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
    uint32_t install_hooks;
} rsf_anomaly_overlay_options;

#include "native_hook_ops.h"
#ifdef __cplusplus
#define RSF_OVERLAY_NOEXCEPT noexcept
extern "C" {
#else
#define RSF_OVERLAY_NOEXCEPT
#endif
uint32_t rsf_anomaly_overlay_start(const rsf_anomaly_overlay_options*, const rsf_anomaly_hook_ops*,
    const char** reason) RSF_OVERLAY_NOEXCEPT;
bool rsf_anomaly_overlay_install_default_input(const rsf_anomaly_hook_ops*, const char** reason) RSF_OVERLAY_NOEXCEPT;
void rsf_anomaly_overlay_present(void* user, void* chain) RSF_OVERLAY_NOEXCEPT;
#ifdef __cplusplus
}
#endif
#undef RSF_OVERLAY_NOEXCEPT
