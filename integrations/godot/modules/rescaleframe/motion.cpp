// SPDX-License-Identifier: MIT
#include "rsf_godot_engine.h"
#include "motion.h"
#include "servers/rendering/renderer_rd/storage_rd/render_scene_buffers_rd.h"
#include "servers/rendering/renderer_rd/uniform_set_cache_rd.h"

namespace {
constexpr char source[] = R"(
#version 450
layout(local_size_x=8, local_size_y=8, local_size_z=1) in;
layout(set=0,binding=0) uniform sampler2D source_motion;
layout(set=0,binding=1) uniform sampler2D source_depth;
layout(set=0,binding=2,rg16f) uniform restrict writeonly image2D resolved_motion;
#ifdef RSF_NORMALIZE_DEPTH
layout(set=0,binding=3,r32f) uniform restrict writeonly image2D resolved_depth;
#endif
layout(push_constant,std430) uniform Constants {
    mat4 clip_to_previous_clip;
    vec2 jitter_uv;
    uvec2 size;
} params;
void main() {
    uvec2 pixel = gl_GlobalInvocationID.xy;
    if (any(greaterThanEqual(pixel, params.size))) return;
    vec2 motion = texelFetch(source_motion, ivec2(pixel), 0).xy;
#ifdef RSF_NORMALIZE_DEPTH
    float sampled_depth = texelFetch(source_depth, ivec2(pixel), 0).x;
    imageStore(resolved_depth, ivec2(pixel), vec4(sampled_depth));
#endif
    // Godot skips static geometry in its temporal motion pass. (-1,-1) means
    // derive camera motion from depth, rather than a real UV displacement.
    if (all(lessThanEqual(motion, vec2(-1.0)))) {
        vec2 uv = (vec2(pixel) + 0.5) / vec2(params.size) - params.jitter_uv;
#ifdef RSF_NORMALIZE_DEPTH
        float depth = sampled_depth;
#else
        float depth = texelFetch(source_depth, ivec2(pixel), 0).x;
#endif
        vec4 previous = params.clip_to_previous_clip * vec4(uv.x*2.0-1.0, 1.0-uv.y*2.0, depth, 1.0);
        motion = vec2(previous.x, -previous.y) / previous.w * 0.5 + 0.5 - uv;
    }
    imageStore(resolved_motion, ivec2(pixel), vec4(motion, 0.0, 0.0));
}
)";
struct Context {
    RD *rd;
    RenderSceneBuffersRD *buffers;
    RID depth, motion;
};

const rsf_godot_motion_calls calls{
    [](void *opaque, uint32_t normalized) -> uint64_t {
        auto *rd = static_cast<Context *>(opaque)->rd;
        String error;
        RD::ShaderStageSPIRVData stage;
        stage.shader_stage = RD::SHADER_STAGE_COMPUTE;
        String shader_source(source);
        if (normalized) shader_source = shader_source.replace("#version 450", "#version 450\n#define RSF_NORMALIZE_DEPTH");
        stage.spirv = rd->shader_compile_spirv_from_source(stage.shader_stage, shader_source, RD::SHADER_LANGUAGE_GLSL, &error);
        if (stage.spirv.is_empty()) { ERR_PRINT("ReScaleFrame motion decoder: " + error); return 0; }
        return rd->shader_create_from_spirv(Vector<RD::ShaderStageSPIRVData>({stage}), "ReScaleFrame Godot motion decoder").get_id();
    },
    [](void *opaque, uint64_t shader) -> uint64_t {
        return static_cast<Context *>(opaque)->rd->compute_pipeline_create(RID::from_uint64(shader)).get_id();
    },
    [](void *opaque) -> uint64_t {
        return static_cast<Context *>(opaque)->rd->sampler_create(RD::SamplerState()).get_id();
    },
    [](void *opaque, uint64_t rid) {
        if (auto *rd = static_cast<Context *>(opaque)->rd) rd->free_rid(RID::from_uint64(rid));
    },
    [](void *opaque, uint32_t depth) -> uint64_t {
        auto *buffers = static_cast<Context *>(opaque)->buffers;
        const StringName scope("rescaleframe"), name(depth ? "depth_r32" : "motion_uv");
        if (!buffers->has_texture(scope, name)) {
            buffers->create_texture(scope, name, depth ? RD::DATA_FORMAT_R32_SFLOAT : RD::DATA_FORMAT_R16G16_SFLOAT,
                RD::TEXTURE_USAGE_SAMPLING_BIT | RD::TEXTURE_USAGE_STORAGE_BIT | RD::TEXTURE_USAGE_CAN_COPY_FROM_BIT);
        }
        return buffers->get_texture(scope, name).get_id();
    },
    [](void *opaque, uint64_t shader_id, uint64_t pipeline, uint64_t sampler_id,
            uint64_t motion, uint64_t normalized, const rsf_godot_motion_constants *constants) {
        const auto &context = *static_cast<Context *>(opaque);
        auto *rd = context.rd;
        const RID shader = RID::from_uint64(shader_id), sampler = RID::from_uint64(sampler_id);
        RD::Uniform velocity(RD::UNIFORM_TYPE_SAMPLER_WITH_TEXTURE, 0, Vector<RID>({sampler, context.motion}));
        RD::Uniform scene_depth(RD::UNIFORM_TYPE_SAMPLER_WITH_TEXTURE, 1, Vector<RID>({sampler, context.depth}));
        RD::Uniform result(RD::UNIFORM_TYPE_IMAGE, 2, Vector<RID>({RID::from_uint64(motion)}));
        RID uniforms;
        if (normalized) {
            RD::Uniform depth_output(RD::UNIFORM_TYPE_IMAGE, 3, Vector<RID>({RID::from_uint64(normalized)}));
            uniforms = UniformSetCacheRD::get_singleton()->get_cache(shader, 0, velocity, scene_depth, result, depth_output);
        } else {
            uniforms = UniformSetCacheRD::get_singleton()->get_cache(shader, 0, velocity, scene_depth, result);
        }
        const auto commands = rd->compute_list_begin();
        rd->compute_list_bind_compute_pipeline(commands, RID::from_uint64(pipeline));
        rd->compute_list_bind_uniform_set(commands, uniforms, 0);
        rd->compute_list_set_push_constant(commands, constants, sizeof(*constants));
        rd->compute_list_dispatch_threads(commands, constants->size[0], constants->size[1], 1);
        rd->compute_list_end();
    },
};
}

namespace RSFGodot {
RID resolve_motion(const Ref<RenderSceneBuffersRD> &buffers, RID depth, RID motion,
        const Projection &to_previous, const Vector2 &jitter, bool normalize_depth, RID &resolved_depth) {
    const Size2i size = buffers->get_internal_size();
    rsf_godot_motion_constants facts{{}, {jitter.x, jitter.y}, {uint32_t(size.x), uint32_t(size.y)}};
    for (int column = 0; column < 4; ++column)
        for (int row = 0; row < 4; ++row) facts.matrix[column * 4 + row] = to_previous.columns[column][row];
    Context context{RD::get_singleton(), buffers.ptr(), depth, motion};
    uint64_t normalized = 0;
    const uint64_t output = rsf_godot_motion_resolve(&calls, &context, normalize_depth, &facts, &normalized);
    resolved_depth = normalize_depth ? RID::from_uint64(normalized) : depth;
    return RID::from_uint64(output);
}

void release_motion() {
    Context context{RD::get_singleton(), nullptr, RID(), RID()};
    rsf_godot_motion_release(&calls, &context);
}
}
