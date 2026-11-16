// SPDX-License-Identifier: MIT
#include "native_regions.h"
#include <rescaleframe/shader_compile.h>
#include <rescaleframe/srv_format.h>
#include <windows.h>
#include <wrl/client.h>
#include <array>
#include <cstring>
#include <cmath>
#include <algorithm>
#include <limits>
namespace {
using Microsoft::WRL::ComPtr;
constexpr char depth_shader[] = R"(
Texture2D<float> Source : register(t0);
RWTexture2D<float> Target : register(u0);
struct Domain { float4 Range; float4 Viewport; row_major float4x4 Transform; uint4 Flags; };
cbuffer Region : register(b0) {
    uint2 Origin; uint2 Size;
    uint Count; uint ClearEnabled; float ClearValue; float FarClear;
    Domain Domains[2];
};
[numthreads(8,8,1)] void main(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= Size)) return;
    float stored = Source.Load(int3(Origin + id.xy, 0));
    if (Count == 0) { Target[id.xy] = stored; return; }
    if (ClearEnabled != 0 && stored == ClearValue) { Target[id.xy] = FarClear; return; }
    // CPU validation proves exhaustive, unambiguous UNORM coverage and finite positive W.
    uint selected = 0;
    if (Count == 2) {
        bool upper = stored < Domains[0].Range.y ||
            (stored == Domains[0].Range.y && (Domains[0].Flags.x & 2) != 0);
        selected = upper ? 0 : 1;
    }
    Domain d = Domains[selected];
    float2 xy = (float2(Origin + id.xy) + 0.5 - d.Viewport.xy) * d.Viewport.zw;
    float z = (stored - d.Range.z) * d.Range.w;
    float4 clip = mul(float4(xy * float2(2,-2) + float2(-1,1), z, 1), d.Transform);
    // Depth-only clipping at the existing composed sample, not spatial reprojection.
    Target[id.xy] = saturate(clip.z / clip.w);
})";
struct DepthDomainConstants {
    float range[4], viewport[4], transform[16];
    uint32_t flags[4];
};
struct RegionConstants {
    uint32_t origin[2], size[2];
    uint32_t count{}, clear_enabled{};
    float clear_value{}, far_clear{};
    DepthDomainConstants domains[2]{};
};
static_assert(sizeof(DepthDomainConstants) == 112 && sizeof(RegionConstants) == 256);
static_assert(sizeof(rsf_game_depth_domain) == 104 && sizeof(rsf_game_depth_domains) == 240);

