// SPDX-License-Identifier: MIT
#include "d3d12.h"
#include "d3d12_platform.h"
#include "rcas.h"
#include "da_rcas.h"
#include "da_das.h"
#include <rescaleframe/shader_compile.h>
#include <rescaleframe/srv_format.h>
#include "../../../backends/common/d3d12_helpers.h"
#include <cstddef>
#include <vector>
#include <wrl/client.h>

using Microsoft::WRL::ComPtr;

namespace rsf::sharpening {
namespace {
static_assert(DXGI_FORMAT_R32G32B32A32_FLOAT == 2 && DXGI_FORMAT_R16G16B16A16_FLOAT == 10);
static_assert(D3D12_RESOURCE_DIMENSION_TEXTURE2D == 3 && D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE == 0x8);
static_assert(static_cast<uint32_t>(Mode::rcas) == 1 && static_cast<uint32_t>(Mode::depth_rcas) == 2 &&
    static_cast<uint32_t>(Mode::depth_das) == 3);
static_assert(sizeof(rsf_sharpen12_texture) == 32 && offsetof(rsf_sharpen12_texture, array_size) == 28);
static_assert(sizeof(rsf_sharpen12_call) == 72 && offsetof(rsf_sharpen12_call, mode) == 56);
static_assert(sizeof(rsf_sharpen12_dispatch) == 176 && offsetof(rsf_sharpen12_dispatch, mode) == 160);
static_assert(sizeof(rsf_sharpen12_platform) == 8 * sizeof(void*));

void* child_device(void* child)
{
    ComPtr<ID3D12Device> device;
    if (FAILED(static_cast<ID3D12DeviceChild*>(child)->GetDevice(IID_PPV_ARGS(&device)))) return nullptr;
    return device.Detach();
}

void describe(void* resource, rsf_sharpen12_texture* out)
{
    const auto desc = static_cast<ID3D12Resource*>(resource)->GetDesc();
    *out = {desc.Width, desc.Height, uint32_t(desc.Format), uint32_t(desc.Dimension),
        desc.SampleDesc.Count, uint32_t(desc.Flags), desc.DepthOrArraySize, desc.MipLevels};
}

void* create_root(void* raw_device)
{
    auto* device = static_cast<ID3D12Device*>(raw_device);
    D3D12_DESCRIPTOR_RANGE ranges[]={{D3D12_DESCRIPTOR_RANGE_TYPE_SRV,4,0,0,0},
        {D3D12_DESCRIPTOR_RANGE_TYPE_UAV,1,0,0,4}};
    D3D12_ROOT_PARAMETER parameters[2]{};
    parameters[0].ParameterType=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
    parameters[0].DescriptorTable={2,ranges};
    parameters[1].ParameterType=D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;
    parameters[1].Constants={0,0,sizeof(Constants)/4};
    D3D12_ROOT_SIGNATURE_DESC description{2,parameters,0,nullptr,D3D12_ROOT_SIGNATURE_FLAG_NONE};
    ComPtr<ID3DBlob> serialized,errors;
    ComPtr<ID3D12RootSignature> root;
    if (FAILED(D3D12SerializeRootSignature(&description,D3D_ROOT_SIGNATURE_VERSION_1,&serialized,&errors)) ||
        FAILED(device->CreateRootSignature(0,serialized->GetBufferPointer(),serialized->GetBufferSize(),IID_PPV_ARGS(&root))))
        return nullptr;
    return root.Detach();
}

void* create_pipeline(void* raw_device, void* root, uint32_t mode) try
{
    auto* device = static_cast<ID3D12Device*>(raw_device);
    const char* sources[]={rcas,da_rcas,da_das};
    std::vector<uint8_t> code;
    if (!rsf::compile_shader(sources[mode - 1],"Sharpening12","CSMain","cs_5_0",code)) return nullptr;
    D3D12_COMPUTE_PIPELINE_STATE_DESC desc{};
    desc.pRootSignature=static_cast<ID3D12RootSignature*>(root); desc.CS={code.data(),code.size()};
    ComPtr<ID3D12PipelineState> pipeline;
    if (FAILED(device->CreateComputePipelineState(&desc,IID_PPV_ARGS(&pipeline)))) return nullptr;
    return pipeline.Detach();
} catch (...) { return nullptr; }

void* create_heap(void* raw_device)
{
    auto* device = static_cast<ID3D12Device*>(raw_device);
    D3D12_DESCRIPTOR_HEAP_DESC desc{}; desc.Type=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
    desc.NumDescriptors=5; desc.Flags=D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
    ComPtr<ID3D12DescriptorHeap> heap;
    if (FAILED(device->CreateDescriptorHeap(&desc,IID_PPV_ARGS(&heap)))) return nullptr;
    return heap.Detach();
}

void* create_scratch(void* device, uint32_t width, uint32_t height, uint32_t format)
{
    ComPtr<ID3D12Resource> scratch;
    return rsf::create_uav_texture(static_cast<ID3D12Device*>(device),width,height,
        static_cast<DXGI_FORMAT>(format),&scratch) ? scratch.Detach() : nullptr;
}

void dispatch(void* raw_device, const rsf_sharpen12_call* call, const rsf_sharpen12_dispatch* pass)
{
    auto* device = static_cast<ID3D12Device*>(raw_device);
    auto* list = static_cast<ID3D12GraphicsCommandList*>(call->commands);
    auto* heap = static_cast<ID3D12DescriptorHeap*>(pass->heap);
    auto* output = static_cast<ID3D12Resource*>(call->textures[0]);
    const auto& desc = pass->descriptions[0];
    auto frame = *static_cast<const Frame*>(call->frame);
    frame.width=static_cast<uint32_t>(desc.width); frame.height=desc.height;
    frame.motion_width=static_cast<uint32_t>(pass->descriptions[1].width); frame.motion_height=pass->descriptions[1].height;
    frame.depth_width=static_cast<uint32_t>(pass->descriptions[2].width); frame.depth_height=pass->descriptions[2].height;
    frame.exposure_texture=call->textures[3]!=nullptr;
    const auto data=constants(*static_cast<const Options*>(call->options),frame,static_cast<Mode>(pass->mode));
    auto cpu=heap->GetCPUDescriptorHandleForHeapStart();
    const auto stride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    const auto format = static_cast<DXGI_FORMAT>(desc.format);
    const DXGI_FORMAT null_formats[]={format,DXGI_FORMAT_R16G16_FLOAT,DXGI_FORMAT_R32_FLOAT,DXGI_FORMAT_R32_FLOAT};
    for (unsigned i=0;i<4;++i) {
        auto* input = static_cast<ID3D12Resource*>(call->textures[i]);
        D3D12_SHADER_RESOURCE_VIEW_DESC view{}; view.ViewDimension=D3D12_SRV_DIMENSION_TEXTURE2D;
        view.Format=input?rsf::srv_format(static_cast<DXGI_FORMAT>(pass->descriptions[i].format)):null_formats[i];
        view.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING; view.Texture2D.MipLevels=1;
        device->CreateShaderResourceView(input,&view,cpu); cpu.ptr+=stride;
    }
    D3D12_UNORDERED_ACCESS_VIEW_DESC target{}; target.ViewDimension=D3D12_UAV_DIMENSION_TEXTURE2D; target.Format=format;
    device->CreateUnorderedAccessView(static_cast<ID3D12Resource*>(pass->scratch),nullptr,&target,cpu);
    rsf::transition(list,output,D3D12_RESOURCE_STATE_UNORDERED_ACCESS,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    list->SetDescriptorHeaps(1,&heap); list->SetComputeRootSignature(static_cast<ID3D12RootSignature*>(pass->root));
    list->SetComputeRootDescriptorTable(0,heap->GetGPUDescriptorHandleForHeapStart());
    list->SetComputeRoot32BitConstants(1,sizeof(data)/4,&data,0);
    list->SetPipelineState(static_cast<ID3D12PipelineState*>(pass->pipeline));
    list->Dispatch(pass->groups_x,pass->groups_y,1);
}

void copy_back(void* list, void* output, void* scratch)
{
    rsf::copy_transitioned(static_cast<ID3D12GraphicsCommandList*>(list),static_cast<ID3D12Resource*>(output),
        D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
        static_cast<ID3D12Resource*>(scratch),D3D12_RESOURCE_STATE_UNORDERED_ACCESS,D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
}

constexpr rsf_sharpen12_platform platform{
    child_device, describe, create_root, create_pipeline, create_heap, create_scratch, dispatch, copy_back,
};
}

D3D12::~D3D12()
{
    rsf_core_sharpen12_destroy(owner_);
}

uint64_t D3D12::dispatches() const
{
    return rsf_core_sharpen12_dispatches(owner_);
}

bool D3D12::run(ID3D12GraphicsCommandList* list, ID3D12Resource* output, ID3D12Resource* motion,
    ID3D12Resource* depth, ID3D12Resource* exposure, const Options& request, Frame frame, uint32_t slot)
{
    const auto options = sanitize(request);
    const rsf_sharpen12_call call{list, {output, motion, depth, exposure}, &options, &frame,
        static_cast<uint32_t>(options.mode), active(options) ? 1u : 0u, slot};
    return rsf_core_sharpen12_run(&owner_, &platform, &call) != 0;
}
}
