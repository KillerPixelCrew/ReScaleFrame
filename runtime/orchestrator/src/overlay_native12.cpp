// SPDX-License-Identifier: MIT
#include "overlay_native12_platform.h"
#include <rescaleframe/overlay_input.h>
#include <windows.h>
#include <d3d12.h>
#include <d3dcompiler.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <algorithm>
#include <array>
#include <cstring>
#include <memory>
#include <string>
namespace {
using Microsoft::WRL::ComPtr;
constexpr char shader[] = R"(
cbuffer View : register(b0) { float2 inverseSize; };
struct V { float2 p:POSITION; float2 uv:TEXCOORD0; float4 c:COLOR0; };
struct P { float4 p:SV_Position; float2 uv:TEXCOORD0; float4 c:COLOR0; };
P vs(V v) { P p; p.p=float4(v.p.x*inverseSize.x-1,1-v.p.y*inverseSize.y,0,1);p.uv=v.uv;p.c=v.c;return p; }
Texture2D atlas:register(t0); SamplerState linearSampler:register(s0);
float4 ps(P p):SV_Target { return p.c*atlas.Sample(linearSampler,p.uv); }
)";
struct Frame {
    ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> commands;
    ComPtr<ID3D12DescriptorHeap> srv, rtv;
    ComPtr<ID3D12Resource> vertices, indices;
    ComPtr<ID3D12PipelineState> pipeline;
    rsf_overlay_draw_data draw{};
};
struct Panel {
    HMODULE module{};
    rsf_overlay* ui{};
    decltype(&rsf_overlay_destroy) destroy{};
    bool module_published{};
    decltype(&rsf_overlay_frame) layout{};
    decltype(&rsf_overlay_texture_updates) updates{};
    decltype(&rsf_overlay_textures_to_free) frees{};
    ComPtr<ID3D12Device> device;
    ComPtr<ID3D12CommandQueue> queue;
    ComPtr<ID3D12RootSignature> root;
    ComPtr<ID3D12PipelineState> pipeline;
    ComPtr<ID3DBlob> vertex_shader, pixel_shader;
    DXGI_FORMAT format{DXGI_FORMAT_UNKNOWN};
    std::array<Frame,3> frames;
    UINT stride{};

