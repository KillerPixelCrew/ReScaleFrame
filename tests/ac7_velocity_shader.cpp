// SPDX-License-Identifier: GPL-3.0-only
#include "../games/ac7/src/velocity_shader.h"
#include <rescaleframe/shader_compile.h>
#include <rescaleframe/ac7_velocity_pass.h>
#include <rescaleframe/frame_tap.h>
#include <d3d11.h>
#include <wrl/client.h>
#include <cstdio>
#include <cmath>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <array>
using Microsoft::WRL::ComPtr;
namespace {
bool ok=true;
void check(bool value,const char* what) { if (!value) { std::fprintf(stderr,"%s\n",what); ok=false; } }
void pass_log(void*,const char* line) {std::fprintf(stderr,"%s\n",line);}
std::vector<uint8_t> compile(const char* source,const char* profile) {
    std::vector<uint8_t> bytes; check(rsf::compile_shader(source,"velocity-fixture","main",profile,bytes),"compile fixture"); return bytes;
}
}
int main(int argc,char** argv)
{
    ComPtr<ID3D11Device> device; ComPtr<ID3D11DeviceContext> context;
    if (FAILED(D3D11CreateDevice(nullptr,D3D_DRIVER_TYPE_WARP,nullptr,0,nullptr,0,D3D11_SDK_VERSION,
        &device,nullptr,&context))) return 77;
    auto vertex=compile(R"(
struct V {float4 position:SV_Position;float4 previous:RSFPREV0;float4 current:RSFCUR0;};
V main(uint id:SV_VertexID) {V o;o.position=float4(id==2?3:-1,id==1?3:-1,.75,1);
o.previous=o.position+float4(.2,0,0,0);o.current=o.position;return o;})","vs_5_0");
    ComPtr<ID3D11VertexShader> vs; check(SUCCEEDED(device->CreateVertexShader(vertex.data(),vertex.size(),nullptr,&vs)),"create VS");
    {
        auto original=compile("float4 main(uint id:SV_VertexID):SV_Position"
            "{return float4(id==2?3:-1,id==1?3:-1,.75,1);}","vs_5_0");
        std::vector<uint8_t> keyed; rsf_ac7_velocity_vertex_info info;
        check(rsf_ac7_velocity_vertex_shader(original.data(),original.size(),keyed,info)==rsf_ac7_velocity_shader_result::patched,
            "augment generated geometry with stable vertex/instance keys");
        ComPtr<ID3D11VertexShader> key_shader;
        check(SUCCEEDED(device->CreateVertexShader(keyed.data(),keyed.size(),nullptr,&key_shader)),"create keyed geometry VS");
        std::vector<uint8_t> geometry;uint32_t previous=0,current_clip=0;
        check(rsf_ac7_velocity_geometry_shader(info,geometry,previous,current_clip),"compile previous-generated-geometry GS");
        ComPtr<ID3D11GeometryShader> gs;
        check(SUCCEEDED(device->CreateGeometryShader(geometry.data(),geometry.size(),nullptr,&gs)),"create previous-generated-geometry GS");
        auto native_gs=compile("struct V{float4 p:SV_Position;};"
            "[maxvertexcount(3)]void main(triangle V v[3],inout TriangleStream<V> s)"
            "{[unroll]for(uint i=0;i<3;i++){V o;o.p=v[i].p+float4(.1,0,0,0);s.Append(o);}}","gs_5_0");
        rsf_ac7_velocity_vertex_info generated;uint32_t maximum=0;
        check(rsf_ac7_velocity_native_geometry_shader(native_gs.data(),native_gs.size(),info,geometry,
            generated,maximum,previous,current_clip)==rsf_ac7_velocity_shader_result::patched && maximum==3,
            "augment native GS at emission preserving generated position");
        gs.Reset();
        check(SUCCEEDED(device->CreateGeometryShader(geometry.data(),geometry.size(),nullptr,&gs)),"create augmented native GS");
    }
    const char* materials[]{
        "Texture2D<float4> data:register(t0);float4 main(float4 p:SV_Position):SV_Target0"
        "{float4 c=data.Load(int3(int2(p.xy),0));return float4(max(c.xyz,0),saturate(c.w));}",
        "Texture2D<float4> data:register(t0);float4 main(float4 p:SV_Position):SV_Target0"
        "{float4 c=data.Load(int3(int2(p.xy),0));float a=saturate(c.w);return float4(max(c.xyz,0)*a,0);}",
    };
    {
        auto tess_vertex=compile("struct V{float4 p:VS_To_DS_Position;};V main(uint i:SV_VertexID)"
            "{V o;o.p=float4(i==2?3:-1,i==1?3:-1,.75,1);return o;}","vs_5_0");
        auto tess_hull=compile("struct V{float4 p:VS_To_DS_Position;};struct C{float e[3]:SV_TessFactor;float i:SV_InsideTessFactor;};"
            "C constants(InputPatch<V,3> p){C o;o.e[0]=o.e[1]=o.e[2]=4;o.i=4;return o;}"
            "[domain(\"tri\")][partitioning(\"integer\")][outputtopology(\"triangle_cw\")]"
            "[outputcontrolpoints(3)][patchconstantfunc(\"constants\")]"
            "V main(InputPatch<V,3> p,uint i:SV_OutputControlPointID){return p[i];}","hs_5_0");
        auto tess_domain=compile("struct V{float4 p:VS_To_DS_Position;};struct C{float e[3]:SV_TessFactor;float i:SV_InsideTessFactor;};"
            "[domain(\"tri\")]float4 main(C c,const OutputPatch<V,3> p,float3 d:SV_DomainLocation):SV_Position"
            "{return p[0].p*d.x+p[1].p*d.y+p[2].p*d.z;}","ds_5_0");
        std::vector<uint8_t> v,h,d,corners;rsf_ac7_velocity_vertex_info vi,hi,di;uint32_t points=0,previous=0,current_clip=0;
        check(rsf_ac7_velocity_vertex_shader(tess_vertex.data(),tess_vertex.size(),v,vi,true)==rsf_ac7_velocity_shader_result::patched,"key tessellation VS world-position outputs");
        check(rsf_ac7_velocity_hull_shader(tess_hull.data(),tess_hull.size(),vi,h,corners,hi,points)==rsf_ac7_velocity_shader_result::patched && points==3,"relay patch keys through native hull control phase");
        check(rsf_ac7_velocity_domain_shader(tess_domain.data(),tess_domain.size(),hi,d,di,previous,current_clip)==rsf_ac7_velocity_shader_result::patched,"augment native domain evaluation with prior control motion");
        ComPtr<ID3D11VertexShader> tv;ComPtr<ID3D11HullShader> th,tc;ComPtr<ID3D11DomainShader> td;
        check(SUCCEEDED(device->CreateVertexShader(v.data(),v.size(),nullptr,&tv)),"create tessellation keyed VS");
        check(SUCCEEDED(device->CreateHullShader(h.data(),h.size(),nullptr,&th)) && SUCCEEDED(device->CreateHullShader(corners.data(),corners.size(),nullptr,&tc)),"create normal/corner native HS variants");
        check(SUCCEEDED(device->CreateDomainShader(d.data(),d.size(),nullptr,&td)),"create native domain motion shader");
        if (argc==2) {
            for (const auto& item:{std::pair{"tess-v",&v},std::pair{"tess-h",&h},std::pair{"tess-d",&d}}) {
                std::ofstream file(std::filesystem::path(argv[1])/(std::string(item.first)+".dxbc"),std::ios::binary);
                file.write(reinterpret_cast<const char*>(item.second->data()),std::streamsize(item.second->size()));
            }
            const auto native_h=std::filesystem::path(argv[1])/"native-tess-h.dxbc";
            const auto native_d=std::filesystem::path(argv[1])/"native-tess-d.dxbc";
            if (std::filesystem::exists(native_h) && std::filesystem::exists(native_d)) {
                auto matched_vertex=compile("struct V{float4 a:TEXCOORD10;float4 b:TEXCOORD11;float4 c:TEXCOORD7;float3 n:TEXCOORD9;"
                    "float4 p:VS_To_DS_Position;uint id:VS_To_DS_VertexID;};V main(uint i:SV_VertexID){V o;"
                    "o.a=o.b=o.c=0;o.n=float3(0,0,1);o.p=float4(i,0,0,1);o.id=i;return o;}","vs_5_0");
                std::ifstream hf(native_h,std::ios::binary),df(native_d,std::ios::binary);
                std::vector<uint8_t> hb{std::istreambuf_iterator<char>(hf),std::istreambuf_iterator<char>()},db{std::istreambuf_iterator<char>(df),std::istreambuf_iterator<char>()};
                check(rsf_ac7_velocity_vertex_shader(matched_vertex.data(),matched_vertex.size(),v,vi,true)==rsf_ac7_velocity_shader_result::patched &&
                    rsf_ac7_velocity_hull_shader(hb.data(),hb.size(),vi,h,corners,hi,points)==rsf_ac7_velocity_shader_result::patched && points==12 &&
                    rsf_ac7_velocity_domain_shader(db.data(),db.size(),hi,d,di,previous,current_clip)==rsf_ac7_velocity_shader_result::patched,
                    "transform retained native 12-control-point PN hull/domain shaders with matching synthetic VS inputs");
                th.Reset();tc.Reset();td.Reset();
                check(SUCCEEDED(device->CreateHullShader(h.data(),h.size(),nullptr,&th)) && SUCCEEDED(device->CreateHullShader(corners.data(),corners.size(),nullptr,&tc)) &&
                    SUCCEEDED(device->CreateDomainShader(d.data(),d.size(),nullptr,&td)),"create retained native PN tessellation variants");
            }
        }
    }
    D3D11_TEXTURE2D_DESC desc{}; desc.Width=2; desc.Height=1; desc.MipLevels=desc.ArraySize=1;
    desc.SampleDesc.Count=1; desc.Format=DXGI_FORMAT_R32G32B32A32_FLOAT;
    desc.BindFlags=D3D11_BIND_SHADER_RESOURCE;
    const float pixels[]{.2f,.4f,.6f,.8f, .2f,.4f,.6f,.1f};
    const D3D11_SUBRESOURCE_DATA initial{pixels,sizeof(pixels),0};
    ComPtr<ID3D11Texture2D> input; ComPtr<ID3D11ShaderResourceView> input_view;
    check(SUCCEEDED(device->CreateTexture2D(&desc,&initial,&input)) &&
        SUCCEEDED(device->CreateShaderResourceView(input.Get(),nullptr,&input_view)),"create material texture");
    desc.Format=DXGI_FORMAT_R32G32_FLOAT; desc.BindFlags=D3D11_BIND_RENDER_TARGET;
    ComPtr<ID3D11Texture2D> output; ComPtr<ID3D11RenderTargetView> target;
    check(SUCCEEDED(device->CreateTexture2D(&desc,nullptr,&output)) &&
        SUCCEEDED(device->CreateRenderTargetView(output.Get(),nullptr,&target)),"create velocity target");
    desc.Format=DXGI_FORMAT_D32_FLOAT; desc.BindFlags=D3D11_BIND_DEPTH_STENCIL;
    ComPtr<ID3D11Texture2D> depth; ComPtr<ID3D11DepthStencilView> depth_view;
    check(SUCCEEDED(device->CreateTexture2D(&desc,nullptr,&depth)) &&
        SUCCEEDED(device->CreateDepthStencilView(depth.Get(),nullptr,&depth_view)),"create translucent depth");
    D3D11_DEPTH_STENCIL_DESC state{}; state.DepthEnable=TRUE; state.DepthWriteMask=D3D11_DEPTH_WRITE_MASK_ALL;
    state.DepthFunc=D3D11_COMPARISON_GREATER_EQUAL;
    ComPtr<ID3D11DepthStencilState> depth_state; check(SUCCEEDED(device->CreateDepthStencilState(&state,&depth_state)),"create depth-writing pass state");
    D3D11_RASTERIZER_DESC raster{}; raster.FillMode=D3D11_FILL_SOLID; raster.CullMode=D3D11_CULL_NONE; raster.DepthClipEnable=TRUE;
    ComPtr<ID3D11RasterizerState> raster_state; check(SUCCEEDED(device->CreateRasterizerState(&raster,&raster_state)),"create raster state");
    struct Params {float rect[4],jitter[4];float cutoff;uint32_t history;float padding[2];};
    const Params params{{0,0,2,1},{0,0,0,0},.333f+1.f/255.f,1,{0,0}};
    D3D11_BUFFER_DESC buffer{}; buffer.ByteWidth=sizeof(params); buffer.Usage=D3D11_USAGE_IMMUTABLE; buffer.BindFlags=D3D11_BIND_CONSTANT_BUFFER;
    const D3D11_SUBRESOURCE_DATA constants{&params,0,0}; ComPtr<ID3D11Buffer> cb;
    check(SUCCEEDED(device->CreateBuffer(&buffer,&constants,&cb)),"create pass constants");
    for (uint32_t mode=0;mode<2;++mode) {
        auto native=compile(materials[mode],"ps_5_0"); std::vector<uint8_t> patched;
        if (argc==2) {
            std::ofstream artifact(std::filesystem::path(argv[1])/("fixture-"+std::to_string(mode)+".dxbc"),std::ios::binary);
            artifact.write(reinterpret_cast<const char*>(native.data()),std::streamsize(native.size()));
        }
        check(rsf_ac7_velocity_pixel_shader(native.data(),native.size(),mode+2,patched)==rsf_ac7_velocity_shader_result::patched,"transform material velocity/opacity shader");
        ComPtr<ID3D11PixelShader> ps;
        const HRESULT made=device->CreatePixelShader(patched.data(),patched.size(),nullptr,&ps);
        if (FAILED(made)) { std::fprintf(stderr,"PS validation failed %08lx\n",static_cast<unsigned long>(made)); ok=false; continue; }
        const float clear[4]{}; context->ClearRenderTargetView(target.Get(),clear);
        context->ClearDepthStencilView(depth_view.Get(),D3D11_CLEAR_DEPTH,.1f,0);
        ID3D11RenderTargetView* rt=target.Get(); context->OMSetRenderTargets(1,&rt,depth_view.Get());
        context->OMSetDepthStencilState(depth_state.Get(),0); context->OMSetBlendState(nullptr,nullptr,UINT32_MAX);
        context->RSSetState(raster_state.Get()); D3D11_VIEWPORT viewport{0,0,2,1,0,1}; context->RSSetViewports(1,&viewport);
        context->VSSetShader(vs.Get(),nullptr,0); context->PSSetShader(ps.Get(),nullptr,0);
        ID3D11ShaderResourceView* srv=input_view.Get(); context->PSSetShaderResources(0,1,&srv);
        ID3D11Buffer* constant=cb.Get(); context->PSSetConstantBuffers(13,1,&constant);
        context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST); context->Draw(3,0);
        context->OMSetRenderTargets(0,nullptr,nullptr);
        for (uint32_t plane=0;plane<2;++plane) {
            auto* source=plane ? depth.Get() : output.Get(); source->GetDesc(&desc);
            desc.Usage=D3D11_USAGE_STAGING; desc.BindFlags=0; desc.CPUAccessFlags=D3D11_CPU_ACCESS_READ;
            ComPtr<ID3D11Texture2D> readable; check(SUCCEEDED(device->CreateTexture2D(&desc,nullptr,&readable)),"create readback");
            context->CopyResource(readable.Get(),source); D3D11_MAPPED_SUBRESOURCE mapped{};
            check(SUCCEEDED(context->Map(readable.Get(),0,D3D11_MAP_READ,0,&mapped)),"read pass output");
            if (!mapped.pData) continue;
            float values[4]{}; std::memcpy(values,mapped.pData,plane ? 8u : 16u); context->Unmap(readable.Get(),0);
            if (plane) check(std::fabs(values[0]-.75f)<1e-6f && std::fabs(values[1]-.1f)<1e-6f,
                "opacity-qualified fragment writes depth; clipped fragment preserves opaque depth");
            else check(std::fabs(values[0]-(32767.f/65535.f-.2f*.2495f))<1e-5f &&
                std::fabs(values[1]-32767.f/65535.f)<1e-5f && values[2]==0 && values[3]==0,
                "generated-geometry velocity survives; below-threshold translucent/additive fragment is unwritten");
        }
    }
    // Complete producer: record generated vertices, reconstruct independent motion on the next
    // frame, clip material opacity, then commit the exact engine velocity/depth allocations.
    context->ClearState();rsf_ac7_velocity_set_log(pass_log,nullptr);
    auto moving_vertex=compile("cbuffer Object:register(b0){float4 offset;}"
        "float4 main(uint id:SV_VertexID):SV_Position"
        "{return float4(id==2?3:-1,id==1?3:-1,.75,1)+offset;}","vs_5_0");
    auto material=compile(materials[0],"ps_5_0");
    ComPtr<ID3D11VertexShader> moving_vs;ComPtr<ID3D11PixelShader> material_ps;
    check(SUCCEEDED(device->CreateVertexShader(moving_vertex.data(),moving_vertex.size(),nullptr,&moving_vs)) &&
        SUCCEEDED(device->CreatePixelShader(material.data(),material.size(),nullptr,&material_ps)),"create native moving material shaders");
    rsf_ac7_velocity_shader_seen(moving_vs.Get(),0,moving_vertex.data(),uint32_t(moving_vertex.size()));
    rsf_ac7_velocity_shader_seen(material_ps.Get(),1,material.data(),uint32_t(material.size()));
    D3D11_BUFFER_DESC object_desc{};object_desc.ByteWidth=16;object_desc.BindFlags=D3D11_BIND_CONSTANT_BUFFER;
    ComPtr<ID3D11Buffer> object_cb;check(SUCCEEDED(device->CreateBuffer(&object_desc,nullptr,&object_cb)),"create native object constants");
    D3D11_TEXTURE2D_DESC engine_desc{};engine_desc.Width=2;engine_desc.Height=1;
    engine_desc.MipLevels=engine_desc.ArraySize=1;engine_desc.SampleDesc.Count=1;
    engine_desc.Format=DXGI_FORMAT_R16G16_UNORM;engine_desc.BindFlags=D3D11_BIND_RENDER_TARGET|D3D11_BIND_SHADER_RESOURCE;
    ComPtr<ID3D11Texture2D> engine_motion;ComPtr<ID3D11RenderTargetView> motion_rt;
    check(SUCCEEDED(device->CreateTexture2D(&engine_desc,nullptr,&engine_motion)) &&
        SUCCEEDED(device->CreateRenderTargetView(engine_motion.Get(),nullptr,&motion_rt)),"create actual engine motion input");
    engine_desc.Format=DXGI_FORMAT_R32_TYPELESS;engine_desc.BindFlags=D3D11_BIND_DEPTH_STENCIL|D3D11_BIND_SHADER_RESOURCE;
    ComPtr<ID3D11Texture2D> engine_depth;ComPtr<ID3D11DepthStencilView> engine_dsv;
    D3D11_DEPTH_STENCIL_VIEW_DESC dsv{};dsv.Format=DXGI_FORMAT_D32_FLOAT;dsv.ViewDimension=D3D11_DSV_DIMENSION_TEXTURE2D;
    check(SUCCEEDED(device->CreateTexture2D(&engine_desc,nullptr,&engine_depth)) &&
        SUCCEEDED(device->CreateDepthStencilView(engine_depth.Get(),&dsv,&engine_dsv)),"create actual engine depth input");
    rsf_game_render_pass pass{};pass.struct_size=sizeof(pass);pass.role=RSF_GAME_RENDER_TRANSLUCENT_MESH;
    pass.session_id=1;pass.family_key=2;pass.view_key=3;pass.history_key=4;pass.flags=RSF_GAME_RENDER_PRIMARY;
    pass.primitive_key=5;pass.vertex_factory_key=6;pass.material_key=7;pass.material_blend=2;pass.material_opacity_clip=.333f;
    pass.camera_valid=1;pass.camera.render_width=2;pass.camera.render_height=1;
    pass.camera.clip_to_previous_clip[0]=pass.camera.clip_to_previous_clip[5]=
        pass.camera.clip_to_previous_clip[10]=pass.camera.clip_to_previous_clip[15]=1;
    rsf_frame_tap_geometry native_geometry{};native_geometry.context=context.Get();native_geometry.width=2;native_geometry.height=1;native_geometry.samples=1;
    native_geometry.target=output.Get();native_geometry.depth_view=engine_dsv.Get();native_geometry.kind=0;native_geometry.count=3;
    native_geometry.vertex_shader=moving_vs.Get();native_geometry.topology=D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
    D3D11_BUFFER_DESC indirect_desc{};indirect_desc.ByteWidth=48;indirect_desc.MiscFlags=D3D11_RESOURCE_MISC_DRAWINDIRECT_ARGS;
    ComPtr<ID3D11Buffer> indirect_args;check(SUCCEEDED(device->CreateBuffer(&indirect_desc,nullptr,&indirect_args)),"create native GPU indirect arguments");
    D3D11_BUFFER_DESC index_desc{};index_desc.ByteWidth=6;index_desc.BindFlags=D3D11_BIND_INDEX_BUFFER;
    const uint16_t indices[]{0,1,2};D3D11_SUBRESOURCE_DATA index_initial{indices,0,0};ComPtr<ID3D11Buffer> index_buffer;
    check(SUCCEEDED(device->CreateBuffer(&index_desc,&index_initial,&index_buffer)),"create native indexed-indirect geometry");
    auto generated_code=compile("cbuffer Generated:register(b1){float4 shift;}struct V{float4 p:SV_Position;};"
        "[maxvertexcount(3)]void main(triangle V input[3],inout TriangleStream<V> stream)"
        "{[unroll]for(uint i=0;i<3;i++){V output;output.p=input[i].p+shift;stream.Append(output);}}","gs_5_0");
    ComPtr<ID3D11GeometryShader> native_gs;
    check(SUCCEEDED(device->CreateGeometryShader(generated_code.data(),generated_code.size(),nullptr,&native_gs)),"create native geometry producer");
    rsf_ac7_velocity_shader_seen(native_gs.Get(),2,generated_code.data(),uint32_t(generated_code.size()));
    ComPtr<ID3D11Buffer> generated_cb;
    check(SUCCEEDED(device->CreateBuffer(&object_desc,nullptr,&generated_cb)),"create native GS deformation constants");
    auto patch_vertex=compile("cbuffer Object:register(b0){float4 offset;}struct V{float4 p:VS_To_DS_Position;};"
        "V main(uint i:SV_VertexID){V o;o.p=float4(i==2?3:-1,i==1?3:-1,.75,1)+offset;return o;}","vs_5_0");
    auto patch_hull=compile("cbuffer Tess:register(b1){float4 factor;}struct V{float4 p:VS_To_DS_Position;};"
        "struct C{float e[3]:SV_TessFactor;float i:SV_InsideTessFactor;};"
        "C constants(InputPatch<V,3> p){C o;o.e[0]=o.e[1]=o.e[2]=factor.x;o.i=factor.x;return o;}"
        "[domain(\"tri\")][partitioning(\"fractional_even\")][outputtopology(\"triangle_cw\")]"
        "[outputcontrolpoints(3)][patchconstantfunc(\"constants\")]V main(InputPatch<V,3> p,uint i:SV_OutputControlPointID){return p[i];}","hs_5_0");
    auto patch_domain=compile("cbuffer Deform:register(b2){float4 shift;}struct V{float4 p:VS_To_DS_Position;};"
        "struct C{float e[3]:SV_TessFactor;float i:SV_InsideTessFactor;};"
        "[domain(\"tri\")]float4 main(C c,const OutputPatch<V,3> p,float3 d:SV_DomainLocation):SV_Position"
        "{float4 o=p[0].p*d.x+p[1].p*d.y+p[2].p*d.z+shift;o.z+=.1*d.x*d.y;return o;}","ds_5_0");
    ComPtr<ID3D11VertexShader> patch_vs;ComPtr<ID3D11HullShader> patch_hs;ComPtr<ID3D11DomainShader> patch_ds;
    check(SUCCEEDED(device->CreateVertexShader(patch_vertex.data(),patch_vertex.size(),nullptr,&patch_vs)) &&
        SUCCEEDED(device->CreateHullShader(patch_hull.data(),patch_hull.size(),nullptr,&patch_hs)) &&
        SUCCEEDED(device->CreateDomainShader(patch_domain.data(),patch_domain.size(),nullptr,&patch_ds)),"create native adaptive tessellation producer");
    rsf_ac7_velocity_shader_seen(patch_vs.Get(),0,patch_vertex.data(),uint32_t(patch_vertex.size()));
    rsf_ac7_velocity_shader_seen(patch_hs.Get(),3,patch_hull.data(),uint32_t(patch_hull.size()));
    rsf_ac7_velocity_shader_seen(patch_ds.Get(),4,patch_domain.data(),uint32_t(patch_domain.size()));
    auto patch_hull12=compile("cbuffer Tess:register(b1){float4 factor;}struct V{float4 p:VS_To_DS_Position;};"
        "struct C{float e[3]:SV_TessFactor;float i:SV_InsideTessFactor;};"
        "C constants(InputPatch<V,12> p){C o;o.e[0]=o.e[1]=o.e[2]=factor.x;o.i=factor.x;return o;}"
        "[domain(\"tri\")][partitioning(\"fractional_even\")][outputtopology(\"triangle_cw\")]"
        "[outputcontrolpoints(3)][patchconstantfunc(\"constants\")]V main(InputPatch<V,12> p,uint i:SV_OutputControlPointID)"
        "{V o;o.p=p[i].p;o.p.z+=.001;return o;}","hs_5_0");
    ComPtr<ID3D11HullShader> patch_hs12;
    check(SUCCEEDED(device->CreateHullShader(patch_hull12.data(),patch_hull12.size(),nullptr,&patch_hs12)),"create 12-point native control phase");
    rsf_ac7_velocity_shader_seen(patch_hs12.Get(),3,patch_hull12.data(),uint32_t(patch_hull12.size()));
    ComPtr<ID3D11Buffer> tess_cb,deform_cb;
    check(SUCCEEDED(device->CreateBuffer(&object_desc,nullptr,&tess_cb)) && SUCCEEDED(device->CreateBuffer(&object_desc,nullptr,&deform_cb)),"create native tessellation/domain constants");
    for (uint32_t frame=1;frame<=13;++frame) {
        pass.native_frame=frame;pass.role=RSF_GAME_RENDER_TRANSLUCENT_MESH;
        pass.flags=RSF_GAME_RENDER_PRIMARY|((frame==3 || frame==7 || frame==10 || frame==13) ? RSF_GAME_RENDER_RESET : 0u);
        pass.material_blend=frame==13 ? 5u : 2u;
        pass.geometry_capacity=3;
        uint32_t arguments[12]{};arguments[4]=3;arguments[5]=frame==4 ? 2u : 1u;
        if (frame==12) arguments[4]=12;
        context->UpdateSubresource(indirect_args.Get(),0,nullptr,arguments,0,0);
        native_geometry.kind=frame==2 ? 5u : frame==3 ? 0u : 6u;
        native_geometry.count=frame==3 ? 3u : 0u;native_geometry.indirect_buffer=indirect_args.Get();native_geometry.indirect_offset=16;
        if (frame>=5) {native_geometry.kind=0;native_geometry.count=3;}
        if (frame>=11) native_geometry.count=12;
        if (frame==12) {native_geometry.kind=6;native_geometry.count=0;}
        context->IASetIndexBuffer(index_buffer.Get(),DXGI_FORMAT_R16_UINT,0);
        rsf_ac7_velocity_scope(&pass);
        const float offset[4]{frame>=11 ? float(frame-11)*.1f : frame>=8 ? float(frame-8)*.1f : frame>=5 ? float(frame-5)*.1f : float(frame-1)*.2f,0,0,0};context->UpdateSubresource(object_cb.Get(),0,nullptr,offset,0,0);
        const float generated_offset[4]{frame>=5 ? float(frame-5)*.1f : 0,0,0,0};
        context->UpdateSubresource(generated_cb.Get(),0,nullptr,generated_offset,0,0);
        ID3D11Buffer* generated=generated_cb.Get();context->GSSetConstantBuffers(1,1,&generated);
        context->GSSetShader(frame>=5 && frame<8 ? native_gs.Get() : nullptr,nullptr,0);
        context->HSSetShader(frame>=11 ? patch_hs12.Get() : frame>=8 ? patch_hs.Get() : nullptr,nullptr,0);context->DSSetShader(frame>=8 ? patch_ds.Get() : nullptr,nullptr,0);
        const float tess_factor[4]{frame==8 ? 2.f : 6.f,0,0,0};context->UpdateSubresource(tess_cb.Get(),0,nullptr,tess_factor,0,0);
        const float domain_shift[4]{frame>=11 ? float(frame-11)*.1f : frame>=8 ? float(frame-8)*.1f : 0,0,0,0};context->UpdateSubresource(deform_cb.Get(),0,nullptr,domain_shift,0,0);
        ID3D11Buffer* tess=tess_cb.Get();ID3D11Buffer* deform=deform_cb.Get();context->HSSetConstantBuffers(1,1,&tess);context->DSSetConstantBuffers(2,1,&deform);
        context->ClearDepthStencilView(engine_dsv.Get(),D3D11_CLEAR_DEPTH,.1f,0);
        const float opaque_motion[4]{.7f,.6f,0,0};context->ClearRenderTargetView(motion_rt.Get(),opaque_motion);
        ID3D11RenderTargetView* rt=target.Get();context->OMSetRenderTargets(1,&rt,engine_dsv.Get());
        context->OMSetDepthStencilState(depth_state.Get(),0);context->OMSetBlendState(nullptr,nullptr,UINT32_MAX);
        context->VSSetShader(frame>=8 ? patch_vs.Get() : moving_vs.Get(),nullptr,0);context->PSSetShader(material_ps.Get(),nullptr,0);
        native_geometry.vertex_shader=frame>=8 ? patch_vs.Get() : moving_vs.Get();
        ID3D11Buffer* object=object_cb.Get();context->VSSetConstantBuffers(0,1,&object);
        ID3D11ShaderResourceView* srv=input_view.Get();context->PSSetShaderResources(0,1,&srv);
        context->RSSetState(raster_state.Get());D3D11_VIEWPORT viewport{0,0,2,1,0,1};context->RSSetViewports(1,&viewport);
        native_geometry.topology=frame>=11 ? D3D11_PRIMITIVE_TOPOLOGY_12_CONTROL_POINT_PATCHLIST : frame>=8 ? D3D11_PRIMITIVE_TOPOLOGY_3_CONTROL_POINT_PATCHLIST : D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
        context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY(native_geometry.topology));
        // The normal colour draw has a read-only depth contract; only the new pass writes depth.
        rsf_ac7_velocity_geometry(&native_geometry);
        ComPtr<ID3D11VertexShader> restored_vs;ComPtr<ID3D11PixelShader> restored_ps;
        context->VSGetShader(&restored_vs,nullptr,nullptr);context->PSGetShader(&restored_ps,nullptr,nullptr);
        check(restored_vs.Get()==(frame>=8 ? patch_vs.Get() : moving_vs.Get()) && restored_ps.Get()==material_ps.Get(),"generated-geometry passes restore native shaders");
        ComPtr<ID3D11GeometryShader> restored_gs;context->GSGetShader(&restored_gs,nullptr,nullptr);
        check(restored_gs.Get()==(frame>=5 && frame<8 ? native_gs.Get() : nullptr),"velocity replay restores native geometry stage");
        ComPtr<ID3D11HullShader> restored_hs;ComPtr<ID3D11DomainShader> restored_ds;
        context->HSGetShader(&restored_hs,nullptr,nullptr);context->DSGetShader(&restored_ds,nullptr,nullptr);
        check(restored_hs.Get()==(frame>=11 ? patch_hs12.Get() : frame>=8 ? patch_hs.Get() : nullptr) && restored_ds.Get()==(frame>=8 ? patch_ds.Get() : nullptr),"velocity replay restores native tessellation stages");
        pass.role=RSF_GAME_RENDER_VELOCITY_FINALIZE;pass.motion=engine_motion.Get();pass.depth=engine_depth.Get();
        check(rsf_ac7_velocity_commit(context.Get(),&pass)!=0,"commit native translucent velocity/depth producer");
        context->OMSetRenderTargets(0,nullptr,nullptr);
        engine_motion->GetDesc(&engine_desc);engine_desc.BindFlags=0;engine_desc.Usage=D3D11_USAGE_STAGING;engine_desc.CPUAccessFlags=D3D11_CPU_ACCESS_READ;
        ComPtr<ID3D11Texture2D> readable;check(SUCCEEDED(device->CreateTexture2D(&engine_desc,nullptr,&readable)),"create committed motion readback");
        context->CopyResource(readable.Get(),engine_motion.Get());D3D11_MAPPED_SUBRESOURCE mapped{};
        check(SUCCEEDED(context->Map(readable.Get(),0,D3D11_MAP_READ,0,&mapped)),"read committed native motion");
        if (mapped.pData) {
            uint16_t values[4]{};std::memcpy(values,mapped.pData,sizeof(values));context->Unmap(readable.Get(),0);
            const float expected=frame==4 ? .7f : 32767.f/65535.f+((frame==2 || frame==6 || frame==9 || frame==12) ? .2f*.2495f : 0);
            std::fprintf(stderr,"frame %u motion %.6f,%.6f opaque %.6f expected %.6f\n",frame,
                float(values[0])/65535.f,float(values[1])/65535.f,float(values[2])/65535.f,expected);
            check(std::fabs(float(values[0])/65535.f-expected)<5e-5f &&
                std::fabs(float(values[2])/65535.f-(frame==13 ? 32767.f/65535.f : .7f))<3e-5f,
                "direct/indirect motion respects GPU arguments, camera cuts, clipped opacity and the native storage bound");
        }
        rsf_ac7_velocity_frame_end();
    }
    rsf_ac7_velocity_release();
    pass.material_blend=2;
    context->HSSetShader(nullptr,nullptr,0);context->DSSetShader(nullptr,nullptr,0);
    context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);native_geometry.topology=D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
    // Particle counters and corners, rather than draw order or pooled vertex-factory addresses,
    // identify the prior billboard. Reverse particle order while moving the foreground sprite.
    auto particle_vertex=compile("float4 main(float4 centre:ATTRIBUTE0,float4 old:ATTRIBUTE1,"
        "float4 size:ATTRIBUTE2,float2 corner:ATTRIBUTE4):SV_Position"
        "{return float4(centre.xy+corner,centre.z,1);}","vs_5_0");
    ComPtr<ID3D11VertexShader> particle_vs;
    check(SUCCEEDED(device->CreateVertexShader(particle_vertex.data(),particle_vertex.size(),nullptr,&particle_vs)),"create native sprite VS");
    rsf_ac7_velocity_shader_seen(particle_vs.Get(),0,particle_vertex.data(),uint32_t(particle_vertex.size()));
    rsf_ac7_velocity_shader_seen(material_ps.Get(),1,material.data(),uint32_t(material.size()));
    const D3D11_INPUT_ELEMENT_DESC particle_layout[]{
        {"ATTRIBUTE",0,DXGI_FORMAT_R32G32B32A32_FLOAT,0,0,D3D11_INPUT_PER_VERTEX_DATA,0},
        {"ATTRIBUTE",1,DXGI_FORMAT_R32G32B32A32_FLOAT,0,16,D3D11_INPUT_PER_VERTEX_DATA,0},
        {"ATTRIBUTE",2,DXGI_FORMAT_R32G32B32A32_FLOAT,0,32,D3D11_INPUT_PER_VERTEX_DATA,0},
        {"ATTRIBUTE",4,DXGI_FORMAT_R32G32_FLOAT,0,48,D3D11_INPUT_PER_VERTEX_DATA,0},
    };
    ComPtr<ID3D11InputLayout> sprite_layout;
    check(SUCCEEDED(device->CreateInputLayout(particle_layout,4,particle_vertex.data(),particle_vertex.size(),&sprite_layout)),"create native sprite declaration");
    struct Particle {float centre[4],old[4],size[4],corner[2];};
    std::array<Particle,6> particles{};
    D3D11_BUFFER_DESC sprite_desc{};sprite_desc.ByteWidth=sizeof(particles);sprite_desc.BindFlags=D3D11_BIND_VERTEX_BUFFER;
    ComPtr<ID3D11Buffer> sprite_buffer;check(SUCCEEDED(device->CreateBuffer(&sprite_desc,nullptr,&sprite_buffer)),"create native sprite stream");
    pass.flags=RSF_GAME_RENDER_PRIMARY;pass.primitive_key=8;pass.material_key=9;
    native_geometry.vertex_shader=particle_vs.Get();native_geometry.count=6;native_geometry.input_layout=sprite_layout.Get();
    native_geometry.kind=0;native_geometry.indirect_buffer=nullptr;native_geometry.indirect_offset=0;
    native_geometry.vertex_buffers[0]=sprite_buffer.Get();native_geometry.strides[0]=sizeof(Particle);
    for (uint32_t frame=0;frame<2;++frame) {
        const float corners[3][2]{{-1,-1},{3,-1},{-1,3}};
        for (uint32_t p=0;p<2;++p) for (uint32_t vertex_index=0;vertex_index<3;++vertex_index) {
            const uint32_t id=frame ? 1-p : p;auto& value=particles[p*3+vertex_index];
            value={};value.centre[0]=(id==0 ? 1.f : -1.f)*float(frame)*.2f;
            value.centre[2]=id==0 ? .75f : .5f;value.old[2]=value.centre[2];value.old[3]=float(id+1)/10000.f;
            value.size[0]=value.size[1]=1;std::memcpy(value.corner,corners[vertex_index],sizeof(value.corner));
        }
        context->UpdateSubresource(sprite_buffer.Get(),0,nullptr,particles.data(),0,0);
        pass.native_frame=frame+8;pass.role=RSF_GAME_RENDER_TRANSLUCENT_MESH;pass.vertex_factory_key=frame ? 900 : 6;
        rsf_ac7_velocity_scope(&pass);
        context->ClearDepthStencilView(engine_dsv.Get(),D3D11_CLEAR_DEPTH,.1f,0);
        const float clear[4]{.7f,.6f,0,0};context->ClearRenderTargetView(motion_rt.Get(),clear);
        ID3D11RenderTargetView* rt=target.Get();context->OMSetRenderTargets(1,&rt,engine_dsv.Get());
        context->VSSetShader(particle_vs.Get(),nullptr,0);context->PSSetShader(material_ps.Get(),nullptr,0);
        context->GSSetShader(nullptr,nullptr,0);context->IASetInputLayout(sprite_layout.Get());
        ID3D11Buffer* vb=sprite_buffer.Get();UINT stride=sizeof(Particle),offset=0;context->IASetVertexBuffers(0,1,&vb,&stride,&offset);
        ID3D11ShaderResourceView* srv=input_view.Get();context->PSSetShaderResources(0,1,&srv);
        rsf_ac7_velocity_geometry(&native_geometry);
        pass.role=RSF_GAME_RENDER_VELOCITY_FINALIZE;pass.motion=engine_motion.Get();pass.depth=engine_depth.Get();
        check(rsf_ac7_velocity_commit(context.Get(),&pass)!=0,"commit sorted sprite motion");
        context->OMSetRenderTargets(0,nullptr,nullptr);engine_motion->GetDesc(&engine_desc);
        engine_desc.BindFlags=0;engine_desc.Usage=D3D11_USAGE_STAGING;engine_desc.CPUAccessFlags=D3D11_CPU_ACCESS_READ;
        ComPtr<ID3D11Texture2D> readable;check(SUCCEEDED(device->CreateTexture2D(&engine_desc,nullptr,&readable)),"create sorted sprite readback");
        context->CopyResource(readable.Get(),engine_motion.Get());D3D11_MAPPED_SUBRESOURCE mapped{};
        check(SUCCEEDED(context->Map(readable.Get(),0,D3D11_MAP_READ,0,&mapped)),"read sorted sprite motion");
        if (mapped.pData) {
            uint16_t values[4]{};std::memcpy(values,mapped.pData,sizeof(values));context->Unmap(readable.Get(),0);
            const float expected=32767.f/65535.f+(frame ? .2f*.2495f : 0);
            check(std::fabs(float(values[0])/65535.f-expected)<5e-5f,"sorted sprite retains its own previous generated geometry after vertex-factory recycling");
        }
        rsf_ac7_velocity_frame_end();
    }
    rsf_ac7_velocity_release();
    // Native mesh-particle previous transforms follow the current sorted instance order. Equal
    // translations with different rotation/scale must still be distinct history identities.
    auto mesh_vertex=compile("float4 main(uint vertex:SV_VertexID,float4 a:ATTRIBUTE8,float4 b:ATTRIBUTE9,"
        "float4 c:ATTRIBUTE10):SV_Position {float4 p=float4(vertex==2?3:-1,vertex==1?3:-1,.75,1);"
        "return float4(dot(p,a),dot(p,b),dot(p,c),1);}","vs_5_0");
    ComPtr<ID3D11VertexShader> mesh_vs;
    check(SUCCEEDED(device->CreateVertexShader(mesh_vertex.data(),mesh_vertex.size(),nullptr,&mesh_vs)),"create native mesh-particle shader");
    rsf_ac7_velocity_shader_seen(mesh_vs.Get(),0,mesh_vertex.data(),uint32_t(mesh_vertex.size()));
    rsf_ac7_velocity_shader_seen(material_ps.Get(),1,material.data(),uint32_t(material.size()));
    const D3D11_INPUT_ELEMENT_DESC mesh_layout[]{
        {"ATTRIBUTE",8,DXGI_FORMAT_R32G32B32A32_FLOAT,0,0,D3D11_INPUT_PER_INSTANCE_DATA,1},
        {"ATTRIBUTE",9,DXGI_FORMAT_R32G32B32A32_FLOAT,0,16,D3D11_INPUT_PER_INSTANCE_DATA,1},
        {"ATTRIBUTE",10,DXGI_FORMAT_R32G32B32A32_FLOAT,0,32,D3D11_INPUT_PER_INSTANCE_DATA,1},
    };
    ComPtr<ID3D11InputLayout> mesh_declaration;
    check(SUCCEEDED(device->CreateInputLayout(mesh_layout,3,mesh_vertex.data(),mesh_vertex.size(),&mesh_declaration)),"create sorted mesh-particle declaration");
    using Transform=std::array<float,12>;std::array<Transform,2> transforms{},previous_transforms{};
    D3D11_BUFFER_DESC instance_desc{};instance_desc.ByteWidth=sizeof(transforms);instance_desc.BindFlags=D3D11_BIND_VERTEX_BUFFER;
    ComPtr<ID3D11Buffer> instance_buffer,previous_buffer;
    check(SUCCEEDED(device->CreateBuffer(&instance_desc,nullptr,&instance_buffer)),"create current instance transforms");
    instance_desc.BindFlags=D3D11_BIND_SHADER_RESOURCE;
    check(SUCCEEDED(device->CreateBuffer(&instance_desc,nullptr,&previous_buffer)),"create native previous instance transforms");
    D3D11_SHADER_RESOURCE_VIEW_DESC previous_desc{};previous_desc.Format=DXGI_FORMAT_R32G32B32A32_FLOAT;
    previous_desc.ViewDimension=D3D11_SRV_DIMENSION_BUFFER;previous_desc.Buffer.NumElements=6;
    ComPtr<ID3D11ShaderResourceView> previous_view;
    check(SUCCEEDED(device->CreateShaderResourceView(previous_buffer.Get(),&previous_desc,&previous_view)),"create native previous-transform SRV");
    pass.primitive_key=50;pass.material_key=51;pass.flags=RSF_GAME_RENDER_PRIMARY;
    native_geometry.vertex_shader=mesh_vs.Get();native_geometry.input_layout=mesh_declaration.Get();
    native_geometry.kind=2;native_geometry.count=3;native_geometry.instances=2;
    native_geometry.vertex_buffers[0]=instance_buffer.Get();native_geometry.strides[0]=sizeof(Transform);
    for (uint32_t frame=0;frame<2;++frame) {
        for (uint32_t slot=0;slot<2;++slot) {
            const uint32_t id=frame ? 1-slot : slot;
            Transform pose{1,0,0,0, 0,1,0,0, 0,0,1,id==0 ? 0.f : -.25f};
            previous_transforms[slot]=pose;pose[3]=(id==0 ? 1.f : -1.f)*float(frame)*.2f;transforms[slot]=pose;
        }
        context->UpdateSubresource(instance_buffer.Get(),0,nullptr,transforms.data(),0,0);
        context->UpdateSubresource(previous_buffer.Get(),0,nullptr,previous_transforms.data(),0,0);
        pass.native_frame=20+frame;pass.role=RSF_GAME_RENDER_TRANSLUCENT_MESH;pass.vertex_factory_key=frame ? 777 : 6;
        rsf_ac7_velocity_scope(&pass);
        context->ClearDepthStencilView(engine_dsv.Get(),D3D11_CLEAR_DEPTH,.1f,0);
        const float clear[4]{.7f,.6f,0,0};context->ClearRenderTargetView(motion_rt.Get(),clear);
        ID3D11RenderTargetView* rt=target.Get();context->OMSetRenderTargets(1,&rt,engine_dsv.Get());
        context->VSSetShader(mesh_vs.Get(),nullptr,0);context->PSSetShader(material_ps.Get(),nullptr,0);
        context->GSSetShader(nullptr,nullptr,0);context->IASetInputLayout(mesh_declaration.Get());
        ID3D11Buffer* vb=instance_buffer.Get();UINT stride=sizeof(Transform),offset=0;context->IASetVertexBuffers(0,1,&vb,&stride,&offset);
        ID3D11ShaderResourceView* history=previous_view.Get();context->VSSetShaderResources(127,1,&history);
        ID3D11ShaderResourceView* texture=input_view.Get();context->PSSetShaderResources(0,1,&texture);
        rsf_ac7_velocity_geometry(&native_geometry);
        pass.role=RSF_GAME_RENDER_VELOCITY_FINALIZE;pass.motion=engine_motion.Get();pass.depth=engine_depth.Get();
        check(rsf_ac7_velocity_commit(context.Get(),&pass)!=0,"commit reordered mesh-particle history");
        context->OMSetRenderTargets(0,nullptr,nullptr);engine_motion->GetDesc(&engine_desc);
        engine_desc.BindFlags=0;engine_desc.Usage=D3D11_USAGE_STAGING;engine_desc.CPUAccessFlags=D3D11_CPU_ACCESS_READ;
        ComPtr<ID3D11Texture2D> readable;check(SUCCEEDED(device->CreateTexture2D(&engine_desc,nullptr,&readable)),"create mesh-particle motion readback");
        context->CopyResource(readable.Get(),engine_motion.Get());D3D11_MAPPED_SUBRESOURCE mapped{};
        check(SUCCEEDED(context->Map(readable.Get(),0,D3D11_MAP_READ,0,&mapped)),"read reordered instance motion");
        if (mapped.pData) {
            uint16_t values[4]{};std::memcpy(values,mapped.pData,sizeof(values));context->Unmap(readable.Get(),0);
            const float expected=32767.f/65535.f+(frame ? .2f*.2495f : 0);
            check(std::fabs(float(values[0])/65535.f-expected)<5e-5f,"mesh-particle motion follows prior transforms despite instance sorting and factory recycling");
        }
        rsf_ac7_velocity_frame_end();
    }
    rsf_ac7_velocity_release();
    if (argc==2) for (const auto& entry:std::filesystem::directory_iterator(argv[1])) {
        const auto name=entry.path().filename().string();
        if ((!name.starts_with("1-") && !name.starts_with("0-")) || entry.path().extension()!=".dxbc") continue;
        std::ifstream file(entry.path(),std::ios::binary); std::vector<uint8_t> native{
            std::istreambuf_iterator<char>(file),std::istreambuf_iterator<char>()};
        std::vector<uint8_t> patched;
        if (name.starts_with("0-")) {
            rsf_ac7_velocity_vertex_info info;
            if (rsf_ac7_velocity_vertex_shader(native.data(),native.size(),patched,info)!=rsf_ac7_velocity_shader_result::patched) {
                std::fprintf(stderr,"retained VS refused: %s\n",name.c_str());ok=false;continue;
            }
            ComPtr<ID3D11VertexShader> created;
            if (FAILED(device->CreateVertexShader(patched.data(),patched.size(),nullptr,&created))) {
                std::fprintf(stderr,"retained VS creation refused: %s\n",name.c_str());ok=false;
            }
            std::vector<uint8_t> geometry;uint32_t previous=0,current_clip=0;
            if (!rsf_ac7_velocity_geometry_shader(info,geometry,previous,current_clip)) {
                std::fprintf(stderr,"retained GS compile refused: %s\n",name.c_str());ok=false;continue;
            }
            ComPtr<ID3D11GeometryShader> gs;
            if (FAILED(device->CreateGeometryShader(geometry.data(),geometry.size(),nullptr,&gs))) {
                std::fprintf(stderr,"retained GS creation refused: %s\n",name.c_str());ok=false;
            }
            continue;
        }
        if (rsf_ac7_velocity_pixel_shader(native.data(),native.size(),2,patched)!=rsf_ac7_velocity_shader_result::patched) {
            std::fprintf(stderr,"retained shader refused: %s\n",name.c_str()); ok=false; continue;
        }
        ComPtr<ID3D11PixelShader> ps;
        if (FAILED(device->CreatePixelShader(patched.data(),patched.size(),nullptr,&ps))) {
            std::fprintf(stderr,"retained shader creation refused: %s\n",name.c_str()); ok=false;
        }
    }
    return ok ? 0 : 1;
}
