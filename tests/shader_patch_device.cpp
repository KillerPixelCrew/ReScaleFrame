// SPDX-License-Identifier: GPL-3.0-only
#include "patch.h"
#include "dxbc.h"
#include <d3d11.h>
#include <d3dcompiler.h>
#include <wrl/client.h>
#include <bit>
#include <cmath>
#include <cstring>
#include <filesystem>
#include <iostream>
using namespace rsf::patch;
using Microsoft::WRL::ComPtr;
extern "C" int32_t rsf_patch_query(rsf_shader_patch_api*);
namespace {
void require(bool value,const char* reason){check(value,RSF_PATCH_INTERNAL,reason);}
std::string compile(const char* text,const char* target){ComPtr<ID3DBlob> blob,error;
    require(SUCCEEDED(D3DCompile(text,std::strlen(text),"",nullptr,nullptr,"main",target,0,0,&blob,&error)),"fixture compile");
    return {static_cast<const char*>(blob->GetBufferPointer()),blob->GetBufferSize()};}
void integer(std::string& s,uint64_t value,size_t n){for(size_t i=0;i<n;++i)s+=static_cast<char>((value>>(i*8))&255);}
std::string pack(std::string rules){
    std::string payload;integer(payload,1,4);integer(payload,12,4);integer(payload,7,8);payload+="metadata.txt";payload+="fixture";
    std::vector<std::string> parts{"{\"id\":\"synthetic\"}",std::move(rules),payload};
    std::string out="RSFPATCH";integer(out,1,4);integer(out,3,4);integer(out,24,8);size_t offset=96;
    for(size_t i=0;i<3;++i){integer(out,i+1,4);integer(out,0,4);integer(out,offset,8);integer(out,parts[i].size(),8);offset+=parts[i].size();}
    for(const auto& p:parts)out+=p;return out;
}
std::string json_string(std::string_view s){std::string out="\"";for(char c:s){if(c=='\n')out+="\\n";else{if(c=='"'||c=='\\')out+='\\';out+=c;}}return out+'"';}
int32_t resolve(void* user,const char* name,rsf_patch_bytes* out){auto& f=*static_cast<Files*>(user);auto i=f.find(name);
    if(i==f.end())return RSF_PATCH_SOURCE;*out={i->second.data(),i->second.size()};return 0;}
uint32_t word(std::string_view s,size_t at){uint32_t n{};require(at+4<=s.size(),"fixture bounds");std::memcpy(&n,s.data()+at,4);return n;}
void source_operations() {
    const auto apply = [](const std::string& source, const std::string& rule) {
        Pack parsed;
        parsed.load(pack("{\"schema\":1,\"sources\":{\"fixture.h\":" + json_string(sha256(source)) +
            "},\"files\":{\"fixture.h\":" + rule + "}}"));
        const Files inputs{{"fixture.h", source}};
        const auto first = parsed.transform(inputs);
        require(first == parsed.transform(inputs), "deterministic source tree");
        return first.at("fixture.h");
    };
    const auto refused = [&](const std::string& source, const std::string& rule) {
        try { (void)apply(source, rule); }
        catch (const Error& e) { require(e.code == RSF_PATCH_ANCHOR, "anchor refusal reason"); return; }
        require(false, "missing/ambiguous anchor must refuse");
    };
    const std::string io = "struct O {float4 p;};";
    const std::string insert = "{\"op\":\"inserts\",\"edits\":[{\"at\":19,\"anchor_at\":10,\"anchor\":\"float4 p;\",\"text\":\"float v;\"}]}";
    require(apply(io, insert) == "struct O {float4 p;float v;};", "anchored signature insertion");
    refused("struct O {float4 q;};", insert);
    const std::string extract = "{\"op\":\"extract\",\"source\":\"fixture.h\",\"start\":\"float skin(\",\"end\":\"#endif\","
        "\"prefix\":\"// RSF clone\\n\",\"suffix\":\"\",\"remap\":{\"skin\":\"previous_skin\",\"x\":\"old_x\"}}";
    require(apply("#define A 1\nfloat skin(float x){return x*A;}\n#endif\n", extract) ==
        "// RSF clone\nfloat previous_skin(float old_x){return old_x*A;}\n", "range extraction and token remap");
    refused("#define A 1\n#endif\n", extract);
    const std::string clone = "{\"op\":\"clone_function\",\"start\":\"float wind(\",\"prefix\":\"\","
        "\"before\":\"float wind(float x)\",\"between\":\"\\nfloat previous_wind(float old_x)\","
        "\"after\":\"\",\"remap\":{\"x\":\"old_x\"}}";
    require(apply("float wind(float x){if(x){return x*2;}return 0;}\n", clone) ==
        "float wind(float x){if(x){return x*2;}return 0;}\nfloat previous_wind(float old_x){if(old_x){return old_x*2;}return 0;}\n",
        "balanced function clone and identifier remap");
    refused("float wind(float x){return x;}\nfloat wind(float y){return y;}\n", clone);
    refused("float wind(float x){return x;\n", clone);
}
float render(ID3D11Device* d,ID3D11DeviceContext* c,ID3D11VertexShader* vs,ID3D11PixelShader* ps){
    D3D11_TEXTURE2D_DESC td{1,1,1,1,DXGI_FORMAT_R32G32B32A32_FLOAT,{1,0},D3D11_USAGE_DEFAULT,D3D11_BIND_RENDER_TARGET,0,0};
    ComPtr<ID3D11Texture2D> target,copy;ComPtr<ID3D11RenderTargetView> view;
    require(SUCCEEDED(d->CreateTexture2D(&td,nullptr,&target))&&SUCCEEDED(d->CreateRenderTargetView(target.Get(),nullptr,&view)),"render target");
    td.Usage=D3D11_USAGE_STAGING;td.BindFlags=0;td.CPUAccessFlags=D3D11_CPU_ACCESS_READ;
    require(SUCCEEDED(d->CreateTexture2D(&td,nullptr,&copy)),"readback texture");
    ID3D11RenderTargetView* rt=view.Get();c->OMSetRenderTargets(1,&rt,nullptr);D3D11_VIEWPORT vp{0,0,1,1,0,1};c->RSSetViewports(1,&vp);
    c->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);c->VSSetShader(vs,nullptr,0);c->PSSetShader(ps,nullptr,0);c->Draw(3,0);
    c->OMSetRenderTargets(0,nullptr,nullptr);c->CopyResource(copy.Get(),target.Get());D3D11_MAPPED_SUBRESOURCE mapped{};
    require(SUCCEEDED(c->Map(copy.Get(),0,D3D11_MAP_READ,0,&mapped)),"readback map");const float value=*static_cast<const float*>(mapped.pData);c->Unmap(copy.Get(),0);return value;
}
}
int main(){try{
    source_operations();
    rsf_shader_patch_api api{sizeof(api),RSF_SHADER_PATCH_VERSION};require(rsf_patch_query(&api)==0,"ABI query");
    auto short_api=api;short_api.struct_size=0;require(rsf_patch_query(&short_api)==RSF_PATCH_ARGUMENT,"short ABI");
    short_api=api;short_api.abi_version=99;require(rsf_patch_query(&short_api)==RSF_PATCH_ABI,"wrong ABI");
    const char* source="float4 main(uint id:SV_VertexID):SV_Position{return float4(id==2?3:-1,id==1?3:-1,0,1);}";
    auto stock=compile(source,"vs_5_0");Files originals{{"fixture.vs",source}};
    const std::string suffix="\n#undef main\nstruct O{float4 p:SV_Position;float v:TEXCOORD12;};O main(uint id:SV_VertexID){O o;o.p=stock_main(id);o.v=1;return o;}";
    const std::string source2 = std::string(source) + "\n";
    originals.emplace("fixture2.vs", source2);
    auto suffix2 = suffix;
    suffix2.replace(suffix2.find("o.v=1"), 5, "o.v=2");
    const auto wrap = [&](const std::string& tail) {
        return "{\"op\":\"wrap\",\"prefix\":\"#define main stock_main\\n\",\"suffix\":" + json_string(tail) + "}";
    };
    const auto blob = pack("{\"schema\":1,\"roots\":[{\"name\":\"fixture.vs\"},{\"name\":\"fixture2.vs\"}],\"sources\":{\"fixture.vs\":" +
        json_string(sha256(source)) + ",\"fixture2.vs\":" + json_string(sha256(source2)) + "},\"files\":{\"fixture.vs\":" +
        wrap(suffix) + ",\"fixture2.vs\":" + wrap(suffix2) + "},\"allowed_bindings\":[],\"added_semantics\":{\"TEXCOORD\":[12]}}");
    Pack parsed;parsed.load(blob);require(parsed.transform(originals).at("fixture.vs").find("stock_main")!=std::string::npos,"synthetic transform");
    auto cache=std::filesystem::temp_directory_path()/("rsf-shader-patch-"+std::to_string(GetCurrentProcessId()));
    rsf_patch_session* session{};require(api.open({blob.data(),blob.size()},cache.c_str(),reinterpret_cast<void*>(&D3DCompile),&session)==0,"session open");
    auto collision=blob;collision.replace(collision.find("synthetic"),9,"collision");rsf_patch_session* later{};
    require(api.open({collision.data(),collision.size()},cache.c_str(),reinterpret_cast<void*>(&D3DCompile),&later)==RSF_PATCH_FORMAT&&!later,
        "later pack touching the same source root refused");
    rsf_patch_request request{sizeof(request),1,{source,std::strlen(source)},{stock.data(),stock.size()},"main","vs_5_0",0,nullptr,0,resolve,&originals};
    rsf_patch_output output{sizeof(output),1};require(api.prepare(session,&request,&output)==0,"original proof/compile/reflection");
    const std::string variant(static_cast<const char*>(output.bytecode.data),output.bytecode.size);
    require(api.prepare(session,&request,&output)==0&&output.cache_hit,"verified cache hit");
    const auto stock2 = compile(source2.c_str(), "vs_5_0");
    require(stock2 == stock, "distinct source roots with identical original DXBC");
    request.source = {source2.data(), source2.size()};
    require(api.prepare(session,&request,&output)==0 && !output.cache_hit, "root-specific cache identity");
    require(std::string_view(static_cast<const char*>(output.bytecode.data),output.bytecode.size)!=variant, "root-specific variant");
    request.source = {source,std::strlen(source)};
    originals.begin()->second+=' ';require(api.prepare(session,&request,&output)==RSF_PATCH_HASH,"changed engine source refused");originals.begin()->second.pop_back();
    request.original_bytecode={variant.data(),variant.size()};require(api.prepare(session,&request,&output)==RSF_PATCH_ORIGINAL,"wrong engine DXBC refused");
    api.close(session);
    std::error_code ec;for(const auto& e:std::filesystem::directory_iterator(cache))std::filesystem::remove(e.path(),ec);std::filesystem::remove(cache,ec);
    for(size_t i=0;i<blob.size();++i){try{Pack bad;bad.load(std::string_view(blob).substr(0,i));return 2;}catch(const Error&){}}
    ComPtr<ID3D11Device> device;ComPtr<ID3D11DeviceContext> context;D3D_FEATURE_LEVEL level{};
    require(SUCCEEDED(D3D11CreateDevice(nullptr,D3D_DRIVER_TYPE_WARP,nullptr,0,nullptr,0,D3D11_SDK_VERSION,&device,&level,&context)),"WARP device");
    require(api.attach_d3d11(device.Get())==0,"API interception");ComPtr<ID3D11VertexShader> vs;
    require(SUCCEEDED(device->CreateVertexShader(stock.data(),stock.size(),nullptr,&vs)),"stock VS");
    auto pixel=compile("float4 main():SV_Target{return float4(0.25,0.3,0.4,1);}","ps_5_0");std::vector<uint32_t> pattern,replacement;
    for(uint32_t i=0;i<word(pixel,28);++i){const size_t at=word(pixel,32+i*4);const auto kind=std::string_view(pixel).substr(at,4);if(kind!="SHDR"&&kind!="SHEX")continue;
        const size_t end=at+8+word(pixel,at+4);for(size_t pos=at+16;pos<end;){const size_t n=word(pixel,pos)>>24&127;require(n!=0,"fixture opcode");
            for(size_t j=1;j<n;++j)if(word(pixel,pos+j*4)==std::bit_cast<uint32_t>(0.25f)){pattern.resize(n);std::memcpy(pattern.data(),pixel.data()+pos,n*4);replacement=pattern;replacement[j]=std::bit_cast<uint32_t>(0.5f);}
            pos+=n*4;}}
    require(!pattern.empty(),"literal instruction pattern");const auto rewritten=rewrite_dxbc(pixel,sha256(pixel),pattern,replacement);
    try{(void)rewrite_dxbc(pixel,std::string(64,'0'),pattern,replacement);return 3;}catch(const Error& e){require(e.code==RSF_PATCH_HASH,"hash refusal");}
    ComPtr<ID3D11PixelShader> old_ps,new_ps;
    require(SUCCEEDED(device->CreatePixelShader(pixel.data(),pixel.size(),nullptr,&old_ps)),"original PS");
    require(api.create_d3d11(device.Get(),"ps_5_0",{rewritten.data(),rewritten.size()},reinterpret_cast<void**>(new_ps.GetAddressOf()))==0,"rewritten PS");
    require(std::abs(render(device.Get(),context.Get(),vs.Get(),old_ps.Get())-0.25f)<1e-6f&&
        std::abs(render(device.Get(),context.Get(),vs.Get(),new_ps.Get())-0.5f)<1e-6f,"original and rewritten GPU results");
    ComPtr<ID3D11Buffer> buffer;require(api.constant_buffer(device.Get(),16,reinterpret_cast<void**>(buffer.GetAddressOf()))==0,"generic buffer allocation");
    const float values[4]{1,2,3,4};require(api.upload_constant(context.Get(),buffer.Get(),{values,sizeof(values)})==0,"generic constant upload");
    require(api.bind_constant(context.Get(),'v',13,buffer.Get())==0,"generic binding");
    ComPtr<ID3D11Buffer> bound;context->VSGetConstantBuffers(13,1,&bound);require(bound.Get()==buffer.Get(),"binding readback");
    rsf_patch_counters counters{sizeof(counters),1};require(api.counters(&counters)==0,"counter ABI");
    require(counters.intercepted>=2&&counters.built&&counters.cache_hits&&counters.refused>=2,"counters");
    context->ClearState();std::cout<<"Synthetic pack/ABI/source proof/cache/refusal and WARP DXBC rewrite/interception/readback passed\n";return 0;
}catch(const Error& e){std::cerr<<e.reason<<'\n';return 1;}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}}

