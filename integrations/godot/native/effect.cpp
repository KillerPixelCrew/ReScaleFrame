// SPDX-License-Identifier: MIT
#include "effect.h"
#include "godot_platform.h"
#include "script_dictionary.h"
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
// scene data and direct RenderingDevice calls. It materializes Rust properties with Godot Variants.
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

// The stack-owned SDK objects live through Rust's synchronous capture sequence.
struct MotionReadback {
    RenderingDevice* rd;
    RID motion;
    PackedByteArray& bytes;
    RID shader, pipeline, sampler, buffer, uniform_set;
    Ref<RDShaderSource> source;
    Ref<RDShaderSPIRV> spirv;
    Ref<RDSamplerState> sampler_state;
    Ref<RDUniform> texture_uniform, buffer_uniform;
    TypedArray<RDUniform> uniforms;

    MotionReadback(RenderingDevice* device, RID motion_layer, PackedByteArray& result)
        : rd(device), motion(motion_layer), bytes(result) {}

};

uint32_t compile_motion(void* call, const rsf_godot_effect_readback* plan) noexcept try {
    auto& value = *static_cast<MotionReadback*>(call);
    value.source.instantiate();
    value.source->set_stage_source(RenderingDevice::SHADER_STAGE_COMPUTE, plan->shader);
    value.spirv = value.rd->shader_compile_spirv_from_source(value.source);
    if (value.spirv.is_null() || !value.spirv->get_stage_compile_error(RenderingDevice::SHADER_STAGE_COMPUTE).is_empty()) return 0;
    value.shader = value.rd->shader_create_from_spirv(value.spirv);
    return value.shader.is_valid();
} catch (...) { return 0; }

uint32_t prepare_motion(void* call, const rsf_godot_effect_readback* plan, uint32_t valid[4]) noexcept try {
    auto& value = *static_cast<MotionReadback*>(call);
    value.pipeline = value.rd->compute_pipeline_create(value.shader);
    value.sampler_state.instantiate();
    value.sampler = value.rd->sampler_create(value.sampler_state);
    value.buffer = value.rd->storage_buffer_create(plan->bytes);
    value.texture_uniform.instantiate();
    value.texture_uniform->set_uniform_type(RenderingDevice::UNIFORM_TYPE_SAMPLER_WITH_TEXTURE);
    value.texture_uniform->set_binding(0);
    value.texture_uniform->add_id(value.sampler);
    value.texture_uniform->add_id(value.motion);
    value.buffer_uniform.instantiate();
    value.buffer_uniform->set_uniform_type(RenderingDevice::UNIFORM_TYPE_STORAGE_BUFFER);
    value.buffer_uniform->set_binding(1);
    value.buffer_uniform->add_id(value.buffer);
    value.uniforms.push_back(value.texture_uniform);
    value.uniforms.push_back(value.buffer_uniform);
    value.uniform_set = value.rd->uniform_set_create(value.uniforms, value.shader, 0);
    valid[0] = value.pipeline.is_valid();
    valid[1] = value.sampler.is_valid();
    valid[2] = value.buffer.is_valid();
    valid[3] = value.uniform_set.is_valid();
    return 1;
} catch (...) { return 0; }

uint32_t dispatch_motion(void* call, const rsf_godot_effect_readback* plan) noexcept try {
    auto& value = *static_cast<MotionReadback*>(call);
    const int64_t list = value.rd->compute_list_begin();
    value.rd->compute_list_bind_compute_pipeline(list, value.pipeline);
    value.rd->compute_list_bind_uniform_set(list, value.uniform_set, 0);
    value.rd->compute_list_dispatch(list, plan->groups_x, plan->groups_y, 1);
    value.rd->compute_list_end();
    return 1;
} catch (...) { return 0; }

uint32_t read_motion(void* call) noexcept try {
    auto& value = *static_cast<MotionReadback*>(call);
    value.bytes = value.rd->buffer_get_data(value.buffer);
    return 1;
} catch (...) { return 0; }

uint32_t free_motion_resource(void* call, uint32_t resource) noexcept try {
    auto& value = *static_cast<MotionReadback*>(call);
    RID rid;
    switch (resource) {
    case RSF_GODOT_MOTION_SHADER: rid = value.shader; break;
    case RSF_GODOT_MOTION_PIPELINE: rid = value.pipeline; break;
    case RSF_GODOT_MOTION_SAMPLER: rid = value.sampler; break;
    case RSF_GODOT_MOTION_BUFFER: rid = value.buffer; break;
    case RSF_GODOT_MOTION_UNIFORMS: rid = value.uniform_set; break;
    default: return 0;
    }
    if (rid.is_valid()) value.rd->free_rid(rid);
    return 1;
} catch (...) { return 0; }

const rsf_godot_motion_calls motion_calls{
    compile_motion, prepare_motion, dispatch_motion, read_motion, free_motion_resource
};

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
    MotionReadback readback{value.rd, value.motion, value.bytes};
    if (!rsf_godot_effect_record_motion(&readback, plan, &motion_calls)) return 0;
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
    Dictionary result;
    ScriptDictionary writer{result, RenderingServer::get_singleton()};
    rsf_godot_effect_properties(probe, RSF_VERSION_STRING, &writer, ScriptDictionary::emit);
    return result;
}
}
