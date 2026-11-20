// SPDX-License-Identifier: MIT
// The cold entry's native leaves. bootstrap.rs owns the entry: build selection, the Runtime borrow, the plugin session,
// the step order, retries, refusals and the adapter log. Each leaf forwards to the C++ owner of its options and hooks.
#include "adapter_options.h"
#include "build_profile.h"
#include "fg_stage.h"
#include "input_hooks.h"
#include "overlay_service.h"
#include "motion_stage.h"
#include "sr_stage.h"
#include <rescaleframe/sha256_file.h>
#include <windows.h>
#include <cstring>
#include <string>

// Layout shared with bootstrap.rs `Leaves`.
struct rsf_anomaly_bootstrap_leaves {
    const rsf_anomaly_build_identity* builds;
    size_t build_count;
    const char* (*label)(size_t build) noexcept;
    bool (*sha256)(const wchar_t* path, char* hex65) noexcept;
    void (*register_options)(const wchar_t* ini) noexcept;
    bool (*sr)(const wchar_t* ini, bool configure, rsf_game_log_fn log) noexcept;
    void (*motion)(const wchar_t* ini, rsf_game_log_fn log) noexcept;
    bool (*fg)(const wchar_t* ini, bool configure, HMODULE runtime, size_t build, rsf_game_log_fn log) noexcept;
    bool (*fg_installed)() noexcept;
    uint32_t (*input)(size_t build, const char** reason) noexcept;
    bool (*overlay)(HMODULE runtime, const wchar_t* ini, rsf_game_log_fn log) noexcept;
    void (*overlay_sink)() noexcept;
};

namespace {
using namespace rsf::anomaly;

constexpr rsf_anomaly_bootstrap_leaves leaves{
    build_identities.data(), build_identities.size(),
    [](size_t build) noexcept { return build_profiles[build].label; },
    [](const wchar_t* path, char* hex65) noexcept {
        try {
            std::string hash;
            if (!rsf::sha256_file(path, hash) || hash.size() != 64) return false;
            std::memcpy(hex65, hash.c_str(), 65);
            return true;
        } catch (...) { return false; }
    },
    [](const wchar_t* ini) noexcept { options::register_restart_options(ini); },
    [](const wchar_t* ini, bool configure, rsf_game_log_fn log) noexcept {
        const auto options = sr::read_options(ini);
        if (configure && options.selected()) sr::configure(options, log, nullptr);
        return options.selected();
    },
    [](const wchar_t* ini, rsf_game_log_fn log) noexcept {
        motion::configure(ini, log, nullptr);
    },
    [](const wchar_t* ini, bool configure, HMODULE runtime, size_t build, rsf_game_log_fn log) noexcept {
        return fg::configure_ini(ini, configure, runtime, build_profiles[build], log);
    },
    []() noexcept { return fg::installed(); },
    [](size_t build, const char** reason) noexcept -> uint32_t {
        if (!overlay::install_input_hooks(overlay::input_gate(), reason)) return 1;
        overlay::set_input_observer(&input::received);
        const auto& profile = build_profiles[build];
        return input::install(reinterpret_cast<std::byte*>(GetModuleHandleW(nullptr)), profile.image_size(), profile,
            reason) ? 0 : 2;
    },
    [](HMODULE runtime, const wchar_t* ini, rsf_game_log_fn log) noexcept {
        overlay::StartOptions options{};
        options.runtime = runtime; options.configuration = ini; options.log = log;
        const char* reason = nullptr;
        const auto status = overlay::start(options, &reason);
        return status == overlay::StartStatus::started || status == overlay::StartStatus::already_started;
    },
    []() noexcept { fg::set_overlay_sink(overlay::present_sink()); },
};
}

extern "C" uint32_t rsf_anomaly_cold_start(const wchar_t* configuration, const rsf_anomaly_bootstrap_leaves*) noexcept;

// Synchronous cold entry called by the thin carrier before genuine DirectInput creation. It borrows the one
// already-loaded Runtime DLL and uses its plugin-session owner (bootstrap.rs).
extern "C" __declspec(dllexport) uint32_t __stdcall rsf_anomaly_start(const wchar_t* configuration) noexcept {
    return rsf_anomaly_cold_start(configuration, &leaves);
}
