// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// Frame-generation policy for Anomaly: which engine frames are FG source frames, which of them may feed generation
// history, and when history restarts. Pure logic, no engine or D3D access; fg_stage.cpp drives it from the native
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
#include <rescaleframe/game_frame.h>
#include <array>
#include <cstddef>
#include <cstdint>
#include <iterator>

namespace rsf::anomaly::fg {
// The game ABI's interactive class. RSF_SCREEN_FLIGHT is the ABI's name for "the player's normal view"; FG is refused
// on MENU, UNKNOWN, VIDEO and LOADING (rsf_frame_allows_fg).
inline constexpr rsf_screen_class gameplay_screen = RSF_SCREEN_FLIGHT;
inline constexpr rsf_screen_class held_screen = RSF_SCREEN_UNKNOWN;

enum class Skip : uint8_t { None, NoLevel, Scope, NoPresent, NoInputs, Count };
inline const char* skip_name(Skip skip) noexcept {
    static constexpr const char* names[] = {"none", "no_level", "scope", "no_present", "no_inputs"};
    const auto i = static_cast<std::size_t>(skip);
    return i < std::size(names) ? names[i] : "unknown";
}
enum class Reset : uint8_t { None, First, Device, Discontinuity, ScopeEnd, Size, Cut, Count };
inline const char* reset_name(Reset reset) noexcept {
    static constexpr const char* names[] = {"none", "first", "device", "discontinuity", "scope_end", "size", "cut"};
    const auto i = static_cast<std::size_t>(reset);
    return i < std::size(names) ? names[i] : "unknown";
}

struct Decision {
    bool tag{};                       // emit FINAL_SCENE (scene identity and HUD-less copy) at the UI boundary
    rsf_screen_class screen{held_screen};
    Reset reset{Reset::None};         // RESET flag on this frame's identity when not None
    Skip skip{Skip::None};            // why this frame cannot feed generation (None: it can, so far)
};

struct Counters {
    uint64_t source_frames{}, level_frames{}, tagged{}, fed{}, presented{}, unpresented{}, scope_frames{};
    std::array<uint64_t, static_cast<std::size_t>(Skip::Count)> skipped{};
    std::array<uint64_t, static_cast<std::size_t>(Reset::Count)> resets{};
};

class Policy {
public:
    explicit Policy(uint32_t scope_hold = 4) noexcept : scope_hold_(scope_hold) {}
    // A device reset (vid_restart, resolution change) or a new swap chain: history restarts.
    void device_reset() noexcept { if (pending_ == Reset::None) pending_ = Reset::Device; }
    // Render begin of engine frame `engine_frame` (Device.dwFrame) at presented size width x height. `cut` is the
    // caller's camera-cut test against the last fed frame's camera.
    Decision begin(uint32_t engine_frame, uint32_t width, uint32_t height, bool cut) noexcept {
        frame_ = {};
        engine_frame_ = engine_frame;
        frame_.width = width; frame_.height = height;
        ++counters_.source_frames;
        Decision d{};
        d.tag = true;
        if (seen_scope_ && engine_frame - last_scope_ <= scope_hold_) {
            d.skip = Skip::Scope; d.screen = held_screen;
        } else {
            d.screen = gameplay_screen;
            if (first_) d.reset = Reset::First;
            else if (pending_ != Reset::None) d.reset = pending_;
            else if (scoped_) d.reset = Reset::ScopeEnd;
            else if (!previous_fed_) d.reset = Reset::Discontinuity;
            else if (width != width_ || height != height_) d.reset = Reset::Size;
            else if (cut) d.reset = Reset::Cut;
        }
        frame_.decision = d;
        return d;
    }
    // RenderToTarget(rtSVP) in this frame: a secondary-viewport frame, which does not Present.
    void scope_frame() noexcept {
        if (!frame_.scope) ++counters_.scope_frames;
        frame_.scope = true;
        last_scope_ = engine_frame_;
        seen_scope_ = true;
        frame_.decision.skip = Skip::Scope; frame_.decision.screen = held_screen; frame_.decision.tag = false;
    }
    // The HUD-less boundary of a level frame. Returns the decision to apply there.
    Decision ui_boundary() noexcept {
        if (!frame_.level) {
            frame_.level = true;
            ++counters_.level_frames;
            if (frame_.decision.tag && frame_.decision.screen == gameplay_screen) ++counters_.tagged;
        }
        return frame_.decision;
    }
    // Whether this frame's depth and motion reached the Runtime (an SR evaluation or the adapter capture held them).
    void inputs(bool ready) noexcept {
        if (frame_.level && frame_.decision.skip == Skip::None && !ready) frame_.decision.skip = Skip::NoInputs;
    }
    // Present for this frame returned successfully.
    void presented() noexcept { frame_.presented = true; }
    // The frame ends (after the engine's End). Applies the outcome to history and counts it.
    Decision end() noexcept {
        auto& d = frame_.decision;
        if (!frame_.level) d.skip = Skip::NoLevel;
        else if (d.skip == Skip::None && !frame_.presented) d.skip = Skip::NoPresent;
        if (frame_.presented) ++counters_.presented; else ++counters_.unpresented;
        ++counters_.skipped[static_cast<std::size_t>(d.skip)];
        const bool fed = d.skip == Skip::None;
        if (fed) {
            ++counters_.fed;
            ++counters_.resets[static_cast<std::size_t>(d.reset)];
            first_ = false; pending_ = Reset::None; scoped_ = false;
            width_ = frame_.width; height_ = frame_.height;
        } else if (d.skip == Skip::Scope) {
            scoped_ = true;
        }
        previous_fed_ = fed;
        return d;
    }
    const Counters& counters() const noexcept { return counters_; }
    Decision current() const noexcept { return frame_.decision; }

private:
    struct Frame {
        bool level{}, scope{}, presented{};
        uint32_t width{}, height{};
        Decision decision{};
    };
    uint32_t scope_hold_{};
    uint32_t engine_frame_{}, last_scope_{};
    bool seen_scope_{}, scoped_{}, first_{true}, previous_fed_{};
    Reset pending_{Reset::None};
    uint32_t width_{}, height_{};
    Frame frame_{};
    Counters counters_{};
};
}
