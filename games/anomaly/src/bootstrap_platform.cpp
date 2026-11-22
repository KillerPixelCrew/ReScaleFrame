// SPDX-License-Identifier: MIT
// Native stage calls that still consume C++ profiles and overlay interfaces.
#include "bootstrap_platform.h"
#include "build_profile.h"
#include "fg_stage.h"
#include "input_hooks.h"
#include "overlay_service.h"
#include "motion_stage.h"
#include "sr_stage.h"
#include <windows.h>

namespace {
using namespace rsf::anomaly;

static_assert(sizeof(wchar_t) == 2 && sizeof(rsf_anomaly_build_identity) == 16);
static_assert(sizeof(rsf_anomaly_bootstrap_platform) == 96 &&
    offsetof(rsf_anomaly_bootstrap_platform, input_hook) == 72);

constexpr rsf_anomaly_bootstrap_platform leaves{
    build_identities.data(), build_identities.size(),
    [](size_t build) noexcept { return build_profiles[build].label; },
    [](const wchar_t* ini, bool configure, rsf_game_log_fn log) noexcept { return sr::configure(ini, configure, log, nullptr); },
    [](const wchar_t* ini, rsf_game_log_fn log) noexcept {
        motion::configure(ini, log, nullptr);
    },
    [](const wchar_t* ini, bool configure, void* runtime, size_t build, rsf_game_log_fn log) noexcept {
        return fg::configure_ini(ini, configure, static_cast<HMODULE>(runtime), build_profiles[build], log);
    },
    []() noexcept { return fg::installed(); },
    [](const char** reason) noexcept {
        return overlay::install_input_hooks(overlay::input_gate(), reason);
    },
    []() noexcept { overlay::set_input_observer(&input::received); },
    [](void* image, size_t build, const char** reason) noexcept {
        const auto& profile = build_profiles[build];
        return input::install(static_cast<std::byte*>(image), profile.image_size(), profile,
            reason);
    },
    [](void* runtime, const wchar_t* ini, rsf_game_log_fn log) noexcept {
        overlay::StartOptions options{};
        options.runtime = static_cast<HMODULE>(runtime); options.configuration = ini; options.log = log;
        const char* reason = nullptr;
        const auto status = overlay::start(options, &reason);
        return status == overlay::StartStatus::started || status == overlay::StartStatus::already_started;
    },
    []() noexcept { fg::set_overlay_sink(overlay::present_sink()); },
};
}

extern "C" const rsf_anomaly_bootstrap_platform* rsf_anomaly_bootstrap_native() noexcept {
    return &leaves;
}
