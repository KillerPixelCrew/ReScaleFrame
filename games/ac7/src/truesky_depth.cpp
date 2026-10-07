// SPDX-License-Identifier: GPL-3.0-only
#include "truesky_depth.h"
#include <d3dcompiler.h>
#include <wrl/client.h>

// One-source-pixel depth-bounds kernel for the guarded TrueSky route. register layout mirrors
// the measured native effect; the native caller validates resources and restores its own shader.
namespace {
constexpr char source[] = R"(
Texture2D<float> SourceDepth : register(t1);
SamplerState NearestClamp : register(s15);
RWTexture2D<float2> DepthBounds : register(u0);
cbuffer MixedResolution : register(b11) {
    float4 ReservedMatrices[13];
    float4 DepthToDistance;
    uint2 SourceSize; uint2 MaxSize;
    uint2 TargetSize; uint2 SourceOffset;
    uint4 TargetRange; uint4 DrawRange;
    uint2 Scale; float2 TexelRange;
    float4 DepthWindow;
    float2 StochasticOffset; float2 TanHalfFov;
    float NearDepth; float NearDistance; int CubeIndex; float FarZ;
};
[numthreads(8,8,1)] void main(uint3 pixel : SV_DispatchThreadID) {
    if (any(pixel.xy >= TargetSize)) return;
    float2 sourceUV = (float2(pixel.xy + SourceOffset) + 0.5) / float2(SourceSize);
    float depth = SourceDepth.SampleLevel(NearestClamp,sourceUV,0);
    float2 ndc = (float2(pixel.xy)+0.5) / float2(TargetSize) * float2(2,-2) + float2(-1,1);
    float2 ray = ndc * TanHalfFov;
    float distance = DepthToDistance.x / (depth * DepthToDistance.y + DepthToDistance.z)
        + depth * DepthToDistance.w;
    float normalized = depth >= 1 ? 0 : saturate(sqrt(1+dot(ray,ray)) * distance);
    // The native texture owner creates a two-channel Texture2D UAV. A single source sample
    // makes the maximum and minimum ray distances equal; float storage retains near geometry.
    DepthBounds[pixel.xy] = float2(normalized,normalized);
})";
}
// Compile/create on the supplied device. System compiler and temporary blobs are released before
// return; the successful output owns one COM reference and no compiler module dependency.
bool rsf_ac7_create_truesky_depth_shader(ID3D11Device* device, ID3D11ComputeShader** shader)
{
    if (!device || !shader) return false;
    *shader = nullptr;
    HMODULE compiler = LoadLibraryExW(L"d3dcompiler_47.dll",nullptr,LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!compiler) return false;
    auto compile = reinterpret_cast<decltype(&D3DCompile)>(reinterpret_cast<void*>(GetProcAddress(compiler,"D3DCompile")));
    Microsoft::WRL::ComPtr<ID3DBlob> bytes, errors;
    HRESULT result = compile ? compile(source,sizeof(source)-1,"AC7TrueSkyDepth1",nullptr,nullptr,
        "main","cs_5_0",D3DCOMPILE_ENABLE_STRICTNESS,0,&bytes,&errors) : E_FAIL;
    if (SUCCEEDED(result)) result = device->CreateComputeShader(bytes->GetBufferPointer(),bytes->GetBufferSize(),nullptr,shader);
    bytes.Reset(); errors.Reset(); FreeLibrary(compiler);
    return SUCCEEDED(result);
}
