// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/overlay_api.h>
#include <rescaleframe/overlay_input.h>

/* Internal to the DLL. Rust also links into hosts that have no overlay service, so only
   the DLL supplies these non-null platform callbacks. */
typedef struct rsf_overlay_platform {
    void* (*window)(void*, uint32_t*, uint32_t*);
    uint32_t (*refresh)(void*);
    uint32_t (*foreground)(void);
    uint64_t (*tick)(void);
    int (*start)(void*, rsf_game_log_fn, void*);
    int (*present)(void*, const rsf_overlay_stats*, rsf_overlay_intent*);
    int (*stop)(void);
    void (*initialize_sharpening)(rsf_render_settings*, rsf_sharpening_options*);
    uint32_t (*valid_settings)(const rsf_render_settings*);
    uint32_t (*valid_preset)(uint32_t);
    void (*sanitize_sharpening)(rsf_render_settings*);
    void (*set_sharpening)(const rsf_render_settings*);

    uint32_t (*input_visible)(void);
    void (*input_set_visible)(uint32_t);
    int (*input_counters_get)(rsf_overlay_input_counters*);
    int (*input_routes_get)(rsf_native_input_routes*);
    int (*latency_status_get)(rsf_native_latency_status*);
    int (*fg_status_get)(rsf_native_fg_status*);
    void (*fg_options_set)(const rsf_native_fg_options*);
    int (*present_has_owner)(void);
    const rsf_generation_provider* (*present_provider)(void);
    void* (*present_session)(void);
    uint32_t (*present_backend)(void);
    uint32_t (*present_backend_choices)(void);
    int32_t (*present_switch_result)(void);
    int32_t (*present_request)(uint32_t);
} rsf_overlay_platform;
