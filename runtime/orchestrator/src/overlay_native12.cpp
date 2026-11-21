// SPDX-License-Identifier: MIT
#include "overlay_native12_platform.h"
#include <rescaleframe/overlay_input.h>
#include <windows.h>
#include <d3d12.h>
#include <d3dcompiler.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <cstddef>
#include <cstring>
#include <memory>
// overlay_native12_abi.rs asserts the same layout.
static_assert(sizeof(rsf_overlay12_frame) == 4 * sizeof(void*) &&
    offsetof(rsf_overlay12_platform, create_frame) == 2 * sizeof(void*) &&
    offsetof(rsf_overlay12_platform, signal) == 18 * sizeof(void*) &&
    sizeof(rsf_overlay12_platform) == 19 * sizeof(void*));
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
// A Present slot borrowed from the Rust owner's lease for one leaf call.
struct Frame {
    ID3D12CommandAllocator* allocator;
    ID3D12GraphicsCommandList* commands;
    ID3D12DescriptorHeap* srv;
    ID3D12DescriptorHeap* rtv;
};
Frame borrow(const rsf_overlay12_frame* frame)
{
    return {static_cast<ID3D12CommandAllocator*>(frame->allocator),
        static_cast<ID3D12GraphicsCommandList*>(frame->commands),
        static_cast<ID3D12DescriptorHeap*>(frame->srv), static_cast<ID3D12DescriptorHeap*>(frame->rtv)};
}
// The three slot frames are leased by the Rust owner, which releases them last slot first
// immediately before destroy_renderer, where these members used to be destroyed.
struct Renderer {
    ComPtr<ID3D12Device> device;
    ComPtr<ID3D12CommandQueue> queue;
    ComPtr<ID3D12RootSignature> root;
    ComPtr<ID3DBlob> vertex_shader, pixel_shader;
    UINT stride{};

};
void transition(ID3D12GraphicsCommandList* commands, ID3D12Resource* resource,
    D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after)
{
    D3D12_RESOURCE_BARRIER barrier{}; barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition={resource,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,before,after};
    commands->ResourceBarrier(1,&barrier);
}
void* create_upload(void* native, uint64_t size, uint64_t* capacity)
{
    if (!size || size>UINT64_MAX-65535) return nullptr;
    auto& p=*static_cast<Renderer*>(native);
    D3D12_HEAP_PROPERTIES heap{}; heap.Type=D3D12_HEAP_TYPE_UPLOAD;
    D3D12_RESOURCE_DESC desc{}; desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;
    desc.Width=(size+65535)&~uint64_t(65535); desc.Height=1;desc.DepthOrArraySize=1;desc.MipLevels=1;
    desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    ComPtr<ID3D12Resource> buffer;
    if (FAILED(p.device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,
            D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,IID_PPV_ARGS(&buffer)))) return nullptr;
    if (capacity) *capacity=desc.Width;
    return buffer.Detach();
}
bool initialize(Renderer& p, ID3D12Device* device, ID3D12CommandQueue* queue, void** completion)
{
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
    *completion=fence.Detach();
    return true;
}
// One slot's objects in their former creation order. Whatever was created passes to the Rust
// owner, which releases a refused slot's objects in reverse creation order.
uint32_t create_frame(void* native, rsf_overlay12_frame* out)
{
    auto& p=*static_cast<Renderer*>(native);
    auto* device=p.device.Get();
    ComPtr<ID3D12CommandAllocator> allocator;ComPtr<ID3D12GraphicsCommandList> commands;
    ComPtr<ID3D12DescriptorHeap> srv,rtv;
    const bool created=[&]{
        if (FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&allocator))) ||
            FAILED(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,allocator.Get(),nullptr,IID_PPV_ARGS(&commands))) ||
            FAILED(commands->Close())) return false;
        D3D12_DESCRIPTOR_HEAP_DESC heap{};heap.Type=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
        heap.NumDescriptors=RSF_OVERLAY12_TEXTURE_LIMIT;heap.Flags=D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
        if (FAILED(device->CreateDescriptorHeap(&heap,IID_PPV_ARGS(&srv)))) return false;
        heap.Type=D3D12_DESCRIPTOR_HEAP_TYPE_RTV;heap.NumDescriptors=1;heap.Flags=D3D12_DESCRIPTOR_HEAP_FLAG_NONE;
        return SUCCEEDED(device->CreateDescriptorHeap(&heap,IID_PPV_ARGS(&rtv)));
    }();
    *out={allocator.Detach(),commands.Detach(),srv.Detach(),rtv.Detach()};
    return created;
}
void* create_pipeline(void* native, uint32_t format)
{
    auto& p=*static_cast<Renderer*>(native);
    D3D12_INPUT_ELEMENT_DESC layout[]{
        {"POSITION",0,DXGI_FORMAT_R32G32_FLOAT,0,0,D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,0},
        {"TEXCOORD",0,DXGI_FORMAT_R32G32_FLOAT,0,8,D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,0},
        {"COLOR",0,DXGI_FORMAT_R8G8B8A8_UNORM,0,16,D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,0}};
    D3D12_GRAPHICS_PIPELINE_STATE_DESC desc{};desc.pRootSignature=p.root.Get();
    desc.VS={p.vertex_shader->GetBufferPointer(),p.vertex_shader->GetBufferSize()};
    desc.PS={p.pixel_shader->GetBufferPointer(),p.pixel_shader->GetBufferSize()};
    desc.InputLayout={layout,3};desc.PrimitiveTopologyType=D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE;
    desc.NumRenderTargets=1;desc.RTVFormats[0]=static_cast<DXGI_FORMAT>(format);desc.SampleDesc.Count=1;desc.SampleMask=UINT_MAX;
    desc.RasterizerState.FillMode=D3D12_FILL_MODE_SOLID;desc.RasterizerState.CullMode=D3D12_CULL_MODE_NONE;
    desc.RasterizerState.DepthClipEnable=TRUE;
    auto& blend=desc.BlendState.RenderTarget[0];blend.BlendEnable=TRUE;
    blend.SrcBlend=blend.SrcBlendAlpha=D3D12_BLEND_ONE;
    blend.DestBlend=blend.DestBlendAlpha=D3D12_BLEND_INV_SRC_ALPHA;
    blend.BlendOp=blend.BlendOpAlpha=D3D12_BLEND_OP_ADD;blend.RenderTargetWriteMask=D3D12_COLOR_WRITE_ENABLE_ALL;
    desc.DepthStencilState.DepthFunc=D3D12_COMPARISON_FUNC_ALWAYS;
    desc.DepthStencilState.FrontFace={D3D12_STENCIL_OP_KEEP,D3D12_STENCIL_OP_KEEP,D3D12_STENCIL_OP_KEEP,D3D12_COMPARISON_FUNC_ALWAYS};
    desc.DepthStencilState.BackFace=desc.DepthStencilState.FrontFace;
    ComPtr<ID3D12PipelineState> pipeline;
    return SUCCEEDED(p.device->CreateGraphicsPipelineState(&desc,IID_PPV_ARGS(&pipeline))) ? pipeline.Detach() : nullptr;
}
void* create_texture(void* native, uint32_t width, uint32_t height)
{
    auto& p=*static_cast<Renderer*>(native);
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width=width;desc.Height=height;desc.DepthOrArraySize=1;desc.MipLevels=1;
    desc.Format=DXGI_FORMAT_R8G8B8A8_UNORM;desc.SampleDesc.Count=1;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    ComPtr<ID3D12Resource> created;
    return SUCCEEDED(p.device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,
        D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&created))) ? created.Detach() : nullptr;
}
void* upload_texture(void* native, const rsf_overlay12_frame* slot, void* resource, const rsf_overlay_texture_update* patch)
{
    auto& p=*static_cast<Renderer*>(native);
    const auto frame=borrow(slot);
    const auto& update=*patch;
    auto* destination=static_cast<ID3D12Resource*>(resource);
    auto desc=destination->GetDesc();
    desc.Width=update.width;desc.Height=update.height;
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{};UINT64 bytes{};
    p.device->GetCopyableFootprints(&desc,0,1,0,&footprint,nullptr,nullptr,&bytes);
    ComPtr<ID3D12Resource> upload;
    upload.Attach(static_cast<ID3D12Resource*>(create_upload(native,bytes,nullptr)));
    if (!upload) return nullptr;
    void* mapped{};const D3D12_RANGE empty{0,0};if (FAILED(upload->Map(0,&empty,&mapped))) return nullptr;
    for (UINT y=0;y<update.height;++y) std::memcpy(static_cast<unsigned char*>(mapped)+uint64_t(y)*footprint.Footprint.RowPitch,
        update.pixels+uint64_t(y)*update.width*4,uint64_t(update.width)*4);
    upload->Unmap(0,nullptr);
    if (!update.is_whole_texture) transition(frame.commands,destination,D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_COPY_DEST);
    D3D12_TEXTURE_COPY_LOCATION source{};source.pResource=upload.Get();source.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;source.PlacedFootprint=footprint;
    D3D12_TEXTURE_COPY_LOCATION target{};target.pResource=destination;target.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    frame.commands->CopyTextureRegion(&target,update.x,update.y,0,&source,nullptr);
    transition(frame.commands,destination,D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
    return upload.Detach();
}
bool draw(Renderer& p, const Frame& frame, const rsf_overlay12_draw& binding,
    void* const* textures, UINT texture_count, const rsf_overlay12_draw_call* calls, UINT call_count)
{
    const auto [target_resource,pipeline,vertex_resource,index_resource,vertex_data,index_data,width,height,vertex_bytes,index_bytes]=binding;
    if (!vertex_bytes||!index_bytes) return true;
    auto* target=static_cast<ID3D12Resource*>(target_resource);
    auto* vertex_buffer=static_cast<ID3D12Resource*>(vertex_resource);
    auto* index_buffer=static_cast<ID3D12Resource*>(index_resource);
    void* mapped{};const D3D12_RANGE empty{0,0};
    if (FAILED(vertex_buffer->Map(0,&empty,&mapped))) return false;
    std::memcpy(mapped,vertex_data,vertex_bytes);vertex_buffer->Unmap(0,nullptr);
    if (FAILED(index_buffer->Map(0,&empty,&mapped))) return false;
    std::memcpy(mapped,index_data,index_bytes);index_buffer->Unmap(0,nullptr);
    for (UINT index=0;index<texture_count;++index) {
        auto cpu=frame.srv->GetCPUDescriptorHandleForHeapStart();cpu.ptr+=uint64_t(index)*p.stride;
        D3D12_SHADER_RESOURCE_VIEW_DESC view{};view.Format=DXGI_FORMAT_R8G8B8A8_UNORM;
        view.ViewDimension=D3D12_SRV_DIMENSION_TEXTURE2D;view.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;view.Texture2D.MipLevels=1;
        p.device->CreateShaderResourceView(static_cast<ID3D12Resource*>(textures[index]),&view,cpu);
    }
    auto* commands=frame.commands;transition(commands,target,D3D12_RESOURCE_STATE_PRESENT,D3D12_RESOURCE_STATE_RENDER_TARGET);
    const auto rtv=frame.rtv->GetCPUDescriptorHandleForHeapStart();p.device->CreateRenderTargetView(target,nullptr,rtv);
    commands->OMSetRenderTargets(1,&rtv,FALSE,nullptr);commands->SetPipelineState(static_cast<ID3D12PipelineState*>(pipeline));commands->SetGraphicsRootSignature(p.root.Get());
    ID3D12DescriptorHeap* heaps[]{frame.srv};commands->SetDescriptorHeaps(1,heaps);
    const float scale[]{2.0f/static_cast<float>(width),2.0f/static_cast<float>(height)};commands->SetGraphicsRoot32BitConstants(0,2,scale,0);
    const D3D12_VIEWPORT viewport{0,0,static_cast<float>(width),static_cast<float>(height),0,1};commands->RSSetViewports(1,&viewport);
    D3D12_VERTEX_BUFFER_VIEW vertices{vertex_buffer->GetGPUVirtualAddress(),vertex_bytes,sizeof(rsf_overlay_vertex)};
    D3D12_INDEX_BUFFER_VIEW indices{index_buffer->GetGPUVirtualAddress(),index_bytes,DXGI_FORMAT_R32_UINT};
    commands->IASetVertexBuffers(0,1,&vertices);commands->IASetIndexBuffer(&indices);commands->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    for (UINT i=0;i<call_count;++i) {
        const auto& call=calls[i];
        const D3D12_RECT clip{call.clip[0],call.clip[1],call.clip[2],call.clip[3]};
        commands->RSSetScissorRects(1,&clip);auto gpu=frame.srv->GetGPUDescriptorHandleForHeapStart();gpu.ptr+=uint64_t(call.descriptor)*p.stride;
        commands->SetGraphicsRootDescriptorTable(1,gpu);commands->DrawIndexedInstanced(call.index_count,1,call.first_index,call.vertex_offset,0);
    }
    transition(commands,target,D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_PRESENT);return true;
}
void* output_window(void* chain) {
    DXGI_SWAP_CHAIN_DESC desc{};
    return SUCCEEDED(static_cast<IDXGISwapChain*>(chain)->GetDesc(&desc)) ? desc.OutputWindow : nullptr;
}

void* create_renderer(void* device, void* queue, void** fence) noexcept try {
    auto renderer=std::make_unique<Renderer>();
    return initialize(*renderer, static_cast<ID3D12Device*>(device), static_cast<ID3D12CommandQueue*>(queue), fence)
        ? renderer.release() : nullptr;
} catch (...) { return nullptr; }

uint32_t install_input(void* window, rsf_overlay12_log logger, void* user) noexcept try {
    const rsf_overlay_input_options input{sizeof(input),RSF_OVERLAY_INPUT_ABI_VERSION,0,logger,user};
    return rsf_overlay_input_install(window, &input) == RSF_OVERLAY_INPUT_OK;
} catch (...) { return 0; }

uint32_t stop_input() {
    const auto result=rsf_overlay_input_uninstall();
    return result==RSF_OVERLAY_INPUT_OK || result==RSF_OVERLAY_INPUT_ERROR_NOT_INSTALLED;
}

void destroy_renderer(void* native) {
    delete static_cast<Renderer*>(native);
}

uint64_t completed(void* fence) {
    return static_cast<ID3D12Fence*>(fence)->GetCompletedValue();
}

uint32_t arm(void* fence, void* event, uint64_t value) {
    return SUCCEEDED(static_cast<ID3D12Fence*>(fence)->SetEventOnCompletion(value,event));
}

void* begin_frame(const rsf_overlay12_frame* slot, void* chain, rsf_overlay12_target* out) noexcept try {
    const auto frame=borrow(slot);
    if (FAILED(frame.allocator->Reset())||FAILED(frame.commands->Reset(frame.allocator,nullptr))) return nullptr;
    ComPtr<IDXGISwapChain3> swapchain;ComPtr<ID3D12Resource> target;
    if (FAILED(static_cast<IDXGISwapChain*>(chain)->QueryInterface(IID_PPV_ARGS(&swapchain)))||
        FAILED(swapchain->GetBuffer(swapchain->GetCurrentBackBufferIndex(),IID_PPV_ARGS(&target)))) return nullptr;
    const auto desc=target->GetDesc();
    if (!desc.Width || desc.Width>UINT_MAX || !desc.Height) return nullptr;
    *out={static_cast<uint32_t>(desc.Width),desc.Height,static_cast<uint32_t>(desc.Format)};
    return target.Detach();
} catch (...) { return nullptr; }

uint32_t collect_input(rsf_overlay_input* input, uint32_t width, uint32_t height, float elapsed) noexcept try {
    return rsf_overlay_input_collect(input, width, height, elapsed) == RSF_OVERLAY_INPUT_OK;
} catch (...) { return 0; }

uint32_t record(void* native, const rsf_overlay12_frame* slot, const rsf_overlay12_draw* binding,
    void* const* textures, uint32_t texture_count, const rsf_overlay12_draw_call* calls, uint32_t call_count) noexcept try {
    auto& renderer=*static_cast<Renderer*>(native);
    const auto frame=borrow(slot);
    return draw(renderer,frame,*binding,textures,texture_count,calls,call_count) &&
        SUCCEEDED(frame.commands->Close());
} catch (...) { return 0; }

void execute(void* native, const rsf_overlay12_frame* slot) {
    auto& renderer=*static_cast<Renderer*>(native);
    ID3D12CommandList* lists[]{borrow(slot).commands};
    renderer.queue->ExecuteCommandLists(1,lists);
}

uint32_t signal(void* native, void* fence, uint64_t value) {
    return SUCCEEDED(static_cast<Renderer*>(native)->queue->Signal(static_cast<ID3D12Fence*>(fence),value));
}

constexpr rsf_overlay12_platform platform{
    output_window, create_renderer, create_frame, install_input, stop_input, destroy_renderer, rsf_overlay_input_visible,
    rsf_overlay_input_set_visible, completed, arm, begin_frame, create_pipeline, create_upload, collect_input,
    create_texture, upload_texture, record, execute, signal,
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
