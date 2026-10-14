// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/ac7_velocity_pass.h>
#include <rescaleframe/frame_tap.h>
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/shader_compile.h>
#include <rescaleframe/d3d11_observer.h>
#include "velocity_shader.h"
#include "contact_shadow.h"
#include <d3d11.h>
#include <wrl/client.h>
#include <algorithm>
#include <array>
#include <map>
#include <mutex>
#include <tuple>
#include <memory>
#include <cstring>
#include <cstdio>
using Microsoft::WRL::ComPtr;
namespace {
struct Code {uint32_t stage=0,hash=0;std::vector<uint8_t> bytes;};
std::mutex code_guard;
std::map<void*,Code> sources;
struct Layout {std::vector<rsf_observer_layout_element> elements;uint64_t serial=0;};
std::map<void*,Layout> layouts;uint64_t layout_serial=0;
size_t source_bytes=0;
thread_local rsf_game_render_pass current{};
void (*log_line)(void*,const char*)=nullptr;void* log_user=nullptr;
uint32_t refusal_reports=0,commit_reports=0;
void refused(const char* reason) {
    if (log_line && refusal_reports++<12) {
        char message[224]{};std::snprintf(message,sizeof(message),"AC7 translucent velocity refused: %s",reason);
        log_line(log_user,message);
    }
}
struct State {
    rsf_d3d11_state saved{};ID3D11DeviceContext* context=nullptr;
    explicit State(ID3D11DeviceContext* c):context(c) {if (!rsf_d3d11_state_save(c,&saved)) context=nullptr;}
    ~State() {if (context) rsf_d3d11_state_restore(context,&saved);}
};
struct Buffer {
    ComPtr<ID3D11Buffer> buffer;ComPtr<ID3D11ShaderResourceView> view;ComPtr<ID3D11UnorderedAccessView> write;
    uint32_t count=0;
};
bool raw_buffer(ID3D11Device* device,uint32_t records,bool output,Buffer& result)
{
    if (!records || records>UINT32_MAX/64) return false;
    Buffer made;D3D11_BUFFER_DESC desc{};desc.ByteWidth=records*64;
    desc.BindFlags=D3D11_BIND_SHADER_RESOURCE|D3D11_BIND_UNORDERED_ACCESS|(output ? D3D11_BIND_STREAM_OUTPUT : 0u);
    desc.MiscFlags=D3D11_RESOURCE_MISC_BUFFER_ALLOW_RAW_VIEWS;
    D3D11_SHADER_RESOURCE_VIEW_DESC srv{};srv.Format=DXGI_FORMAT_R32_TYPELESS;srv.ViewDimension=D3D11_SRV_DIMENSION_BUFFEREX;
    srv.BufferEx.NumElements=records*16;srv.BufferEx.Flags=D3D11_BUFFEREX_SRV_FLAG_RAW;
    D3D11_UNORDERED_ACCESS_VIEW_DESC uav{};uav.Format=DXGI_FORMAT_R32_TYPELESS;uav.ViewDimension=D3D11_UAV_DIMENSION_BUFFER;
    uav.Buffer.NumElements=records*16;uav.Buffer.Flags=D3D11_BUFFER_UAV_FLAG_RAW;
    const HRESULT created=device->CreateBuffer(&desc,nullptr,&made.buffer);
    if (FAILED(created)) {char reason[112]{};std::snprintf(reason,sizeof(reason),"buffer flags=%u misc=%u bytes=%u failed %08lx",desc.BindFlags,desc.MiscFlags,desc.ByteWidth,static_cast<unsigned long>(created));refused(reason);return false;}
    if (FAILED(device->CreateShaderResourceView(made.buffer.Get(),&srv,&made.view)) ||
        FAILED(device->CreateUnorderedAccessView(made.buffer.Get(),&uav,&made.write))) return false;
    made.count=records;result=std::move(made);return true;
}
struct Parameters {
    float rect[4]{},jitter[4]{};float cutoff=0;uint32_t history=0,mask=0,padding=0;
    float matrix[16]{};
};
static_assert(sizeof(Parameters)==112);
constexpr char hash_source[]=R"(
ByteAddressBuffer Source:register(t0);RWByteAddressBuffer Destination:register(u0);
cbuffer Parameters:register(b0) {uint Count;uint Mask;uint Mode;uint Padding;};
uint key_hash(uint4 key) {
    uint value=2166136261;
    [unroll]for(uint i=0;i<4;i++)value=(value^key[i])*16777619;
    value^=value>>16;return max(1,value&0x7fffffff);
}
void insert(uint4 clip,uint4 key,uint3 centre) {
    float4 position=asfloat(clip);if(!all(isfinite(position)) || position.w<=0)return;
    uint hash=key_hash(key);
    [loop]for(uint probe=0;probe<32;probe++) {
        uint address=((hash+probe)&Mask)*64;uint old;
        Destination.InterlockedCompareExchange(address,0,0xffffffff,old);
        if(old==0) {
            Destination.Store4(address+4,key);Destination.Store4(address+20,clip);
            Destination.Store3(address+36,centre);Destination.Store(address+48,0);
            DeviceMemoryBarrier();Destination.Store(address,hash);return;
        }
        if(old==hash && all(Destination.Load4(address+4)==key) &&
           all(Destination.Load4(address+20)==clip) && all(Destination.Load3(address+36)==centre))return;
    }
}
[numthreads(64,1,1)]void main(uint3 thread:SV_DispatchThreadID) {
    if(thread.x>=Count)return;uint offset=thread.x*64;
    if(Mode!=0 && Source.Load(offset)==0)return;
    uint4 clip=Source.Load4(offset+(Mode==0 ? 0 : 20));
    uint4 key=Source.Load4(offset+(Mode==0 ? 16 : 4));
    uint3 centre=Source.Load3(offset+(Mode==0 ? 32 : 36));insert(clip,key,centre);
    uint a=(key.x&0x1fffffff)|((key.x>>2)&0x20000000);
    uint b=(key.y&0x1fffffff)|((key.y>>2)&0x20000000);
    if((key.w==asuint(4.f) || key.w==asuint(5.f)) && key.z==0 && centre.x==0 && a>>28!=0) {
        b=(b&~0x3ffffff)|(a&0x3ffffff);
        key.y=0x20000000|(b&0x1fffffff)|((b&0x20000000)<<2);insert(clip,key,centre);
    }
})";
constexpr char merge_source[]=R"(
Texture2D<float2> Native:register(t0);Texture2D<float2> Layer:register(t1);
float2 main(float4 pixel:SV_Position):SV_Target0 {
    int3 address=int3(int2(pixel.xy),0);float2 layer=Layer.Load(address);
    return layer.x>0 ? layer : Native.Load(address);
})";
constexpr char fullscreen_source[]=R"(
float4 main(uint id:SV_VertexID):SV_Position {return float4(id==2?3:-1,id==1?3:-1,0,1);}
)";
constexpr char indirect_source[]=R"(
RWByteAddressBuffer Arguments:register(u0);
cbuffer Parameters:register(b0) {uint Capacity;uint3 Padding;};
[numthreads(1,1,1)]void main(uint3 id:SV_DispatchThreadID) {
    uint vertices=Arguments.Load(0),instances=Arguments.Load(4);
    if(vertices==0 || instances>Capacity/vertices)Arguments.Store(4,0);
})";
struct Variant {
    ComPtr<ID3D11VertexShader> vertex;ComPtr<ID3D11PixelShader> pixel;
    ComPtr<ID3D11GeometryShader> capture,geometry;
    ComPtr<ID3D11HullShader> hull,corners;ComPtr<ID3D11DomainShader> domain;
    ComPtr<ID3D11InputLayout> layout;
    bool particle=false;
    uint32_t maximum_vertices=1,history_slot=0,geometry_hash=0,input_points=0;
};
using VariantKey=std::tuple<void*,void*,void*,void*,void*,void*,uint64_t,uint32_t,uint32_t,uint32_t,uint32_t,uint32_t,uint32_t>;
using ModelKey=std::tuple<uint64_t,uint64_t,uint64_t,uint64_t,uint32_t,uint64_t,uint32_t,int32_t,uint32_t>;
using ViewKey=std::tuple<uint64_t,uint64_t>;
struct Model {
    Buffer previous,now;uint32_t frame=UINT32_MAX,records=0;uint64_t epoch=0;bool usable=false;
};
struct View {
    uint32_t frame=UINT32_MAX,width=0,height=0;uint64_t family=0,view=0,epoch=0;
    ComPtr<ID3D11Texture2D> native_depth,depth,motion,merged;
    ComPtr<ID3D11DepthStencilView> depth_target;
    ComPtr<ID3D11RenderTargetView> motion_target,merged_target;
    ComPtr<ID3D11ShaderResourceView> motion_view;
    bool written=false;
};
struct Renderer {
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11Buffer> parameters,hash_parameters;
    ComPtr<ID3D11ComputeShader> hash;
    ComPtr<ID3D11ComputeShader> validate_indirect;
    ComPtr<ID3D11VertexShader> fullscreen;
    ComPtr<ID3D11PixelShader> merge;
    ComPtr<ID3D11DepthStencilState> depth_state;
    ComPtr<ID3D11BlendState> blend;
    ComPtr<ID3D11RasterizerState> raster;
    Buffer stream;
    Buffer indirect;
    ComPtr<ID3D11Buffer> capture_buffer;
    std::map<VariantKey,Variant> variants;
    std::map<std::vector<uint8_t>,ComPtr<ID3D11VertexShader>> vertex_shaders;
    std::map<std::vector<uint8_t>,ComPtr<ID3D11GeometryShader>> geometry_shaders,capture_shaders;
    std::map<ModelKey,Model> models;
    std::map<ViewKey,View> views;
    uint64_t epoch=1;
    explicit Renderer(ID3D11Device* d):device(d) {}
    bool initialize() {
        std::vector<uint8_t> bytes;
        if (!rsf::compile_shader(hash_source,"ac7-geometry-history","main","cs_5_0",bytes) ||
            FAILED(device->CreateComputeShader(bytes.data(),bytes.size(),nullptr,&hash)) ||
            !rsf::compile_shader(merge_source,"ac7-velocity-merge","main","ps_5_0",bytes) ||
            FAILED(device->CreatePixelShader(bytes.data(),bytes.size(),nullptr,&merge)) ||
            !rsf::compile_shader(fullscreen_source,"ac7-velocity-merge","main","vs_5_0",bytes) ||
            FAILED(device->CreateVertexShader(bytes.data(),bytes.size(),nullptr,&fullscreen))) return false;
        if (!rsf::compile_shader(indirect_source,"ac7-indirect-geometry-bounds","main","cs_5_0",bytes) ||
            FAILED(device->CreateComputeShader(bytes.data(),bytes.size(),nullptr,&validate_indirect))) return false;
        D3D11_BUFFER_DESC desc{};desc.ByteWidth=sizeof(Parameters);desc.BindFlags=D3D11_BIND_CONSTANT_BUFFER;
        if (FAILED(device->CreateBuffer(&desc,nullptr,&parameters))) return false;
        desc.ByteWidth=16;if (FAILED(device->CreateBuffer(&desc,nullptr,&hash_parameters))) return false;
        D3D11_BUFFER_DESC arguments{};arguments.ByteWidth=64;
        arguments.BindFlags=D3D11_BIND_SHADER_RESOURCE|D3D11_BIND_UNORDERED_ACCESS;
        arguments.MiscFlags=D3D11_RESOURCE_MISC_BUFFER_ALLOW_RAW_VIEWS|D3D11_RESOURCE_MISC_DRAWINDIRECT_ARGS;
        D3D11_UNORDERED_ACCESS_VIEW_DESC arguments_view{};arguments_view.Format=DXGI_FORMAT_R32_TYPELESS;
        arguments_view.ViewDimension=D3D11_UAV_DIMENSION_BUFFER;arguments_view.Buffer.NumElements=16;
        arguments_view.Buffer.Flags=D3D11_BUFFER_UAV_FLAG_RAW;
        if (FAILED(device->CreateBuffer(&arguments,nullptr,&indirect.buffer)) ||
            FAILED(device->CreateUnorderedAccessView(indirect.buffer.Get(),&arguments_view,&indirect.write))) return false;
        D3D11_DEPTH_STENCIL_DESC depth_desc{};depth_desc.DepthEnable=TRUE;
        depth_desc.DepthWriteMask=D3D11_DEPTH_WRITE_MASK_ALL;depth_desc.DepthFunc=D3D11_COMPARISON_GREATER_EQUAL;
        D3D11_BLEND_DESC blend_desc{};blend_desc.RenderTarget[0].RenderTargetWriteMask=15;
        D3D11_RASTERIZER_DESC raster_desc{};raster_desc.FillMode=D3D11_FILL_SOLID;raster_desc.CullMode=D3D11_CULL_NONE;
        raster_desc.DepthClipEnable=TRUE;
        return SUCCEEDED(device->CreateDepthStencilState(&depth_desc,&depth_state)) &&
            SUCCEEDED(device->CreateBlendState(&blend_desc,&blend)) && SUCCEEDED(device->CreateRasterizerState(&raster_desc,&raster));
    }
    bool prepare_indirect(ID3D11DeviceContext* c,const rsf_frame_tap_geometry& g,uint32_t capacity) {
        auto* source=static_cast<ID3D11Buffer*>(g.indirect_buffer);
        if (!source || !capacity) return false;
        D3D11_BUFFER_DESC desc{};source->GetDesc(&desc);
        const uint32_t bytes=g.kind==5 ? 20u : 16u;
        if (g.indirect_offset>desc.ByteWidth || bytes>desc.ByteWidth-g.indirect_offset || g.indirect_offset%4) return false;
        ComPtr<ID3D11Device> owner;source->GetDevice(&owner);if (owner.Get()!=device.Get()) return false;
        const D3D11_BOX box{g.indirect_offset,0,0,g.indirect_offset+bytes,1,1};
        c->CopySubresourceRegion(indirect.buffer.Get(),0,0,0,0,source,0,&box);
        const uint32_t bounds[]{capacity,0,0,0};c->UpdateSubresource(hash_parameters.Get(),0,nullptr,bounds,0,0);
        ID3D11Buffer* cb=hash_parameters.Get();c->CSSetConstantBuffers(0,1,&cb);
        ID3D11UnorderedAccessView* uav=indirect.write.Get();c->CSSetUnorderedAccessViews(0,1,&uav,nullptr);
        c->CSSetShader(validate_indirect.Get(),nullptr,0);c->Dispatch(1,1,1);
        uav=nullptr;c->CSSetUnorderedAccessViews(0,1,&uav,nullptr);return true;
    }
    Variant* variant(const rsf_frame_tap_geometry& g,void* pixel,void* native_geometry,void* native_hull,void* native_domain,
        Code& vertex_source,Code& pixel_source) {
        Code geometry_source,hull_source,domain_source;Layout declaration;
        {
            std::lock_guard<std::mutex> guard(code_guard);
            auto v=sources.find(g.vertex_shader),p=sources.find(pixel);
            if (v==sources.end() || p==sources.end() || v->second.stage!=0 || p->second.stage!=1) return nullptr;
            vertex_source=v->second;pixel_source=p->second;
            if (const auto layout=layouts.find(g.input_layout);layout!=layouts.end()) declaration=layout->second;
            if (native_geometry) {
                const auto gs=sources.find(native_geometry);
                if (gs==sources.end() || gs->second.stage!=2) return nullptr;
                geometry_source=gs->second;
            }
            if (native_hull || native_domain) {
                const auto h=sources.find(native_hull),d=sources.find(native_domain);
                if (h==sources.end() || d==sources.end() || h->second.stage!=3 || d->second.stage!=4) return nullptr;
                hull_source=h->second;domain_source=d->second;
            }
        }
        const VariantKey key{g.vertex_shader,pixel,native_geometry,native_hull,native_domain,g.input_layout,declaration.serial,
            vertex_source.hash,pixel_source.hash,geometry_source.hash,hull_source.hash,domain_source.hash,current.material_blend};
        if (auto found=variants.find(key);found!=variants.end()) return found->second.vertex ? &found->second : nullptr;
        Variant made;rsf_ac7_velocity_vertex_info info;std::vector<uint8_t> vertex,geometry,pixel_code;uint32_t previous=0,current_clip=0;
        bool beam_layout=g.strides[0]==80 && declaration.elements.size()>=5 && declaration.elements.size()<=6;
        bool seen_fields[5]{};const uint32_t beam_offsets[]{0,16,32,64,48,0};
        for (const auto& e:declaration.elements) {
            if (e.semantic_index>5 || e.per_instance || e.input_slot!=(e.semantic_index==5 ? 1u : 0u) ||
                e.byte_offset!=beam_offsets[e.semantic_index] || e.format!=uint32_t(e.semantic_index==1 ? DXGI_FORMAT_R32G32B32_FLOAT : DXGI_FORMAT_R32G32B32A32_FLOAT)) {beam_layout=false;break;}
            if (e.semantic_index<5) {if (seen_fields[e.semantic_index]) {beam_layout=false;break;}seen_fields[e.semantic_index]=true;}
        }
        for (bool field:seen_fields) beam_layout=beam_layout && field;
        bool transformed=rsf_ac7_velocity_vertex_shader(vertex_source.bytes.data(),vertex_source.bytes.size(),vertex,info,native_hull!=nullptr,beam_layout)==rsf_ac7_velocity_shader_result::patched;
        if (transformed && info.particle==3) {
            // Beam/trail factories declare old position as float3. Only this independent layout
            // widens it to read the producer's fourth-word identity; native colour uses its layout.
            std::vector<D3D11_INPUT_ELEMENT_DESC> elements;
            transformed=!native_geometry && !native_hull;
            for (const auto& e:declaration.elements) {
                elements.push_back({"ATTRIBUTE",e.semantic_index,DXGI_FORMAT_R32G32B32A32_FLOAT,e.input_slot,e.byte_offset,D3D11_INPUT_PER_VERTEX_DATA,0});
            }
            if (transformed) transformed=SUCCEEDED(device->CreateInputLayout(elements.data(),UINT(elements.size()),vertex.data(),vertex.size(),&made.layout));
        }
        if (transformed && native_hull) {
            rsf_ac7_velocity_vertex_info h,d;std::vector<uint8_t> normal,corners;
            transformed=rsf_ac7_velocity_hull_shader(hull_source.bytes.data(),hull_source.bytes.size(),info,normal,corners,h,made.input_points)==rsf_ac7_velocity_shader_result::patched &&
                rsf_ac7_velocity_domain_shader(domain_source.bytes.data(),domain_source.bytes.size(),h,geometry,d,previous,current_clip)==rsf_ac7_velocity_shader_result::patched &&
                SUCCEEDED(device->CreateHullShader(normal.data(),normal.size(),nullptr,&made.hull)) &&
                SUCCEEDED(device->CreateHullShader(corners.data(),corners.size(),nullptr,&made.corners)) &&
                SUCCEEDED(device->CreateDomainShader(geometry.data(),geometry.size(),nullptr,&made.domain));
            info=std::move(d);made.history_slot=127;made.geometry_hash=hull_source.hash^(domain_source.hash<<1)^(domain_source.hash>>31);
            made.maximum_vertices=made.input_points ? (3u+made.input_points-1)/made.input_points : 1u;
        } else if (transformed && native_geometry) {
            rsf_ac7_velocity_vertex_info generated;
            transformed=rsf_ac7_velocity_native_geometry_shader(geometry_source.bytes.data(),geometry_source.bytes.size(),info,
                geometry,generated,made.maximum_vertices,previous,current_clip)==rsf_ac7_velocity_shader_result::patched;
            info=std::move(generated);made.history_slot=127;made.geometry_hash=geometry_source.hash;
        } else if (transformed) transformed=rsf_ac7_velocity_geometry_shader(info,geometry,previous,current_clip);
        if (transformed) {
            if (const auto cached=vertex_shaders.find(vertex);cached!=vertex_shaders.end()) made.vertex=cached->second;
            else if (SUCCEEDED(device->CreateVertexShader(vertex.data(),vertex.size(),nullptr,&made.vertex))) vertex_shaders.emplace(vertex,made.vertex);
            if (!native_hull) {
                if (const auto cached=geometry_shaders.find(geometry);cached!=geometry_shaders.end()) made.geometry=cached->second;
                else if (SUCCEEDED(device->CreateGeometryShader(geometry.data(),geometry.size(),nullptr,&made.geometry))) geometry_shaders.emplace(geometry,made.geometry);
            }
        }
        if (!transformed || !made.vertex || (!native_hull && !made.geometry) ||
            rsf_ac7_velocity_pixel_shader(pixel_source.bytes.data(),pixel_source.bytes.size(),current.material_blend,pixel_code,previous,current_clip)!=rsf_ac7_velocity_shader_result::patched ||
            FAILED(device->CreatePixelShader(pixel_code.data(),pixel_code.size(),nullptr,&made.pixel))) {
            refused("native material/vertex shader transform or D3D11 variant creation failed");variants.emplace(key,Variant{});return nullptr;
        }
        const auto position=std::find_if(info.outputs.begin(),info.outputs.end(),[](const auto& s) {return s.system==1;});
        if (position==info.outputs.end()) return nullptr;
        const D3D11_SO_DECLARATION_ENTRY declarations[]{
            {0,position->name.c_str(),position->index,0,4,0},
            {0,"RSFKEY",0,0,4,0},{0,"RSFCENTRE",0,0,4,0},{0,"RSFOLD",0,0,4,0},
        };
        const UINT stride=64;
        const auto& capture_code=native_geometry || native_hull ? geometry : vertex;
        if (const auto cached=capture_shaders.find(capture_code);cached!=capture_shaders.end()) made.capture=cached->second;
        else if (SUCCEEDED(device->CreateGeometryShaderWithStreamOutput(capture_code.data(),capture_code.size(),declarations,4,
            &stride,1,D3D11_SO_NO_RASTERIZED_STREAM,nullptr,&made.capture))) capture_shaders.emplace(capture_code,made.capture);
        if (!made.capture) {
            refused("stream-output signature creation failed");variants.emplace(key,Variant{});return nullptr;
        }
        made.particle=info.particle!=0;
        return &variants.emplace(key,std::move(made)).first->second;
    }
    void build(ID3D11DeviceContext* c,Buffer& source,Buffer& destination,uint32_t count,uint32_t mode) {
        const uint32_t params[]{count,destination.count-1,mode,0};
        c->UpdateSubresource(hash_parameters.Get(),0,nullptr,params,0,0);
        ID3D11Buffer* cb=hash_parameters.Get();c->CSSetConstantBuffers(0,1,&cb);
        ID3D11ShaderResourceView* srv=source.view.Get();c->CSSetShaderResources(0,1,&srv);
        ID3D11UnorderedAccessView* uav=destination.write.Get();c->CSSetUnorderedAccessViews(0,1,&uav,nullptr);
        c->CSSetShader(hash.Get(),nullptr,0);c->Dispatch((count+63)/64,1,1);
        srv=nullptr;uav=nullptr;c->CSSetShaderResources(0,1,&srv);c->CSSetUnorderedAccessViews(0,1,&uav,nullptr);
    }
    bool model(ID3D11DeviceContext* c,Model& model,uint32_t count) {
        const uint32_t frame=uint32_t(current.native_frame);
        if (model.frame!=frame) {
            model.usable=model.frame!=UINT32_MAX && uint32_t(model.frame+1)==frame && !(current.flags&RSF_GAME_RENDER_RESET);
            std::swap(model.previous,model.now);model.frame=frame;model.records=0;
            if (model.now.write) {const UINT zero[4]{};c->ClearUnorderedAccessViewUint(model.now.write.Get(),zero);}
        }
        model.epoch=epoch;
        if (count>UINT32_MAX-model.records || model.records+count>UINT32_MAX/128) return false;
        model.records+=count;uint32_t capacity=1024;
        while (capacity<model.records*2) {if (capacity>UINT32_MAX/2) return false;capacity*=2;}
        if (model.now.count<capacity) {
            Buffer larger;if (!raw_buffer(device.Get(),capacity,false,larger)) return false;
            const UINT zero[4]{};c->ClearUnorderedAccessViewUint(larger.write.Get(),zero);
            if (model.now.buffer) build(c,model.now,larger,model.now.count,1);
            model.now=std::move(larger);
        }
        return true;
    }
    bool view(ID3D11DeviceContext* c,const rsf_frame_tap_geometry& g,View& v) {
        auto* dsv=static_cast<ID3D11DepthStencilView*>(g.depth_view);
        ComPtr<ID3D11Resource> source;dsv->GetResource(&source);ComPtr<ID3D11Texture2D> depth;
        if (FAILED(source.As(&depth))) return false;
        D3D11_TEXTURE2D_DESC desc{};depth->GetDesc(&desc);
        if (desc.Width!=g.width || desc.Height!=g.height || desc.ArraySize!=1 || desc.MipLevels!=1 || desc.SampleDesc.Count!=1) return false;
        const uint32_t frame=uint32_t(current.native_frame);
        if (v.frame==frame) return v.native_depth.Get()==depth.Get();
        if (!v.depth || !v.depth_target || !v.motion || !v.motion_target || !v.motion_view || !v.merged ||
            !v.merged_target || v.width!=desc.Width || v.height!=desc.Height) {
            v={};v.width=desc.Width;v.height=desc.Height;
            D3D11_TEXTURE2D_DESC owned=desc;owned.Usage=D3D11_USAGE_DEFAULT;owned.CPUAccessFlags=owned.MiscFlags=0;
            owned.BindFlags=D3D11_BIND_DEPTH_STENCIL|D3D11_BIND_SHADER_RESOURCE;
            D3D11_DEPTH_STENCIL_VIEW_DESC depth_view{};dsv->GetDesc(&depth_view);depth_view.Flags=0;
            if (FAILED(device->CreateTexture2D(&owned,nullptr,&v.depth)) ||
                FAILED(device->CreateDepthStencilView(v.depth.Get(),&depth_view,&v.depth_target))) return false;
            owned.Format=DXGI_FORMAT_R16G16_UNORM;owned.BindFlags=D3D11_BIND_RENDER_TARGET|D3D11_BIND_SHADER_RESOURCE;
            if (FAILED(device->CreateTexture2D(&owned,nullptr,&v.motion)) ||
                FAILED(device->CreateRenderTargetView(v.motion.Get(),nullptr,&v.motion_target)) ||
                FAILED(device->CreateShaderResourceView(v.motion.Get(),nullptr,&v.motion_view)) ||
                FAILED(device->CreateTexture2D(&owned,nullptr,&v.merged)) ||
                FAILED(device->CreateRenderTargetView(v.merged.Get(),nullptr,&v.merged_target))) return false;
        }
        v.frame=frame;v.family=current.family_key;v.view=current.view_key;v.epoch=epoch;v.native_depth=depth;v.written=false;
        c->CopyResource(v.depth.Get(),depth.Get());const float zero[4]{};c->ClearRenderTargetView(v.motion_target.Get(),zero);
        return true;
    }
};
std::unique_ptr<Renderer> renderer;
void draw(ID3D11DeviceContext* c,const rsf_frame_tap_geometry& g,ID3D11Buffer* arguments=nullptr) {
    switch (g.kind) {
    case 0:c->Draw(g.count,g.start);break;
    case 1:c->DrawIndexed(g.count,g.start,g.base_vertex);break;
    case 2:c->DrawInstanced(g.count,g.instances,g.start,g.start_instance);break;
    case 3:c->DrawIndexedInstanced(g.count,g.instances,g.start,g.base_vertex,g.start_instance);break;
    case 4:c->DrawAuto();break;
    case 5:if (arguments) c->DrawIndexedInstancedIndirect(arguments,0);break;
    case 6:if (arguments) c->DrawInstancedIndirect(arguments,0);break;
    default:break;
    }
}
ViewKey view_key(const rsf_game_render_pass& p) {return {p.session_id,p.history_key ? p.history_key : p.view_key};}
}
extern "C" void rsf_ac7_velocity_shader_seen(void* shader,uint32_t stage,const void* bytes,uint32_t size) try
{
    if (!shader || !bytes || !size || stage>4 || size>1024*1024) return;
    std::lock_guard<std::mutex> guard(code_guard);
    if (auto old=sources.find(shader);old!=sources.end()) {source_bytes-=old->second.bytes.size();sources.erase(old);}
    if (source_bytes+size>64*1024*1024) return;
    Code code;code.stage=stage;code.hash=rsf_ac7_contact_shadow_crc(bytes,size);
    const auto* data=static_cast<const uint8_t*>(bytes);code.bytes.assign(data,data+size);source_bytes+=size;sources.emplace(shader,std::move(code));
} catch (...) {}
extern "C" void rsf_ac7_velocity_set_log(void (*log)(void*,const char*),void* user)
{log_line=log;log_user=user;refusal_reports=commit_reports=0;}
extern "C" void rsf_ac7_velocity_layout_seen(void* layout,const rsf_observer_layout_element* elements,uint32_t copied,uint32_t count) try
{
    if (!layout) return;std::lock_guard<std::mutex> guard(code_guard);layouts.erase(layout);
    if (!elements || copied!=count || count>16 || layouts.size()>=4096) return;
    Layout record;record.elements.assign(elements,elements+count);record.serial=++layout_serial;layouts.emplace(layout,std::move(record));
} catch (...) {}
extern "C" void rsf_ac7_velocity_scope(const rsf_game_render_pass* scope)
{current=scope && scope->struct_size>=sizeof(current) ? *scope : rsf_game_render_pass{};}
extern "C" void rsf_ac7_velocity_geometry(const rsf_frame_tap_geometry* pointer) try
{
    if (!pointer || current.role!=RSF_GAME_RENDER_TRANSLUCENT_MESH || !current.camera_valid ||
        !(current.flags&RSF_GAME_RENDER_PRIMARY)) return;
    const auto& g=*pointer;auto* c=static_cast<ID3D11DeviceContext*>(g.context);
    if (!c || c->GetType()!=D3D11_DEVICE_CONTEXT_IMMEDIATE || !g.depth_view || g.samples!=1 ||
        g.kind>6 || (g.kind<4 && !g.count)) return;
    ComPtr<ID3D11GeometryShader> existing;ComPtr<ID3D11HullShader> hull;ComPtr<ID3D11DomainShader> domain;
    c->GSGetShader(&existing,nullptr,nullptr);c->HSGetShader(&hull,nullptr,nullptr);c->DSGetShader(&domain,nullptr,nullptr);
    if ((hull && !domain) || (domain && !hull) || (hull && existing)) {refused("incomplete tessellation pipeline or combined native GS/tessellation requires a matching route");return;}
    if (!hull && g.topology!=D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST && g.topology!=D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP) return;
    ComPtr<ID3D11Device> device;c->GetDevice(&device);
    if (!renderer || renderer->device.Get()!=device.Get()) {
        auto made=std::make_unique<Renderer>(device.Get());if (!made->initialize()) {refused("device-side pass initialization failed");return;}renderer=std::move(made);
    }
    ComPtr<ID3D11PixelShader> native_pixel;c->PSGetShader(&native_pixel,nullptr,nullptr);
    Code vertex_code,pixel_code;auto* variant=renderer->variant(g,native_pixel.Get(),existing.Get(),hull.Get(),domain.Get(),vertex_code,pixel_code);
    if (!variant) return;
    if (hull && g.topology!=D3D11_PRIMITIVE_TOPOLOGY_1_CONTROL_POINT_PATCHLIST+variant->input_points-1) return;
    const uint64_t count64=g.kind>=4 ? uint64_t(current.geometry_capacity)*(hull ? variant->input_points : 3u)/3u :
        uint64_t(g.count)*(g.kind>=2 ? g.instances : 1u);
    // A triangle strip can expand to three SO records per emitted vertex. Input vertex
    // capacity already includes three vertices per primitive, so this bound covers it.
    const uint32_t expansion=variant->maximum_vertices*(g.topology==D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP ? 3u : 1u);
    if (!count64 || count64>uint64_t(UINT32_MAX/128)/expansion) return;
    const uint32_t count=uint32_t(count64*expansion);
    State saved(c);if (!saved.context) return;
    ComPtr<ID3D11Predicate> predicate;BOOL predicate_value=FALSE;c->GetPredication(&predicate,&predicate_value);
    c->SetPredication(nullptr,FALSE);
    if (g.kind>=5 && !renderer->prepare_indirect(c,g,uint32_t(count64))) {refused("indirect argument buffer or native mesh capacity is invalid");return;}
    auto& view=renderer->views[view_key(current)];if (!renderer->view(c,g,view)) {refused("native scene depth allocation/identity differs from the draw");return;}
    const ModelKey key{current.session_id,current.history_key ? current.history_key : current.view_key,
        current.primitive_key,current.material_key,vertex_code.hash,variant->particle ? uint64_t(0) : current.vertex_factory_key,
        variant->particle ? 0u : g.start,variant->particle ? 0 : g.base_vertex,variant->geometry_hash};
    auto& model=renderer->models[key];if (!renderer->model(c,model,count)) {refused("geometry-history allocation failed");return;}
    if (!renderer->capture_buffer || renderer->stream.count<count) {
        // Stream output and raw UAV storage have different D3D11 allocation contracts. Copy
        // completed generated vertices into shader-readable storage on the same GPU stream.
        Buffer readable;ComPtr<ID3D11Buffer> capture_buffer;
        if (!raw_buffer(device.Get(),count,false,readable)) {refused("generated-geometry read allocation failed");return;}
        D3D11_BUFFER_DESC capture{};capture.ByteWidth=readable.count*64;capture.BindFlags=D3D11_BIND_STREAM_OUTPUT;
        if (FAILED(device->CreateBuffer(&capture,nullptr,&capture_buffer))) {refused("stream-output allocation failed");return;}
        renderer->stream=std::move(readable);renderer->capture_buffer=std::move(capture_buffer);
    }
    const UINT zero[4]{};c->ClearUnorderedAccessViewUint(renderer->stream.write.Get(),zero);
    c->CopyResource(renderer->capture_buffer.Get(),renderer->stream.buffer.Get());
    if (existing || hull) {
        const Parameters empty{};c->UpdateSubresource(renderer->parameters.Get(),0,nullptr,&empty,0,0);
        ID3D11Buffer* cb=renderer->parameters.Get();c->GSSetConstantBuffers(13,1,&cb);
        ID3D11ShaderResourceView* unavailable=nullptr;c->GSSetShaderResources(127,1,&unavailable);
        c->DSSetConstantBuffers(13,1,&cb);c->DSSetShaderResources(127,1,&unavailable);
    }
    c->OMSetRenderTargets(0,nullptr,nullptr);c->VSSetShader(variant->vertex.Get(),nullptr,0);
    if (variant->layout) c->IASetInputLayout(variant->layout.Get());
    if (hull) {c->HSSetShader(variant->corners.Get(),nullptr,0);c->DSSetShader(variant->domain.Get(),nullptr,0);}
    c->GSSetShader(variant->capture.Get(),nullptr,0);c->PSSetShader(nullptr,nullptr,0);
    ID3D11Buffer* stream=renderer->capture_buffer.Get();const UINT offset=0;c->SOSetTargets(1,&stream,&offset);
    c->SetPredication(predicate.Get(),predicate_value);draw(c,g,renderer->indirect.buffer.Get());c->SetPredication(nullptr,FALSE);c->SOSetTargets(0,nullptr,nullptr);
    c->CopyResource(renderer->stream.buffer.Get(),renderer->capture_buffer.Get());
    D3D11_VIEWPORT viewport{};UINT viewports=1;c->RSGetViewports(&viewports,&viewport);
    if (viewports!=1 || viewport.Width<=0 || viewport.Height<=0) return;
    Parameters parameters{};parameters.rect[0]=viewport.TopLeftX;parameters.rect[1]=viewport.TopLeftY;
    parameters.rect[2]=viewport.Width;parameters.rect[3]=viewport.Height;
    parameters.jitter[0]=current.camera.jitter_pixels[0]*2/viewport.Width;
    parameters.jitter[1]=current.camera.jitter_pixels[1]*-2/viewport.Height;
    parameters.jitter[2]=current.camera.previous_jitter_pixels[0]*2/viewport.Width;
    parameters.jitter[3]=current.camera.previous_jitter_pixels[1]*-2/viewport.Height;
    // UE4.27 clips translucent/additive/modulate opacity, while AlphaComposite has no
    // opacity discard in GetMaterialClippingVelocity. Its native opacity is saturated.
    parameters.cutoff=current.material_blend==5 ? -1.f : current.material_opacity_clip+1.f/255.f;
    parameters.history=model.usable && model.previous.buffer ? 1u : 0u;
    parameters.mask=model.previous.count ? model.previous.count-1 : 0;
    std::memcpy(parameters.matrix,current.camera.clip_to_previous_clip,sizeof(parameters.matrix));
    c->UpdateSubresource(renderer->parameters.Get(),0,nullptr,&parameters,0,0);
    ID3D11Buffer* cb=renderer->parameters.Get();c->GSSetConstantBuffers(13,1,&cb);c->PSSetConstantBuffers(13,1,&cb);
    ID3D11ShaderResourceView* previous=model.previous.view.Get();c->GSSetShaderResources(variant->history_slot,1,&previous);
    if (hull) {
        c->HSSetShader(variant->hull.Get(),nullptr,0);c->DSSetShader(variant->domain.Get(),nullptr,0);
        c->DSSetConstantBuffers(13,1,&cb);c->DSSetShaderResources(127,1,&previous);
    }
    c->GSSetShader(variant->geometry.Get(),nullptr,0);c->PSSetShader(variant->pixel.Get(),nullptr,0);
    ID3D11RenderTargetView* rt=view.motion_target.Get();c->OMSetRenderTargets(1,&rt,view.depth_target.Get());
    c->OMSetDepthStencilState(renderer->depth_state.Get(),0);c->OMSetBlendState(renderer->blend.Get(),nullptr,UINT32_MAX);
    c->SetPredication(predicate.Get(),predicate_value);draw(c,g,renderer->indirect.buffer.Get());c->SetPredication(nullptr,FALSE);view.written=true;
    previous=nullptr;c->GSSetShaderResources(variant->history_slot,1,&previous);c->OMSetRenderTargets(0,nullptr,nullptr);
    if (hull) c->DSSetShaderResources(127,1,&previous);
    renderer->build(c,renderer->stream,model.now,count,0);
} catch (...) {}
extern "C" int rsf_ac7_velocity_commit(void* context,const rsf_game_render_pass* pass) try
{
    if (!renderer || !context || !pass || pass->struct_size<sizeof(*pass) || !pass->motion || !pass->depth) return 0;
    const auto found=renderer->views.find(view_key(*pass));if (found==renderer->views.end()) return 0;
    auto& view=found->second;
    if (!view.written || view.frame!=uint32_t(pass->native_frame) || view.view!=pass->view_key || view.family!=pass->family_key ||
        view.native_depth.Get()!=pass->depth) return 0;
    auto* c=static_cast<ID3D11DeviceContext*>(context);auto* native=static_cast<ID3D11Texture2D*>(pass->motion);
    D3D11_TEXTURE2D_DESC desc{};native->GetDesc(&desc);
    if (desc.Width!=view.width || desc.Height!=view.height || desc.SampleDesc.Count!=1 || desc.ArraySize!=1 ||
        (desc.Format!=DXGI_FORMAT_R16G16_UNORM && desc.Format!=DXGI_FORMAT_R16G16_TYPELESS)) return 0;
    ComPtr<ID3D11Device> device;c->GetDevice(&device);if (device.Get()!=renderer->device.Get()) return 0;
    State saved(c);if (!saved.context) return 0;c->SetPredication(nullptr,FALSE);
    c->SOSetTargets(0,nullptr,nullptr);c->GSSetShader(nullptr,nullptr,0);c->HSSetShader(nullptr,nullptr,0);c->DSSetShader(nullptr,nullptr,0);
    c->VSSetShader(renderer->fullscreen.Get(),nullptr,0);c->PSSetShader(renderer->merge.Get(),nullptr,0);
    c->OMSetDepthStencilState(nullptr,0);c->OMSetBlendState(renderer->blend.Get(),nullptr,UINT32_MAX);
    c->RSSetState(renderer->raster.Get());D3D11_VIEWPORT viewport{0,0,float(view.width),float(view.height),0,1};c->RSSetViewports(1,&viewport);
    c->IASetInputLayout(nullptr);c->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    D3D11_SHADER_RESOURCE_VIEW_DESC srv{};srv.Format=DXGI_FORMAT_R16G16_UNORM;srv.ViewDimension=D3D11_SRV_DIMENSION_TEXTURE2D;srv.Texture2D.MipLevels=1;
    ComPtr<ID3D11ShaderResourceView> native_view;if (FAILED(device->CreateShaderResourceView(native,&srv,&native_view))) return 0;
    ID3D11ShaderResourceView* views[]{native_view.Get(),view.motion_view.Get()};c->PSSetShaderResources(0,2,views);
    ID3D11RenderTargetView* rt=view.merged_target.Get();c->OMSetRenderTargets(1,&rt,nullptr);c->Draw(3,0);
    c->OMSetRenderTargets(0,nullptr,nullptr);
    ID3D11ShaderResourceView* empty[128]{};
    c->VSSetShaderResources(0,128,empty);c->PSSetShaderResources(0,128,empty);c->GSSetShaderResources(0,128,empty);
    c->HSSetShaderResources(0,128,empty);c->DSSetShaderResources(0,128,empty);c->CSSetShaderResources(0,128,empty);
    c->CopyResource(native,view.merged.Get());c->CopyResource(view.native_depth.Get(),view.depth.Get());
    view.written=false;
    if (log_line && commit_reports++<3) log_line(log_user,"AC7 translucent velocity: opacity-clipped generated-geometry motion and depth committed before native post-processing");
    return 1;
} catch (...) {return 0;}
extern "C" void rsf_ac7_velocity_frame_end(void)
{
    if (!renderer) return;++renderer->epoch;
    std::erase_if(renderer->models,[&](const auto& entry) {return entry.second.epoch+2<renderer->epoch;});
    std::erase_if(renderer->views,[&](const auto& entry) {return entry.second.epoch+2<renderer->epoch;});
}
extern "C" void rsf_ac7_velocity_release(void)
{renderer.reset();current={};std::lock_guard<std::mutex> guard(code_guard);sources.clear();layouts.clear();source_bytes=0;}
