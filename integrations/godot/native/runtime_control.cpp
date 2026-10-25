// SPDX-License-Identifier: GPL-3.0-only
#include "runtime_control.h"
#include "api.h"
#include <godot_cpp/core/class_db.hpp>
#include <godot_cpp/classes/rendering_server.hpp>

namespace rsf {
using namespace godot;
void ReScaleFrameRuntime::_bind_methods()
{
    ClassDB::bind_method(D_METHOD("configure", "options", "viewport"), &ReScaleFrameRuntime::configure);
    ClassDB::bind_method(D_METHOD("get_status"), &ReScaleFrameRuntime::get_status);
    ClassDB::bind_method(D_METHOD("reset_history"), &ReScaleFrameRuntime::reset_history);
    ClassDB::bind_method(D_METHOD("request_latency_ping"), &ReScaleFrameRuntime::request_latency_ping);
}
bool ReScaleFrameRuntime::configure(const Dictionary& options, RID viewport)
{
    if (!viewport.is_valid()) return false;
    auto* rendering = RenderingServer::get_singleton();
    if (!rendering || (rendering->get_current_rendering_driver_name() != "d3d12" &&
        !(rendering->get_current_rendering_driver_name() == "vulkan" && rsfg_vulkan_supported())) ||
        rendering->get_current_rendering_method() != "forward_plus") return false;
    const char* fields[] = {"sr_backend", "quality", "dlss_preset", "fg_backend", "fg_mode", "generated_frames", "latency_mode", "frame_limit_us", "gameplay"};
    for (const auto* name : fields) if (options.has(name)) {
        const Variant raw = options[name];
        if (raw.get_type() != Variant::INT && raw.get_type() != Variant::BOOL) return false;
        const int64_t number = raw;
        if (number < 0 || number > UINT32_MAX) return false;
    }
    rsf_godot_settings value{sizeof(value), RSF_GODOT_ABI_VERSION, 7, 1, 0, 7, 0, 1, 0, 0, 1};
    auto field = [&options](const char* name, uint32_t fallback) { return static_cast<uint32_t>(static_cast<int64_t>(options.get(name, fallback))); };
    value.sr_backend = field("sr_backend", value.sr_backend);
    value.quality = field("quality", value.quality);
    value.dlss_preset = field("dlss_preset", value.dlss_preset);
    value.fg_backend = field("fg_backend", value.fg_backend);
    value.fg_mode = field("fg_mode", value.fg_mode);
    value.generated_frames = field("generated_frames", value.generated_frames);
    value.latency_mode = field("latency_mode", value.latency_mode);
    value.frame_limit_us = field("frame_limit_us", value.frame_limit_us);
    value.gameplay = field("gameplay", value.gameplay);
    return rsfg_configure_engine(&value, viewport.get_id()) != 0;
}
Dictionary ReScaleFrameRuntime::get_status() const
{
    Dictionary result;
    auto* rendering = RenderingServer::get_singleton();
    result["graphics_driver"] = rendering ? rendering->get_current_rendering_driver_name() : String();
    result["renderer"] = rendering ? rendering->get_current_rendering_method() : String();
    result["graphics_driver_supported"] = rendering && (rendering->get_current_rendering_driver_name() == "d3d12" ||
        (rendering->get_current_rendering_driver_name() == "vulkan" && rsfg_vulkan_supported()));
    result["renderer_supported"] = rendering && rendering->get_current_rendering_method() == "forward_plus";
    rsf_godot_status status{sizeof(status), RSF_GODOT_ABI_VERSION};
    const bool available = rsfg_status(&status) && status.session_id != 0;
    result["engine_bridge_ready"] = available;
    if (!available) return result;
    rsf_godot_settings requested{sizeof(requested), RSF_GODOT_ABI_VERSION};
    if (rsfg_settings(&requested)) {
        result["requested_dlss_preset"] = requested.dlss_preset;
        result["requested_fg_backend"] = requested.fg_backend;
        result["requested_fg_mode"] = requested.fg_mode;
        result["requested_generated_frames"] = requested.generated_frames;
        result["requested_latency_mode"] = requested.latency_mode;
    }
#define RSF_FIELD(name) result[#name] = status.name
    RSF_FIELD(session_id); RSF_FIELD(requested_sr); RSF_FIELD(active_sr);
    RSF_FIELD(render_width); RSF_FIELD(render_height); RSF_FIELD(output_width); RSF_FIELD(output_height);
    RSF_FIELD(sr_result); RSF_FIELD(fg_result); RSF_FIELD(fg_backend); RSF_FIELD(fg_available);
    RSF_FIELD(fg_active); RSF_FIELD(max_generated_frames); RSF_FIELD(generated_frames);
    RSF_FIELD(latency_available); RSF_FIELD(latency_mode); RSF_FIELD(reason); RSF_FIELD(pcl_available);
    RSF_FIELD(latency_report_available); RSF_FIELD(evaluated_frames); RSF_FIELD(refused_frames);
    RSF_FIELD(tagged_frames); RSF_FIELD(sleep_calls); RSF_FIELD(marker_calls);
    RSF_FIELD(total_presented); RSF_FIELD(generated_presented); RSF_FIELD(presentation_statistics_valid);
    RSF_FIELD(presentation_statistics_flags); RSF_FIELD(source_frame_id); RSF_FIELD(provider_generation); RSF_FIELD(generated_callbacks);
    RSF_FIELD(window_foreground); RSF_FIELD(window_visible); RSF_FIELD(foreground_process_id);
    RSF_FIELD(keyboard_inputs); RSF_FIELD(mouse_inputs); RSF_FIELD(controller_inputs); RSF_FIELD(pcl_pings);
#undef RSF_FIELD
    return result;
}
void ReScaleFrameRuntime::reset_history() { rsfg_reset_history(); }
bool ReScaleFrameRuntime::request_latency_ping() { return rsfg_request_latency_ping() != 0; }
}
