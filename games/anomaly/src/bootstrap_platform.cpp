// SPDX-License-Identifier: MIT
// Native stage calls and the CInput detour's C++ unwind boundary.
#include "bootstrap_platform.h"
#include "build_profile.h"
#include "fg_stage.h"
#include "input_hooks.h"
#include "motion_stage.h"
#include "native_hook_ops.h"
#include "overlay_platform.h"
#include "sr_stage.h"
#include <windows.h>

namespace {
using namespace rsf::anomaly;

static_assert(sizeof(wchar_t) == 2 && sizeof(rsf_anomaly_build_identity) == 16);
static_assert(sizeof(rsf_anomaly_bootstrap_platform) == 72 &&
    offsetof(rsf_anomaly_bootstrap_platform, input_hook) == 48);

using InputFrame = void(__fastcall*)(void*);
void __fastcall input_frame(void* self) {
    struct Flight {
        uint32_t ticket;
        ~Flight() { rsf_anomaly_input_frame_exit(ticket); }
    } flight{rsf_anomaly_input_frame_begin(guarded_copy, fg::input_begin)};
    reinterpret_cast<InputFrame>(rsf_anomaly_slot_original(RSF_ANOMALY_INPUT_SLOT))(self);
}

constexpr rsf_anomaly_bootstrap_platform leaves{
    [](const wchar_t* ini, bool configure, rsf_game_log_fn log) noexcept { return sr::configure(ini, configure, log, nullptr); },
    [](const wchar_t* ini, rsf_game_log_fn log) noexcept {
        motion::configure(ini, log, nullptr);
    },
    [](const wchar_t* ini, bool configure, void* runtime, size_t build, rsf_game_log_fn log) noexcept {
        return fg::configure_ini(ini, configure, static_cast<HMODULE>(runtime), build_profiles[build], log);
    },
    []() noexcept { return fg::installed(); },
    [](const char** reason) noexcept {
        return rsf_anomaly_overlay_install_default_input(&hook_ops, reason);
    },
    []() noexcept {
        rsf_anomaly_overlay_set_input_observer([](uint8_t kind) noexcept {
            rsf_anomaly_input_received(kind, fg::input_received);
        });
    },
    [](void* image, size_t build, const char** reason) noexcept {
        const auto& profile = build_profiles[build];
        const auto checked = guard_image(static_cast<std::byte*>(image), profile.image_size());
        return rsf_anomaly_input_install(&checked, build, profile.native->device_frame_rva,
            reinterpret_cast<void*>(&input_frame), compare_slot, reason);
    },
    [](void* runtime, const wchar_t* ini, rsf_game_log_fn log) noexcept {
        rsf_anomaly_overlay_options options{};
        options.runtime = runtime;
        options.configuration = ini;
        options.log = log;
        options.install_hooks = 1;
        const char* reason = nullptr;
        const auto status = rsf_anomaly_overlay_start(&options, &hook_ops, &reason);
        return status == RSF_ANOMALY_OVERLAY_STARTED || status == RSF_ANOMALY_OVERLAY_ALREADY_STARTED;
    },
    []() noexcept {
        fg::set_overlay_sink([](void* user, IDXGISwapChain* chain) noexcept {
            rsf_anomaly_overlay_present(user, chain);
        });
    },
};
}

extern "C" const rsf_anomaly_bootstrap_platform* rsf_anomaly_bootstrap_native() noexcept {
    return &leaves;
}