bool nonsingular(const float source[16]) {
    double a[4][4]{};
    for (size_t i=0;i<4;++i) for (size_t j=0;j<4;++j) a[i][j]=source[i*4+j];
    for (size_t col=0;col<4;++col) {
        size_t pivot=col;
        for (size_t row=col+1;row<4;++row) if (std::abs(a[row][col])>std::abs(a[pivot][col])) pivot=row;
        double scale=0;
        for (size_t j=col;j<4;++j) scale=std::max(scale,std::abs(a[pivot][j]));
        if (!std::isfinite(scale) || scale==0 || std::abs(a[pivot][col])<=scale*1e-12) return false;
        for (size_t j=col;j<4;++j) std::swap(a[pivot][j],a[col][j]);
        for (size_t row=col+1;row<4;++row) {
            const double factor=a[row][col]/a[col][col];
            for (size_t j=col+1;j<4;++j) a[row][j]-=factor*a[col][j];
        }
    }
    return true;
}
bool depth_constants(const rsf_game_depth_domains* facts, DXGI_FORMAT format, const int32_t rect[4],
    bool inverted, RegionConstants& out) {
    if (!facts || !rsf_game_depth_domains_requested(facts)) return true;
    if (facts->struct_size!=sizeof(*facts) || facts->version!=RSF_DEPTH_DOMAINS_VERSION ||
        !facts->domain_count || facts->domain_count>2 || facts->clear_enabled>1 ||
        facts->reserved[0] || facts->reserved[1] || facts->reserved[2] ||
        !std::isfinite(facts->clear_value) || facts->clear_value<0 || facts->clear_value>1 ||
        (!facts->clear_enabled && facts->clear_value!=0)) return false;
    const auto view=rsf::srv_format(format);
    const double quantum=view==DXGI_FORMAT_R16_UNORM ? 1.0/65535 :
        view==DXGI_FORMAT_R24_UNORM_X8_TYPELESS ? 1.0/16777215 : 0;
    if (!quantum) return false; // Float depth has no finite-sample guarantee.
    out.count=facts->domain_count; out.clear_enabled=facts->clear_enabled;
    out.clear_value=facts->clear_value; out.far_clear=inverted ? 0.f : 1.f;
    const auto clear=[&](float value) { return facts->clear_enabled && facts->clear_value==value; };
    for (uint32_t i=0;i<2;++i) {
        const auto& d=facts->domains[i];
        if (i>=facts->domain_count) {
            const rsf_game_depth_domain zero{};
            if (std::memcmp(&d,&zero,sizeof(d))) return false;
            continue;
        }
        if (!std::isfinite(d.classify_min) || !std::isfinite(d.classify_max) ||
            d.classify_min<0 || d.classify_max>1 || d.classify_min>=d.classify_max ||
            d.reserved || (d.endpoint_flags & ~3u) ||
            !std::isfinite(d.viewport_min_depth) || !std::isfinite(d.viewport_max_depth) ||
            d.viewport_min_depth<0 || d.viewport_max_depth>1 || d.viewport_min_depth>=d.viewport_max_depth ||
            double(d.classify_min)<double(d.viewport_min_depth)-quantum ||
            double(d.classify_max)>double(d.viewport_max_depth)+quantum) return false;
        if (!i && (d.classify_min!=0 || (!(d.endpoint_flags&1) && !clear(0)))) return false;
        if (i) {
            const auto& previous=facts->domains[i-1];
            const bool left=(previous.endpoint_flags&2)!=0, right=(d.endpoint_flags&1)!=0;
            if (previous.classify_max!=d.classify_min || (left && right) ||
                (!left && !right && !clear(d.classify_min))) return false;
        }
        if (i+1==facts->domain_count && (d.classify_max!=1 || (!(d.endpoint_flags&2) && !clear(1)))) return false;
        for (float f:d.viewport_xywh) if (!std::isfinite(f) || std::abs(f)>1e8f) return false;
        const auto* v=d.viewport_xywh;
        // Rasterized texels are sampled at their centers. Fractional native
        // viewports need not cover the outer texel edges; right/bottom are exclusive.
        if (double(v[0])>double(rect[0])+.5 || double(v[1])>double(rect[1])+.5 || v[2]<=0 || v[3]<=0 ||
            double(v[0])+v[2]<=double(rect[2])-.5 || double(v[1])+v[3]<=double(rect[3])-.5) return false;
        auto& c=out.domains[i];
        c.range[0]=d.classify_min; c.range[1]=d.classify_max;
        c.range[2]=d.viewport_min_depth; c.range[3]=1.f/(d.viewport_max_depth-d.viewport_min_depth);
        c.viewport[0]=v[0]; c.viewport[1]=v[1]; c.viewport[2]=1.f/v[2]; c.viewport[3]=1.f/v[3];
        for (float f:c.range) if (!std::isfinite(f) || std::abs(f)>1e20f) return false;
        for (float f:c.viewport) if (!std::isfinite(f) || std::abs(f)>1e20f) return false;
        for (float f:d.source_clip_to_target_clip) if (!std::isfinite(f) || std::abs(f)>1e20f) return false;
        if (!nonsingular(d.source_clip_to_target_clip)) return false;
        std::memcpy(c.transform,d.source_clip_to_target_clip,sizeof(c.transform)); c.flags[0]=d.endpoint_flags;
        // Affine homogeneous components attain extrema at box corners. Enlarge by an
        // absolute float32 roundoff bound for viewport unmapping and the four-term dot.
        double bounds[3][2]{};
        for (size_t axis=0;axis<2;++axis) {
            const double origin=v[axis], inverse=c.viewport[axis+2];
            bounds[axis][0]=(double(rect[axis])+.5-origin)*inverse*2-1;
            bounds[axis][1]=(double(rect[axis+2])-.5-origin)*inverse*2-1;
            if (axis==1) { bounds[axis][0]=-bounds[axis][0]; bounds[axis][1]=-bounds[axis][1]; }
        }
        bounds[2][0]=(double(d.classify_min)-c.range[2])*c.range[3];
        bounds[2][1]=(double(d.classify_max)-c.range[2])*c.range[3];
        const double epsilon=std::numeric_limits<float>::epsilon();
        for (size_t axis=0;axis<3;++axis) {
            const double input_error=axis<2 ? 16*epsilon*((double(rect[axis+2])+1+std::abs(double(v[axis])))*
                c.viewport[axis+2]*2+1) : 16*epsilon*(std::abs(double(d.classify_max))+
                std::abs(double(c.range[2])))*c.range[3];
            const double lower=std::min(bounds[axis][0],bounds[axis][1])-input_error;
            const double upper=std::max(bounds[axis][0],bounds[axis][1])+input_error;
            bounds[axis][0]=lower; bounds[axis][1]=upper;
        }
        double magnitude[4]{};
        for (size_t col=0;col<4;++col) {
            magnitude[col]=std::abs(double(c.transform[12+col]));
            for (size_t row=0;row<3;++row) magnitude[col]+=std::max(std::abs(bounds[row][0]),std::abs(bounds[row][1]))*
                std::abs(double(c.transform[row*4+col]));
            if (!std::isfinite(magnitude[col]) || magnitude[col]>1e20) return false;
        }
        double minimum_w=std::numeric_limits<double>::infinity();
        for (unsigned corner=0;corner<8;++corner) {
            double w=c.transform[15];
            for (size_t axis=0;axis<3;++axis) w+=bounds[axis][(corner>>axis)&1]*c.transform[axis*4+3];
            minimum_w=std::min(minimum_w,w);
        }
        const double error=64*epsilon*magnitude[3];
        if (minimum_w-error<=1e-6 || magnitude[2]/(minimum_w-error)>1e30) return false;
    }
    return true;
}
constexpr char color_shader[] = R"(
Texture2D<float3> Source : register(t0);
RWTexture2D<float4> Target : register(u0);
cbuffer Region : register(b0) { uint2 Origin; uint2 Size; };
[numthreads(8,8,1)] void main(uint3 id : SV_DispatchThreadID) {
    if (all(id.xy < Size)) Target[id.xy] = float4(Source.Load(int3(Origin + id.xy, 0)), 1);
})";
constexpr char temporal_mask_shader[] = R"(
Texture2D<float4> Source : register(t0);
Texture2D<float> Reactive : register(t1);
Texture2D<float> Bias : register(t2);
RWTexture2D<float> OutReactive : register(u0);
RWTexture2D<float> OutBias : register(u1);
cbuffer Region : register(b0) { uint2 Origin; uint2 Size; float4 Weights; };
[numthreads(8,8,1)] void main(uint3 id : SV_DispatchThreadID) {
    if (all(id.xy < Size)) {
        float hint = dot(Source.Load(int3(Origin + id.xy, 0)), Weights);
        hint = isfinite(hint) ? saturate(hint) : 1;
        OutReactive[id.xy] = max(hint, Reactive.Load(int3(id.xy, 0)));
        OutBias[id.xy] = max(hint, Bias.Load(int3(id.xy, 0)));
    }
})";
struct Resources {
    ComPtr<ID3D11Device> device;
    std::array<ComPtr<ID3D11Texture2D>,3> textures;
    ComPtr<ID3D11ComputeShader> shader;
    ComPtr<ID3D11ComputeShader> color_shader;
    ComPtr<ID3D11Buffer> constants;
    ComPtr<ID3D11UnorderedAccessView> depth_uav;
    ComPtr<ID3D11Texture2D> readable_depth;
    ComPtr<ID3D11UnorderedAccessView> color_uav;
    uint32_t width = 0, height = 0;
    DXGI_FORMAT color = DXGI_FORMAT_UNKNOWN, motion = DXGI_FORMAT_UNKNOWN;
    // Views of the caller's depth and colour, kept while the same texture comes back. Holding the
    // texture keeps its address from being taken by another one while the view is cached.
    struct Source {
        ComPtr<ID3D11Texture2D> texture;
        ComPtr<ID3D11ShaderResourceView> view;
    } depth_source, color_source;
    struct TemporalMask {
        ComPtr<ID3D11ComputeShader> shader;
        ComPtr<ID3D11Buffer> constants;
        std::array<ComPtr<ID3D11Texture2D>,2> textures;
        std::array<ComPtr<ID3D11UnorderedAccessView>,2> targets;
        ComPtr<ID3D11Texture2D> readable;
        Source source, reactive, bias;
        uint32_t width = 0, height = 0;
    } temporal;
};
Resources& resources() { static Resources value; return value; }
ID3D11ShaderResourceView* source_view(ID3D11Device* device, Resources::Source& cached, ID3D11Texture2D* texture,
    DXGI_FORMAT format) {
    if (cached.texture.Get() == texture) return cached.view.Get();
    cached = {};
    D3D11_SHADER_RESOURCE_VIEW_DESC view{}; view.Format=rsf::srv_format(format);
    view.ViewDimension=D3D11_SRV_DIMENSION_TEXTURE2D; view.Texture2D.MipLevels=1;
    if (FAILED(device->CreateShaderResourceView(texture,&view,&cached.view))) return nullptr;
    cached.texture=texture; return cached.view.Get();
}
bool build_shader(Resources& r) {
    if (!rsf::compile_compute(r.device.Get(), depth_shader, "NativeDepthRegion", "main", r.shader.ReleaseAndGetAddressOf()) ||
        !rsf::compile_compute(r.device.Get(), color_shader, "NativeColorRegion", "main", r.color_shader.ReleaseAndGetAddressOf())) {
        r.shader.Reset(); r.color_shader.Reset(); return false;
    }
    D3D11_BUFFER_DESC b{}; b.ByteWidth = sizeof(RegionConstants); b.Usage = D3D11_USAGE_DEFAULT; b.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
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
bool rsf_native_regions_depth_valid(const rsf_game_depth_domains* domains, DXGI_FORMAT format, const int32_t rect[4]) {
    if (!rect || rect[0]<0 || rect[1]<0 || rect[2]<=rect[0] || rect[3]<=rect[1]) return false;
    RegionConstants constants{};
    return depth_constants(domains,format,rect,false,constants);
}
bool rsf_native_regions_prepare(ID3D11DeviceContext* context, ID3D11Texture2D* color,
    ID3D11Texture2D* depth, ID3D11Texture2D* motion, const int32_t rect[4],
    ID3D11Texture2D** out_color, ID3D11Texture2D** out_depth, ID3D11Texture2D** out_motion,
    const rsf_game_depth_domains* domains, bool inverted) {
    if (!context || !color || !depth || !motion || !rect || !out_color || !out_depth || !out_motion ||
        rect[0]<0 || rect[1]<0 || rect[2]<=rect[0] || rect[3]<=rect[1]) return false;
    ComPtr<ID3D11Device> device; context->GetDevice(&device);
    if (!device || context->GetType()!=D3D11_DEVICE_CONTEXT_IMMEDIATE) return false;
    ID3D11Texture2D* input[]{color,depth,motion}; D3D11_TEXTURE2D_DESC descriptions[3]{};
    ComPtr<IUnknown> device_identity; if (FAILED(device.As(&device_identity))) return false;
    for (auto* texture:input) {
        ComPtr<ID3D11Device> owner; texture->GetDevice(&owner);
        ComPtr<IUnknown> identity; if (!owner || FAILED(owner.As(&identity)) || identity.Get()!=device_identity.Get()) return false;
    }
    for (size_t i=0;i<3;++i) {
        input[i]->GetDesc(&descriptions[i]); const auto& d=descriptions[i];
        if (d.SampleDesc.Count!=1 || d.ArraySize!=1 || d.MipLevels!=1 || uint32_t(rect[2])>d.Width || uint32_t(rect[3])>d.Height) return false;
    }
    const uint32_t width=uint32_t(rect[2]-rect[0]), height=uint32_t(rect[3]-rect[1]);
    RegionConstants constants{};
    constants.origin[0]=uint32_t(rect[0]); constants.origin[1]=uint32_t(rect[1]);
    constants.size[0]=width; constants.size[1]=height;
    if (!depth_constants(domains,descriptions[1].Format,rect,inverted,constants)) return false;
    auto& r=resources();
    if (r.device.Get()!=device.Get()) { r={}; r.device=device; }
    if (!r.shader && !build_shader(r)) return false;
    if (r.width!=width || r.height!=height || r.color!=descriptions[0].Format || r.motion!=descriptions[2].Format)
        if (!allocate(r,width,height,descriptions[0].Format,descriptions[2].Format)) return false;
    auto* readable_depth=depth;
    const bool copy_depth=!(descriptions[1].BindFlags&D3D11_BIND_SHADER_RESOURCE);
    if (copy_depth) {
        D3D11_TEXTURE2D_DESC previous{}; if (r.readable_depth) r.readable_depth->GetDesc(&previous);
        auto d=descriptions[1]; d.Usage=D3D11_USAGE_DEFAULT; d.CPUAccessFlags=d.MiscFlags=0;
        d.BindFlags=D3D11_BIND_SHADER_RESOURCE;
        if (d.Format==DXGI_FORMAT_D16_UNORM) d.Format=DXGI_FORMAT_R16_TYPELESS;
        if (d.Format==DXGI_FORMAT_D24_UNORM_S8_UINT) d.Format=DXGI_FORMAT_R24G8_TYPELESS;
        if (d.Format==DXGI_FORMAT_D32_FLOAT) d.Format=DXGI_FORMAT_R32_TYPELESS;
        if (!r.readable_depth || previous.Width!=d.Width || previous.Height!=d.Height || previous.Format!=d.Format) {
            r.depth_source={};
            if (FAILED(device->CreateTexture2D(&d,nullptr,r.readable_depth.ReleaseAndGetAddressOf()))) return false;
        }
        readable_depth=r.readable_depth.Get();
    }
    auto* depth_view=source_view(device.Get(),r.depth_source,readable_depth,descriptions[1].Format);
    auto* color_view=source_view(device.Get(),r.color_source,color,descriptions[0].Format);
    if (!depth_view || !color_view) return false;
    context->OMSetRenderTargets(0,nullptr,nullptr);
    if (copy_depth) context->CopyResource(readable_depth,depth);
    const D3D11_BOX box{uint32_t(rect[0]),uint32_t(rect[1]),0,uint32_t(rect[2]),uint32_t(rect[3]),1};
    context->CopySubresourceRegion(r.textures[2].Get(),0,0,0,0,motion,0,&box);
    context->UpdateSubresource(r.constants.Get(),0,nullptr,&constants,0,0);
    auto* srv=depth_view; auto* uav=r.depth_uav.Get(); auto* cb=r.constants.Get();
    context->CSSetShader(r.shader.Get(),nullptr,0); context->CSSetShaderResources(0,1,&srv);
    context->CSSetUnorderedAccessViews(0,1,&uav,nullptr); context->CSSetConstantBuffers(0,1,&cb);
    context->Dispatch((width+7)/8,(height+7)/8,1);
    srv=nullptr; uav=nullptr; context->CSSetShaderResources(0,1,&srv); context->CSSetUnorderedAccessViews(0,1,&uav,nullptr);
    srv=color_view; uav=r.color_uav.Get();
    context->CSSetShader(r.color_shader.Get(),nullptr,0); context->CSSetShaderResources(0,1,&srv);
    context->CSSetUnorderedAccessViews(0,1,&uav,nullptr);
    context->Dispatch((width+7)/8,(height+7)/8,1);
    srv=nullptr; uav=nullptr; context->CSSetShaderResources(0,1,&srv); context->CSSetUnorderedAccessViews(0,1,&uav,nullptr);
    *out_color=r.textures[0].Get(); *out_depth=r.textures[1].Get(); *out_motion=r.textures[2].Get(); return true;
}
bool rsf_native_regions_temporal_mask(ID3D11DeviceContext* context, ID3D11Texture2D* mask,
    const int32_t rect[4], const float weights[4], ID3D11Texture2D* reactive, ID3D11Texture2D* bias,
    ID3D11Texture2D** out_reactive, ID3D11Texture2D** out_bias) {
    if (!context || !mask || !rect || !weights || !out_reactive || !out_bias ||
        rect[0] < 0 || rect[1] < 0 || rect[2] <= rect[0] || rect[3] <= rect[1]) return false;
    for (size_t i = 0; i < 4; ++i) if (!std::isfinite(weights[i])) return false;
    ComPtr<ID3D11Device> device; context->GetDevice(&device);
    // Reject foreign resources before changing cached resources or issuing commands,
    // including the readable copy for masks without a shader-resource binding.
    ID3D11Texture2D* leased[]{mask,reactive,bias};
    for (auto* input : leased) if (input) {
        ComPtr<ID3D11Device> owner; input->GetDevice(&owner);
        if (owner.Get() != device.Get()) return false;
    }
    D3D11_TEXTURE2D_DESC source{}; mask->GetDesc(&source);
    if (source.SampleDesc.Count != 1 || source.ArraySize != 1 || uint32_t(rect[2]) > source.Width ||
        uint32_t(rect[3]) > source.Height) return false;
    // Numeric masks only. Integer and sRGB views have different shader/channel semantics.
    switch (rsf::srv_format(source.Format)) {
    case DXGI_FORMAT_R32_FLOAT: case DXGI_FORMAT_R32G32_FLOAT: case DXGI_FORMAT_R32G32B32A32_FLOAT:
    case DXGI_FORMAT_R16_FLOAT: case DXGI_FORMAT_R16G16_FLOAT: case DXGI_FORMAT_R16G16B16A16_FLOAT:
    case DXGI_FORMAT_R16_UNORM: case DXGI_FORMAT_R16G16_UNORM: case DXGI_FORMAT_R16G16B16A16_UNORM:
    case DXGI_FORMAT_R8_UNORM: case DXGI_FORMAT_R8G8_UNORM: case DXGI_FORMAT_R8G8B8A8_UNORM:
    case DXGI_FORMAT_R16_SNORM: case DXGI_FORMAT_R16G16_SNORM: case DXGI_FORMAT_R16G16B16A16_SNORM:
    case DXGI_FORMAT_R8_SNORM: case DXGI_FORMAT_R8G8_SNORM: case DXGI_FORMAT_R8G8B8A8_SNORM:
    case DXGI_FORMAT_R10G10B10A2_UNORM: case DXGI_FORMAT_B8G8R8A8_UNORM: break;
    default: return false;
    }
    const auto width = uint32_t(rect[2] - rect[0]), height = uint32_t(rect[3] - rect[1]);
    ID3D11Texture2D* inputs[]{reactive,bias};
    for (auto* input : inputs) if (input) {
        D3D11_TEXTURE2D_DESC d{}; input->GetDesc(&d);
        if (d.Width != width || d.Height != height || d.SampleDesc.Count != 1 || d.ArraySize != 1 ||
            d.Format != DXGI_FORMAT_R32_FLOAT) return false;
    }
    auto& r = resources();
    if (r.device.Get() != device.Get()) { r = {}; r.device = device; }
    auto& output = r.temporal;
    if (!output.shader) {
        if (!rsf::compile_compute(device.Get(), temporal_mask_shader, "NativeTemporalMaskRegion", "main",
                output.shader.ReleaseAndGetAddressOf())) return false;
        D3D11_BUFFER_DESC d{}; d.ByteWidth = 32; d.Usage = D3D11_USAGE_DEFAULT; d.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
        if (FAILED(device->CreateBuffer(&d, nullptr, &output.constants))) { output = {}; return false; }
    }
    if (output.width != width || output.height != height) {
        std::array<ComPtr<ID3D11Texture2D>,2> textures;
        std::array<ComPtr<ID3D11UnorderedAccessView>,2> targets;
        D3D11_TEXTURE2D_DESC d{}; d.Width = width; d.Height = height; d.MipLevels = d.ArraySize = d.SampleDesc.Count = 1;
        d.Format = DXGI_FORMAT_R32_FLOAT; d.BindFlags = D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS;
        for (size_t i = 0; i < textures.size(); ++i)
            if (FAILED(device->CreateTexture2D(&d, nullptr, &textures[i])) ||
                FAILED(device->CreateUnorderedAccessView(textures[i].Get(), nullptr, &targets[i]))) return false;
        output.textures = std::move(textures); output.targets = std::move(targets);
        output.width = width; output.height = height;
    }
    // Some native mask allocations are output-only. Copy their current contents
    // into a readable allocation without changing the adapter's resource contract.
    context->OMSetRenderTargets(0,nullptr,nullptr);
    auto* readable = mask;
    if (!(source.BindFlags & D3D11_BIND_SHADER_RESOURCE)) {
        D3D11_TEXTURE2D_DESC cached{};
        if (output.readable) output.readable->GetDesc(&cached);
        if (!output.readable || cached.Width != source.Width || cached.Height != source.Height ||
            cached.Format != source.Format || cached.MipLevels != source.MipLevels) {
            auto copy = source; copy.Usage = D3D11_USAGE_DEFAULT; copy.CPUAccessFlags = copy.MiscFlags = 0;
            copy.BindFlags = D3D11_BIND_SHADER_RESOURCE;
            output.source = {};
            if (FAILED(device->CreateTexture2D(&copy,nullptr,output.readable.ReleaseAndGetAddressOf()))) return false;
        }
        context->CopyResource(output.readable.Get(),mask);
        readable = output.readable.Get();
    }
    ID3D11ShaderResourceView* views[]{source_view(device.Get(),output.source,readable,source.Format),nullptr,nullptr};
    if (!views[0]) return false;
    Resources::Source* cached[]{&output.reactive,&output.bias};
    for (size_t i = 0; i < 2; ++i) {
        if (inputs[i]) {
            views[i+1] = source_view(device.Get(),*cached[i],inputs[i],DXGI_FORMAT_R32_FLOAT);
            if (!views[i+1]) return false;
        } else *cached[i] = {};
    }
    struct Constants { uint32_t origin[2], size[2]; float weights[4]; } constants{
        {uint32_t(rect[0]),uint32_t(rect[1])},{width,height},{weights[0],weights[1],weights[2],weights[3]}};
    context->OMSetRenderTargets(0,nullptr,nullptr);
    context->UpdateSubresource(output.constants.Get(),0,nullptr,&constants,0,0);
    auto* cb = output.constants.Get();
    ID3D11UnorderedAccessView* targets[]{output.targets[0].Get(),output.targets[1].Get()};
    context->CSSetShader(output.shader.Get(),nullptr,0); context->CSSetShaderResources(0,3,views);
    context->CSSetUnorderedAccessViews(0,2,targets,nullptr); context->CSSetConstantBuffers(0,1,&cb);
    context->Dispatch((width+7)/8,(height+7)/8,1);
    for (auto& view : views) view = nullptr;
    for (auto& target : targets) target = nullptr;
    context->CSSetShaderResources(0,3,views); context->CSSetUnorderedAccessViews(0,2,targets,nullptr);
    *out_reactive = output.textures[0].Get(); *out_bias = output.textures[1].Get();
    return true;
}
void rsf_native_regions_release() { resources()={}; }
