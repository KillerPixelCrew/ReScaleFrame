// SPDX-License-Identifier: MIT
#include <rescaleframe/ac7_velocity_pass.h>
#include <rescaleframe/frame_tap.h>
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/shader_compile.h>
#include <rescaleframe/d3d11_observer.h>
#include "velocity_shader.h"
#include "velocity_pass_cache.h"
#include "velocity_history.h"
#include "velocity_stream.h"
#include "velocity_commit.h"
#include "velocity_renderer.h"
#include <d3d11.h>
#include <wrl/client.h>
#include <algorithm>
#include <array>
#include <map>
#include <span>
#include <memory>
#include <mutex>
#include <cstdio>
using Microsoft::WRL::ComPtr;
namespace {
thread_local rsf_game_render_pass current{};
thread_local bool merging=false;
// A native draw borrows a renderer, variant and history slots for the entire replay.
// The frame tap suppresses nested draw observation. Shader/layout creation notifications
// use their own Rust cache and never acquire this lock.
std::mutex replay_mutex;
struct MergeDraw {
    MergeDraw() {merging=true;}
    ~MergeDraw() {merging=false;}
};
void (*log_line)(void*,const char*)=nullptr;void* log_user=nullptr;
// The refusal and commit log budgets are velocity_commit.rs.
void refused(const char* reason) {
    if (log_line && rsf_ac7_velocity_report(VELOCITY_REPORT_REFUSAL)) {
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
// Draw admission, record bounds, buffer sizing, the argument range and both cbuffers' contents
// are velocity_stream.rs; VelocityDrawParameters is the generated shaders' b13 layout.
struct Variant {
    ComPtr<ID3D11VertexShader> vertex;ComPtr<ID3D11PixelShader> pixel;
    ComPtr<ID3D11GeometryShader> capture,geometry;
    ComPtr<ID3D11HullShader> hull,corners;ComPtr<ID3D11DomainShader> domain;
    ComPtr<ID3D11InputLayout> layout;
    bool particle=false;
    uint32_t maximum_vertices=1,history_slot=0,geometry_hash=0,input_points=0;
};
// Identity, frame, validity, records, written state and epochs are velocity_history.rs; these
// hold the native resources at its slots.
struct Model {
    Buffer previous,now;
};
struct View {
    ComPtr<ID3D11Texture2D> native_depth,depth,motion,merged;
    ComPtr<ID3D11DepthStencilView> depth_target;
    ComPtr<ID3D11RenderTargetView> motion_target,merged_target;
    ComPtr<ID3D11ShaderResourceView> motion_view;
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
    Buffer captured;
    // Indexed by the slots velocity_pass_cache.rs records; refusals have no entry here.
    std::vector<Variant> variants;
    std::map<std::vector<uint8_t>,ComPtr<ID3D11VertexShader>> vertex_shaders;
    std::map<std::vector<uint8_t>,ComPtr<ID3D11PixelShader>> pixel_shaders;
    std::map<std::vector<uint8_t>,ComPtr<ID3D11HullShader>> hull_shaders;
    std::map<std::vector<uint8_t>,ComPtr<ID3D11DomainShader>> domain_shaders;
    std::map<std::vector<uint8_t>,ComPtr<ID3D11GeometryShader>> geometry_shaders,capture_shaders;
    std::vector<Model> models;
    std::vector<View> views;
    // The frame's CPU profile is velocity_commit.rs; this counts the device's shader objects.
    uint32_t shader_objects=0;
    explicit Renderer(ID3D11Device* d):device(d) {}
    bool initialize() {
        std::vector<uint8_t> bytes;
        if (!rsf::compile_shader(rsf_core_ac7_velocity_history_source(),"ac7-geometry-history","main","cs_5_0",bytes) ||
            FAILED(device->CreateComputeShader(bytes.data(),bytes.size(),nullptr,&hash)) ||
            !rsf::compile_shader(rsf_ac7_velocity_merge_source(),"ac7-velocity-merge","main","ps_5_0",bytes) ||
            FAILED(device->CreatePixelShader(bytes.data(),bytes.size(),nullptr,&merge)) ||
            !rsf::compile_shader(rsf_ac7_velocity_fullscreen_source(),"ac7-velocity-merge","main","vs_5_0",bytes) ||
            FAILED(device->CreateVertexShader(bytes.data(),bytes.size(),nullptr,&fullscreen))) return false;
        if (!rsf::compile_shader(rsf_ac7_velocity_indirect_source(),"ac7-indirect-geometry-bounds","main","cs_5_0",bytes) ||
            FAILED(device->CreateComputeShader(bytes.data(),bytes.size(),nullptr,&validate_indirect))) return false;
        D3D11_BUFFER_DESC desc{};desc.ByteWidth=sizeof(VelocityDrawParameters);desc.BindFlags=D3D11_BIND_CONSTANT_BUFFER;
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
        D3D11_SHADER_RESOURCE_VIEW_DESC argument_source{};argument_source.Format=DXGI_FORMAT_R32_TYPELESS;
        argument_source.ViewDimension=D3D11_SRV_DIMENSION_BUFFEREX;argument_source.BufferEx.NumElements=16;
        argument_source.BufferEx.Flags=D3D11_BUFFEREX_SRV_FLAG_RAW;
        if (FAILED(device->CreateShaderResourceView(indirect.buffer.Get(),&argument_source,&indirect.view))) return false;
        D3D11_DEPTH_STENCIL_DESC depth_desc{};depth_desc.DepthEnable=TRUE;
        depth_desc.DepthWriteMask=D3D11_DEPTH_WRITE_MASK_ALL;depth_desc.DepthFunc=D3D11_COMPARISON_GREATER_EQUAL;
        D3D11_BLEND_DESC blend_desc{};blend_desc.RenderTarget[0].RenderTargetWriteMask=15;
        D3D11_RASTERIZER_DESC raster_desc{};raster_desc.FillMode=D3D11_FILL_SOLID;raster_desc.CullMode=D3D11_CULL_NONE;
        raster_desc.DepthClipEnable=TRUE;
        return SUCCEEDED(device->CreateDepthStencilState(&depth_desc,&depth_state)) &&
            SUCCEEDED(device->CreateBlendState(&blend_desc,&blend)) && SUCCEEDED(device->CreateRasterizerState(&raster_desc,&raster));
    }
    bool prepare_indirect(ID3D11DeviceContext* c,const rsf_frame_tap_geometry& g,const VelocityIndirectBounds& bounds) {
        auto* source=static_cast<ID3D11Buffer*>(g.indirect_buffer);
        if (!source || !bounds.vertices) return false;
        D3D11_BUFFER_DESC desc{};source->GetDesc(&desc);
        const uint32_t end=rsf_ac7_velocity_indirect_end(g.kind,g.indirect_offset,desc.ByteWidth);
        if (!end) return false;
        ComPtr<ID3D11Device> owner;source->GetDevice(&owner);if (owner.Get()!=device.Get()) return false;
        const D3D11_BOX box{g.indirect_offset,0,0,end,1,1};
        c->CopySubresourceRegion(indirect.buffer.Get(),0,0,0,0,source,0,&box);
        c->UpdateSubresource(hash_parameters.Get(),0,nullptr,&bounds,0,0);
        ID3D11Buffer* cb=hash_parameters.Get();c->CSSetConstantBuffers(0,1,&cb);
        ID3D11UnorderedAccessView* uav=indirect.write.Get();c->CSSetUnorderedAccessViews(0,1,&uav,nullptr);
        c->CSSetShader(validate_indirect.Get(),nullptr,0);c->Dispatch(1,1,1);
        uav=nullptr;c->CSSetUnorderedAccessViews(0,1,&uav,nullptr);return true;
    }
    // Source lookup, identity, reuse and the refusal cache are velocity_pass_cache.rs. It lends
    // retained bytecode to make_variant once per new key.
    struct VariantBuild {Renderer* owner;bool native_geometry,native_hull;};
    static int32_t build_variant(void* user,const VelocityVariantSources* sources,uint32_t* slot) noexcept try {
        const auto& context=*static_cast<const VariantBuild*>(user);
        return context.owner->make_variant(*sources,context.native_geometry,context.native_hull,*slot);
    } catch (...) {return VELOCITY_VARIANT_UNSTORED;}
    Variant* variant(const rsf_frame_tap_geometry& g,void* pixel,void* native_geometry,void* native_hull,void* native_domain,
        uint32_t& vertex_hash) {
        const VelocityVariantRequest request{{g.vertex_shader,pixel,native_geometry,native_hull,native_domain},g.input_layout,
            current.material_blend,g.strides[0]};
        VariantBuild context{this,native_geometry!=nullptr,native_hull!=nullptr};
        const uint32_t slot=rsf_ac7_velocity_admit_variant(&request,&context,build_variant,&vertex_hash);
        return slot<variants.size() ? &variants[slot] : nullptr;
    }
    int32_t make_variant(const VelocityVariantSources& lent,bool native_geometry,bool native_hull,uint32_t& slot) {
        const std::span<const rsf_observer_layout_element> declaration(lent.elements,lent.element_count);
        Variant made;rsf_ac7_velocity_vertex_info info;std::vector<uint8_t> vertex,geometry,pixel_code;uint32_t previous=0,current_clip=0;
        bool transformed=rsf_ac7_velocity_vertex_shader(lent.bytes[0],lent.sizes[0],vertex,info,native_hull,lent.beam_layout)==rsf_ac7_velocity_shader_result::patched;
        if (transformed && info.particle==3) {
            // Beam/trail factories declare old position as float3. Only this independent layout
            // widens it to read the producer's fourth-word identity; native colour uses its layout.
            std::vector<D3D11_INPUT_ELEMENT_DESC> elements;
            if (native_geometry || native_hull) return VELOCITY_VARIANT_REFUSED;
            for (const auto& e:declaration) {
                elements.push_back({"ATTRIBUTE",e.semantic_index,DXGI_FORMAT_R32G32B32A32_FLOAT,e.input_slot,e.byte_offset,D3D11_INPUT_PER_VERTEX_DATA,0});
            }
            if (FAILED(device->CreateInputLayout(elements.data(),UINT(elements.size()),vertex.data(),vertex.size(),&made.layout))) {
                refused("D3D11 beam layout creation failed");
                return VELOCITY_VARIANT_UNSTORED;
            }
        }
        if (transformed && native_hull) {
            rsf_ac7_velocity_vertex_info d;std::vector<uint8_t> normal,corners;
            transformed=rsf_ac7_velocity_tessellation(lent.bytes[3],lent.sizes[3],lent.bytes[4],lent.sizes[4],
                info,normal,corners,geometry,d,made.input_points,previous,current_clip)==rsf_ac7_velocity_shader_result::patched;
            if (transformed) {
                if (const auto cached=hull_shaders.find(normal);cached!=hull_shaders.end()) made.hull=cached->second;
                else if (SUCCEEDED(device->CreateHullShader(normal.data(),normal.size(),nullptr,&made.hull))) {hull_shaders.emplace(normal,made.hull);++shader_objects;}
                if (const auto cached=hull_shaders.find(corners);cached!=hull_shaders.end()) made.corners=cached->second;
                else if (SUCCEEDED(device->CreateHullShader(corners.data(),corners.size(),nullptr,&made.corners))) {hull_shaders.emplace(corners,made.corners);++shader_objects;}
                if (const auto cached=domain_shaders.find(geometry);cached!=domain_shaders.end()) made.domain=cached->second;
                else if (SUCCEEDED(device->CreateDomainShader(geometry.data(),geometry.size(),nullptr,&made.domain))) {domain_shaders.emplace(geometry,made.domain);++shader_objects;}
                if (!made.hull || !made.corners || !made.domain) {
                    refused("D3D11 tessellation variant creation failed");return VELOCITY_VARIANT_UNSTORED;
                }
            }
            info=std::move(d);made.history_slot=127;made.geometry_hash=lent.geometry_hash;
            made.maximum_vertices=made.input_points ? (3u+made.input_points-1)/made.input_points : 1u;
        } else if (transformed && native_geometry) {
            rsf_ac7_velocity_vertex_info generated;
            transformed=rsf_ac7_velocity_native_geometry_shader(lent.bytes[2],lent.sizes[2],info,
                geometry,generated,made.maximum_vertices,previous,current_clip)==rsf_ac7_velocity_shader_result::patched;
            info=std::move(generated);made.history_slot=127;made.geometry_hash=lent.geometry_hash;
        } else if (transformed && !rsf_ac7_velocity_geometry_shader(info,geometry,previous,current_clip)) {
            return VELOCITY_VARIANT_UNSTORED;
        }
        if (!transformed || rsf_ac7_velocity_pixel_shader(lent.bytes[1],lent.sizes[1],current.material_blend,
            pixel_code,previous,current_clip)!=rsf_ac7_velocity_shader_result::patched) {
            // The transform interface also reports compiler and output allocation failures
            // as refusals, so this result is insufficient for a permanent negative entry.
            refused("native material or vertex shader transform refused");return VELOCITY_VARIANT_UNSTORED;
        }
        if (const auto cached=vertex_shaders.find(vertex);cached!=vertex_shaders.end()) made.vertex=cached->second;
        else if (SUCCEEDED(device->CreateVertexShader(vertex.data(),vertex.size(),nullptr,&made.vertex))) {vertex_shaders.emplace(vertex,made.vertex);++shader_objects;}
        if (!native_hull) {
            if (const auto cached=geometry_shaders.find(geometry);cached!=geometry_shaders.end()) made.geometry=cached->second;
            else if (SUCCEEDED(device->CreateGeometryShader(geometry.data(),geometry.size(),nullptr,&made.geometry))) {geometry_shaders.emplace(geometry,made.geometry);++shader_objects;}
        }
        if (const auto cached=pixel_shaders.find(pixel_code);cached!=pixel_shaders.end()) made.pixel=cached->second;
        else if (SUCCEEDED(device->CreatePixelShader(pixel_code.data(),pixel_code.size(),nullptr,&made.pixel))) {pixel_shaders.emplace(pixel_code,made.pixel);++shader_objects;}
        if (!made.vertex || (!native_hull && !made.geometry) || !made.pixel) {
            refused("D3D11 variant creation failed");return VELOCITY_VARIANT_UNSTORED;
        }
        const auto position=std::find_if(info.outputs.begin(),info.outputs.end(),[](const auto& s) {return s.system==1;});
        if (position==info.outputs.end()) return VELOCITY_VARIANT_REFUSED;
        const D3D11_SO_DECLARATION_ENTRY declarations[]{
            {0,position->name.c_str(),position->index,0,4,0},
            {0,"RSFKEY",0,0,4,0},{0,"RSFCENTRE",0,0,4,0},{0,"RSFOLD",0,0,4,0},
        };
        const UINT stride=64;
        const auto& capture_code=native_geometry || native_hull ? geometry : vertex;
        if (const auto cached=capture_shaders.find(capture_code);cached!=capture_shaders.end()) made.capture=cached->second;
        else if (SUCCEEDED(device->CreateGeometryShaderWithStreamOutput(capture_code.data(),capture_code.size(),declarations,4,
            &stride,1,D3D11_SO_NO_RASTERIZED_STREAM,nullptr,&made.capture))) {capture_shaders.emplace(capture_code,made.capture);++shader_objects;}
        if (!made.capture) {
            refused("stream-output signature creation failed");return VELOCITY_VARIANT_UNSTORED;
        }
        made.particle=info.particle!=0;
        variants.push_back(std::move(made));slot=uint32_t(variants.size()-1);return VELOCITY_VARIANT_BUILT;
    }
    bool build(ID3D11DeviceContext* c,Buffer& source,Buffer& destination,uint32_t count,uint32_t mode,
        bool dynamic=false,ID3D11Predicate* predicate=nullptr,BOOL predicate_value=FALSE) {
        VelocityHashDispatch dispatch{};
        if (!rsf_ac7_velocity_hash_dispatch(count,destination.count,mode,dynamic ? 1u : 0u,&dispatch)) return false;
        c->UpdateSubresource(hash_parameters.Get(),0,nullptr,&dispatch.parameters,0,0);
        ID3D11Buffer* cb=hash_parameters.Get();c->CSSetConstantBuffers(0,1,&cb);
        ID3D11ShaderResourceView* bound_sources[]{source.view.Get(),dynamic ? indirect.view.Get() : nullptr};c->CSSetShaderResources(0,2,bound_sources);
        ID3D11UnorderedAccessView* uav=destination.write.Get();c->CSSetUnorderedAccessViews(0,1,&uav,nullptr);
        c->CSSetShader(hash.Get(),nullptr,0);c->SetPredication(predicate,predicate_value);
        if (dynamic) c->DispatchIndirect(indirect.buffer.Get(),36);
        else c->Dispatch(dispatch.groups[0],dispatch.groups[1],1);
        c->SetPredication(nullptr,FALSE);
        bound_sources[0]=bound_sources[1]=nullptr;uav=nullptr;c->CSSetShaderResources(0,2,bound_sources);c->CSSetUnorderedAccessViews(0,1,&uav,nullptr);
        return true;
    }
    // velocity_history.rs rotates before it refuses records, so the swap and clear run either way.
    Model* model(ID3D11DeviceContext* c,const VelocityModelDraw& draw,VelocityModelDecision& decision) {
        const bool admitted=rsf_ac7_velocity_model_begin(&draw,&decision)!=0;
        if (decision.slot==VELOCITY_NO_SLOT) return nullptr;
        if (decision.slot>=models.size()) models.resize(size_t(decision.slot)+1);
        Model& model=models[decision.slot];
        if (decision.rotate) {
            std::swap(model.previous,model.now);
            if (model.now.write) {const UINT zero[4]{};c->ClearUnorderedAccessViewUint(model.now.write.Get(),zero);}
        }
        if (!admitted) return nullptr;
        if (model.now.count<decision.capacity) {
            Buffer larger;if (!raw_buffer(device.Get(),decision.capacity,false,larger)) return nullptr;
            const UINT zero[4]{};c->ClearUnorderedAccessViewUint(larger.write.Get(),zero);
            if (model.now.buffer) build(c,model.now,larger,model.now.count,1);
            model.now=std::move(larger);
        }
        return &model;
    }
    // The view's slot once its targets hold this frame's native depth, or VELOCITY_NO_SLOT.
    uint32_t view(ID3D11DeviceContext* c,const rsf_frame_tap_geometry& g) {
        auto* dsv=static_cast<ID3D11DepthStencilView*>(g.depth_view);
        ComPtr<ID3D11Resource> source;dsv->GetResource(&source);ComPtr<ID3D11Texture2D> depth;
        if (FAILED(source.As(&depth))) return VELOCITY_NO_SLOT;
        D3D11_TEXTURE2D_DESC desc{};depth->GetDesc(&desc);
        if (desc.Width!=g.width || desc.Height!=g.height || desc.ArraySize!=1 || desc.MipLevels!=1 || desc.SampleDesc.Count!=1) return VELOCITY_NO_SLOT;
        const VelocityViewDraw draw{current.session_id,current.history_key,current.view_key,current.family_key,
            uint32_t(current.native_frame),desc.Width,desc.Height,0};
        VelocityViewDecision decision{VELOCITY_NO_SLOT,VELOCITY_VIEW_UNAVAILABLE,0,0};rsf_ac7_velocity_view_begin(&draw,&decision);
        if (decision.action==VELOCITY_VIEW_UNAVAILABLE || decision.slot==VELOCITY_NO_SLOT) return VELOCITY_NO_SLOT;
        if (decision.slot>=views.size()) views.resize(size_t(decision.slot)+1);
        View& v=views[decision.slot];
        if (decision.action==VELOCITY_VIEW_SAME_FRAME) return v.native_depth.Get()==depth.Get() ? decision.slot : VELOCITY_NO_SLOT;
        if (decision.action==VELOCITY_VIEW_ALLOCATE) {
            v={};
            D3D11_TEXTURE2D_DESC owned=desc;owned.Usage=D3D11_USAGE_DEFAULT;owned.CPUAccessFlags=owned.MiscFlags=0;
            owned.BindFlags=D3D11_BIND_DEPTH_STENCIL|D3D11_BIND_SHADER_RESOURCE;
            D3D11_DEPTH_STENCIL_VIEW_DESC depth_view{};dsv->GetDesc(&depth_view);depth_view.Flags=0;
            if (FAILED(device->CreateTexture2D(&owned,nullptr,&v.depth)) ||
                FAILED(device->CreateDepthStencilView(v.depth.Get(),&depth_view,&v.depth_target))) return VELOCITY_NO_SLOT;
            owned.Format=DXGI_FORMAT_R16G16_UNORM;owned.BindFlags=D3D11_BIND_RENDER_TARGET|D3D11_BIND_SHADER_RESOURCE;
            if (FAILED(device->CreateTexture2D(&owned,nullptr,&v.motion)) ||
                FAILED(device->CreateRenderTargetView(v.motion.Get(),nullptr,&v.motion_target)) ||
                FAILED(device->CreateShaderResourceView(v.motion.Get(),nullptr,&v.motion_view)) ||
                FAILED(device->CreateTexture2D(&owned,nullptr,&v.merged)) ||
                FAILED(device->CreateRenderTargetView(v.merged.Get(),nullptr,&v.merged_target))) return VELOCITY_NO_SLOT;
        }
        v.native_depth=depth;
        c->CopyResource(v.depth.Get(),depth.Get());const float zero[4]{};c->ClearRenderTargetView(v.motion_target.Get(),zero);
        rsf_ac7_velocity_view_ready(decision.slot,&draw);
        return decision.slot;
    }
    static void retire(void* user,uint32_t kind,uint32_t slot) noexcept {
        auto& owner=*static_cast<Renderer*>(user);
        if (kind==VELOCITY_RETIRE_MODEL && slot<owner.models.size()) owner.models[slot]={};
        else if (kind==VELOCITY_RETIRE_VIEW && slot<owner.views.size()) owner.views[slot]={};
    }
};
// velocity_renderer.rs publishes one renderer per device and replaces and destroys it through these.
void* create_renderer(void* device) noexcept try {
    auto made=std::make_unique<Renderer>(static_cast<ID3D11Device*>(device));
    return made->initialize() ? made.release() : nullptr;
} catch (...) {return nullptr;}
void destroy_renderer(void* renderer) noexcept {delete static_cast<Renderer*>(renderer);}
constexpr VelocityRendererLeaves renderer_leaves{create_renderer,destroy_renderer};
Renderer* published() noexcept {return static_cast<Renderer*>(rsf_ac7_velocity_renderer_published());}
double cpu_clock_ms() {
    static const double scale=[] {LARGE_INTEGER frequency{};QueryPerformanceFrequency(&frequency);return 1000./double(frequency.QuadPart);}();
    LARGE_INTEGER ticks{};QueryPerformanceCounter(&ticks);return double(ticks.QuadPart)*scale;
}
struct CpuSample {
    double entered,phase,shader_ms=0,resource_ms=0;uint32_t frame,records=0;
    explicit CpuSample(double start):entered(start),phase(start),frame(uint32_t(current.native_frame)) {}
    ~CpuSample() {
        const VelocityProfileSample sample{cpu_clock_ms()-entered,shader_ms,resource_ms,frame,records};
        rsf_ac7_velocity_profile_sample(&sample);
    }
};
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
}
extern "C" void rsf_ac7_velocity_set_log(void (*log)(void*,const char*),void* user)
{const std::lock_guard lock(replay_mutex);log_line=log;log_user=user;rsf_ac7_velocity_reports_reset();}
extern "C" void rsf_ac7_velocity_scope(const rsf_game_render_pass* scope)
{current=scope && scope->struct_size>=sizeof(current) ? *scope : rsf_game_render_pass{};}
extern "C" void rsf_ac7_velocity_geometry(const rsf_frame_tap_geometry* pointer) try
{
    if (merging || !rsf_ac7_velocity_stream_draw(&current,pointer)) return;
    const std::lock_guard lock(replay_mutex);
    const auto& g=*pointer;auto* c=static_cast<ID3D11DeviceContext*>(g.context);
    if (c->GetType()!=D3D11_DEVICE_CONTEXT_IMMEDIATE) return;
    const double entered_ms=cpu_clock_ms();
    ComPtr<ID3D11GeometryShader> existing;ComPtr<ID3D11HullShader> hull;ComPtr<ID3D11DomainShader> domain;
    c->GSGetShader(&existing,nullptr,nullptr);c->HSGetShader(&hull,nullptr,nullptr);c->DSGetShader(&domain,nullptr,nullptr);
    const uint32_t stages=(existing ? VELOCITY_STAGE_GEOMETRY : 0u)|(hull ? VELOCITY_STAGE_HULL : 0u)|(domain ? VELOCITY_STAGE_DOMAIN : 0u);
    const uint32_t route=rsf_ac7_velocity_stream_route(g.topology,stages);
    if (route==VELOCITY_ROUTE_MISMATCHED) {refused("incomplete tessellation pipeline or combined native GS/tessellation requires a matching route");return;}
    if (route!=VELOCITY_ROUTE_ADMITTED) return;
    ComPtr<ID3D11Device> device;c->GetDevice(&device);
    auto* renderer=static_cast<Renderer*>(rsf_ac7_velocity_renderer_admit(device.Get(),&renderer_leaves));
    if (!renderer) {refused("device-side pass initialization failed");return;}
    CpuSample timing(entered_ms);
    ComPtr<ID3D11PixelShader> native_pixel;c->PSGetShader(&native_pixel,nullptr,nullptr);
    uint32_t vertex_hash=0;auto* variant=renderer->variant(g,native_pixel.Get(),existing.Get(),hull.Get(),domain.Get(),vertex_hash);
    timing.phase=cpu_clock_ms();timing.shader_ms=timing.phase-entered_ms;
    if (!variant) return;
    VelocityStreamPlan plan{};
    if (!rsf_ac7_velocity_stream_plan(&g,stages,current.geometry_capacity,variant->maximum_vertices,variant->input_points,&plan)) return;
    const uint32_t count=plan.bounds.records;
    timing.records=count;
    State saved(c);if (!saved.context) return;
    ComPtr<ID3D11Predicate> predicate;BOOL predicate_value=FALSE;c->GetPredication(&predicate,&predicate_value);
    c->SetPredication(nullptr,FALSE);
    if (g.kind>=5 && !renderer->prepare_indirect(c,g,plan.bounds)) {
        refused("indirect argument buffer or native mesh capacity is invalid");return;
    }
    const uint32_t view_slot=renderer->view(c,g);
    if (view_slot==VELOCITY_NO_SLOT) {refused("native scene depth allocation/identity differs from the draw");return;}
    auto& view=renderer->views[view_slot];
    const VelocityModelDraw model_draw{current.session_id,current.history_key,current.view_key,current.primitive_key,
        current.material_key,current.vertex_factory_key,vertex_hash,variant->geometry_hash,g.start,g.base_vertex,
        variant->particle ? 1u : 0u,uint32_t(current.native_frame),(current.flags&RSF_GAME_RENDER_RESET) ? 1u : 0u,count};
    VelocityModelDecision model_decision{VELOCITY_NO_SLOT,0,0,0};
    Model* model=renderer->model(c,model_draw,model_decision);
    if (!model) {refused("geometry-history allocation failed");return;}
    if (!renderer->captured.buffer || renderer->captured.count<count) {
        // SO cannot share the raw UAV contract, but it can expose a raw read-only SRV.
        // Keep a separate zero source for variable emissions and read completed SO directly.
        Buffer captured;const uint32_t capacity=plan.capacity;
        D3D11_BUFFER_DESC capture{};capture.ByteWidth=capacity*64;
        capture.BindFlags=D3D11_BIND_STREAM_OUTPUT|D3D11_BIND_SHADER_RESOURCE;
        capture.MiscFlags=D3D11_RESOURCE_MISC_BUFFER_ALLOW_RAW_VIEWS;
        D3D11_SHADER_RESOURCE_VIEW_DESC srv{};srv.Format=DXGI_FORMAT_R32_TYPELESS;
        srv.ViewDimension=D3D11_SRV_DIMENSION_BUFFEREX;srv.BufferEx.NumElements=capacity*16;
        srv.BufferEx.Flags=D3D11_BUFFEREX_SRV_FLAG_RAW;
        if (FAILED(device->CreateBuffer(&capture,nullptr,&captured.buffer)) ||
            FAILED(device->CreateShaderResourceView(captured.buffer.Get(),&srv,&captured.view))) {
            refused("shader-readable stream-output allocation failed");return;
        }
        captured.count=capacity;renderer->captured=std::move(captured);
    }
    // Native GS emission and DrawAuto have no readable record count. Fixed vertex/patch
    // producers use the actual direct or validated GPU indirect count and need no tail clear.
    if (plan.zero_source) {
        if (renderer->stream.count<count) {
            if (!raw_buffer(device.Get(),plan.capacity,false,renderer->stream)) {refused("generated-geometry zero allocation failed");return;}
            const UINT zero[4]{};c->ClearUnorderedAccessViewUint(renderer->stream.write.Get(),zero);
        }
        const D3D11_BOX capture_box{0,0,0,count*64,1,1};
        c->CopySubresourceRegion(renderer->captured.buffer.Get(),0,0,0,0,renderer->stream.buffer.Get(),0,&capture_box);
    }
    timing.resource_ms=cpu_clock_ms()-timing.phase;
    if (existing || hull) {
        const VelocityDrawParameters empty{};c->UpdateSubresource(renderer->parameters.Get(),0,nullptr,&empty,0,0);
        ID3D11Buffer* cb=renderer->parameters.Get();c->GSSetConstantBuffers(13,1,&cb);
        ID3D11ShaderResourceView* unavailable=nullptr;c->GSSetShaderResources(127,1,&unavailable);
        c->DSSetConstantBuffers(13,1,&cb);c->DSSetShaderResources(127,1,&unavailable);
    }
    c->OMSetRenderTargets(0,nullptr,nullptr);c->VSSetShader(variant->vertex.Get(),nullptr,0);
    if (variant->layout) c->IASetInputLayout(variant->layout.Get());
    if (hull) {c->HSSetShader(variant->corners.Get(),nullptr,0);c->DSSetShader(variant->domain.Get(),nullptr,0);}
    c->GSSetShader(variant->capture.Get(),nullptr,0);c->PSSetShader(nullptr,nullptr,0);
    ID3D11Buffer* stream=renderer->captured.buffer.Get();const UINT offset=0;c->SOSetTargets(1,&stream,&offset);
    c->SetPredication(predicate.Get(),predicate_value);draw(c,g,renderer->indirect.buffer.Get());c->SetPredication(nullptr,FALSE);c->SOSetTargets(0,nullptr,nullptr);
    D3D11_VIEWPORT viewport{};UINT viewports=1;c->RSGetViewports(&viewports,&viewport);
    const float rect[4]{viewport.TopLeftX,viewport.TopLeftY,viewport.Width,viewport.Height};
    // A history buffer's count is nonzero exactly when it exists.
    VelocityDrawParameters parameters{};
    if (!rsf_ac7_velocity_draw_parameters(&current,viewports,&rect,model_decision.usable,model->previous.count,&parameters)) return;
    c->UpdateSubresource(renderer->parameters.Get(),0,nullptr,&parameters,0,0);
    ID3D11Buffer* cb=renderer->parameters.Get();c->GSSetConstantBuffers(13,1,&cb);c->PSSetConstantBuffers(13,1,&cb);
    ID3D11ShaderResourceView* previous=model->previous.view.Get();c->GSSetShaderResources(variant->history_slot,1,&previous);
    if (hull) {
        c->HSSetShader(variant->hull.Get(),nullptr,0);c->DSSetShader(variant->domain.Get(),nullptr,0);
        c->DSSetConstantBuffers(13,1,&cb);c->DSSetShaderResources(127,1,&previous);
    }
    c->GSSetShader(variant->geometry.Get(),nullptr,0);c->PSSetShader(variant->pixel.Get(),nullptr,0);
    ID3D11RenderTargetView* rt=view.motion_target.Get();c->OMSetRenderTargets(1,&rt,view.depth_target.Get());
    c->OMSetDepthStencilState(renderer->depth_state.Get(),0);c->OMSetBlendState(renderer->blend.Get(),nullptr,UINT32_MAX);
    c->SetPredication(predicate.Get(),predicate_value);draw(c,g,renderer->indirect.buffer.Get());c->SetPredication(nullptr,FALSE);rsf_ac7_velocity_view_written(view_slot);
    previous=nullptr;c->GSSetShaderResources(variant->history_slot,1,&previous);c->OMSetRenderTargets(0,nullptr,nullptr);
    if (hull) c->DSSetShaderResources(127,1,&previous);
    if (renderer->build(c,renderer->captured,model->now,plan.built,0,g.kind>=5,predicate.Get(),predicate_value))
        rsf_ac7_velocity_model_record(model_decision.slot,model_draw.frame,model_draw.records);
} catch (...) {}
extern "C" int rsf_ac7_velocity_commit(void* context,const rsf_game_render_pass* pass) try
{
    const std::lock_guard lock(replay_mutex);
    // Pass admission, its view identity and the target check are velocity_commit.rs.
    auto* renderer=published();
    if (!renderer || !context) return 0;
    VelocityViewDecision found{VELOCITY_NO_SLOT,VELOCITY_VIEW_UNAVAILABLE,0,0};
    if (!rsf_ac7_velocity_commit_view(pass,&found) || found.slot>=renderer->views.size()) return 0;
    auto& view=renderer->views[found.slot];
    if (view.native_depth.Get()!=pass->depth) return 0;
    auto* c=static_cast<ID3D11DeviceContext*>(context);auto* native=static_cast<ID3D11Texture2D*>(pass->motion);
    D3D11_TEXTURE2D_DESC desc{};native->GetDesc(&desc);
    const VelocityCommitTarget target{desc.Width,desc.Height,desc.SampleDesc.Count,desc.ArraySize,uint32_t(desc.Format)};
    if (!rsf_ac7_velocity_commit_target(&found,&target)) return 0;
    ComPtr<ID3D11Device> device;c->GetDevice(&device);if (rsf_ac7_velocity_renderer_for(device.Get())!=renderer) return 0;
    State saved(c);if (!saved.context) return 0;c->SetPredication(nullptr,FALSE);
    c->SOSetTargets(0,nullptr,nullptr);c->GSSetShader(nullptr,nullptr,0);c->HSSetShader(nullptr,nullptr,0);c->DSSetShader(nullptr,nullptr,0);
    c->VSSetShader(renderer->fullscreen.Get(),nullptr,0);c->PSSetShader(renderer->merge.Get(),nullptr,0);
    c->OMSetDepthStencilState(nullptr,0);c->OMSetBlendState(renderer->blend.Get(),nullptr,UINT32_MAX);
    c->RSSetState(renderer->raster.Get());D3D11_VIEWPORT viewport{0,0,float(found.width),float(found.height),0,1};c->RSSetViewports(1,&viewport);
    c->IASetInputLayout(nullptr);c->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    D3D11_SHADER_RESOURCE_VIEW_DESC srv{};srv.Format=DXGI_FORMAT_R16G16_UNORM;srv.ViewDimension=D3D11_SRV_DIMENSION_TEXTURE2D;srv.Texture2D.MipLevels=1;
    ComPtr<ID3D11ShaderResourceView> native_view;if (FAILED(device->CreateShaderResourceView(native,&srv,&native_view))) return 0;
    ID3D11ShaderResourceView* views[]{native_view.Get(),view.motion_view.Get()};c->PSSetShaderResources(0,2,views);
    ID3D11RenderTargetView* rt=view.merged_target.Get();c->OMSetRenderTargets(1,&rt,nullptr);
    {const MergeDraw merge;c->Draw(3,0);}
    c->OMSetRenderTargets(0,nullptr,nullptr);
    ID3D11ShaderResourceView* empty[128]{};
    c->VSSetShaderResources(0,128,empty);c->PSSetShaderResources(0,128,empty);c->GSSetShaderResources(0,128,empty);
    c->HSSetShaderResources(0,128,empty);c->DSSetShaderResources(0,128,empty);c->CSSetShaderResources(0,128,empty);
    c->CopyResource(native,view.merged.Get());c->CopyResource(view.native_depth.Get(),view.depth.Get());
    rsf_ac7_velocity_view_committed(found.slot);
    if (log_line && rsf_ac7_velocity_report(VELOCITY_REPORT_COMMIT)) log_line(log_user,"AC7 translucent velocity: opacity-clipped generated-geometry motion and depth committed before native post-processing");
    return 1;
} catch (...) {return 0;}
extern "C" void rsf_ac7_velocity_frame_end(void)
{
    const std::lock_guard lock(replay_mutex);
    auto* renderer=published();
    if (!renderer) return;
    // velocity_commit.rs restarts the totals whether or not the frame is reported.
    VelocityProfileReport report{};
    if (rsf_ac7_velocity_profile_frame_end(&report) && log_line) {
        char message[320]{};std::snprintf(message,sizeof(message),
            "AC7 velocity CPU frame=%u callbacks=%u total_ms=%.2f shader_ms=%.2f resource_ms=%.2f peak_ms=%.2f largest_records=%u models=%zu variants=%zu shader_objects=%u",
            report.frame,report.callbacks,report.total_ms,report.shader_ms,
            report.resource_ms,report.peak_ms,report.largest_records,rsf_ac7_velocity_model_count(),rsf_ac7_velocity_variant_count(),renderer->shader_objects);
        log_line(log_user,message);
    }
    rsf_ac7_velocity_history_frame_end(renderer,Renderer::retire);
}
// velocity_renderer.rs destroys the renderer, then forgets the sources, layouts, variants,
// history and profile.
extern "C" void rsf_ac7_velocity_release(void)
{const std::lock_guard lock(replay_mutex);rsf_ac7_velocity_renderer_release();current={};}
uint32_t rsf_ac7_velocity_device_shader_creation_count() noexcept
{const std::lock_guard lock(replay_mutex);const auto* renderer=published();return renderer ? renderer->shader_objects : 0;}
