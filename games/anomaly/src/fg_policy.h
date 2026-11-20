// SPDX-License-Identifier: MIT
#pragma once
// Frame-generation policy for Anomaly: which engine frames are FG source frames, which of them may feed generation
// history, and when history restarts. Rust owns the state; fg_stage.cpp reports the native
// boundaries and tests/anomaly_fg_policy.cpp drives it directly. See docs/research/anomaly-fg.md.
//
// One engine frame (CRenderDevice::on_idle: Begin ... End) is one FG source frame. The decision is made at render
// begin, because the SR seam inside CRender::Render evaluates (and hands FG its depth and motion) before the HUD-less
// boundary is reached. A frame is a candidate only if it then reaches the HUD-less boundary (CHUDManager::RenderUI
// entry inside CLevel::OnRender). Frames that never reach it: the main menu (MainMenu.cpp removes the level from
// seqRender), loading draws (CApplication::LoadDraw) and frames without a level.
// Secondary-viewport (scope) frames render the scope view into the swap chain and do not Present
// (dxRenderDeviceRender::End). They call RenderToTarget(rtSVP) before RenderUI. While they occur, the presented frames
// alternate cameras with them, so every frame within `scope_hold` engine frames of the last SVP frame is held back
// (screen class UNKNOWN: the Runtime keeps its inputs but refuses to generate around it).
// History restarts (RESET on the next fed frame) after any break: the first frame, a device reset, any frame that did
// not feed history, the end of a scope sequence, a presented-size change and a camera cut.
#include "fg_policy_platform.h"
#include <cstddef>
#include <cstdint>
#include <iterator>

namespace rsf::anomaly::fg {
inline constexpr rsf_screen_class gameplay_screen = RSF_ANOMALY_GAMEPLAY_SCREEN;
inline constexpr rsf_screen_class held_screen = RSF_ANOMALY_HELD_SCREEN;

enum class Skip : uint8_t {
    None = RSF_ANOMALY_SKIP_NONE, NoLevel = RSF_ANOMALY_SKIP_NO_LEVEL, Scope = RSF_ANOMALY_SKIP_SCOPE,
    NoPresent = RSF_ANOMALY_SKIP_NO_PRESENT, NoInputs = RSF_ANOMALY_SKIP_NO_INPUTS, Count = RSF_ANOMALY_SKIP_COUNT,
};
inline const char* skip_name(Skip skip) noexcept {
    static constexpr const char* names[] = {"none", "no_level", "scope", "no_present", "no_inputs"};
    const auto i = static_cast<std::size_t>(skip);
    return i < std::size(names) ? names[i] : "unknown";
}
enum class Reset : uint8_t {
    None = RSF_ANOMALY_RESET_NONE, First = RSF_ANOMALY_RESET_FIRST, Device = RSF_ANOMALY_RESET_DEVICE,
    Discontinuity = RSF_ANOMALY_RESET_DISCONTINUITY, ScopeEnd = RSF_ANOMALY_RESET_SCOPE_END,
    Size = RSF_ANOMALY_RESET_SIZE, Cut = RSF_ANOMALY_RESET_CUT, Count = RSF_ANOMALY_RESET_COUNT,
};
inline const char* reset_name(Reset reset) noexcept {
    static constexpr const char* names[] = {"none", "first", "device", "discontinuity", "scope_end", "size", "cut"};
    const auto i = static_cast<std::size_t>(reset);
    return i < std::size(names) ? names[i] : "unknown";
}

struct Decision {
    bool tag{};
    rsf_screen_class screen{held_screen};
    Reset reset{Reset::None};
    Skip skip{Skip::None};
};

using Counters = rsf_anomaly_fg_counters;

// Native callers supply the observed engine boundaries. The opaque Rust owner
// tracks the continuity and returns each decision with a snapshot of its counters.
class Policy {
public:
    explicit Policy(uint32_t scope_hold = 4) noexcept : history_(rsf_anomaly_fg_history_create(scope_hold)) {}
    ~Policy() { rsf_anomaly_fg_history_destroy(history_); }
    Policy(const Policy&) = delete;
    Policy& operator=(const Policy&) = delete;

    void reset(uint32_t scope_hold) noexcept { rsf_anomaly_fg_history_reset(history_, scope_hold); }
    void device_reset() noexcept { rsf_anomaly_fg_device_reset(history_); }
    Decision begin(uint32_t engine_frame, uint32_t width, uint32_t height, bool cut) noexcept {
        return decode(rsf_anomaly_fg_begin(history_, engine_frame, width, height, cut));
    }
    void scope_frame() noexcept { rsf_anomaly_fg_scope(history_); }
    Decision ui_boundary() noexcept { return decode(rsf_anomaly_fg_ui_boundary(history_)); }
    Decision inputs(bool ready) noexcept { return decode(rsf_anomaly_fg_inputs(history_, ready)); }
    void presented() noexcept { rsf_anomaly_fg_presented(history_); }
    Decision end() noexcept { return decode(rsf_anomaly_fg_end(history_)); }
    Decision current() const noexcept { return decode(rsf_anomaly_fg_current(history_)); }
    Counters counters() const noexcept {
        Counters result{};
        rsf_anomaly_fg_count(history_, &result);
        return result;
    }

private:
    static Decision decode(rsf_anomaly_fg_decision decision) noexcept {
        return {decision.tag != 0, decision.screen, static_cast<Reset>(decision.reset), static_cast<Skip>(decision.skip)};
    }
    rsf_anomaly_fg_history* history_;
};
}
