// SPDX-License-Identifier: GPL-3.0-only
#include "d3d11.h"
#include "rcas.h"
#include "da_rcas.h"
#include "da_das.h"
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/shader_compile.h>
#include <rescaleframe/srv_format.h>
#include "../device_owner.h"
using Microsoft::WRL::ComPtr;
namespace rsf::sharpening {
bool D3D11::prepare(ID3D11Device* device, const D3D11_TEXTURE2D_DESC& output) {
    if (device_.Get() != device) {
        for (auto& shader : shaders_) shader.Reset();
        constants_.Reset(); scratch_.Reset(); target_.Reset(); device_ = device;
    }
    const char* sources[] = {rcas, da_rcas, da_das};
    for (unsigned i=0;i<3;++i)
        if (!shaders_[i] && !rsf::compile_compute(device,sources[i],"Sharpening","CSMain",&shaders_[i])) return false;
    if (!constants_) {
        D3D11_BUFFER_DESC d{}; d.ByteWidth=sizeof(Constants); d.BindFlags=D3D11_BIND_CONSTANT_BUFFER;
        if (FAILED(device->CreateBuffer(&d,nullptr,&constants_))) return false;
    }
    D3D11_TEXTURE2D_DESC old{}; if (scratch_) scratch_->GetDesc(&old);
    if (old.Width != output.Width || old.Height != output.Height || old.Format != output.Format) {
        scratch_.Reset(); target_.Reset();
        auto d=output; d.Usage=D3D11_USAGE_DEFAULT; d.CPUAccessFlags=0; d.MiscFlags=0;
        d.BindFlags=D3D11_BIND_UNORDERED_ACCESS;
        if (FAILED(device->CreateTexture2D(&d,nullptr,&scratch_)) ||
            FAILED(device->CreateUnorderedAccessView(scratch_.Get(),nullptr,&target_))) return false;
    }
    return true;
}
bool D3D11::run(ID3D11DeviceContext* context, ID3D11Texture2D* output, ID3D11Texture2D* motion,
    ID3D11Texture2D* depth, ID3D11Texture2D* exposure, const Options& request, Frame frame) {
    const auto options=sanitize(request);
    if (!active(options)) return true;
    if (!context || !output || context->GetType()!=D3D11_DEVICE_CONTEXT_IMMEDIATE) return false;
    ComPtr<ID3D11Device> device; context->GetDevice(&device);
    if (!rsf::same_device(output,device.Get())) return false;
    D3D11_TEXTURE2D_DESC desc{}; output->GetDesc(&desc);
    if (!desc.Width || !desc.Height || desc.SampleDesc.Count!=1 || desc.ArraySize!=1 || desc.MipLevels!=1 ||
        !(desc.BindFlags&D3D11_BIND_SHADER_RESOURCE) ||
        (desc.Format!=DXGI_FORMAT_R16G16B16A16_FLOAT && desc.Format!=DXGI_FORMAT_R32G32B32A32_FLOAT)) return false;
    frame.width=desc.Width; frame.height=desc.Height;
    frame.motion_width=frame.motion_height=frame.depth_width=frame.depth_height=frame.exposure_texture=0;
    ID3D11Texture2D* textures[]={output,motion,depth,exposure};
    ComPtr<ID3D11ShaderResourceView> views[4];
    for (unsigned i=0;i<4;++i) {
        if (!textures[i]) continue;
        D3D11_TEXTURE2D_DESC d{}; textures[i]->GetDesc(&d);
        if (!rsf::same_device(textures[i],device.Get()) || d.SampleDesc.Count!=1 || d.ArraySize!=1 ||
            !(d.BindFlags&D3D11_BIND_SHADER_RESOURCE)) return false;
        if (i==3 && (d.Width!=1 || d.Height!=1)) return false;
        D3D11_SHADER_RESOURCE_VIEW_DESC v{}; v.Format=rsf::srv_format(d.Format);
        v.ViewDimension=D3D11_SRV_DIMENSION_TEXTURE2D; v.Texture2D.MipLevels=1;
        if (FAILED(device->CreateShaderResourceView(textures[i],&v,&views[i]))) return false;
        if (i==1) { frame.motion_width=d.Width; frame.motion_height=d.Height; }
        if (i==2) { frame.depth_width=d.Width; frame.depth_height=d.Height; }
        if (i==3) frame.exposure_texture=1;
    }
    auto effective=options.mode;
    if (!depth && (effective==Mode::depth_rcas || effective==Mode::depth_das)) effective=Mode::rcas;
    if (!prepare(device.Get(),desc)) return false;
    const auto data=constants(options,frame,effective);
    rsf_d3d11_state state{};
    if (!rsf_d3d11_state_save(context,&state)) return false;
    // Detach inherited output bindings before binding their SRV.
    ID3D11UnorderedAccessView* empty_uavs[D3D11_PS_CS_UAV_REGISTER_COUNT]{};
    context->OMSetRenderTargetsAndUnorderedAccessViews(0,nullptr,nullptr,0,D3D11_PS_CS_UAV_REGISTER_COUNT,empty_uavs,nullptr);
    context->CSSetUnorderedAccessViews(0,D3D11_PS_CS_UAV_REGISTER_COUNT,empty_uavs,nullptr);
    context->UpdateSubresource(constants_.Get(),0,nullptr,&data,0,0);
    ID3D11ShaderResourceView* inputs[]={views[0].Get(),views[1].Get(),views[2].Get(),views[3].Get()};
    ID3D11UnorderedAccessView* target=target_.Get(); ID3D11Buffer* buffer=constants_.Get();
    context->CSSetShader(shaders_[static_cast<unsigned>(effective)-1].Get(),nullptr,0);
    context->CSSetConstantBuffers(0,1,&buffer); context->CSSetShaderResources(0,4,inputs);
    context->CSSetUnorderedAccessViews(0,1,&target,nullptr);
    context->Dispatch((desc.Width+15)/16,(desc.Height+15)/16,1); ++dispatches_;
    ID3D11ShaderResourceView* empty[4]{};
    context->CSSetShaderResources(0,4,empty); context->CSSetUnorderedAccessViews(0,1,empty_uavs,nullptr);
    context->CopyResource(output,scratch_.Get());
    rsf_d3d11_state_restore(context,&state);
    return true;
}
}
