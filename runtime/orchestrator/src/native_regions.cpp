// SPDX-License-Identifier: GPL-3.0-only
#include "native_regions.h"
#include <windows.h>
#include <d3dcompiler.h>
#include <wrl/client.h>
#include <array>
#include <cstring>
namespace {
using Microsoft::WRL::ComPtr;
constexpr char depth_shader[] = R"(
Texture2D<float> Source : register(t0);
RWTexture2D<float> Target : register(u0);
cbuffer Region : register(b0) { uint2 Origin; uint2 Size; };
[numthreads(8,8,1)] void main(uint3 id : SV_DispatchThreadID) {
    if (all(id.xy < Size)) Target[id.xy] = Source.Load(int3(Origin + id.xy, 0));
})";
constexpr char color_shader[] = R"(
Texture2D<float3> Source : register(t0);
RWTexture2D<float4> Target : register(u0);
cbuffer Region : register(b0) { uint2 Origin; uint2 Size; };
[numthreads(8,8,1)] void main(uint3 id : SV_DispatchThreadID) {
    if (all(id.xy < Size)) Target[id.xy] = float4(Source.Load(int3(Origin + id.xy, 0)), 1);
})";
struct Resources {
    ComPtr<ID3D11Device> device;
    std::array<ComPtr<ID3D11Texture2D>,3> textures;
    ComPtr<ID3D11ComputeShader> shader;
    ComPtr<ID3D11ComputeShader> color_shader;
    ComPtr<ID3D11Buffer> constants;
    ComPtr<ID3D11UnorderedAccessView> depth_uav;
    ComPtr<ID3D11UnorderedAccessView> color_uav;
    uint32_t width = 0, height = 0;
    DXGI_FORMAT color = DXGI_FORMAT_UNKNOWN, motion = DXGI_FORMAT_UNKNOWN;
};
Resources& resources() { static Resources value; return value; }
DXGI_FORMAT depth_format(DXGI_FORMAT format) {
    switch (format) {
    case DXGI_FORMAT_R24G8_TYPELESS: case DXGI_FORMAT_D24_UNORM_S8_UINT: return DXGI_FORMAT_R24_UNORM_X8_TYPELESS;
    case DXGI_FORMAT_R32_TYPELESS: case DXGI_FORMAT_D32_FLOAT: return DXGI_FORMAT_R32_FLOAT;
    case DXGI_FORMAT_R32G8X24_TYPELESS: case DXGI_FORMAT_D32_FLOAT_S8X24_UINT: return DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS;
    case DXGI_FORMAT_R16_TYPELESS: case DXGI_FORMAT_D16_UNORM: return DXGI_FORMAT_R16_UNORM;
    default: return format;
    }
}
bool build_shader(Resources& r) {
    HMODULE compiler = LoadLibraryExW(L"d3dcompiler_47.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!compiler) return false;
    using Compile = decltype(&D3DCompile);
    auto compile = reinterpret_cast<Compile>(reinterpret_cast<void*>(GetProcAddress(compiler,"D3DCompile")));
    ComPtr<ID3DBlob> bytes, errors;
    HRESULT result = compile ? compile(depth_shader, sizeof(depth_shader)-1, "NativeDepthRegion", nullptr,
        nullptr, "main", "cs_5_0", D3DCOMPILE_ENABLE_STRICTNESS, 0, &bytes, &errors) : E_FAIL;
    if (SUCCEEDED(result)) result = r.device->CreateComputeShader(bytes->GetBufferPointer(), bytes->GetBufferSize(), nullptr, &r.shader);
    bytes.Reset(); errors.Reset();
    if (SUCCEEDED(result)) result = compile(color_shader, sizeof(color_shader)-1, "NativeColorRegion", nullptr,
        nullptr, "main", "cs_5_0", D3DCOMPILE_ENABLE_STRICTNESS, 0, &bytes, &errors);
    if (SUCCEEDED(result)) result = r.device->CreateComputeShader(bytes->GetBufferPointer(), bytes->GetBufferSize(), nullptr, &r.color_shader);
    bytes.Reset(); errors.Reset(); FreeLibrary(compiler);
    if (FAILED(result)) { r.shader.Reset(); r.color_shader.Reset(); return false; }
    D3D11_BUFFER_DESC b{}; b.ByteWidth = 16; b.Usage = D3D11_USAGE_DEFAULT; b.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    return SUCCEEDED(r.device->CreateBuffer(&b, nullptr, &r.constants));
}
bool allocate(Resources& r, uint32_t width, uint32_t height, DXGI_FORMAT color, DXGI_FORMAT motion) {
    std::array<ComPtr<ID3D11Texture2D>,3> next;
    const DXGI_FORMAT formats[]{DXGI_FORMAT_R16G16B16A16_FLOAT,DXGI_FORMAT_R32_FLOAT,motion};
    for (size_t i=0;i<next.size();++i) {
        D3D11_TEXTURE2D_DESC d{}; d.Width=width; d.Height=height; d.MipLevels=d.ArraySize=1;
        d.Format=formats[i]; d.SampleDesc.Count=1; d.BindFlags=D3D11_BIND_SHADER_RESOURCE;
        if (i<=1) d.BindFlags|=D3D11_BIND_UNORDERED_ACCESS;
        if (FAILED(r.device->CreateTexture2D(&d,nullptr,&next[i]))) return false;
    }
    ComPtr<ID3D11UnorderedAccessView> uav;
    if (FAILED(r.device->CreateUnorderedAccessView(next[1].Get(),nullptr,&uav))) return false;
    ComPtr<ID3D11UnorderedAccessView> color_uav;
    if (FAILED(r.device->CreateUnorderedAccessView(next[0].Get(),nullptr,&color_uav))) return false;
    r.textures=std::move(next); r.depth_uav=std::move(uav); r.color_uav=std::move(color_uav); r.width=width; r.height=height;
    r.color=color; r.motion=motion; return true;
}
}
bool rsf_native_regions_prepare(ID3D11DeviceContext* context, ID3D11Texture2D* color,
    ID3D11Texture2D* depth, ID3D11Texture2D* motion, const int32_t rect[4],
    ID3D11Texture2D** out_color, ID3D11Texture2D** out_depth, ID3D11Texture2D** out_motion) {
    if (!context || !color || !depth || !motion || !rect || !out_color || !out_depth || !out_motion ||
        rect[0]<0 || rect[1]<0 || rect[2]<=rect[0] || rect[3]<=rect[1]) return false;
    ID3D11Texture2D* input[]{color,depth,motion}; D3D11_TEXTURE2D_DESC descriptions[3]{};
    for (size_t i=0;i<3;++i) {
        input[i]->GetDesc(&descriptions[i]); const auto& d=descriptions[i];
        if (d.SampleDesc.Count!=1 || d.ArraySize!=1 || uint32_t(rect[2])>d.Width || uint32_t(rect[3])>d.Height) return false;
    }
    const uint32_t width=uint32_t(rect[2]-rect[0]), height=uint32_t(rect[3]-rect[1]);
    auto& r=resources(); ComPtr<ID3D11Device> device; context->GetDevice(&device);
    if (r.device.Get()!=device.Get()) { r={}; r.device=device; }
    if (!r.shader && !build_shader(r)) return false;
    if (r.width!=width || r.height!=height || r.color!=descriptions[0].Format || r.motion!=descriptions[2].Format)
        if (!allocate(r,width,height,descriptions[0].Format,descriptions[2].Format)) return false;
    D3D11_SHADER_RESOURCE_VIEW_DESC view{}; view.Format=depth_format(descriptions[1].Format);
    view.ViewDimension=D3D11_SRV_DIMENSION_TEXTURE2D; view.Texture2D.MipLevels=1;
    ComPtr<ID3D11ShaderResourceView> source;
    if (FAILED(device->CreateShaderResourceView(depth,&view,&source))) return false;
    ComPtr<ID3D11ShaderResourceView> color_source;
    if (FAILED(device->CreateShaderResourceView(color,nullptr,&color_source))) return false;
    context->OMSetRenderTargets(0,nullptr,nullptr);
    const D3D11_BOX box{uint32_t(rect[0]),uint32_t(rect[1]),0,uint32_t(rect[2]),uint32_t(rect[3]),1};
    context->CopySubresourceRegion(r.textures[2].Get(),0,0,0,0,motion,0,&box);
    const uint32_t constants[]{uint32_t(rect[0]),uint32_t(rect[1]),width,height};
    context->UpdateSubresource(r.constants.Get(),0,nullptr,constants,0,0);
    auto* srv=source.Get(); auto* uav=r.depth_uav.Get(); auto* cb=r.constants.Get();
    context->CSSetShader(r.shader.Get(),nullptr,0); context->CSSetShaderResources(0,1,&srv);
    context->CSSetUnorderedAccessViews(0,1,&uav,nullptr); context->CSSetConstantBuffers(0,1,&cb);
    context->Dispatch((width+7)/8,(height+7)/8,1);
    srv=nullptr; uav=nullptr; context->CSSetShaderResources(0,1,&srv); context->CSSetUnorderedAccessViews(0,1,&uav,nullptr);
    srv=color_source.Get(); uav=r.color_uav.Get();
    context->CSSetShader(r.color_shader.Get(),nullptr,0); context->CSSetShaderResources(0,1,&srv);
    context->CSSetUnorderedAccessViews(0,1,&uav,nullptr);
    context->Dispatch((width+7)/8,(height+7)/8,1);
    srv=nullptr; uav=nullptr; context->CSSetShaderResources(0,1,&srv); context->CSSetUnorderedAccessViews(0,1,&uav,nullptr);
    *out_color=r.textures[0].Get(); *out_depth=r.textures[1].Get(); *out_motion=r.textures[2].Get(); return true;
}
void rsf_native_regions_release() { resources()={}; }
