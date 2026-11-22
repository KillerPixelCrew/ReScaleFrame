// SPDX-License-Identifier: MIT
#include "runtime_control.h"
#include "api.h"
#include "godot_platform.h"
#include "script_dictionary.h"
#include <godot_cpp/core/class_db.hpp>
#include <godot_cpp/classes/rendering_server.hpp>
#include <godot_cpp/variant/callable_method_pointer.hpp>

// The option schema, its type/range/default rules, renderer admission and bridge readiness are
// owned by the Rust control owner (native/extension.rs, native/control.rs). This class keeps the
// typed godot-cpp Dictionary, RID and RenderingServer calls.
namespace rsf {
using namespace godot;
namespace {
uint32_t admission(RenderingServer* rendering)
{
    if (!rendering) return 0;
    const CharString driver = rendering->get_current_rendering_driver_name().utf8();
    const CharString method = rendering->get_current_rendering_method().utf8();
    return rsf_godot_renderer_admission(driver.get_data(), method.get_data());
}
uint32_t read_option(const void* dictionary, const rsf_godot_option_field* field,
    rsf_godot_option_value* value) noexcept try
{
    const auto& options = *static_cast<const Dictionary*>(dictionary);
    if (!options.has(field->name)) return 1;
    const Variant raw = options[field->name];
    value->present = 1;
    value->variant = raw.get_type() == Variant::INT ? RSF_GODOT_VARIANT_INT :
        raw.get_type() == Variant::BOOL ? RSF_GODOT_VARIANT_BOOL : RSF_GODOT_VARIANT_OTHER;
    if (field->conversion == RSF_GODOT_OPTION_REAL) value->real = static_cast<double>(raw);
    else value->integer = static_cast<int64_t>(raw);
    return 1;
} catch (...) { return 0; }
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
    return rsf_godot_extension_configure(admission(RenderingServer::get_singleton()), viewport.get_id(), &options, read_option) != 0;
}
Dictionary ReScaleFrameRuntime::get_status() const
{
    Dictionary result;
    auto* rendering = RenderingServer::get_singleton();
    ScriptDictionary writer{result, rendering};
    rsf_godot_extension_status(admission(rendering), &writer, ScriptDictionary::emit);
    return result;
}
void ReScaleFrameRuntime::reset_history() { rsfg_reset_history(); }
bool ReScaleFrameRuntime::request_latency_ping() { return rsfg_request_latency_ping() != 0; }
void ReScaleFrameRuntime::request_frame_dump() {
    if (auto* rendering = RenderingServer::get_singleton())
        rendering->call_on_render_thread(callable_mp_static(&rsf_godot_extension_capture));
}
}
