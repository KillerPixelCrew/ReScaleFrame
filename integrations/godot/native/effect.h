// SPDX-License-Identifier: MIT
#pragma once

#include <godot_cpp/classes/compositor_effect.hpp>
#include <godot_cpp/classes/render_data.hpp>
#include <godot_cpp/variant/dictionary.hpp>
#include <cstdint>

// The Rust effect owner (native/effect.rs) behind godot_platform.h.
struct rsf_godot_effect;

namespace rsf {
// Snapshot, capture request and classification state live in the Rust owner created with each
// instance. This class keeps the godot-cpp registration, callback contract and typed reads.
class ReScaleFrameEffect final : public godot::CompositorEffect {
    GDCLASS(ReScaleFrameEffect, godot::CompositorEffect)

    rsf_godot_effect* probe = nullptr;

protected:
    static void _bind_methods();

public:
    ReScaleFrameEffect();
    ~ReScaleFrameEffect() override;
    void _render_callback(int32_t type, godot::RenderData* data) override;
    godot::Dictionary get_status() const;
    void request_motion_capture();
};
}
