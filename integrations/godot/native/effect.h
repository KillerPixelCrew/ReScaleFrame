// SPDX-License-Identifier: GPL-3.0-only
#pragma once

#include <godot_cpp/classes/compositor_effect.hpp>
#include <godot_cpp/classes/render_data.hpp>
#include <godot_cpp/variant/dictionary.hpp>
#include <godot_cpp/variant/projection.hpp>
#include <godot_cpp/variant/transform3d.hpp>
#include <godot_cpp/variant/vector2i.hpp>
#include <cstdint>
#include <atomic>
#include <mutex>

namespace rsf {
class ReScaleFrameEffect final : public godot::CompositorEffect {
    GDCLASS(ReScaleFrameEffect, godot::CompositorEffect)

    struct Snapshot {
        uint64_t callbacks = 0;
        uint64_t render_target = 0;
        uint32_t views = 0;
        godot::Vector2i internal_size;
        godot::Vector2i output_size;
        godot::Transform3D camera_transform;
        godot::Projection jittered_projection;
        int32_t color_format = -1;
        int32_t depth_format = -1;
        int32_t motion_format = -1;
        bool buffers_available = false;
        bool native_device_available = false;
        bool native_queue_available = false;
        bool native_textures_available = false;
    };
    mutable std::mutex guard;
    Snapshot snapshot;
    struct MotionCapture {
        uint64_t count = 0;
        uint64_t callback = 0;
        uint64_t pixels = 0;
        uint64_t moving_pixels = 0;
        uint64_t nonfinite_pixels = 0;
        uint64_t unwritten_pixels = 0;
        double max_abs_uv = 0.0;
        bool succeeded = false;
    } capture;
    std::atomic<bool> capture_requested{false};

protected:
    static void _bind_methods();

public:
    ReScaleFrameEffect();
    void _render_callback(int32_t type, godot::RenderData* data) override;
    godot::Dictionary get_status() const;
    void request_motion_capture();
};
}
