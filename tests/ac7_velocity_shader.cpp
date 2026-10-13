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
    }
    const char* materials[]{
        "Texture2D<float4> data:register(t0);float4 main(float4 p:SV_Position):SV_Target0"
        "{float4 c=data.Load(int3(int2(p.xy),0));return float4(max(c.xyz,0),saturate(c.w));}",
        "Texture2D<float4> data:register(t0);float4 main(float4 p:SV_Position):SV_Target0"
        "{float4 c=data.Load(int3(int2(p.xy),0));float a=saturate(c.w);return float4(max(c.xyz,0)*a,0);}",
    };
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
    for (uint32_t frame=1;frame<=3;++frame) {
        pass.native_frame=frame;pass.role=RSF_GAME_RENDER_TRANSLUCENT_MESH;
        if (frame==3) pass.flags|=RSF_GAME_RENDER_RESET;
        rsf_ac7_velocity_scope(&pass);
        const float offset[4]{float(frame-1)*.2f,0,0,0};context->UpdateSubresource(object_cb.Get(),0,nullptr,offset,0,0);
        context->ClearDepthStencilView(engine_dsv.Get(),D3D11_CLEAR_DEPTH,.1f,0);
        const float opaque_motion[4]{.7f,.6f,0,0};context->ClearRenderTargetView(motion_rt.Get(),opaque_motion);
        ID3D11RenderTargetView* rt=target.Get();context->OMSetRenderTargets(1,&rt,engine_dsv.Get());
        context->OMSetDepthStencilState(depth_state.Get(),0);context->OMSetBlendState(nullptr,nullptr,UINT32_MAX);
        context->VSSetShader(moving_vs.Get(),nullptr,0);context->PSSetShader(material_ps.Get(),nullptr,0);
        ID3D11Buffer* object=object_cb.Get();context->VSSetConstantBuffers(0,1,&object);
        ID3D11ShaderResourceView* srv=input_view.Get();context->PSSetShaderResources(0,1,&srv);
        context->RSSetState(raster_state.Get());D3D11_VIEWPORT viewport{0,0,2,1,0,1};context->RSSetViewports(1,&viewport);
        context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        // The normal colour draw has a read-only depth contract; only the new pass writes depth.
        rsf_ac7_velocity_geometry(&native_geometry);
        ComPtr<ID3D11VertexShader> restored_vs;ComPtr<ID3D11PixelShader> restored_ps;
        context->VSGetShader(&restored_vs,nullptr,nullptr);context->PSGetShader(&restored_ps,nullptr,nullptr);
        check(restored_vs.Get()==moving_vs.Get() && restored_ps.Get()==material_ps.Get(),"generated-geometry passes restore native shaders");
        pass.role=RSF_GAME_RENDER_VELOCITY_FINALIZE;pass.motion=engine_motion.Get();pass.depth=engine_depth.Get();
        check(rsf_ac7_velocity_commit(context.Get(),&pass)!=0,"commit native translucent velocity/depth producer");
        context->OMSetRenderTargets(0,nullptr,nullptr);
        engine_motion->GetDesc(&engine_desc);engine_desc.BindFlags=0;engine_desc.Usage=D3D11_USAGE_STAGING;engine_desc.CPUAccessFlags=D3D11_CPU_ACCESS_READ;
        ComPtr<ID3D11Texture2D> readable;check(SUCCEEDED(device->CreateTexture2D(&engine_desc,nullptr,&readable)),"create committed motion readback");
        context->CopyResource(readable.Get(),engine_motion.Get());D3D11_MAPPED_SUBRESOURCE mapped{};
        check(SUCCEEDED(context->Map(readable.Get(),0,D3D11_MAP_READ,0,&mapped)),"read committed native motion");
        if (mapped.pData) {
            uint16_t values[4]{};std::memcpy(values,mapped.pData,sizeof(values));context->Unmap(readable.Get(),0);
            const float expected=32767.f/65535.f+(frame==2 ? .2f*.2495f : 0);
            std::fprintf(stderr,"frame %u motion %.6f,%.6f opaque %.6f expected %.6f\n",frame,
                float(values[0])/65535.f,float(values[1])/65535.f,float(values[2])/65535.f,expected);
            check(std::fabs(float(values[0])/65535.f-expected)<5e-5f &&
                std::fabs(float(values[2])/65535.f-.7f)<3e-5f,
                "native input contains independent generated-geometry motion, resets on cuts and preserves clipped opaque motion");
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
