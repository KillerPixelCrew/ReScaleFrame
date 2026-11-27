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