    ~Panel() {
        if (ui) destroy(ui);
        // A successfully published panel module stays mapped, as in the shared overlay host.
        if (module && !module_published) FreeLibrary(module);
    }
};
void transition(ID3D12GraphicsCommandList* commands, ID3D12Resource* resource,
    D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after)
{
    D3D12_RESOURCE_BARRIER barrier{}; barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition={resource,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,before,after};
    commands->ResourceBarrier(1,&barrier);
}
bool upload_buffer(Panel& p, uint64_t size, ComPtr<ID3D12Resource>& buffer)
{
    if (buffer && buffer->GetDesc().Width>=size) return true;
    D3D12_HEAP_PROPERTIES heap{}; heap.Type=D3D12_HEAP_TYPE_UPLOAD;
    D3D12_RESOURCE_DESC desc{}; desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;
    desc.Width=(size+65535)&~uint64_t(65535); desc.Height=1;desc.DepthOrArraySize=1;desc.MipLevels=1;
    desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    buffer.Reset();
    return SUCCEEDED(p.device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,
        D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,IID_PPV_ARGS(&buffer)));
}
bool initialize(Panel& p, ID3D12Device* device, ID3D12CommandQueue* queue, HWND window, rsf_overlay12_log logger, void* user,
    void** completion)
{
    wchar_t path[32768]{};
    const auto configured=GetEnvironmentVariableW(L"RSF_OVERLAY_DLL",path,32768);
    if (configured>=32768) return false;
    if (!configured) {
        HMODULE own{};
        if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            reinterpret_cast<LPCWSTR>(&initialize),&own)) return false;
        const auto length=GetModuleFileNameW(own,path,32768);
        if (!length || length>=32768) return false;
        std::wstring file=path;const auto slash=file.find_last_of(L"/\\");
        if (slash==std::wstring::npos) return false;
        file.resize(slash+1);file+=L"rescaleframe_overlay.dll";
        if (file.size()>=32768) return false;
        std::memcpy(path,file.c_str(),(file.size()+1)*sizeof(wchar_t));
    }
    p.module=LoadLibraryW(path);if (!p.module) return false;
    const auto create=reinterpret_cast<decltype(&rsf_overlay_create)>(GetProcAddress(p.module,"rsf_overlay_create"));
    p.destroy=reinterpret_cast<decltype(p.destroy)>(GetProcAddress(p.module,"rsf_overlay_destroy"));
    p.layout=reinterpret_cast<decltype(p.layout)>(GetProcAddress(p.module,"rsf_overlay_frame"));
    p.updates=reinterpret_cast<decltype(p.updates)>(GetProcAddress(p.module,"rsf_overlay_texture_updates"));
    p.frees=reinterpret_cast<decltype(p.frees)>(GetProcAddress(p.module,"rsf_overlay_textures_to_free"));
    if (!create||!p.destroy||!p.layout||!p.updates||!p.frees || !(p.ui=create(RSF_OVERLAY_ABI_VERSION))) return false;
    p.device=device;p.queue=queue;p.stride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    ComPtr<ID3D12Fence> fence;
    if (FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence)))) return false;
    ComPtr<ID3DBlob> errors, signature;
    if (FAILED(D3DCompile(shader,sizeof(shader)-1,nullptr,nullptr,nullptr,"vs","vs_5_0",D3DCOMPILE_ENABLE_STRICTNESS,0,&p.vertex_shader,&errors)) ||
        FAILED(D3DCompile(shader,sizeof(shader)-1,nullptr,nullptr,nullptr,"ps","ps_5_0",D3DCOMPILE_ENABLE_STRICTNESS,0,&p.pixel_shader,&errors))) return false;
    D3D12_DESCRIPTOR_RANGE range{D3D12_DESCRIPTOR_RANGE_TYPE_SRV,1,0,0,0};
    D3D12_ROOT_PARAMETER parameters[2]{};
    parameters[0].ParameterType=D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;
    parameters[0].Constants={0,0,2};parameters[0].ShaderVisibility=D3D12_SHADER_VISIBILITY_VERTEX;
    parameters[1].ParameterType=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
    parameters[1].DescriptorTable={1,&range};parameters[1].ShaderVisibility=D3D12_SHADER_VISIBILITY_PIXEL;
    D3D12_STATIC_SAMPLER_DESC sampler{};sampler.Filter=D3D12_FILTER_MIN_MAG_MIP_LINEAR;
    sampler.AddressU=sampler.AddressV=sampler.AddressW=D3D12_TEXTURE_ADDRESS_MODE_CLAMP;
    sampler.MaxLOD=D3D12_FLOAT32_MAX;sampler.ComparisonFunc=D3D12_COMPARISON_FUNC_ALWAYS;
    sampler.MaxAnisotropy=1;
    sampler.ShaderVisibility=D3D12_SHADER_VISIBILITY_PIXEL;
    D3D12_ROOT_SIGNATURE_DESC desc{2,parameters,1,&sampler,D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT};
    if (FAILED(D3D12SerializeRootSignature(&desc,D3D_ROOT_SIGNATURE_VERSION_1,&signature,&errors)) ||
        FAILED(device->CreateRootSignature(0,signature->GetBufferPointer(),signature->GetBufferSize(),IID_PPV_ARGS(&p.root)))) return false;
    for (auto& frame:p.frames) {
        if (FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&frame.allocator))) ||
            FAILED(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,frame.allocator.Get(),nullptr,IID_PPV_ARGS(&frame.commands))) ||
            FAILED(frame.commands->Close())) return false;
        D3D12_DESCRIPTOR_HEAP_DESC heap{};heap.Type=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
        heap.NumDescriptors=RSF_OVERLAY12_TEXTURE_LIMIT;heap.Flags=D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
        if (FAILED(device->CreateDescriptorHeap(&heap,IID_PPV_ARGS(&frame.srv)))) return false;
        heap.Type=D3D12_DESCRIPTOR_HEAP_TYPE_RTV;heap.NumDescriptors=1;heap.Flags=D3D12_DESCRIPTOR_HEAP_FLAG_NONE;
        if (FAILED(device->CreateDescriptorHeap(&heap,IID_PPV_ARGS(&frame.rtv)))) return false;
    }
    const rsf_overlay_input_options input{sizeof(input),RSF_OVERLAY_INPUT_ABI_VERSION,0,logger,user};
    if (rsf_overlay_input_install(window,&input)!=RSF_OVERLAY_INPUT_OK) return false;
    p.module_published=true;
    *completion=fence.Detach();
    return true;
}
bool pipeline(Panel& p, DXGI_FORMAT format)
{
    if (p.pipeline && p.format==format) return true;
    D3D12_INPUT_ELEMENT_DESC layout[]{
        {"POSITION",0,DXGI_FORMAT_R32G32_FLOAT,0,0,D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,0},
        {"TEXCOORD",0,DXGI_FORMAT_R32G32_FLOAT,0,8,D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,0},
        {"COLOR",0,DXGI_FORMAT_R8G8B8A8_UNORM,0,16,D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,0}};
    D3D12_GRAPHICS_PIPELINE_STATE_DESC desc{};desc.pRootSignature=p.root.Get();
    desc.VS={p.vertex_shader->GetBufferPointer(),p.vertex_shader->GetBufferSize()};
    desc.PS={p.pixel_shader->GetBufferPointer(),p.pixel_shader->GetBufferSize()};
    desc.InputLayout={layout,3};desc.PrimitiveTopologyType=D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE;
    desc.NumRenderTargets=1;desc.RTVFormats[0]=format;desc.SampleDesc.Count=1;desc.SampleMask=UINT_MAX;
    desc.RasterizerState.FillMode=D3D12_FILL_MODE_SOLID;desc.RasterizerState.CullMode=D3D12_CULL_MODE_NONE;
    desc.RasterizerState.DepthClipEnable=TRUE;
    auto& blend=desc.BlendState.RenderTarget[0];blend.BlendEnable=TRUE;
    blend.SrcBlend=blend.SrcBlendAlpha=D3D12_BLEND_ONE;
    blend.DestBlend=blend.DestBlendAlpha=D3D12_BLEND_INV_SRC_ALPHA;
    blend.BlendOp=blend.BlendOpAlpha=D3D12_BLEND_OP_ADD;blend.RenderTargetWriteMask=D3D12_COLOR_WRITE_ENABLE_ALL;
    desc.DepthStencilState.DepthFunc=D3D12_COMPARISON_FUNC_ALWAYS;
    desc.DepthStencilState.FrontFace={D3D12_STENCIL_OP_KEEP,D3D12_STENCIL_OP_KEEP,D3D12_STENCIL_OP_KEEP,D3D12_COMPARISON_FUNC_ALWAYS};
    desc.DepthStencilState.BackFace=desc.DepthStencilState.FrontFace;
    p.pipeline.Reset();p.format=format;
    return SUCCEEDED(p.device->CreateGraphicsPipelineState(&desc,IID_PPV_ARGS(&p.pipeline)));
}
void* create_texture(void* native, uint32_t width, uint32_t height)
{
    auto& p=*static_cast<Panel*>(native);
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width=width;desc.Height=height;desc.DepthOrArraySize=1;desc.MipLevels=1;
    desc.Format=DXGI_FORMAT_R8G8B8A8_UNORM;desc.SampleDesc.Count=1;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    ComPtr<ID3D12Resource> created;
    return SUCCEEDED(p.device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,
        D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&created))) ? created.Detach() : nullptr;
}
void* upload_texture(void* native, uint32_t slot, void* resource, const rsf_overlay_texture_update* patch)
{
    auto& p=*static_cast<Panel*>(native);
    auto& frame=p.frames[slot];
    const auto& update=*patch;
    auto* destination=static_cast<ID3D12Resource*>(resource);
    auto desc=destination->GetDesc();
    desc.Width=update.width;desc.Height=update.height;
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{};UINT64 bytes{};
    p.device->GetCopyableFootprints(&desc,0,1,0,&footprint,nullptr,nullptr,&bytes);
    ComPtr<ID3D12Resource> upload;if (!upload_buffer(p,bytes,upload)) return nullptr;
    void* mapped{};const D3D12_RANGE empty{0,0};if (FAILED(upload->Map(0,&empty,&mapped))) return nullptr;
    for (UINT y=0;y<update.height;++y) std::memcpy(static_cast<unsigned char*>(mapped)+uint64_t(y)*footprint.Footprint.RowPitch,
        update.pixels+uint64_t(y)*update.width*4,uint64_t(update.width)*4);
    upload->Unmap(0,nullptr);
    if (!update.is_whole_texture) transition(frame.commands.Get(),destination,D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_COPY_DEST);
    D3D12_TEXTURE_COPY_LOCATION source{};source.pResource=upload.Get();source.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;source.PlacedFootprint=footprint;
    D3D12_TEXTURE_COPY_LOCATION target{};target.pResource=destination;target.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    frame.commands->CopyTextureRegion(&target,update.x,update.y,0,&source,nullptr);
    transition(frame.commands.Get(),destination,D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
    return upload.Detach();
}
bool draw(Panel& p, Frame& frame, ID3D12Resource* target, UINT width, UINT height,
    const rsf_overlay12_texture* textures, UINT texture_count)
{
    const auto& data=frame.draw;
    if (!data.vertex_count||!data.index_count) return true;
    const uint64_t vertex_bytes=uint64_t(data.vertex_count)*sizeof(rsf_overlay_vertex), index_bytes=uint64_t(data.index_count)*4;
    if (vertex_bytes>UINT_MAX||index_bytes>UINT_MAX||!upload_buffer(p,vertex_bytes,frame.vertices)||!upload_buffer(p,index_bytes,frame.indices)) return false;
    void* mapped{};const D3D12_RANGE empty{0,0};
    if (FAILED(frame.vertices->Map(0,&empty,&mapped))) return false;
    std::memcpy(mapped,data.vertices,static_cast<size_t>(vertex_bytes));frame.vertices->Unmap(0,nullptr);
    if (FAILED(frame.indices->Map(0,&empty,&mapped))) return false;
    std::memcpy(mapped,data.indices,static_cast<size_t>(index_bytes));frame.indices->Unmap(0,nullptr);
    for (UINT index=0;index<texture_count;++index) {
        auto cpu=frame.srv->GetCPUDescriptorHandleForHeapStart();cpu.ptr+=uint64_t(index)*p.stride;
        D3D12_SHADER_RESOURCE_VIEW_DESC view{};view.Format=DXGI_FORMAT_R8G8B8A8_UNORM;
        view.ViewDimension=D3D12_SRV_DIMENSION_TEXTURE2D;view.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;view.Texture2D.MipLevels=1;
        p.device->CreateShaderResourceView(static_cast<ID3D12Resource*>(textures[index].resource),&view,cpu);
    }
    auto* commands=frame.commands.Get();transition(commands,target,D3D12_RESOURCE_STATE_PRESENT,D3D12_RESOURCE_STATE_RENDER_TARGET);
    const auto rtv=frame.rtv->GetCPUDescriptorHandleForHeapStart();p.device->CreateRenderTargetView(target,nullptr,rtv);
    commands->OMSetRenderTargets(1,&rtv,FALSE,nullptr);commands->SetPipelineState(frame.pipeline.Get());commands->SetGraphicsRootSignature(p.root.Get());
    ID3D12DescriptorHeap* heaps[]{frame.srv.Get()};commands->SetDescriptorHeaps(1,heaps);
    const float scale[]{2.0f/static_cast<float>(width),2.0f/static_cast<float>(height)};commands->SetGraphicsRoot32BitConstants(0,2,scale,0);
    const D3D12_VIEWPORT viewport{0,0,static_cast<float>(width),static_cast<float>(height),0,1};commands->RSSetViewports(1,&viewport);
    D3D12_VERTEX_BUFFER_VIEW vertices{frame.vertices->GetGPUVirtualAddress(),static_cast<UINT>(vertex_bytes),sizeof(rsf_overlay_vertex)};
    D3D12_INDEX_BUFFER_VIEW indices{frame.indices->GetGPUVirtualAddress(),static_cast<UINT>(index_bytes),DXGI_FORMAT_R32_UINT};
    commands->IASetVertexBuffers(0,1,&vertices);commands->IASetIndexBuffer(&indices);commands->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    const auto end=textures+texture_count;
    for (UINT i=0;i<data.call_count;++i) {
        const auto& call=data.calls[i];
        const auto descriptor=std::find_if(textures,end,[&](const auto& texture){return texture.id==call.texture_id;});
        if (descriptor==end||uint64_t(call.index_offset)+call.index_count>data.index_count||call.vertex_offset>data.vertex_count) return false;
        const D3D12_RECT clip{static_cast<LONG>(std::min(call.clip_x,width)),static_cast<LONG>(std::min(call.clip_y,height)),
            static_cast<LONG>(std::min(uint64_t(call.clip_x)+call.clip_width,uint64_t(width))),static_cast<LONG>(std::min(uint64_t(call.clip_y)+call.clip_height,uint64_t(height)))};
        commands->RSSetScissorRects(1,&clip);auto gpu=frame.srv->GetGPUDescriptorHandleForHeapStart();gpu.ptr+=uint64_t(descriptor-textures)*p.stride;
        commands->SetGraphicsRootDescriptorTable(1,gpu);commands->DrawIndexedInstanced(call.index_count,1,call.index_offset,static_cast<INT>(call.vertex_offset),0);
    }
    transition(commands,target,D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_PRESENT);return true;
}
void* create_panel(void* device, void* queue, void* chain, rsf_overlay12_log logger, void* user, void** fence) noexcept try {
    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(static_cast<IDXGISwapChain*>(chain)->GetDesc(&desc))) return nullptr;
    auto panel=std::make_unique<Panel>();
    return initialize(*panel, static_cast<ID3D12Device*>(device), static_cast<ID3D12CommandQueue*>(queue),
        desc.OutputWindow, logger, user, fence) ? panel.release() : nullptr;
} catch (...) { return nullptr; }

