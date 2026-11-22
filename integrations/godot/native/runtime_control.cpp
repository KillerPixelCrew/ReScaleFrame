// SPDX-License-Identifier: MIT
#include "runtime_control.h"
#include "api.h"
#include "godot_platform.h"
#include <rescaleframe/input_dump.h>
#include <godot_cpp/core/class_db.hpp>
#include <godot_cpp/classes/rendering_server.hpp>
#include <godot_cpp/variant/callable_method_pointer.hpp>

// The option schema, its type/range/default rules, renderer admission and bridge readiness are
// owned by the Rust control owner (native/extension.rs, native/control.rs). This class keeps the
// typed godot-cpp Dictionary, RID and RenderingServer calls.
namespace rsf {
using namespace godot;
namespace {
void capture_on_render_thread() { rsf_input_dump_request(); }
uint32_t admission(RenderingServer* rendering)
{
    if (!rendering) return 0;
    const CharString driver = rendering->get_current_rendering_driver_name().utf8();
    const CharString method = rendering->get_current_rendering_method().utf8();
    return rsf_godot_renderer_admission(driver.get_data(), method.get_data(), rsfg_vulkan_supported() ? 1u : 0u);
}
void read_option(const Dictionary& options, const rsf_godot_option_field& field, rsf_godot_option_value& value)
{
    if (!field.name || !options.has(field.name)) return;
    const Variant raw = options[field.name];
    value.present = 1;
    value.variant = raw.get_type() == Variant::INT ? RSF_GODOT_VARIANT_INT :
        raw.get_type() == Variant::BOOL ? RSF_GODOT_VARIANT_BOOL : RSF_GODOT_VARIANT_OTHER;
    if (field.conversion == RSF_GODOT_OPTION_REAL) value.real = static_cast<double>(raw);
    else value.integer = static_cast<int64_t>(raw);
}
}
void ReScaleFrameRuntime::_bind_methods()
{
    ClassDB::bind_method(D_METHOD("configure", "options", "viewport"), &ReScaleFrameRuntime::configure);
    ClassDB::bind_method(D_METHOD("get_status"), &ReScaleFrameRuntime::get_status);
    ClassDB::bind_method(D_METHOD("reset_history"), &ReScaleFrameRuntime::reset_history);
    ClassDB::bind_method(D_METHOD("request_latency_ping"), &ReScaleFrameRuntime::request_latency_ping);
    ClassDB::bind_method(D_METHOD("request_frame_dump"), &ReScaleFrameRuntime::request_frame_dump);
}
bool ReScaleFrameRuntime::configure(const Dictionary& options, RID viewport)
{
    if (!viewport.is_valid()) return false;
    constexpr uint32_t required = RSF_GODOT_ADMIT_DRIVER | RSF_GODOT_ADMIT_RENDERER;
    if ((admission(RenderingServer::get_singleton()) & required) != required) return false;
    uint32_t count = 0;
    const rsf_godot_option_field* fields = rsf_godot_option_fields(&count);
    if (!fields || count != RSF_GODOT_OPTION_COUNT) return false;
    rsf_godot_option_value values[RSF_GODOT_OPTION_COUNT]{};
    for (uint32_t i = 0; i < count; ++i) read_option(options, fields[i], values[i]);
    rsf_godot_settings value{};
    if (!rsf_godot_compose_settings(rsf_godot_platform_leaves(), values, count, &value)) return false;
    return rsfg_configure_engine(&value, viewport.get_id()) != 0;
}
Dictionary ReScaleFrameRuntime::get_status() const
{
    Dictionary result;
    auto* rendering = RenderingServer::get_singleton();
    const uint32_t admitted = admission(rendering);
    result["graphics_driver"] = rendering ? rendering->get_current_rendering_driver_name() : String();
    result["renderer"] = rendering ? rendering->get_current_rendering_method() : String();
    result["graphics_driver_supported"] = (admitted & RSF_GODOT_ADMIT_DRIVER) != 0;
    result["renderer_supported"] = (admitted & RSF_GODOT_ADMIT_RENDERER) != 0;
    rsf_godot_status status{sizeof(status), RSF_GODOT_ABI_VERSION};
    const bool available = rsfg_status(&status) != 0 && status.session_id != 0;
    result["engine_bridge_ready"] = available;
    if (!available) return result;
    rsf_godot_settings requested{sizeof(requested), RSF_GODOT_SETTINGS_ABI_VERSION};
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
    RSF_FIELD(latency_owner); RSF_FIELD(input_substitutes); RSF_FIELD(order_violations); RSF_FIELD(derived_boundaries);
    RSF_FIELD(keyboard_inputs); RSF_FIELD(mouse_inputs); RSF_FIELD(controller_inputs); RSF_FIELD(pcl_pings);
#undef RSF_FIELD
    return result;
}
void ReScaleFrameRuntime::reset_history() { rsfg_reset_history(); }
bool ReScaleFrameRuntime::request_latency_ping() { return rsfg_request_latency_ping() != 0; }
void ReScaleFrameRuntime::request_frame_dump() {
    if (auto* rendering = RenderingServer::get_singleton())
        rendering->call_on_render_thread(callable_mp_static(&capture_on_render_thread));
}
}
