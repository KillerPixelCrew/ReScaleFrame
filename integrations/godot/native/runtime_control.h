// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <godot_cpp/classes/ref_counted.hpp>
#include <godot_cpp/variant/dictionary.hpp>
#include <godot_cpp/variant/rid.hpp>

namespace rsf {
class ReScaleFrameRuntime final : public godot::RefCounted {
    GDCLASS(ReScaleFrameRuntime, godot::RefCounted)
protected:
    static void _bind_methods();
public:
    bool configure(const godot::Dictionary& options, godot::RID viewport);
    godot::Dictionary get_status() const;
    void reset_history();
    bool request_latency_ping();
};
}
