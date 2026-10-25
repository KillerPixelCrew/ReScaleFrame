// SPDX-License-Identifier: GPL-3.0-only
#include "rsf_godot_engine.h"
#include "servers/rendering/renderer_rd/storage_rd/render_scene_buffers_rd.h"
#include "servers/rendering/renderer_rd/uniform_set_cache_rd.h"

namespace {
RID shader, pipeline, sampler;
constexpr char source[] = R"(
#version 450
layout(local_size_x=8, local_size_y=8, local_size_z=1) in;
layout(set=0,binding=0) uniform sampler2D source_motion;
layout(set=0,binding=1) uniform sampler2D source_depth;
layout(set=0,binding=2,rg16f) uniform restrict writeonly image2D resolved_motion;
layout(push_constant,std430) uniform Constants {
    mat4 clip_to_previous_clip;
    vec2 jitter_uv;
    uvec2 size;
} params;
void main() {
    uvec2 pixel = gl_GlobalInvocationID.xy;
    if (any(greaterThanEqual(pixel, params.size))) return;
    vec2 motion = texelFetch(source_motion, ivec2(pixel), 0).xy;
    // Godot skips static geometry in its temporal motion pass. (-1,-1) means
    // derive camera motion from depth, rather than a real UV displacement.
    if (all(lessThanEqual(motion, vec2(-1.0)))) {
        vec2 uv = (vec2(pixel) + 0.5) / vec2(params.size) - params.jitter_uv;
        float depth = texelFetch(source_depth, ivec2(pixel), 0).x;
        vec4 previous = params.clip_to_previous_clip * vec4(uv.x*2.0-1.0, 1.0-uv.y*2.0, depth, 1.0);
        motion = vec2(previous.x, -previous.y) / previous.w * 0.5 + 0.5 - uv;
    }
    imageStore(resolved_motion, ivec2(pixel), vec4(motion, 0.0, 0.0));
}
)";
bool prepare(RD *rd) {
    if (pipeline.is_valid()) return true;
    String error;
    RD::ShaderStageSPIRVData stage;
    stage.shader_stage = RD::SHADER_STAGE_COMPUTE;
    stage.spirv = rd->shader_compile_spirv_from_source(stage.shader_stage, source, RD::SHADER_LANGUAGE_GLSL, &error);
    if (stage.spirv.is_empty()) { ERR_PRINT("ReScaleFrame motion decoder: " + error); return false; }
    shader = rd->shader_create_from_spirv(Vector<RD::ShaderStageSPIRVData>({stage}), "ReScaleFrame Godot motion decoder");
    if (!shader.is_valid()) return false;
    pipeline = rd->compute_pipeline_create(shader);
    sampler = rd->sampler_create(RD::SamplerState());
    return pipeline.is_valid() && sampler.is_valid();
}
}

namespace RSFGodot {
RID resolve_motion(const Ref<RenderSceneBuffersRD> &buffers, RID depth, RID motion,
    const Projection &to_previous, const Vector2 &jitter) {
    RD *rd = RD::get_singleton();
    if (!prepare(rd)) return RID();
    const StringName scope("rescaleframe"), name("motion_uv");
    if (!buffers->has_texture(scope, name)) {
        buffers->create_texture(scope, name, RD::DATA_FORMAT_R16G16_SFLOAT,
            RD::TEXTURE_USAGE_SAMPLING_BIT | RD::TEXTURE_USAGE_STORAGE_BIT);
    }
    const RID output = buffers->get_texture(scope, name);
    if (!output.is_valid()) return RID();
    RD::Uniform velocity(RD::UNIFORM_TYPE_SAMPLER_WITH_TEXTURE, 0, Vector<RID>({sampler, motion}));
    RD::Uniform scene_depth(RD::UNIFORM_TYPE_SAMPLER_WITH_TEXTURE, 1, Vector<RID>({sampler, depth}));
    RD::Uniform result(RD::UNIFORM_TYPE_IMAGE, 2, Vector<RID>({output}));
    const RID uniforms = UniformSetCacheRD::get_singleton()->get_cache(shader, 0, velocity, scene_depth, result);
    struct Constants { float matrix[16], jitter[2]; uint32_t size[2]; } constants{};
    for (int column = 0; column < 4; ++column)
        for (int row = 0; row < 4; ++row) constants.matrix[column * 4 + row] = to_previous.columns[column][row];
    constants.jitter[0] = jitter.x * 0.5f; constants.jitter[1] = jitter.y * 0.5f;
    const Size2i size = buffers->get_internal_size();
    constants.size[0] = size.x; constants.size[1] = size.y;
    const auto commands = rd->compute_list_begin();
    rd->compute_list_bind_compute_pipeline(commands, pipeline);
    rd->compute_list_bind_uniform_set(commands, uniforms, 0);
    rd->compute_list_set_push_constant(commands, &constants, sizeof(constants));
    rd->compute_list_dispatch_threads(commands, size.x, size.y, 1);
    rd->compute_list_end();
    return output;
}
void release_motion() {
    RD *rd = RD::get_singleton();
    if (rd) {
        if (shader.is_valid()) rd->free(shader);
        if (sampler.is_valid()) rd->free(sampler);
    }
    shader = RID(); pipeline = RID(); sampler = RID();
}
}
