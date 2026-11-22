// SPDX-License-Identifier: MIT
#include "effect.h"
#include "godot_platform.h"
#include <rescaleframe/version.h>
#include <godot_cpp/classes/rd_texture_format.hpp>
#include <godot_cpp/classes/rd_shader_source.hpp>
#include <godot_cpp/classes/rd_shader_spirv.hpp>
#include <godot_cpp/classes/rd_sampler_state.hpp>
#include <godot_cpp/classes/rd_uniform.hpp>
#include <godot_cpp/classes/render_data.hpp>
#include <godot_cpp/classes/render_scene_buffers_rd.hpp>
#include <godot_cpp/classes/render_scene_data.hpp>
#include <godot_cpp/classes/rendering_device.hpp>
#include <godot_cpp/classes/rendering_server.hpp>
#include <godot_cpp/core/class_db.hpp>
#include <godot_cpp/variant/projection.hpp>
#include <godot_cpp/variant/transform3d.hpp>
#include <godot_cpp/variant/vector2i.hpp>
#include <cstddef>

// The snapshot, callback count, capture request, buffer and capture admission, the motion
// classification and the probe's published stage, reason and inactive pipeline claims are owned by
// native/effect.rs. This class keeps its registration and callback contract, the scene buffer,
// scene data and RenderingDevice reads, the readback recording calls and the status Dictionary keys,
// and lends them as rsf_godot_effect_leaves (godot_platform.h).
static_assert(sizeof(rsf_godot_effect_frame) == 272 && offsetof(rsf_godot_effect_frame, motion_valid) == 36 &&
    offsetof(rsf_godot_effect_frame, camera_transform) == 48 &&
    offsetof(rsf_godot_effect_frame, jittered_projection) == 144 && sizeof(rsf_godot_effect_resources) == 24 &&
    sizeof(rsf_godot_effect_status) == 416 && offsetof(rsf_godot_effect_status, frame) == 16 &&
    offsetof(rsf_godot_effect_status, resources) == 288 && offsetof(rsf_godot_effect_status, buffers_available) == 312 &&
    offsetof(rsf_godot_effect_status, captures) == 320 && offsetof(rsf_godot_effect_status, capture_max_abs_uv) == 368 &&
    offsetof(rsf_godot_effect_status, stage) == 376 && offsetof(rsf_godot_effect_status, rendering_ready) == 400 &&
    sizeof(rsf_godot_effect_readback) == 24 && offsetof(rsf_godot_effect_readback, groups_y) == 16 &&
    sizeof(rsf_godot_effect_leaves) == 32 && offsetof(rsf_godot_effect_leaves, read_motion) == 24,
    "native/platform.rs mirrors these layouts");

