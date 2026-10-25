// SPDX-License-Identifier: GPL-3.0-only
#include "effect.h"
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
#include <algorithm>
#include <cmath>

namespace rsf {
using namespace godot;

namespace {
PackedByteArray read_motion(RenderingDevice* rd, RID motion, Vector2i size)
{
    // The engine velocity target is sampleable, but deliberately is not a copy source.
    // Convert into an owned buffer through the graph before bounded CPU readback.
    Ref<RDShaderSource> source;
    source.instantiate();
    source->set_stage_source(RenderingDevice::SHADER_STAGE_COMPUTE, R"(#version 450
layout(local_size_x=8, local_size_y=8) in;
layout(set=0,binding=0) uniform sampler2D motion_tex;
layout(set=0,binding=1,std430) writeonly buffer Samples { vec2 velocity[]; };
void main() {
    ivec2 p=ivec2(gl_GlobalInvocationID.xy), size=textureSize(motion_tex,0);
    if(any(greaterThanEqual(p,size))) return;
    velocity[p.y*size.x+p.x]=texelFetch(motion_tex,p,0).xy;
})");
    Ref<RDShaderSPIRV> spirv = rd->shader_compile_spirv_from_source(source);
    if (spirv.is_null() || !spirv->get_stage_compile_error(RenderingDevice::SHADER_STAGE_COMPUTE).is_empty()) return {};
    const RID shader = rd->shader_create_from_spirv(spirv);
    if (!shader.is_valid()) return {};
    const RID pipeline = rd->compute_pipeline_create(shader);
    Ref<RDSamplerState> sampler_state;
    sampler_state.instantiate();
    const RID sampler = rd->sampler_create(sampler_state);
    const RID buffer = rd->storage_buffer_create(static_cast<uint32_t>(size.x) * size.y * 8);
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
        rd->compute_list_dispatch(list, (size.x + 7) / 8, (size.y + 7) / 8, 1);
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
}

ReScaleFrameEffect::ReScaleFrameEffect()
{
    set_effect_callback_type(EFFECT_CALLBACK_TYPE_POST_TRANSPARENT);
    set_access_resolved_color(true);
    set_access_resolved_depth(true);
    set_needs_motion_vectors(true);
}

void ReScaleFrameEffect::_bind_methods()
{
    ClassDB::bind_method(D_METHOD("get_status"), &ReScaleFrameEffect::get_status);
    ClassDB::bind_method(D_METHOD("request_motion_capture"), &ReScaleFrameEffect::request_motion_capture);
}

void ReScaleFrameEffect::request_motion_capture()
{
    capture_requested.store(true);
}

void ReScaleFrameEffect::_render_callback(int32_t type, RenderData* data)
{
    if (type != EFFECT_CALLBACK_TYPE_POST_TRANSPARENT || !data) return;
    Snapshot next;
    MotionCapture sampled;
    const bool requested = capture_requested.exchange(false);
    Ref<RenderSceneBuffersRD> buffers = data->get_render_scene_buffers();
    RenderSceneData* scene = data->get_render_scene_data();
    RenderingDevice* rd = RenderingServer::get_singleton()->get_rendering_device();
    if (rd && buffers.is_valid() && scene) {
        next.render_target = buffers->get_render_target().get_id();
        next.views = buffers->get_view_count();
        next.internal_size = buffers->get_internal_size();
        next.output_size = buffers->get_target_size();
        next.camera_transform = scene->get_cam_transform();
        next.jittered_projection = scene->get_cam_projection();
        const RID color = buffers->get_color_layer(0);
        const RID depth = buffers->get_depth_layer(0);
        const RID motion = buffers->get_velocity_layer(0);
        next.buffers_available = next.views == 1 && next.internal_size.x > 0 &&
            next.internal_size.y > 0 && rd->texture_is_valid(color) &&
            rd->texture_is_valid(depth) && rd->texture_is_valid(motion);
        if (next.buffers_available) {
            next.color_format = rd->texture_get_format(color)->get_format();
            next.depth_format = rd->texture_get_format(depth)->get_format();
            next.motion_format = rd->texture_get_format(motion)->get_format();
            next.native_device_available = rd->get_driver_resource(RenderingDevice::DRIVER_RESOURCE_LOGICAL_DEVICE, RID(), 0) != 0;
            next.native_queue_available = rd->get_driver_resource(RenderingDevice::DRIVER_RESOURCE_COMMAND_QUEUE, RID(), 0) != 0;
            next.native_textures_available = rd->get_driver_resource(RenderingDevice::DRIVER_RESOURCE_TEXTURE, color, 0) != 0 &&
                rd->get_driver_resource(RenderingDevice::DRIVER_RESOURCE_TEXTURE, depth, 0) != 0 &&
                rd->get_driver_resource(RenderingDevice::DRIVER_RESOURCE_TEXTURE, motion, 0) != 0;
            if (requested && next.motion_format == RenderingDevice::DATA_FORMAT_R16G16_SFLOAT) {
                // Explicit bounded diagnostics only. This readback stalls the GPU.
                // Packed data owns its CPU storage; the borrowed texture never leaves the callback.
                const PackedByteArray bytes = read_motion(rd, motion, next.internal_size);
                sampled.pixels = static_cast<uint64_t>(next.internal_size.x) * next.internal_size.y;
                sampled.succeeded = static_cast<uint64_t>(bytes.size()) == sampled.pixels * 8;
                if (sampled.succeeded) {
                    for (int64_t offset = 0; offset < bytes.size(); offset += 8) {
                        const double x = bytes.decode_float(offset);
                        const double y = bytes.decode_float(offset + 4);
                        if (!std::isfinite(x) || !std::isfinite(y)) {
                            ++sampled.nonfinite_pixels;
                        } else if (x <= -1.0 && y <= -1.0) {
                            ++sampled.unwritten_pixels;
                        } else {
                            const double magnitude = std::max(std::abs(x), std::abs(y));
                            if (magnitude > 0.00001) ++sampled.moving_pixels;
                            sampled.max_abs_uv = std::max(sampled.max_abs_uv, magnitude);
                        }
                    }
                }
            }
        }
    }
    // Scene objects, RIDs and native pointers are borrowed only inside this callback.
    // Publish copied CPU metadata; no resource lease or GPU completion is implied.
    std::lock_guard lock(guard);
    next.callbacks = snapshot.callbacks + 1;
    if (requested) {
        sampled.count = capture.count + 1;
        sampled.callback = next.callbacks;
        capture = sampled;
    }
    snapshot = next;
}

Dictionary ReScaleFrameEffect::get_status() const
{
    Snapshot value;
    MotionCapture sampled;
    {
        std::lock_guard lock(guard);
        value = snapshot;
        sampled = capture;
    }
    Dictionary result;
    result["version"] = RSF_VERSION_STRING;
    result["stage"] = "renderer_probe";
    result["rendering_ready"] = false;
    result["sr_active"] = false;
    result["fg_active"] = false;
    result["latency_active"] = false;
    result["reason"] = "Compositor motion producer/probe only. Query ReScaleFrameRuntime for active SR, FG and latency.";
    result["renderer"] = RenderingServer::get_singleton()->get_current_rendering_method();
    result["driver"] = RenderingServer::get_singleton()->get_current_rendering_driver_name();
    result["snapshot_scope"] = "last_post_transparent_callback";
    result["callbacks"] = static_cast<int64_t>(value.callbacks);
    result["render_target_id"] = static_cast<int64_t>(value.render_target);
    result["view_count"] = value.views;
    result["internal_size"] = value.internal_size;
    result["output_size"] = value.output_size;
    result["camera_transform"] = value.camera_transform;
    result["jittered_projection"] = value.jittered_projection;
    result["color_format"] = value.color_format;
    result["depth_format"] = value.depth_format;
    result["motion_format"] = value.motion_format;
    result["buffers_available"] = value.buffers_available;
    result["native_device_available"] = value.native_device_available;
    result["native_queue_available"] = value.native_queue_available;
    result["native_textures_available"] = value.native_textures_available;
    result["motion_captures"] = static_cast<int64_t>(sampled.count);
    result["capture_callback"] = static_cast<int64_t>(sampled.callback);
    result["capture_succeeded"] = sampled.succeeded;
    result["capture_pixels"] = static_cast<int64_t>(sampled.pixels);
    result["capture_moving_pixels"] = static_cast<int64_t>(sampled.moving_pixels);
    result["capture_nonfinite_pixels"] = static_cast<int64_t>(sampled.nonfinite_pixels);
    result["capture_unwritten_pixels"] = static_cast<int64_t>(sampled.unwritten_pixels);
    result["capture_max_abs_uv"] = sampled.max_abs_uv;
    return result;
}
}
