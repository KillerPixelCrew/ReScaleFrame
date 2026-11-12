// SPDX-License-Identifier: GPL-3.0-only
#include "../runtime/graphics/src/sharpening/d3d11.h"
#include <d3d11.h>
#include <wrl/client.h>
#include <array>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <limits>
using Microsoft::WRL::ComPtr;
using namespace rsf::sharpening;
namespace {
int failures=0;
void check(bool ok,const char* label) { if (!ok) { std::printf("FAILED: %s\n",label); ++failures; } }
ComPtr<ID3D11Device> device; ComPtr<ID3D11DeviceContext> context;
constexpr unsigned width=33,height=17,count=width*height;
using Image=std::array<float,count*4>;
ComPtr<ID3D11Texture2D> texture(DXGI_FORMAT format,const void* pixels,unsigned stride) {
    D3D11_TEXTURE2D_DESC d{}; d.Width=width; d.Height=height; d.Format=format;
    d.MipLevels=d.ArraySize=d.SampleDesc.Count=1; d.BindFlags=D3D11_BIND_SHADER_RESOURCE|D3D11_BIND_UNORDERED_ACCESS;
    D3D11_SUBRESOURCE_DATA initial{pixels,width*stride,0}; ComPtr<ID3D11Texture2D> result;
    check(SUCCEEDED(device->CreateTexture2D(&d,&initial,&result)),"texture"); return result;
}
Image read(ID3D11Texture2D* input) {
    Image result{}; D3D11_TEXTURE2D_DESC d{}; input->GetDesc(&d);
    d.Usage=D3D11_USAGE_STAGING; d.BindFlags=0; d.CPUAccessFlags=D3D11_CPU_ACCESS_READ;
    ComPtr<ID3D11Texture2D> staging;
    if (FAILED(device->CreateTexture2D(&d,nullptr,&staging))) { check(false,"staging"); return result; }
    context->CopyResource(staging.Get(),input); D3D11_MAPPED_SUBRESOURCE m{};
    if (FAILED(context->Map(staging.Get(),0,D3D11_MAP_READ,0,&m))) { check(false,"map"); return result; }
    for (unsigned y=0;y<height;++y) std::memcpy(result.data()+y*width*4,static_cast<const char*>(m.pData)+y*m.RowPitch,width*16);
    context->Unmap(staging.Get(),0); return result;
}
}
int main() {
    Options o;
    check(o.mode==Mode::off && o.sharpness==0.4f && !o.mas && o.motion_sharpness==0.2f &&
        o.motion_threshold==0 && o.motion_scale_limit==10 && o.depth_scale==35 && o.depth_bias==0.001f &&
        !o.contrast_enabled && o.contrast==0 && !o.clamp_output && !o.debug,"defaults");
    o.mas=1; o.motion_threshold=2; o.motion_scale_limit=10;
    check(adaptive(0.4f,2,o)==0.4f && std::fabs(adaptive(0.4f,6,o)-0.5f)<1e-6f &&
        std::fabs(adaptive(0.4f,100,o)-0.6f)<1e-6f,"MAS ramp");
    o.motion_sharpness=-0.2f; check(std::fabs(adaptive(0.4f,100,o)-0.2f)<1e-6f,"negative MAS");
    o.motion_scale_limit=1; check(adaptive(0.4f,100,o)==0.4f,"degenerate MAS ramp");
    o.sharpness=std::numeric_limits<float>::quiet_NaN(); o.contrast=-1; o.depth_bias=2;
    auto sanitized=sanitize(o); check(sanitized.sharpness==0.4f && sanitized.contrast==0 && sanitized.depth_bias==1,"sanitize");
    o=Options{};
    Frame f{width,height,11,7,19,9}; f.depth_inverted=1;
    for (Mode mode:{Mode::off,Mode::rcas,Mode::depth_rcas,Mode::depth_das,Mode::vendor}) {
        const auto c=constants(o,f,mode);
        check(c.sharpness==((mode==Mode::depth_rcas || mode==Mode::depth_das)?0.8f:0.4f),"sharpness mapping");
        check(c.motion_scale[0]==11.0f/width && c.motion_scale[1]==7.0f/height &&
            c.depth_texture_scale[0]==19.0f/width && c.depth_texture_scale[1]==9.0f/height &&
            !c.known_planes && c.depth_inverted==1,"frame mapping");
    }
    f.near_plane=1; f.far_plane=100;
    auto c=constants(o,f,Mode::rcas); check(c.known_planes && c.depth_a==100 && c.depth_b==100 && c.depth_c==99,"planes");
    Options all;
    all.sharpness=0.7f; all.mas=1; all.motion_sharpness=-0.3f; all.motion_threshold=2; all.motion_scale_limit=11;
    all.contrast_enabled=1; all.contrast=1.2f; all.depth_scale=45; all.depth_bias=0.002f;
    all.clamp_output=1; all.debug=1;
    f.exposure_texture=1; f.exposure=2; f.motion_x=3; f.motion_y=4;
    for (Mode mode:{Mode::rcas,Mode::depth_rcas,Mode::depth_das}) {
        auto mapped=constants(all,f,mode);
        check(mapped.sharpness==(mode==Mode::rcas?0.7f:1.4f) && mapped.contrast==1.2f &&
            mapped.mas==1 && mapped.debug==(mode==Mode::rcas?1u:3u) && mapped.motion_sharpness==-0.3f &&
            mapped.threshold==2 && mapped.limit==11 && mapped.depth_scale==45 && mapped.depth_bias==0.002f &&
            mapped.clamp_output==(mode==Mode::rcas?0u:1u) && mapped.hdr==1 && mapped.exposure_texture==1 &&
            mapped.exposure==2 && mapped.motion_x==3 && mapped.motion_y==4,"all options map to shader constants");
    }
    // The following device fixture uses the default exposure and motion convention.
    f.exposure_texture=0; f.exposure=1; f.motion_x=f.motion_y=1;
    D3D11 pass;
    check(pass.run(nullptr,nullptr,nullptr,nullptr,nullptr,o,f) && !pass.dispatches(),"Off no GPU work");
    o.mode=Mode::rcas; o.sharpness=0;
    check(pass.run(nullptr,nullptr,nullptr,nullptr,nullptr,o,f) && !pass.dispatches(),"zero no GPU work");
    if (FAILED(D3D11CreateDevice(nullptr,D3D_DRIVER_TYPE_WARP,nullptr,0,nullptr,0,D3D11_SDK_VERSION,&device,nullptr,&context))) return 77;
    Image image{};
    for (unsigned i=0;i<count;++i) { image[i*4]=image[i*4+1]=image[i*4+2]=10; image[i*4+3]=float(i)/count; }
    std::array<float,count> depth{}; depth.fill(0.5f);
    auto z=texture(DXGI_FORMAT_R32_FLOAT,depth.data(),4);
    for (Mode mode:{Mode::rcas,Mode::depth_rcas,Mode::depth_das}) {
        o=Options{}; o.mode=mode; o.sharpness=1;
        auto output=texture(DXGI_FORMAT_R32G32B32A32_FLOAT,image.data(),16);
        check(pass.run(context.Get(),output.Get(),nullptr,z.Get(),nullptr,o,f),"flat dispatch");
        auto actual=read(output.Get()); check(actual==image,"HDR flat bit exact with alpha");
        for (unsigned y=0;y<height;++y) for (unsigned x=0;x<width;++x) {
            // Ramp around two edges leaves room for sharpening without new extrema.
            const float value=x<width/2?10.0f+float(x)*0.2f:80.0f+float(x)*0.2f;
            for (unsigned ch=0;ch<3;++ch) image[(y*width+x)*4+ch]=value;
        }
        output=texture(DXGI_FORMAT_R32G32B32A32_FLOAT,image.data(),16);
        check(pass.run(context.Get(),output.Get(),nullptr,z.Get(),nullptr,o,f),"HDR edge dispatch");
        actual=read(output.Get());
        bool range=true,alpha=true,changed=false;
        for (unsigned i=0;i<count;++i) {
            alpha &= actual[i*4+3]==image[i*4+3];
            for (unsigned ch=0;ch<3;++ch) {
                range &= std::isfinite(actual[i*4+ch]) && actual[i*4+ch]>=10-1e-4f && actual[i*4+ch]<=87+1e-4f;
                changed |= std::fabs(actual[i*4+ch]-image[i*4+ch])>1e-3f;
            }
        }
        check(range && alpha && changed,"HDR edge bounded, alpha retained, nonidentity");
        // Missing depth must select RCAS and still produce a frame.
        check(pass.run(context.Get(),output.Get(),nullptr,nullptr,nullptr,o,f),"missing depth fallback");
        for (unsigned i=0;i<count;++i) image[i*4]=image[i*4+1]=image[i*4+2]=10;
    }
    // A flat SDR value at one exposed an upstream zero denominator in hitMax.
    // Verify it stays finite and constant after the sign-correct epsilon guard.
    for (unsigned i=0;i<count;++i) image[i*4]=image[i*4+1]=image[i*4+2]=1;
    f.hdr=0;
    for (Mode mode:{Mode::rcas,Mode::depth_rcas,Mode::depth_das}) {
        o=Options{}; o.mode=mode;
        auto output=texture(DXGI_FORMAT_R32G32B32A32_FLOAT,image.data(),16);
        check(pass.run(context.Get(),output.Get(),nullptr,z.Get(),nullptr,o,f),"SDR white dispatch");
        auto actual=read(output.Get()); check(actual==image,"SDR white finite and flat");
    }
    o=Options{};
    auto untouched=texture(DXGI_FORMAT_R32G32B32A32_FLOAT,image.data(),16);
    const auto before=pass.dispatches();
    check(pass.run(context.Get(),untouched.Get(),nullptr,nullptr,nullptr,o,f),"Off valid texture");
    auto unchanged=read(untouched.Get());
    check(!std::memcmp(image.data(),unchanged.data(),sizeof(image)) && before==pass.dispatches(),"Off bit exact, dispatch count unchanged");
    std::printf("sharpening: %d failures, %llu dispatches\n",failures,static_cast<unsigned long long>(pass.dispatches()));
    return failures?1:0;
}