namespace rsf {
using namespace godot;
static_assert(RenderingDevice::DATA_FORMAT_R16G16_SFLOAT == RSF_GODOT_EFFECT_MOTION_FORMAT,
    "native/effect.rs admits this velocity format");

namespace {
// One POST_TRANSPARENT callback. Scene objects, RIDs and native pointers are borrowed only for the
// callback; the readback bytes are owned here until it returns.
struct Call {
    RenderData* data = nullptr;
    Ref<RenderSceneBuffersRD> buffers;
    RenderingDevice* rd = nullptr;
    RID color, depth, motion;
    PackedByteArray bytes;
};
Call& call_of(void* call) { return *static_cast<Call*>(call); }

PackedByteArray read_motion(RenderingDevice* rd, RID motion, const rsf_godot_effect_readback& plan)
{
    // The engine velocity target is sampleable, but deliberately is not a copy source.
    // Convert into an owned buffer through the graph before bounded CPU readback.
    Ref<RDShaderSource> source;
    source.instantiate();
    source->set_stage_source(RenderingDevice::SHADER_STAGE_COMPUTE, plan.shader);
    Ref<RDShaderSPIRV> spirv = rd->shader_compile_spirv_from_source(source);
    if (spirv.is_null() || !spirv->get_stage_compile_error(RenderingDevice::SHADER_STAGE_COMPUTE).is_empty()) return {};
    const RID shader = rd->shader_create_from_spirv(spirv);
    if (!shader.is_valid()) return {};
    const RID pipeline = rd->compute_pipeline_create(shader);
    Ref<RDSamplerState> sampler_state;
    sampler_state.instantiate();
    const RID sampler = rd->sampler_create(sampler_state);
    const RID buffer = rd->storage_buffer_create(plan.bytes);
    Ref<RDUniform> texture_uniform;
    texture_uniform.instantiate();
    texture_uniform->set_uniform_type(RenderingDevice::UNIFORM_TYPE_SAMPLER_WITH_TEXTURE);
    texture_uniform->set_binding(0);
    texture_uniform->add_id(sampler);
    texture_uniform->add_id(motion);
    Ref<RDUniform> buffer_uniform;
    buffer_uniform.instantiate();
    buffer_uniform->set_uniform_type(RenderingDevice::UNIFORM_TYPE_STORAGE_BUFFER);
    buffer_uniform->set_binding(1);
    buffer_uniform->add_id(buffer);
    TypedArray<RDUniform> uniforms;
    uniforms.push_back(texture_uniform);
    uniforms.push_back(buffer_uniform);
    const RID uniform_set = rd->uniform_set_create(uniforms, shader, 0);
    PackedByteArray result;
    if (pipeline.is_valid() && sampler.is_valid() && buffer.is_valid() && uniform_set.is_valid()) {
        const int64_t list = rd->compute_list_begin();
        rd->compute_list_bind_compute_pipeline(list, pipeline);
        rd->compute_list_bind_uniform_set(list, uniform_set, 0);
        rd->compute_list_dispatch(list, plan.groups_x, plan.groups_y, 1);
        rd->compute_list_end();
        result = rd->buffer_get_data(buffer);
    }
    if (uniform_set.is_valid()) rd->free_rid(uniform_set);
    if (buffer.is_valid()) rd->free_rid(buffer);
    if (sampler.is_valid()) rd->free_rid(sampler);
    if (pipeline.is_valid()) rd->free_rid(pipeline);
    rd->free_rid(shader);
    return result;
}

uint32_t leaf_frame(void* call, rsf_godot_effect_frame* frame) noexcept try {
    auto& value = call_of(call);
    value.buffers = value.data->get_render_scene_buffers();
    RenderSceneData* scene = value.data->get_render_scene_data();
    RenderingDevice* rd = RenderingServer::get_singleton()->get_rendering_device();
    if (!rd || !value.buffers.is_valid() || !scene) return 0;
    value.rd = rd;
    const Ref<RenderSceneBuffersRD>& buffers = value.buffers;
    frame->render_target = buffers->get_render_target().get_id();
    frame->views = buffers->get_view_count();
    const Vector2i internal_size = buffers->get_internal_size();
    const Vector2i output = buffers->get_target_size();
    frame->internal_width = internal_size.x;
    frame->internal_height = internal_size.y;
    frame->output_width = output.x;
    frame->output_height = output.y;
    const Transform3D camera = scene->get_cam_transform();
    const Projection projection = scene->get_cam_projection();
    for (int row = 0; row < 3; ++row) {
        for (int column = 0; column < 3; ++column) frame->camera_transform[row * 3 + column] = camera.basis[row][column];
        frame->camera_transform[9 + row] = camera.origin[row];
    }
    for (int column = 0; column < 4; ++column)
        for (int row = 0; row < 4; ++row) frame->jittered_projection[column * 4 + row] = projection[column][row];
    value.color = buffers->get_color_layer(0);
    value.depth = buffers->get_depth_layer(0);
    value.motion = buffers->get_velocity_layer(0);
    frame->color_valid = rd->texture_is_valid(value.color) ? 1u : 0u;
    frame->depth_valid = rd->texture_is_valid(value.depth) ? 1u : 0u;
    frame->motion_valid = rd->texture_is_valid(value.motion) ? 1u : 0u;
    return 1;
} catch (...) { return 0; }

uint32_t leaf_resources(void* call, rsf_godot_effect_resources* resources) noexcept try {
    const auto& value = call_of(call);
    RenderingDevice* rd = value.rd;
    resources->color_format = rd->texture_get_format(value.color)->get_format();
    resources->depth_format = rd->texture_get_format(value.depth)->get_format();
    resources->motion_format = rd->texture_get_format(value.motion)->get_format();
    resources->native_device = rd->get_driver_resource(RenderingDevice::DRIVER_RESOURCE_LOGICAL_DEVICE, RID(), 0) != 0;
    resources->native_queue = rd->get_driver_resource(RenderingDevice::DRIVER_RESOURCE_COMMAND_QUEUE, RID(), 0) != 0;
    resources->native_textures = rd->get_driver_resource(RenderingDevice::DRIVER_RESOURCE_TEXTURE, value.color, 0) != 0 &&
        rd->get_driver_resource(RenderingDevice::DRIVER_RESOURCE_TEXTURE, value.depth, 0) != 0 &&
        rd->get_driver_resource(RenderingDevice::DRIVER_RESOURCE_TEXTURE, value.motion, 0) != 0;
    return 1;
} catch (...) { return 0; }

uint32_t leaf_read_motion(void* call, const rsf_godot_effect_readback* plan, const uint8_t** data, uint64_t* size) noexcept try {
    auto& value = call_of(call);
    *data = nullptr;
    *size = 0;
    // Packed data owns its CPU storage; the borrowed texture never leaves the callback.
    value.bytes = read_motion(value.rd, value.motion, *plan);
    *data = value.bytes.ptr();
    *size = static_cast<uint64_t>(value.bytes.size());
    return 1;
} catch (...) { return 0; }

const rsf_godot_effect_leaves effect_leaves{sizeof(effect_leaves), RSF_GODOT_EFFECT_VERSION,
    leaf_frame, leaf_resources, leaf_read_motion};
}

ReScaleFrameEffect::ReScaleFrameEffect()
    : probe(rsf_godot_effect_create())
{
    set_effect_callback_type(EFFECT_CALLBACK_TYPE_POST_TRANSPARENT);
    set_access_resolved_color(true);
    set_access_resolved_depth(true);
    set_needs_motion_vectors(true);
}

ReScaleFrameEffect::~ReScaleFrameEffect()
{
    rsf_godot_effect_destroy(probe);
}

void ReScaleFrameEffect::_bind_methods()
{
    ClassDB::bind_method(D_METHOD("get_status"), &ReScaleFrameEffect::get_status);
    ClassDB::bind_method(D_METHOD("request_motion_capture"), &ReScaleFrameEffect::request_motion_capture);
}

void ReScaleFrameEffect::request_motion_capture()
{
    rsf_godot_effect_request_capture(probe);
}

void ReScaleFrameEffect::_render_callback(int32_t type, RenderData* data)
{
    if (type != EFFECT_CALLBACK_TYPE_POST_TRANSPARENT || !data) return;
    Call call;
    call.data = data;
    rsf_godot_effect_render(probe, &effect_leaves, &call);
}

Dictionary ReScaleFrameEffect::get_status() const
{
    rsf_godot_effect_status value{};
    value.struct_size = sizeof(value);
    value.version = RSF_GODOT_EFFECT_VERSION;
    Dictionary result;
    if (!rsf_godot_effect_status_get(probe, &value)) return result;
    Transform3D camera;
    for (int row = 0; row < 3; ++row) {
        for (int column = 0; column < 3; ++column)
            camera.basis[row][column] = static_cast<real_t>(value.frame.camera_transform[row * 3 + column]);
        camera.origin[row] = static_cast<real_t>(value.frame.camera_transform[9 + row]);
    }
    Projection projection;
    for (int column = 0; column < 4; ++column)
        for (int row = 0; row < 4; ++row)
            projection[column][row] = static_cast<real_t>(value.frame.jittered_projection[column * 4 + row]);
    result["version"] = RSF_VERSION_STRING;
    result["stage"] = value.stage;
    result["rendering_ready"] = value.rendering_ready != 0;
    result["sr_active"] = value.sr_active != 0;
    result["fg_active"] = value.fg_active != 0;
    result["latency_active"] = value.latency_active != 0;
    result["reason"] = value.reason;
    result["renderer"] = RenderingServer::get_singleton()->get_current_rendering_method();
    result["driver"] = RenderingServer::get_singleton()->get_current_rendering_driver_name();
    result["snapshot_scope"] = value.snapshot_scope;
    result["callbacks"] = static_cast<int64_t>(value.callbacks);
    result["render_target_id"] = static_cast<int64_t>(value.frame.render_target);
    result["view_count"] = value.frame.views;
    result["internal_size"] = Vector2i(value.frame.internal_width, value.frame.internal_height);
    result["output_size"] = Vector2i(value.frame.output_width, value.frame.output_height);
    result["camera_transform"] = camera;
    result["jittered_projection"] = projection;
    result["color_format"] = value.resources.color_format;
    result["depth_format"] = value.resources.depth_format;
    result["motion_format"] = value.resources.motion_format;
    result["buffers_available"] = value.buffers_available != 0;
    result["native_device_available"] = value.resources.native_device != 0;
    result["native_queue_available"] = value.resources.native_queue != 0;
    result["native_textures_available"] = value.resources.native_textures != 0;
    result["motion_captures"] = static_cast<int64_t>(value.captures);
    result["capture_callback"] = static_cast<int64_t>(value.capture_callback);
    result["capture_succeeded"] = value.capture_succeeded != 0;
    result["capture_pixels"] = static_cast<int64_t>(value.capture_pixels);
    result["capture_moving_pixels"] = static_cast<int64_t>(value.capture_moving_pixels);
    result["capture_nonfinite_pixels"] = static_cast<int64_t>(value.capture_nonfinite_pixels);
    result["capture_unwritten_pixels"] = static_cast<int64_t>(value.capture_unwritten_pixels);
    result["capture_max_abs_uv"] = value.capture_max_abs_uv;
    return result;
}
}