uint32_t stop_input() {
    const auto result=rsf_overlay_input_uninstall();
    return result==RSF_OVERLAY_INPUT_OK || result==RSF_OVERLAY_INPUT_ERROR_NOT_INSTALLED;
}

void destroy_panel(void* native) {
    delete static_cast<Panel*>(native);
}

uint64_t completed(void* fence) {
    return static_cast<ID3D12Fence*>(fence)->GetCompletedValue();
}

uint32_t arm(void* fence, void* event, uint64_t value) {
    return SUCCEEDED(static_cast<ID3D12Fence*>(fence)->SetEventOnCompletion(value,event));
}

void* begin_frame(void* native, uint32_t slot, void* chain, uint32_t* width, uint32_t* height) noexcept try {
    auto& panel=*static_cast<Panel*>(native);
    auto& frame=panel.frames[slot];
    frame.pipeline.Reset();
    if (FAILED(frame.allocator->Reset())||FAILED(frame.commands->Reset(frame.allocator.Get(),nullptr))) return nullptr;
    ComPtr<IDXGISwapChain3> swapchain;ComPtr<ID3D12Resource> target;
    if (FAILED(static_cast<IDXGISwapChain*>(chain)->QueryInterface(IID_PPV_ARGS(&swapchain)))||
        FAILED(swapchain->GetBuffer(swapchain->GetCurrentBackBufferIndex(),IID_PPV_ARGS(&target)))) return nullptr;
    const auto desc=target->GetDesc();
    if (!desc.Width || desc.Width>UINT_MAX || !desc.Height || !pipeline(panel,desc.Format)) return nullptr;
    frame.pipeline=panel.pipeline;
    *width=static_cast<uint32_t>(desc.Width);
    *height=desc.Height;
    return target.Detach();
} catch (...) { return nullptr; }

