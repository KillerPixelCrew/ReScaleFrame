// SPDX-License-Identifier: MIT
// Public entry points of the shared translucency owner. native_translucency.rs owns its state,
// pass order and resources; this file keeps the HLSL and the calls that need the CMake build:
// the compile, the shared D3D11 state scope and the frame tap sampler bias.
#include <rescaleframe/native_translucency.h>
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/frame_tap.h>
#include <rescaleframe/shader_compile.h>
#include <d3d11.h>
#include <wrl/client.h>
namespace {
// Mode 0 compares scene colour after a pass drawn into it with the opaque snapshot. Mode 1 adds
// the offscreen layer exactly as the game composites it (scene * a + rgb) and merges the result.
// Differences are taken after a max-channel Reinhard so HDR highlights do not saturate the mask.
constexpr char shader_source[] = R"(
Texture2D<float4> Scene : register(t0);
Texture2D<float4> Opaque : register(t1);
Texture2D<float4> Layer : register(t2);
SamplerState Linear : register(s0);
RWTexture2D<float> Reactive : register(u0);
RWTexture2D<float> Coverage : register(u1);
RWTexture2D<float> Bias : register(u2);
cbuffer Params : register(b0) {
    int2 Origin;
    uint2 Size;
    float2 SceneInvSize;
    uint Mode;
    float CoverageScale;
};
float3 tonemap(float3 c) { c = max(c, 0); return c / (1 + max(c.r, max(c.g, c.b))); }
float change(float3 a, float3 b) { float3 d = abs(tonemap(a) - tonemap(b)); return max(d.r, max(d.g, d.b)); }
[numthreads(8,8,1)] void main(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= Size)) return;
    int2 pixel = Origin + int2(id.xy);
    float3 scene = Scene.Load(int3(pixel, 0)).rgb;
    float reactive, coverage;
    if (Mode == 0) {
        reactive = change(scene, Opaque.Load(int3(id.xy, 0)).rgb);
        coverage = saturate(reactive * CoverageScale);
    } else {
        float4 layer = Layer.SampleLevel(Linear, (float2(pixel) + 0.5) * SceneInvSize, 0);
        float opacity = saturate(1 - layer.a);
        float visible = change(scene * saturate(layer.a) + max(layer.rgb, 0), scene);
        reactive = max(Reactive[id.xy], max(visible, opacity));
        coverage = max(Coverage[id.xy], max(opacity, saturate(visible * CoverageScale)));
    }
    reactive = isfinite(reactive) ? reactive : 1;
    coverage = isfinite(coverage) ? coverage : 1;
    Reactive[id.xy] = min(reactive, 0.9);
    Coverage[id.xy] = coverage;
    Bias[id.xy] = coverage >= 0.5 ? 1 : 0;
})";

void* compile_masks(void* device) try
{
    Microsoft::WRL::ComPtr<ID3D11ComputeShader> shader;
    // Compiled without strictness, as it always has been.
    if (!rsf::compile_compute(static_cast<ID3D11Device*>(device), shader_source, "translucency_masks", "main",
            shader.GetAddressOf(), nullptr, nullptr, 0)) return nullptr;
    return shader.Detach();
}
catch (...) { return nullptr; }

void material_bias(float bias) { (void)rsf_frame_tap_set_sampler_bias(bias); }

struct Leaves {
    decltype(&compile_masks) compile;
    decltype(&rsf_d3d11_state_save) save;
    decltype(&rsf_d3d11_state_restore) restore;
    decltype(&material_bias) sampler_bias;
};
constexpr Leaves leaves{compile_masks, rsf_d3d11_state_save, rsf_d3d11_state_restore, material_bias};
static_assert(sizeof(Leaves) == 32, "Rust translucency leaf layout");
static_assert(sizeof(rsf_native_translucency_masks) == 56, "Rust translucency mask layout");
}

extern "C" {
void rsf_core_native_translucency_pass(void* context, const rsf_game_render_pass* pass, uint32_t begin,
    const Leaves* leaves);
int rsf_core_native_translucency_take(const rsf_game_render_pass* sr_pass, rsf_native_translucency_masks* out);
void rsf_core_native_translucency_release();
}

namespace {
// The owner held COM references in a static before; release them at unload as it did.
struct ReleaseAtUnload {
    ~ReleaseAtUnload() { rsf_core_native_translucency_release(); }
} release_at_unload;
}

extern "C" RSF_RUNTIME_API void rsf_native_translucency_pass(void* context_pointer, const rsf_game_render_pass* pass,
    uint32_t begin)
{
    rsf_core_native_translucency_pass(context_pointer, pass, begin, &leaves);
}

extern "C" RSF_RUNTIME_API int rsf_native_translucency_take(const rsf_game_render_pass* sr_pass, rsf_native_translucency_masks* out)
{
    return rsf_core_native_translucency_take(sr_pass, out);
}

extern "C" RSF_RUNTIME_API void rsf_native_translucency_release(void)
{
    rsf_core_native_translucency_release();
}