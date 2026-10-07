// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/colour_fidelity.h>
#include <rescaleframe/d3d11_state.h>
#include <d3d11.h>
#include <d3dcompiler.h>
#include <wrl/client.h>
#include <cmath>
#include <new>
using Microsoft::WRL::ComPtr;
namespace {
constexpr char shaders[] = R"(
Texture2D<float4> Scene : register(t0);
Texture2D<float4> Reconstructed : register(t1);
Texture2D<float4> Residual : register(t2);
Texture2D<float> Depth : register(t3);
RWTexture2D<float4> Result : register(u0);
SamplerState LinearClamp : register(s0);
cbuffer Params : register(b0) { uint2 RenderSize; uint2 OutputSize; float2 Jitter; uint DepthInverted; uint Padding; };
float sourceDepth(int2 p) {
    float value = Depth.Load(int3(clamp(p,int2(0,0),int2(RenderSize)-1),0));
    return DepthInverted != 0 ? value : 1-value;
}
float surfaceConfidence(float lower, float upper) {
    return 1-smoothstep(0.01,0.05,(upper-lower)/max(upper,1e-7));
}
[numthreads(8,8,1)] void project(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= RenderSize)) return;
    float2 scale = float2(OutputSize) / float2(RenderSize);
    float2 a = (float2(id.xy) - Jitter) * scale;
    float2 b = a + scale;
    int2 first = int2(floor(a)), last = int2(ceil(b));
    float3 sum = 0; float total = 0;
    [loop] for (int y = first.y; y < last.y; ++y) {
        [loop] for (int x = first.x; x < last.x; ++x) {
            float2 area = max(0, min(b,float2(x+1,y+1)) - max(a,float2(x,y)));
            float weight = area.x * area.y;
            sum += Reconstructed.Load(int3(clamp(int2(x,y),int2(0,0),int2(OutputSize)-1),0)).rgb * weight;
            total += weight;
        }
    }
    float3 expected = max(0,Scene.Load(int3(id.xy,0)).rgb);
    float lower = sourceDepth(int2(id.xy)), upper = lower;
    [unroll] for (int y = -1; y <= 1; ++y) {
        [unroll] for (int x = -1; x <= 1; ++x) {
            float neighbour = sourceDepth(int2(id.xy)+int2(x,y));
            lower = min(lower,neighbour); upper = max(upper,neighbour);
        }
    }
    float3 average = sum / max(total,1e-8);
    Result[id.xy] = float4((expected - average)*surfaceConfidence(lower,upper),0);
}
[numthreads(8,8,1)] void apply(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= OutputSize)) return;
    float2 uv = (float2(id.xy)+0.5) / float2(OutputSize) + Jitter / float2(RenderSize);
    float4 colour = Reconstructed.Load(int3(id.xy,0));
    float3 correction = Residual.SampleLevel(LinearClamp,uv,0).rgb;
    int2 first = int2(floor(uv * float2(RenderSize) - 0.5));
    float3 lower = colour.rgb, upper = colour.rgb;
    float depthLower = 3.402823e38, depthUpper = 0;
    [unroll] for (int y = 0; y < 2; ++y) {
        [unroll] for (int x = 0; x < 2; ++x) {
            float3 source = max(0,Scene.Load(int3(clamp(first+int2(x,y),int2(0,0),int2(RenderSize)-1),0)).rgb);
            lower = min(lower,source); upper = max(upper,source);
            float z = sourceDepth(first+int2(x,y));
            depthLower = min(depthLower,z); depthUpper = max(depthUpper,z);
        }
    }
    correction *= surfaceConfidence(depthLower,depthUpper);
    // Only move toward the source's colour range. Existing reconstructed detail remains valid
    // when the residual is zero; correction cannot introduce brighter peaks or dark ringing.
    Result[id.xy] = float4(clamp(colour.rgb+correction,lower,upper),colour.a);
})";
struct Constants { uint32_t render[2], output[2]; float jitter[2]; uint32_t depth_inverted, padding; };
}
// Per-device scratch cache. Residual lives at render size; correction ping-pongs at output size.
// All ComPtr fields own their references. Extent/format changes replace the whole scratch set.
struct rsf_colour_fidelity {
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11ComputeShader> project, apply;
    ComPtr<ID3D11Buffer> constants;
    ComPtr<ID3D11SamplerState> sampler;
    ComPtr<ID3D11Texture2D> residual, corrected[2];
    ComPtr<ID3D11ShaderResourceView> residual_view, corrected_views[2];
    ComPtr<ID3D11UnorderedAccessView> residual_target, corrected_targets[2];
    uint32_t render_width=0,render_height=0,width=0,height=0;
    DXGI_FORMAT format=DXGI_FORMAT_UNKNOWN;
};
extern "C" int rsf_colour_fidelity_create(void* device, rsf_colour_fidelity** out) {
    if (!device || !out) return 0;
    *out=nullptr;
    auto* pass=new(std::nothrow) rsf_colour_fidelity;
    if (!pass) return 0;
    pass->device=static_cast<ID3D11Device*>(device);
    HMODULE module=LoadLibraryExW(L"d3dcompiler_47.dll",nullptr,LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module) { delete pass; return 0; }
    auto compile=reinterpret_cast<decltype(&D3DCompile)>(reinterpret_cast<void*>(GetProcAddress(module,"D3DCompile")));
    ComPtr<ID3DBlob> code,errors;
    HRESULT result=compile ? compile(shaders,sizeof(shaders)-1,"ColourFidelity",nullptr,nullptr,"project",
        "cs_5_0",D3DCOMPILE_ENABLE_STRICTNESS,0,&code,&errors) : E_FAIL;
    if (SUCCEEDED(result)) result=pass->device->CreateComputeShader(code->GetBufferPointer(),code->GetBufferSize(),nullptr,&pass->project);
    code.Reset(); errors.Reset();
    if (SUCCEEDED(result)) result=compile(shaders,sizeof(shaders)-1,"ColourFidelity",nullptr,nullptr,"apply",
        "cs_5_0",D3DCOMPILE_ENABLE_STRICTNESS,0,&code,&errors);
    if (SUCCEEDED(result)) result=pass->device->CreateComputeShader(code->GetBufferPointer(),code->GetBufferSize(),nullptr,&pass->apply);
    code.Reset(); errors.Reset(); FreeLibrary(module);
    D3D11_BUFFER_DESC buffer{}; buffer.ByteWidth=sizeof(Constants); buffer.BindFlags=D3D11_BIND_CONSTANT_BUFFER;
    if (SUCCEEDED(result)) result=pass->device->CreateBuffer(&buffer,nullptr,&pass->constants);
    D3D11_SAMPLER_DESC sampler{}; sampler.Filter=D3D11_FILTER_MIN_MAG_LINEAR_MIP_POINT;
    sampler.AddressU=sampler.AddressV=sampler.AddressW=D3D11_TEXTURE_ADDRESS_CLAMP;
    if (SUCCEEDED(result)) result=pass->device->CreateSamplerState(&sampler,&pass->sampler);
    if (FAILED(result)) { delete pass; return 0; }
    *out=pass; return 1;
}
namespace {
// Build a complete replacement set before publishing it, keeping the prior cache on failure.
bool allocate(rsf_colour_fidelity& pass, const D3D11_TEXTURE2D_DESC& low, const D3D11_TEXTURE2D_DESC& high) {
    ComPtr<ID3D11Texture2D> textures[3]; ComPtr<ID3D11ShaderResourceView> views[3];
    ComPtr<ID3D11UnorderedAccessView> targets[3];
    for (uint32_t i=0;i<3;++i) {
        D3D11_TEXTURE2D_DESC d{}; d.Width=i?high.Width:low.Width; d.Height=i?high.Height:low.Height;
        d.Format=i?high.Format:DXGI_FORMAT_R32G32B32A32_FLOAT;
        d.MipLevels=d.ArraySize=d.SampleDesc.Count=1; d.BindFlags=D3D11_BIND_SHADER_RESOURCE|D3D11_BIND_UNORDERED_ACCESS;
        if (FAILED(pass.device->CreateTexture2D(&d,nullptr,&textures[i])) ||
            FAILED(pass.device->CreateShaderResourceView(textures[i].Get(),nullptr,&views[i])) ||
            FAILED(pass.device->CreateUnorderedAccessView(textures[i].Get(),nullptr,&targets[i]))) return false;
    }
    pass.residual=std::move(textures[0]); pass.residual_view=std::move(views[0]); pass.residual_target=std::move(targets[0]);
    for (uint32_t i=0;i<2;++i) {
        pass.corrected[i]=std::move(textures[i+1]); pass.corrected_views[i]=std::move(views[i+1]);
        pass.corrected_targets[i]=std::move(targets[i+1]);
    }
    pass.render_width=low.Width; pass.render_height=low.Height; pass.width=high.Width; pass.height=high.Height;
    pass.format=high.Format; return true;
}
}
extern "C" int rsf_colour_fidelity_run(rsf_colour_fidelity* pass, void* raw_context,
    void* raw_scene, void* raw_output, void* raw_depth, uint32_t depth_inverted, const float jitter[2]) {
    if (!pass || !raw_context || !raw_scene || !raw_output || !raw_depth || !jitter ||
        !std::isfinite(jitter[0]) || !std::isfinite(jitter[1])) return 0;
    auto* c=static_cast<ID3D11DeviceContext*>(raw_context);
    if (c->GetType()!=D3D11_DEVICE_CONTEXT_IMMEDIATE) return 0;
    ComPtr<ID3D11Device> owner; c->GetDevice(&owner); if (owner.Get()!=pass->device.Get()) return 0;
    auto* scene=static_cast<ID3D11Texture2D*>(raw_scene); auto* output=static_cast<ID3D11Texture2D*>(raw_output);
    auto* depth=static_cast<ID3D11Texture2D*>(raw_depth);
    if (scene==output) return 0;
    scene->GetDevice(&owner); if (owner.Get()!=pass->device.Get()) return 0;
    output->GetDevice(&owner); if (owner.Get()!=pass->device.Get()) return 0;
    depth->GetDevice(&owner); if (owner.Get()!=pass->device.Get()) return 0;
    D3D11_TEXTURE2D_DESC low{},high{}; scene->GetDesc(&low); output->GetDesc(&high);
    D3D11_TEXTURE2D_DESC depth_desc{}; depth->GetDesc(&depth_desc);
    if (depth_desc.Width != low.Width || depth_desc.Height != low.Height ||
        depth_desc.SampleDesc.Count != 1 || depth_desc.ArraySize != 1) return 0;
    if (!low.Width || !low.Height || low.SampleDesc.Count!=1 || high.SampleDesc.Count!=1 ||
        low.ArraySize!=1 || high.ArraySize!=1 || high.Width<low.Width || high.Height<low.Height ||
        high.Width>low.Width*8u || high.Height>low.Height*8u ||
        (high.Format!=DXGI_FORMAT_R16G16B16A16_FLOAT && high.Format!=DXGI_FORMAT_R32G32B32A32_FLOAT)) return 0;
    if (pass->render_width!=low.Width || pass->render_height!=low.Height || pass->width!=high.Width ||
        pass->height!=high.Height || pass->format!=high.Format) if (!allocate(*pass,low,high)) return 0;
    ComPtr<ID3D11ShaderResourceView> scene_view,output_view,depth_view;
    D3D11_SHADER_RESOURCE_VIEW_DESC depth_srv{};
    depth_srv.ViewDimension=D3D11_SRV_DIMENSION_TEXTURE2D; depth_srv.Texture2D.MipLevels=1;
    depth_srv.Format=depth_desc.Format;
    if (depth_srv.Format==DXGI_FORMAT_R32_TYPELESS) depth_srv.Format=DXGI_FORMAT_R32_FLOAT;
    if (depth_srv.Format==DXGI_FORMAT_R24G8_TYPELESS) depth_srv.Format=DXGI_FORMAT_R24_UNORM_X8_TYPELESS;
    if (depth_srv.Format==DXGI_FORMAT_R32G8X24_TYPELESS) depth_srv.Format=DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS;
    if (depth_srv.Format==DXGI_FORMAT_R16_TYPELESS) depth_srv.Format=DXGI_FORMAT_R16_UNORM;
    if (FAILED(pass->device->CreateShaderResourceView(scene,nullptr,&scene_view)) ||
        FAILED(pass->device->CreateShaderResourceView(output,nullptr,&output_view)) ||
        FAILED(pass->device->CreateShaderResourceView(depth,&depth_srv,&depth_view))) return 0;
    rsf_d3d11_state saved{}; if (!rsf_d3d11_state_save(c,&saved)) return 0;
    // Vendor work can leave the output bound in a different stage or UAV slot. Restore the
    // complete engine bindings after dispatch, including UAV counters and CB ranges.
    c->ClearState();
    Constants data{{low.Width,low.Height},{high.Width,high.Height},{jitter[0],jitter[1]}, depth_inverted,0};
    c->UpdateSubresource(pass->constants.Get(),0,nullptr,&data,0,0);
    auto* cb=pass->constants.Get(); auto* sampler=pass->sampler.Get();
    c->CSSetConstantBuffers(0,1,&cb); c->CSSetSamplers(0,1,&sampler);
    ID3D11ShaderResourceView* current=output_view.Get();
    ID3D11ShaderResourceView* no_views[4]{}; ID3D11UnorderedAccessView* no_target=nullptr;
    // Three spatial residual corrections: project current output into jittered source pixels,
    // apply that residual at output size, then feed the corrected image into the next iteration.
    // Two outputs prevent reading and writing one texture during a dispatch; iteration 3 ends in 0.
    for (uint32_t i=0;i<3;++i) {
        c->CSSetUnorderedAccessViews(0,1,&no_target,nullptr);
        c->CSSetShaderResources(0,4,no_views);
        ID3D11ShaderResourceView* inputs[]{scene_view.Get(),current,nullptr,depth_view.Get()};
        auto* target=pass->residual_target.Get(); c->CSSetShader(pass->project.Get(),nullptr,0);
        c->CSSetShaderResources(0,4,inputs); c->CSSetUnorderedAccessViews(0,1,&target,nullptr);
        c->Dispatch((low.Width+7)/8,(low.Height+7)/8,1);
        c->CSSetUnorderedAccessViews(0,1,&no_target,nullptr); c->CSSetShaderResources(0,4,no_views);
        inputs[2]=pass->residual_view.Get(); target=pass->corrected_targets[i%2].Get();
        c->CSSetShader(pass->apply.Get(),nullptr,0); c->CSSetShaderResources(0,4,inputs);
        c->CSSetUnorderedAccessViews(0,1,&target,nullptr);
        c->Dispatch((high.Width+7)/8,(high.Height+7)/8,1);
        current=pass->corrected_views[i%2].Get();
    }
    c->CSSetShaderResources(0,4,no_views); c->CSSetUnorderedAccessViews(0,1,&no_target,nullptr);
    c->CopyResource(output,pass->corrected[0].Get());
    rsf_d3d11_state_restore(c,&saved); return 1;
}
extern "C" void rsf_colour_fidelity_destroy(rsf_colour_fidelity* pass) { delete pass; }
