// SPDX-License-Identifier: MIT
#pragma once
#include "build_identity.h"
#include <rescaleframe/game_api.h>
#include <stdbool.h>
#include <stddef.h>

typedef struct rsf_anomaly_bootstrap_platform {
    const rsf_anomaly_build_identity* builds;
    size_t build_count;
    const char* (*label)(size_t);
    bool (*sr)(const wchar_t*, bool, rsf_game_log_fn);
    void (*motion)(const wchar_t*, rsf_game_log_fn);
    bool (*fg)(const wchar_t*, bool, void*, size_t, rsf_game_log_fn);
    bool (*fg_installed)(void);
    bool (*input_observation)(const char**);
    void (*publish_input_observer)(void);
    bool (*input_hook)(void*, size_t, const char**);
    bool (*overlay)(void*, const wchar_t*, rsf_game_log_fn);
    void (*overlay_sink)(void);
} rsf_anomaly_bootstrap_platform;
