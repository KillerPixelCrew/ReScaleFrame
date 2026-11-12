// SPDX-License-Identifier: GPL-3.0-only
#include "d3d12.h"
#include "rcas.h"
#include "da_rcas.h"
#include "da_das.h"
#include <rescaleframe/shader_compile.h>
#include <rescaleframe/srv_format.h>
#include "../../../backends/common/d3d12_helpers.h"
#include <vector>
#include <limits>
using Microsoft::WRL::ComPtr;
namespace rsf::sharpening {
bool D3D12::prepare(ID3D12Device* device,const D3D12_RESOURCE_DESC& output,uint32_t slot) {
    if (device_ && device_.Get()!=device) return false;
    device_=device;
    if (!root_) {
        D3D12_DESCRIPTOR_RANGE ranges[]={{D3D12_DESCRIPTOR_RANGE_TYPE_SRV,4,0,0,0},
            {D3D12_DESCRIPTOR_RANGE_TYPE_UAV,1,0,0,4}};
        D3D12_ROOT_PARAMETER parameters[2]{};
        parameters[0].ParameterType=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
        parameters[0].DescriptorTable={2,ranges};
        parameters[1].ParameterType=D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;
        parameters[1].Constants={0,0,sizeof(Constants)/4};
        D3D12_ROOT_SIGNATURE_DESC description{2,parameters,0,nullptr,D3D12_ROOT_SIGNATURE_FLAG_NONE};
        ComPtr<ID3DBlob> serialized,errors;
        if (FAILED(D3D12SerializeRootSignature(&description,D3D_ROOT_SIGNATURE_VERSION_1,&serialized,&errors)) ||
            FAILED(device->CreateRootSignature(0,serialized->GetBufferPointer(),serialized->GetBufferSize(),IID_PPV_ARGS(&root_)))) return false;
    }
    const char* sources[]={rcas,da_rcas,da_das};
    for (unsigned i=0;i<3;++i) if (!pipelines_[i]) {
        std::vector<uint8_t> code;
        if (!rsf::compile_shader(sources[i],"Sharpening12","CSMain","cs_5_0",code)) return false;
        D3D12_COMPUTE_PIPELINE_STATE_DESC desc{}; desc.pRootSignature=root_.Get(); desc.CS={code.data(),code.size()};
        if (FAILED(device->CreateComputePipelineState(&desc,IID_PPV_ARGS(&pipelines_[i])))) return false;
    }
    if (!heaps_[slot]) {
        D3D12_DESCRIPTOR_HEAP_DESC desc{}; desc.Type=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
        desc.NumDescriptors=5; desc.Flags=D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
        if (FAILED(device->CreateDescriptorHeap(&desc,IID_PPV_ARGS(&heaps_[slot])))) return false;
    }
    auto& scratch=scratch_[slot];
    const auto old=scratch?scratch->GetDesc():D3D12_RESOURCE_DESC{};
    if (old.Width!=output.Width || old.Height!=output.Height || old.Format!=output.Format) {
        ComPtr<ID3D12Resource> next;
        if (!rsf::create_uav_texture(device,static_cast<uint32_t>(output.Width),output.Height,output.Format,&next)) return false;
        scratch=std::move(next); // Only this retired slot's old allocation is released.
    }
    return true;
}
bool D3D12::run(ID3D12GraphicsCommandList* list,ID3D12Resource* output,ID3D12Resource* motion,
    ID3D12Resource* depth,ID3D12Resource* exposure,const Options& request,Frame frame,uint32_t slot) {
    const auto options=sanitize(request);
    if (!active(options)) return true;
    if (!list || !output || slot>=3) return false;
    ComPtr<ID3D12Device> device;
    if (FAILED(list->GetDevice(IID_PPV_ARGS(&device)))) return false;
    ID3D12Resource* inputs[]={output,motion,depth,exposure};
    D3D12_RESOURCE_DESC descriptions[4]{};
    for (unsigned i=0;i<4;++i) {
        if (!inputs[i]) continue;
        ComPtr<ID3D12Device> owner;
        if (FAILED(inputs[i]->GetDevice(IID_PPV_ARGS(&owner))) || owner.Get()!=device.Get()) return false;
        auto& desc=descriptions[i]; desc=inputs[i]->GetDesc();
        if (desc.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || desc.DepthOrArraySize!=1 || desc.MipLevels!=1 ||
            desc.SampleDesc.Count!=1 || desc.Width>UINT32_MAX || !desc.Width || !desc.Height ||
            (desc.Flags&D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE)) return false;
    }
    const auto& desc=descriptions[0];
    if ((desc.Format!=DXGI_FORMAT_R16G16B16A16_FLOAT && desc.Format!=DXGI_FORMAT_R32G32B32A32_FLOAT) ||
        (exposure && (descriptions[3].Width!=1 || descriptions[3].Height!=1))) return false;
    if (!prepare(device.Get(),desc,slot)) return false;
    frame.width=static_cast<uint32_t>(desc.Width); frame.height=desc.Height;
    frame.motion_width=static_cast<uint32_t>(descriptions[1].Width); frame.motion_height=descriptions[1].Height;
    frame.depth_width=static_cast<uint32_t>(descriptions[2].Width); frame.depth_height=descriptions[2].Height;
    frame.exposure_texture=exposure!=nullptr;
    auto effective=options.mode;
    if (!depth && (effective==Mode::depth_rcas || effective==Mode::depth_das)) effective=Mode::rcas;
    const auto data=constants(options,frame,effective);
    auto* heap=heaps_[slot].Get(); auto cpu=heap->GetCPUDescriptorHandleForHeapStart();
    const auto stride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    const DXGI_FORMAT null_formats[]={desc.Format,DXGI_FORMAT_R16G16_FLOAT,DXGI_FORMAT_R32_FLOAT,DXGI_FORMAT_R32_FLOAT};
    for (unsigned i=0;i<4;++i) {
        D3D12_SHADER_RESOURCE_VIEW_DESC view{}; view.ViewDimension=D3D12_SRV_DIMENSION_TEXTURE2D;
        view.Format=inputs[i]?rsf::srv_format(descriptions[i].Format):null_formats[i];
        view.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING; view.Texture2D.MipLevels=1;
        device->CreateShaderResourceView(inputs[i],&view,cpu); cpu.ptr+=stride;
    }
    D3D12_UNORDERED_ACCESS_VIEW_DESC target{}; target.ViewDimension=D3D12_UAV_DIMENSION_TEXTURE2D; target.Format=desc.Format;
    device->CreateUnorderedAccessView(scratch_[slot].Get(),nullptr,&target,cpu);
    rsf::transition(list,output,D3D12_RESOURCE_STATE_UNORDERED_ACCESS,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    list->SetDescriptorHeaps(1,&heap); list->SetComputeRootSignature(root_.Get());
    list->SetComputeRootDescriptorTable(0,heap->GetGPUDescriptorHandleForHeapStart());
    list->SetComputeRoot32BitConstants(1,sizeof(data)/4,&data,0);
    list->SetPipelineState(pipelines_[static_cast<unsigned>(effective)-1].Get());
    list->Dispatch((frame.width+15)/16,(frame.height+15)/16,1); ++dispatches_;
    rsf::copy_transitioned(list,output,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
        scratch_[slot].Get(),D3D12_RESOURCE_STATE_UNORDERED_ACCESS,D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
    return true;
}
}
