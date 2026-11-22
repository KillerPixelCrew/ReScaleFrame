// SPDX-License-Identifier: MIT
// Native stage calls and the CInput detour's C++ unwind boundary.
#include "bootstrap_platform.h"
#include "build_profile.h"
#include "engine_slots.h"
#include "fg_stage.h"
#include "fg_stage_platform.h"
#include "input_hooks.h"
#include "motion_stage.h"
#include "native_hook_ops.h"
#include "native_memory.h"
#include "overlay_platform.h"
#include "sr_stage.h"
#include <windows.h>

extern "C" {
bool rsf_anomaly_bootstrap_install_input(void*, size_t, void*, rsf_anomaly_input_exchange,
    rsf_anomaly_input_read, const char**) noexcept;
void rsf_anomaly_bootstrap_publish_input_observer() noexcept;
bool rsf_anomaly_bootstrap_start_overlay(void*, const wchar_t*, rsf_game_log_fn,
    const rsf::anomaly::HookOps*) noexcept;
void rsf_anomaly_bootstrap_set_overlay_sink() noexcept;
}

namespace {
using namespace rsf::anomaly;

static_assert(sizeof(wchar_t) == 2);
static_assert(sizeof(rsf_anomaly_bootstrap_platform) == 72 &&
    offsetof(rsf_anomaly_bootstrap_platform, input_hook) == 48);

using InputFrame = void(__fastcall*)(void*);
void __fastcall input_frame(void* self) {
    struct Flight {
        uint32_t ticket;
        ~Flight() { rsf_anomaly_input_frame_exit(ticket); }
    } flight{rsf_anomaly_input_frame_begin(guarded_copy, rsf_anomaly_fg_stage_input_begin)};
    reinterpret_cast<InputFrame>(rsf_anomaly_slot_original(RSF_ANOMALY_INPUT_SLOT))(self);
}

constexpr rsf_anomaly_bootstrap_platform leaves{
    [](const wchar_t* ini, bool configure, rsf_game_log_fn log) noexcept { return sr::configure(ini, configure, log, nullptr); },
    [](const wchar_t* ini, rsf_game_log_fn log) noexcept {
        motion::configure(ini, log, nullptr);
    },
    [](const wchar_t* ini, bool configure, void* runtime, size_t build, rsf_game_log_fn log) noexcept {
        return rsf_anomaly_fg_configure_native(ini, configure, runtime, build_profiles[build].sr->hw_base_zb_rva, log);
    },
    rsf_anomaly_fg_installed,
    [](const char** reason) noexcept {
        return rsf_anomaly_overlay_install_default_input(&hook_ops, reason);
    },
    rsf_anomaly_bootstrap_publish_input_observer,
    [](void* image, size_t build, const char** reason) noexcept {
        return rsf_anomaly_bootstrap_install_input(image, build, reinterpret_cast<void*>(&input_frame),
            rsf_anomaly_compare_slot, guarded_copy, reason);
    },
    [](void* runtime, const wchar_t* ini, rsf_game_log_fn log) noexcept {
        return rsf_anomaly_bootstrap_start_overlay(runtime, ini, log, &hook_ops);
    },
    rsf_anomaly_bootstrap_set_overlay_sink,
};
}

extern "C" const rsf_anomaly_bootstrap_platform* rsf_anomaly_bootstrap_native() noexcept {
    return &leaves;
}
