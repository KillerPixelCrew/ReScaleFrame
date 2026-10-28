// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_sr_d3d12.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <filesystem>
#include <fstream>
#include <cmath>
#include <cstdio>
#include <cstring>
using Microsoft::WRL::ComPtr;
namespace {
bool texture(ID3D12Device* device, UINT size, DXGI_FORMAT format, D3D12_RESOURCE_STATES state, ComPtr<ID3D12Resource>& out)
{
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;desc.Width=desc.Height=size;
    desc.DepthOrArraySize=desc.MipLevels=1;desc.SampleDesc.Count=1;desc.Format=format;
    return SUCCEEDED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,state,nullptr,IID_PPV_ARGS(&out)));
}
}
int main() try
{
    ComPtr<IDXGIFactory4> factory;ComPtr<IDXGIAdapter> adapter;ComPtr<ID3D12Device> device;
    if(FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory)))||FAILED(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)))||
        FAILED(D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&device))))return 1;
    D3D12_COMMAND_QUEUE_DESC q{};q.Type=D3D12_COMMAND_LIST_TYPE_DIRECT;ComPtr<ID3D12CommandQueue> queue;
    ComPtr<ID3D12CommandAllocator> allocator;ComPtr<ID3D12GraphicsCommandList> list;
    if(FAILED(device->CreateCommandQueue(&q,IID_PPV_ARGS(&queue)))||FAILED(device->CreateCommandAllocator(q.Type,IID_PPV_ARGS(&allocator)))||
        FAILED(device->CreateCommandList(0,q.Type,allocator.Get(),nullptr,IID_PPV_ARGS(&list))))return 1;
    ComPtr<ID3D12Resource> color,depth,motion,exposure,upload;
    if(!texture(device.Get(),8,DXGI_FORMAT_R16G16B16A16_FLOAT,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,color)||
        !texture(device.Get(),8,DXGI_FORMAT_R32_FLOAT,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,depth)||
        !texture(device.Get(),8,DXGI_FORMAT_R16G16_FLOAT,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,motion)||
        !texture(device.Get(),1,DXGI_FORMAT_R32G32B32A32_FLOAT,D3D12_RESOURCE_STATE_COPY_DEST,exposure))return 1;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_UPLOAD;D3D12_RESOURCE_DESC buffer{};
    buffer.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;buffer.Width=256;buffer.Height=buffer.DepthOrArraySize=buffer.MipLevels=1;
    buffer.SampleDesc.Count=1;buffer.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    if(FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&buffer,D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,IID_PPV_ARGS(&upload))))return 1;
    void* data{};D3D12_RANGE none{};if(FAILED(upload->Map(0,&none,&data)))return 1;
    const float values[]={3.25f,17.f,29.f,43.f};std::memcpy(data,values,sizeof(values));upload->Unmap(0,nullptr);
    D3D12_TEXTURE_COPY_LOCATION source{},target{};source.pResource=upload.Get();source.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
    source.PlacedFootprint.Footprint={DXGI_FORMAT_R32G32B32A32_FLOAT,1,1,1,256};
    target.pResource=exposure.Get();target.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;list->CopyTextureRegion(&target,0,0,0,&source,nullptr);
    D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition={exposure.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE};
    list->ResourceBarrier(1,&barrier);
    rsf_sr12_setup setup{};setup.struct_size=sizeof(setup);setup.abi_version=2;setup.device=device.Get();
    setup.output_width=setup.output_height=setup.render_width=setup.render_height=8;setup.engine_input_extents=1;setup.external_exposure=1;
    rsf_sr12* context{};if(rsf_sr12_create(&setup,&context))return 1;
    const auto directory=std::filesystem::temp_directory_path()/ ("rsf-exposure-"+std::to_string(GetCurrentProcessId()));
    if(!std::filesystem::create_directory(directory))return 1;
    if(rsf_sr12_capture(context,directory.string().c_str(),1,1))return 1;
    rsf_game_render_pass pass{};pass.struct_size=sizeof(pass);pass.session_id=7;pass.native_frame=pass.source_frame_id=pass.history_key=1;
    pass.resource_generation=pass.camera_valid=1;pass.color_input=color.Get();pass.depth=depth.Get();pass.motion=motion.Get();pass.exposure=exposure.Get();
    pass.motion_to_uv[0]=pass.motion_to_uv[1]=-0.125f;pass.camera.struct_size=sizeof(pass.camera);pass.camera.abi_version=RSF_GAME_FRAME_ABI_VERSION;
    pass.camera.render_width=pass.camera.render_height=pass.camera.output_width=pass.camera.output_height=8;
    if(rsf_sr12_evaluate(context,list.Get(),&pass,0)||FAILED(list->Close()))return 1;
    ID3D12CommandList* lists[]={list.Get()};queue->ExecuteCommandLists(1,lists);ComPtr<ID3D12Fence> fence;
    if(FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence)))||FAILED(queue->Signal(fence.Get(),1)))return 1;
    const HANDLE event=CreateEventW(nullptr,FALSE,FALSE,nullptr);if(!event)return 1;
    const bool joined=SUCCEEDED(fence->SetEventOnCompletion(1,event))&&WaitForSingleObject(event,10000)==WAIT_OBJECT_0;CloseHandle(event);
    if(!joined||rsf_sr12_capture_flush(context,0))return 1;rsf_sr12_destroy(context);
    bool found=false;
    for(const auto& file:std::filesystem::directory_iterator(directory))if(file.path().extension()==".raw"&&file.path().filename().string().find("sr-exposure")!=std::string::npos){
        float value{};std::ifstream input(file.path(),std::ios::binary);input.read(reinterpret_cast<char*>(&value),sizeof(value));
        if(!input||std::abs(value-values[0])>1e-6f)return 1;found=true;
    }
    if(!found)return 1;
    if(directory.filename()!=("rsf-exposure-"+std::to_string(GetCurrentProcessId())))return 1;
    std::filesystem::remove_all(directory);
    std::puts("External RGBA32F exposure normalized to R32F: GPU readback preserves the declared first component.");return 0;
}catch(...){return 1;}