uint32_t layout(void* native, uint32_t slot, uint32_t width, uint32_t height, float elapsed,
    float hint_alpha, const rsf_overlay_stats* stats, rsf_overlay_intent* decided) noexcept try {
    auto& panel=*static_cast<Panel*>(native);
    auto& data=panel.frames[slot].draw;
    rsf_overlay_input input{};input.struct_size=sizeof(input);
    if (rsf_overlay_input_collect(&input,width,height,elapsed)!=RSF_OVERLAY_INPUT_OK) return 0;
    input.startup_hint_alpha=hint_alpha;
    data={};data.struct_size=sizeof(data);
    return panel.layout(panel.ui,&input,stats,&data,decided)==RSF_OVERLAY_OK;
} catch (...) { return 0; }

uint32_t updates(void* native, rsf_overlay_texture_update* output, uint32_t capacity) {
    auto& panel=*static_cast<Panel*>(native);
    return panel.updates(panel.ui,output,capacity);
}

uint32_t frees(void* native, uint64_t* output, uint32_t capacity) {
    auto& panel=*static_cast<Panel*>(native);
    return panel.frees(panel.ui,output,capacity);
}

uint32_t record(void* native, uint32_t slot, void* target, uint32_t width, uint32_t height,
    const rsf_overlay12_texture* textures, uint32_t texture_count) noexcept try {
    auto& panel=*static_cast<Panel*>(native);
    auto& frame=panel.frames[slot];
    return draw(panel,frame,static_cast<ID3D12Resource*>(target),width,height,textures,texture_count) &&
        SUCCEEDED(frame.commands->Close());
} catch (...) { return 0; }

void execute(void* native, uint32_t slot) {
    auto& panel=*static_cast<Panel*>(native);
    ID3D12CommandList* lists[]{panel.frames[slot].commands.Get()};
    panel.queue->ExecuteCommandLists(1,lists);
}

uint32_t signal(void* native, void* fence, uint64_t value) {
    return SUCCEEDED(static_cast<Panel*>(native)->queue->Signal(static_cast<ID3D12Fence*>(fence),value));
}

constexpr rsf_overlay12_platform platform{
    create_panel, stop_input, destroy_panel, rsf_overlay_input_visible, rsf_overlay_input_set_visible,
    completed, arm, begin_frame, layout, updates, frees, create_texture, upload_texture, record, execute, signal,
};
}

uint32_t rsf_overlay_native12_visible() {
    return rsf_core_overlay12_visible(&platform);
}
void rsf_overlay_native12_toggle() {
    rsf_core_overlay12_toggle(&platform);
}
int rsf_overlay_native12_present(void* device, void* queue, void* chain, const rsf_overlay_stats* stats,
    rsf_overlay_intent* intent, void (*logger)(void*,const char*), void* user) {
    return rsf_core_overlay12_present(&platform, device, queue, chain, stats, intent, logger, user);
}
