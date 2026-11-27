// SPDX-License-Identifier: MIT
#include "rsf_godot_engine.h"
#include "motion.h"
#include "servers/rendering/renderer_rd/storage_rd/render_scene_buffers_rd.h"
#include "servers/rendering/renderer_rd/uniform_set_cache_rd.h"
#include <cstring>

namespace {
struct Context {
    RD *rd;
    RenderSceneBuffersRD *buffers;
    RID depth, motion;
};

const rsf_godot_motion_calls calls{
    [](void *opaque, const char *source) -> uint64_t {
        auto *rd = static_cast<Context *>(opaque)->rd;
        String error;
        RD::ShaderStageSPIRVData stage;
        stage.shader_stage = RD::SHADER_STAGE_COMPUTE;
        stage.spirv = rd->shader_compile_spirv_from_source(stage.shader_stage, String(source), RD::SHADER_LANGUAGE_GLSL, &error);
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
        const float *to_previous, const Vector2 &jitter, bool normalize_depth, RID &resolved_depth) {
    const Size2i size = buffers->get_internal_size();
    rsf_godot_motion_constants facts{{}, {jitter.x, jitter.y}, {uint32_t(size.x), uint32_t(size.y)}};
    std::memcpy(facts.matrix, to_previous, sizeof(facts.matrix));
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
