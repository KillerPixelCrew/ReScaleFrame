// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/overlay_d3d12.h>
#include <rescaleframe/overlay_input.h>
#include <windows.h>
#include <d3d12.h>
#include <d3dcompiler.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <algorithm>
#include <array>
#include <atomic>
#include <cstring>
#include <mutex>
#include <string>
#include <unordered_map>
#include <vector>
namespace {
using Microsoft::WRL::ComPtr;
constexpr UINT kTextures = 64;
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
    std::vector<ComPtr<IUnknown>> keep;
    uint64_t completion{};
};
struct Texture { ComPtr<ID3D12Resource> resource; };
struct Panel {
    std::mutex guard;
    std::atomic<bool> ready{};
    bool failed{};
    HMODULE module{};
    rsf_overlay* ui{};
    decltype(&rsf_overlay_frame) layout{};
    decltype(&rsf_overlay_texture_updates) updates{};
    decltype(&rsf_overlay_textures_to_free) frees{};
    ComPtr<ID3D12Device> device;
    ComPtr<ID3D12CommandQueue> queue;
    ComPtr<ID3D12Fence> fence;
    ComPtr<ID3D12RootSignature> root;
    ComPtr<ID3D12PipelineState> pipeline;
    ComPtr<ID3DBlob> vertex_shader, pixel_shader;
    DXGI_FORMAT format{DXGI_FORMAT_UNKNOWN};
    std::array<Frame,3> frames;
    std::unordered_map<uint64_t,Texture> textures;
    uint64_t serial{}, started{}, previous{};
    UINT stride{}, cursor{};
    bool was_visible{};
    unsigned trace_left{};
    void (*logger)(void*,const char*){};
    void* user{};
    void log(const char* message) { if (logger) logger(user,message); }
};
Panel& panel() { static auto* value = new Panel; return *value; }
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
bool initialize(Panel& p, ID3D12Device* device, ID3D12CommandQueue* queue, HWND window)
{
    wchar_t path[32768]{};
    if (!GetEnvironmentVariableW(L"RSF_OVERLAY_DLL",path,32768)) {
        HMODULE own{};
        if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            reinterpret_cast<LPCWSTR>(&panel),&own) || !GetModuleFileNameW(own,path,32768)) return false;
        std::wstring file=path;const auto slash=file.find_last_of(L"/\\");
        if (slash==std::wstring::npos) return false;
        file.resize(slash+1);file+=L"rescaleframe_overlay.dll";
        if (file.size()>=32768) return false;
        std::memcpy(path,file.c_str(),(file.size()+1)*sizeof(wchar_t));
    }
    p.module=LoadLibraryW(path);if (!p.module) return false;
    const auto create=reinterpret_cast<decltype(&rsf_overlay_create)>(GetProcAddress(p.module,"rsf_overlay_create"));
    p.layout=reinterpret_cast<decltype(p.layout)>(GetProcAddress(p.module,"rsf_overlay_frame"));
    p.updates=reinterpret_cast<decltype(p.updates)>(GetProcAddress(p.module,"rsf_overlay_texture_updates"));
    p.frees=reinterpret_cast<decltype(p.frees)>(GetProcAddress(p.module,"rsf_overlay_textures_to_free"));
    if (!create||!p.layout||!p.updates||!p.frees || !(p.ui=create(RSF_OVERLAY_ABI_VERSION))) return false;
    p.device=device;p.queue=queue;p.stride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    if (FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&p.fence)))) return false;
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
        heap.NumDescriptors=kTextures;heap.Flags=D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
        if (FAILED(device->CreateDescriptorHeap(&heap,IID_PPV_ARGS(&frame.srv)))) return false;
        heap.Type=D3D12_DESCRIPTOR_HEAP_TYPE_RTV;heap.NumDescriptors=1;heap.Flags=D3D12_DESCRIPTOR_HEAP_FLAG_NONE;
        if (FAILED(device->CreateDescriptorHeap(&heap,IID_PPV_ARGS(&frame.rtv)))) return false;
    }
    const rsf_overlay_input_options input{sizeof(input),RSF_OVERLAY_INPUT_ABI_VERSION,0,p.logger,p.user};
    if (rsf_overlay_input_install(window,&input)!=RSF_OVERLAY_INPUT_OK) return false;
    p.started=p.previous=GetTickCount64();p.ready=true;
    p.log("RSF native D3D12 overlay ready; Insert opens settings. No D3D11-on-12 resources.");
    return true;
}
bool pipeline(Panel& p, Frame& frame, DXGI_FORMAT format)
{
    if (p.pipeline && p.format==format) return true;
    if (p.pipeline) frame.keep.emplace_back(p.pipeline);
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
bool texture(Panel& p, Frame& frame, const rsf_overlay_texture_update& update)
{
    if (!update.pixels||!update.width||!update.height||update.width>16384||update.height>16384) return false;
    auto found=p.textures.find(update.id);
    bool fresh=update.is_whole_texture!=0;
    if (fresh) {
        if (found==p.textures.end()&&p.textures.size()>=kTextures) return false;
        D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;
        desc.Width=update.width;desc.Height=update.height;desc.DepthOrArraySize=1;desc.MipLevels=1;
        desc.Format=DXGI_FORMAT_R8G8B8A8_UNORM;desc.SampleDesc.Count=1;
        D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
        ComPtr<ID3D12Resource> created;
        if (FAILED(p.device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&created)))) return false;
        auto& current=p.textures[update.id];if (current.resource) frame.keep.emplace_back(current.resource);
        current.resource=created;found=p.textures.find(update.id);
    }
    if (found==p.textures.end()) return false;
    auto* destination=found->second.resource.Get();auto desc=destination->GetDesc();
    if (uint64_t(update.x)+update.width>desc.Width||uint64_t(update.y)+update.height>desc.Height) return false;
    desc.Width=update.width;desc.Height=update.height;
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{};UINT64 bytes{};
    p.device->GetCopyableFootprints(&desc,0,1,0,&footprint,nullptr,nullptr,&bytes);
    ComPtr<ID3D12Resource> upload;if (!upload_buffer(p,bytes,upload)) return false;
    void* mapped{};const D3D12_RANGE empty{0,0};if (FAILED(upload->Map(0,&empty,&mapped))) return false;
    for (UINT y=0;y<update.height;++y) std::memcpy(static_cast<unsigned char*>(mapped)+uint64_t(y)*footprint.Footprint.RowPitch,
        update.pixels+uint64_t(y)*update.width*4,uint64_t(update.width)*4);
    upload->Unmap(0,nullptr);
    if (!fresh) transition(frame.commands.Get(),destination,D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_COPY_DEST);
    D3D12_TEXTURE_COPY_LOCATION source{};source.pResource=upload.Get();source.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;source.PlacedFootprint=footprint;
    D3D12_TEXTURE_COPY_LOCATION target{};target.pResource=destination;target.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    frame.commands->CopyTextureRegion(&target,update.x,update.y,0,&source,nullptr);
    transition(frame.commands.Get(),destination,D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
    frame.keep.emplace_back(upload);return true;
}
bool draw(Panel& p, Frame& frame, ID3D12Resource* target, const rsf_overlay_draw_data& data, UINT width, UINT height)
{
    if (!data.vertex_count||!data.index_count) return true;
    const uint64_t vertex_bytes=uint64_t(data.vertex_count)*sizeof(rsf_overlay_vertex), index_bytes=uint64_t(data.index_count)*4;
    if (vertex_bytes>UINT_MAX||index_bytes>UINT_MAX||!upload_buffer(p,vertex_bytes,frame.vertices)||!upload_buffer(p,index_bytes,frame.indices)) return false;
    void* mapped{};const D3D12_RANGE empty{0,0};
    if (FAILED(frame.vertices->Map(0,&empty,&mapped))) return false;
    std::memcpy(mapped,data.vertices,static_cast<size_t>(vertex_bytes));frame.vertices->Unmap(0,nullptr);
    if (FAILED(frame.indices->Map(0,&empty,&mapped))) return false;
    std::memcpy(mapped,data.indices,static_cast<size_t>(index_bytes));frame.indices->Unmap(0,nullptr);
    std::unordered_map<uint64_t,UINT> descriptors;UINT index{};
    for (const auto& [id,entry]:p.textures) {
        auto cpu=frame.srv->GetCPUDescriptorHandleForHeapStart();cpu.ptr+=uint64_t(index)*p.stride;
        D3D12_SHADER_RESOURCE_VIEW_DESC view{};view.Format=DXGI_FORMAT_R8G8B8A8_UNORM;
        view.ViewDimension=D3D12_SRV_DIMENSION_TEXTURE2D;view.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;view.Texture2D.MipLevels=1;
        p.device->CreateShaderResourceView(entry.resource.Get(),&view,cpu);descriptors[id]=index++;
    }
    auto* commands=frame.commands.Get();transition(commands,target,D3D12_RESOURCE_STATE_PRESENT,D3D12_RESOURCE_STATE_RENDER_TARGET);
    const auto rtv=frame.rtv->GetCPUDescriptorHandleForHeapStart();p.device->CreateRenderTargetView(target,nullptr,rtv);
    commands->OMSetRenderTargets(1,&rtv,FALSE,nullptr);commands->SetPipelineState(p.pipeline.Get());commands->SetGraphicsRootSignature(p.root.Get());
    ID3D12DescriptorHeap* heaps[]{frame.srv.Get()};commands->SetDescriptorHeaps(1,heaps);
    const float scale[]{2.0f/static_cast<float>(width),2.0f/static_cast<float>(height)};commands->SetGraphicsRoot32BitConstants(0,2,scale,0);
    const D3D12_VIEWPORT viewport{0,0,static_cast<float>(width),static_cast<float>(height),0,1};commands->RSSetViewports(1,&viewport);
    D3D12_VERTEX_BUFFER_VIEW vertices{frame.vertices->GetGPUVirtualAddress(),static_cast<UINT>(vertex_bytes),sizeof(rsf_overlay_vertex)};
    D3D12_INDEX_BUFFER_VIEW indices{frame.indices->GetGPUVirtualAddress(),static_cast<UINT>(index_bytes),DXGI_FORMAT_R32_UINT};
    commands->IASetVertexBuffers(0,1,&vertices);commands->IASetIndexBuffer(&indices);commands->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    for (UINT i=0;i<data.call_count;++i) {
        const auto& call=data.calls[i];const auto descriptor=descriptors.find(call.texture_id);
        if (descriptor==descriptors.end()||uint64_t(call.index_offset)+call.index_count>data.index_count||call.vertex_offset>data.vertex_count) return false;
        const D3D12_RECT clip{static_cast<LONG>(std::min(call.clip_x,width)),static_cast<LONG>(std::min(call.clip_y,height)),
            static_cast<LONG>(std::min(uint64_t(call.clip_x)+call.clip_width,uint64_t(width))),static_cast<LONG>(std::min(uint64_t(call.clip_y)+call.clip_height,uint64_t(height)))};
        commands->RSSetScissorRects(1,&clip);auto gpu=frame.srv->GetGPUDescriptorHandleForHeapStart();gpu.ptr+=uint64_t(descriptor->second)*p.stride;
        commands->SetGraphicsRootDescriptorTable(1,gpu);commands->DrawIndexedInstanced(call.index_count,1,call.index_offset,static_cast<INT>(call.vertex_offset),0);
    }
    transition(commands,target,D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_PRESENT);return true;
}
}
uint32_t rsf_overlay_native12_visible() { return panel().ready ? rsf_overlay_input_visible() : 0; }
void rsf_overlay_native12_toggle() { if (panel().ready) rsf_overlay_input_set_visible(rsf_overlay_input_visible()?0u:1u); }
int rsf_overlay_native12_present(void* device, void* queue, void* chain, const rsf_overlay_stats* stats,
    rsf_overlay_intent* intent, void (*logger)(void*,const char*), void* user) try
{
    auto& p=panel();std::unique_lock lock(p.guard,std::try_to_lock);
    if (!lock.owns_lock()||p.failed||!device||!queue||!chain||!stats) return 0;
    auto fail=[&] {p.log("Native D3D12 overlay refused graphics work; see validation evidence.");p.failed=true;rsf_overlay_input_set_visible(0);return 0;};
    if (!p.ready) {
        p.logger=logger;p.user=user;DXGI_SWAP_CHAIN_DESC desc{};
        if (FAILED(static_cast<IDXGISwapChain*>(chain)->GetDesc(&desc))||
            !initialize(p,static_cast<ID3D12Device*>(device),static_cast<ID3D12CommandQueue*>(queue),desc.OutputWindow)) return fail();
    }
    if (p.device.Get()!=device||p.queue.Get()!=queue) return 0;
    const auto now=GetTickCount64();if (!rsf_overlay_input_visible()&&!stats->show_performance_hud&&now-p.started>=8000) return 0;
    Frame* selected{};const auto completed=p.fence->GetCompletedValue();if (completed==UINT64_MAX) return fail();
    for (UINT i=0;i<p.frames.size();++i) {auto& candidate=p.frames[(p.cursor+i)%p.frames.size()];if (candidate.completion<=completed){selected=&candidate;p.cursor=(p.cursor+i+1)%static_cast<UINT>(p.frames.size());break;}}
    // A full ring is backpressure, not permission to omit this frame's UI.
    // Wait only for our oldest already-submitted list, whose Signal was queued
    // before returning from its Present callback. No unsubmitted engine work
    // or future Present is needed to satisfy this fence.
    if (!selected) {
        selected=&*std::min_element(p.frames.begin(),p.frames.end(),
            [](const Frame& a,const Frame& b){return a.completion<b.completion;});
        const auto event=CreateEventW(nullptr,FALSE,FALSE,nullptr);
        if (!event) return fail();
        const auto armed=p.fence->SetEventOnCompletion(selected->completion,event);
        const auto waited=SUCCEEDED(armed)?WaitForSingleObject(event,5000):WAIT_FAILED;
        CloseHandle(event);
        if (waited!=WAIT_OBJECT_0 || p.fence->GetCompletedValue()==UINT64_MAX) return fail();
        p.cursor=static_cast<UINT>(selected-p.frames.data()+1)%static_cast<UINT>(p.frames.size());
    }
    auto& frame=*selected;frame.keep.clear();
    if (FAILED(frame.allocator->Reset())||FAILED(frame.commands->Reset(frame.allocator.Get(),nullptr))) return fail();
    ComPtr<IDXGISwapChain3> swapchain;ComPtr<ID3D12Resource> target;
    if (FAILED(static_cast<IDXGISwapChain*>(chain)->QueryInterface(IID_PPV_ARGS(&swapchain)))||
        FAILED(swapchain->GetBuffer(swapchain->GetCurrentBackBufferIndex(),IID_PPV_ARGS(&target)))) return fail();
    const auto desc=target->GetDesc();const auto width=static_cast<UINT>(desc.Width),height=desc.Height;
    if (!pipeline(p,frame,desc.Format)) return fail();
    const bool visible=rsf_overlay_input_visible()!=0;
    if (visible&&!p.was_visible) p.trace_left=4;
    p.was_visible=visible;const bool trace=visible&&p.trace_left;
    if (trace) {--p.trace_left;p.log("Overlay12: input/layout begin");}
    rsf_overlay_input input{};input.struct_size=sizeof(input);
    if (rsf_overlay_input_collect(&input,width,height,static_cast<float>(now-p.previous)/1000)!=RSF_OVERLAY_INPUT_OK) return fail();
    p.previous=now;input.startup_hint_alpha=now-p.started<7000?1.0f:now-p.started<8000?static_cast<float>(8000-(now-p.started))/1000:0;
    rsf_overlay_draw_data data{};data.struct_size=sizeof(data);rsf_overlay_intent decided{};decided.struct_size=sizeof(decided);
    if (p.layout(p.ui,&input,stats,&data,&decided)!=RSF_OVERLAY_OK) return fail();
    if (trace) p.log("Overlay12: atlas uploads and draw recording");
    rsf_overlay_texture_update updates[32]{};
    for (UINT count;(count=p.updates(p.ui,updates,32))!=0;) for (UINT i=0;i<count;++i) if (!texture(p,frame,updates[i])) return fail();
    if (!draw(p,frame,target.Get(),data,width,height)) return fail();
    uint64_t ids[32]{};
    for (UINT count;(count=p.frees(p.ui,ids,32))!=0;) for (UINT i=0;i<count;++i) {
        const auto found=p.textures.find(ids[i]);if(found!=p.textures.end()){frame.keep.emplace_back(found->second.resource);p.textures.erase(found);}
    }
    if (FAILED(frame.commands->Close())) return fail();
    if (trace) p.log("Overlay12: queue submission begin");
    ID3D12CommandList* lists[]{frame.commands.Get()};p.queue->ExecuteCommandLists(1,lists);
    frame.completion=++p.serial;if (FAILED(p.queue->Signal(p.fence.Get(),frame.completion))) return fail();
    if (trace) p.log("Overlay12: submission completed");
    if (intent) *intent=decided;return 1;
} catch (...) { panel().failed=true;return 0; }
