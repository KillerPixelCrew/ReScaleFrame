// SPDX-License-Identifier: GPL-3.0-only
#include "patch.h"
#include <windows.h>
#include <d3dcompiler.h>
#include <d3d11shader.h>
#include <wrl/client.h>
#include <algorithm>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <mutex>
#include <sstream>
#include <atomic>
#include <array>
#include <rescaleframe/version.h>
using Microsoft::WRL::ComPtr;
using namespace rsf::patch;
namespace rsf::patch {
extern std::atomic<uint64_t> built,cache_hits,refused;
extern std::array<std::atomic<uint64_t>,11> refusal_reasons;
int32_t attach_d3d11(void*) noexcept;
int32_t create_d3d11(void*,const char*,rsf_patch_bytes,void**) noexcept;
int32_t constant_buffer(void*,uint32_t,void**) noexcept;
int32_t upload_constant(void*,void*,rsf_patch_bytes) noexcept;
int32_t bind_constant(void*,uint32_t,uint32_t,void*) noexcept;
int32_t counters(rsf_patch_counters*) noexcept;
int32_t dump(const wchar_t*) noexcept;
}
struct rsf_patch_session {
    Pack pack;
    std::filesystem::path cache;
    decltype(&D3DCompile) compile{};
    std::string compiler_hash;
    std::string output;
    std::vector<std::string> claims;
    std::mutex mutex;
};
namespace {
std::mutex claims_mutex;
std::map<std::string,std::pair<std::string,size_t>> owners;
void claim(rsf_patch_session& s) {
    auto* root=yyjson_doc_get_root(s.pack.rules);
    auto* roots=yyjson_obj_get(root,"roots");check(yyjson_is_arr(roots),RSF_PATCH_FORMAT,"root allowlist");
    size_t i,n;yyjson_val* entry;
    yyjson_arr_foreach(roots,i,n,entry) {
        const auto name=text(entry,"name");s.claims.push_back(text(yyjson_obj_get(root,"sources"),name.c_str()));
    }
    std::sort(s.claims.begin(),s.claims.end());s.claims.erase(std::unique(s.claims.begin(),s.claims.end()),s.claims.end());
    std::lock_guard lock(claims_mutex);auto next=owners;
    for(const auto& hash:s.claims){auto found=next.find(hash);
        check(found==next.end()||found->second.first==s.pack.id,RSF_PATCH_FORMAT,"shader already owned by another pack");
        auto& owner=next[hash];owner.first=s.pack.id;++owner.second;
    }
    owners.swap(next);
}
void unclaim(const rsf_patch_session& s) {
    std::lock_guard lock(claims_mutex);
    for(const auto& hash:s.claims){auto found=owners.find(hash);if(found!=owners.end()&&!--found->second.second)owners.erase(found);}
}
std::string read(const std::filesystem::path& p) {
    std::ifstream f(p,std::ios::binary);return {std::istreambuf_iterator<char>(f),std::istreambuf_iterator<char>()};
}
std::string canonical(std::string name) {
    for(auto& c:name){if(c=='\\')c='/';else if(c>='A'&&c<='Z')c=char(c-'A'+'a');}
    return name;
}
class Includes final:public ID3DInclude {
    const Files& files_;
public:
    explicit Includes(const Files& f):files_(f){}
    HRESULT __stdcall Open(D3D_INCLUDE_TYPE,LPCSTR name,LPCVOID,LPCVOID* data,UINT* size) override {
        if(!name||!data||!size)return E_INVALIDARG;
        const auto found=files_.find(canonical(name));
        if(found==files_.end()||found->second.size()>UINT_MAX)return E_FAIL;
        *data=found->second.data();*size=static_cast<UINT>(found->second.size());return S_OK;
    }
    HRESULT __stdcall Close(LPCVOID) override{return S_OK;}
};
std::string compile(rsf_patch_session& s,const rsf_patch_request& r,const Files& files,std::string_view source) {
    std::vector<D3D_SHADER_MACRO> defines;
    for(size_t i=0;i<r.define_count;++i){
        check(r.defines[i].name&&r.defines[i].value,RSF_PATCH_ARGUMENT,"null define");
        defines.push_back({r.defines[i].name,r.defines[i].value});
    }
    defines.push_back({nullptr,nullptr});Includes includes(files);ComPtr<ID3DBlob> code,errors;
    const auto hr=s.compile(source.data(),source.size(),"",defines.data(),&includes,r.entry,r.target,r.flags,0,&code,&errors);
    if(FAILED(hr)||!code)throw Error{RSF_PATCH_COMPILE,errors?std::string(static_cast<const char*>(errors->GetBufferPointer()),
        std::min<size_t>(errors->GetBufferSize(),240)):"D3DCompile failed"};
    return {static_cast<const char*>(code->GetBufferPointer()),code->GetBufferSize()};
}
bool signature(ID3D11ShaderReflection* old,ID3D11ShaderReflection* changed,bool input,yyjson_val* additions) {
    D3D11_SHADER_DESC a{},b{};if(FAILED(old->GetDesc(&a))||FAILED(changed->GetDesc(&b)))return false;
    const auto na=input?a.InputParameters:a.OutputParameters,nb=input?b.InputParameters:b.OutputParameters;
    std::vector<D3D11_SIGNATURE_PARAMETER_DESC> first,second;
    for(UINT i=0;i<na;++i){D3D11_SIGNATURE_PARAMETER_DESC p{};
        if(FAILED(input?old->GetInputParameterDesc(i,&p):old->GetOutputParameterDesc(i,&p)))return false;first.push_back(p);}
    for(UINT i=0;i<nb;++i){D3D11_SIGNATURE_PARAMETER_DESC p{};
        if(FAILED(input?changed->GetInputParameterDesc(i,&p):changed->GetOutputParameterDesc(i,&p)))return false;second.push_back(p);}
    const auto equal=[](const auto& x,const auto& y){return _stricmp(x.SemanticName,y.SemanticName)==0&&
        x.SemanticIndex==y.SemanticIndex&&x.SystemValueType==y.SystemValueType&&x.ComponentType==y.ComponentType&&x.Mask==y.Mask;};
    for(const auto& p:first)if(std::none_of(second.begin(),second.end(),[&](const auto& q){return equal(p,q);}))return false;
    for(const auto& p:second)if(std::none_of(first.begin(),first.end(),[&](const auto& q){return equal(p,q);})){
        std::string name=p.SemanticName;for(auto& c:name)if(c>='a'&&c<='z')c=char(c-'a'+'A');
        auto* indices=yyjson_obj_get(additions,name.c_str());bool allowed=false;size_t i,n;yyjson_val* v;
        yyjson_arr_foreach(indices,i,n,v)allowed|=yyjson_get_uint(v)==p.SemanticIndex;
        if(!allowed)return false;
    }
    return true;
}
void reflection(const rsf_patch_session& s,std::string_view stock,std::string_view variant) {
    ComPtr<ID3D11ShaderReflection> a,b;
    check(SUCCEEDED(D3DReflect(stock.data(),stock.size(),IID_ID3D11ShaderReflection,&a))&&
        SUCCEEDED(D3DReflect(variant.data(),variant.size(),IID_ID3D11ShaderReflection,&b)),RSF_PATCH_REFLECTION,"D3DReflect");
    D3D11_SHADER_DESC ad{},bd{};
    check(SUCCEEDED(a->GetDesc(&ad))&&SUCCEEDED(b->GetDesc(&bd))&&ad.Version==bd.Version,RSF_PATCH_REFLECTION,"shader model");
    for(UINT i=0;i<ad.BoundResources;++i){
        D3D11_SHADER_INPUT_BIND_DESC x{},y{};
        check(SUCCEEDED(a->GetResourceBindingDesc(i,&x))&&SUCCEEDED(b->GetResourceBindingDescByName(x.Name,&y))&&
            x.Type==y.Type&&x.BindPoint==y.BindPoint&&x.BindCount==y.BindCount&&x.ReturnType==y.ReturnType&&x.Dimension==y.Dimension,
            RSF_PATCH_REFLECTION,"native resource changed");
    }
    auto* root=yyjson_doc_get_root(s.pack.rules);
    for(UINT i=0;i<bd.BoundResources;++i){
        D3D11_SHADER_INPUT_BIND_DESC x{},y{};check(SUCCEEDED(b->GetResourceBindingDesc(i,&x)),RSF_PATCH_REFLECTION,"resource descriptor");
        if(SUCCEEDED(a->GetResourceBindingDescByName(x.Name,&y)))continue;
        bool allowed=false;size_t j,n;yyjson_val* v;auto* list=yyjson_obj_get(root,"allowed_bindings");
        yyjson_arr_foreach(list,j,n,v)allowed|=text(v,"name")==x.Name &&
            yyjson_get_uint(yyjson_obj_get(v,"point"))==x.BindPoint &&
            yyjson_get_uint(yyjson_obj_get(v,"type"))==x.Type &&
            yyjson_get_uint(yyjson_obj_get(v,"count"))==x.BindCount;
        check(allowed,RSF_PATCH_REFLECTION,"undeclared resource addition");
    }
    for(UINT i=0;i<ad.ConstantBuffers;++i){
        auto* old=a->GetConstantBufferByIndex(i);D3D11_SHADER_BUFFER_DESC x{},y{};
        check(old&&SUCCEEDED(old->GetDesc(&x)),RSF_PATCH_REFLECTION,"native buffer");auto* changed=b->GetConstantBufferByName(x.Name);
        check(changed&&SUCCEEDED(changed->GetDesc(&y))&&x.Size==y.Size,RSF_PATCH_REFLECTION,"native buffer layout");
        for(UINT j=0;j<x.Variables;++j){D3D11_SHADER_VARIABLE_DESC p{},q{};
            check(SUCCEEDED(old->GetVariableByIndex(j)->GetDesc(&p))&&SUCCEEDED(changed->GetVariableByName(p.Name)->GetDesc(&q))&&
                p.StartOffset==q.StartOffset&&p.Size==q.Size,RSF_PATCH_REFLECTION,"native variable layout");}
    }
    auto* added=yyjson_obj_get(root,"added_semantics");
    check(signature(a.Get(),b.Get(),true,added)&&signature(a.Get(),b.Get(),false,added),RSF_PATCH_REFLECTION,"stage interface");
    check(ad.PatchConstantParameters==bd.PatchConstantParameters,RSF_PATCH_REFLECTION,"patch constants");
    for(UINT i=0;i<ad.PatchConstantParameters;++i){D3D11_SIGNATURE_PARAMETER_DESC x{},y{};
        check(SUCCEEDED(a->GetPatchConstantParameterDesc(i,&x))&&SUCCEEDED(b->GetPatchConstantParameterDesc(i,&y))&&
            std::strcmp(x.SemanticName,y.SemanticName)==0&&x.SemanticIndex==y.SemanticIndex&&x.Register==y.Register&&
            x.Mask==y.Mask&&x.ComponentType==y.ComponentType,RSF_PATCH_REFLECTION,"patch signature");}
}
int32_t open(rsf_patch_bytes blob,const wchar_t* cache,void* compiler,rsf_patch_session** out) noexcept {
    if(out)*out=nullptr;
    try {
        check(out&&compiler,RSF_PATCH_ARGUMENT,"session arguments");auto s=std::make_unique<rsf_patch_session>();s->pack.load(bytes(blob));
        s->compile=reinterpret_cast<decltype(&D3DCompile)>(compiler);if(cache)s->cache=cache;
        HMODULE module{};check(GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            reinterpret_cast<LPCWSTR>(compiler),&module)!=0,RSF_PATCH_ARGUMENT,"compiler module");
        wchar_t path[32768]{};check(GetModuleFileNameW(module,path,32768)!=0,RSF_PATCH_ARGUMENT,"compiler path");
        const auto compiler_bytes=read(path);check(!compiler_bytes.empty(),RSF_PATCH_ARGUMENT,"compiler identity unreadable");
        s->compiler_hash=sha256(compiler_bytes);
        claim(*s);
        *out=s.release();return RSF_PATCH_OK;
    }catch(const Error& e){return e.code;}catch(...){return RSF_PATCH_INTERNAL;}
}
const char* identity(const rsf_patch_session* s) noexcept{return s?s->pack.hash.c_str():nullptr;}
uint32_t root_count(const rsf_patch_session* s) noexcept {
    return s?static_cast<uint32_t>(yyjson_arr_size(yyjson_obj_get(yyjson_doc_get_root(s->pack.rules),"roots"))):0;
}
int32_t root_info(const rsf_patch_session* s,uint32_t index,rsf_patch_root* out) noexcept {
    if(!s||!out||out->struct_size<sizeof(*out))return RSF_PATCH_ARGUMENT;
    if(out->abi_version!=RSF_SHADER_PATCH_VERSION)return RSF_PATCH_ABI;
    auto* value=yyjson_arr_get(yyjson_obj_get(yyjson_doc_get_root(s->pack.rules),"roots"),index);
    if(!value)return RSF_PATCH_ARGUMENT;
    const auto string=[&](const char* name){return yyjson_get_str(yyjson_obj_get(value,name));};
    const auto name=string("name"),source=string("original_sha256"),patched=string("patched_sha256"),tag=string("tag"),stage=string("stage");
    if(!name||!source||!patched||!tag||!stage||!*stage)return RSF_PATCH_FORMAT;
    *out={sizeof(*out),RSF_SHADER_PATCH_VERSION,name,source,patched,tag,static_cast<uint32_t>(*stage)};return 0;
}
int32_t prepare(rsf_patch_session* s,const rsf_patch_request* r,rsf_patch_output* out) noexcept {
    if(!out||out->struct_size<sizeof(*out))return RSF_PATCH_ARGUMENT;
    if(out->abi_version!=RSF_SHADER_PATCH_VERSION)return RSF_PATCH_ABI;
    out->bytecode={};out->cache_hit=0;out->reason[0]=0;
    try {
        check(s&&r&&r->struct_size>=sizeof(*r),RSF_PATCH_ARGUMENT,"request size");
        check(r->abi_version==RSF_SHADER_PATCH_VERSION,RSF_PATCH_ABI,"request version");
        check(r->entry&&r->target&&r->resolve&&(!r->define_count||r->defines),RSF_PATCH_ARGUMENT,"compile inputs");
        std::lock_guard guard(s->mutex);Files originals;
        auto* sources=yyjson_obj_get(yyjson_doc_get_root(s->pack.rules),"sources");
        check(yyjson_is_obj(sources),RSF_PATCH_FORMAT,"source list");size_t i,n;yyjson_val *k,*v;
        yyjson_obj_foreach(sources,i,n,k,v){(void)v;rsf_patch_bytes b{};
            check(r->resolve(r->user,yyjson_get_str(k),&b)==0,RSF_PATCH_SOURCE,"engine source provider");
            originals.emplace(yyjson_get_str(k),bytes(b));}
        auto patched=s->pack.transform(originals);const auto source=bytes(r->source),stock=bytes(r->original_bytecode);
        std::string root_name;
        auto* roots=yyjson_obj_get(yyjson_doc_get_root(s->pack.rules),"roots");
        check(yyjson_is_arr(roots),RSF_PATCH_FORMAT,"root allowlist");
        yyjson_val* candidate;
        yyjson_arr_foreach(roots,i,n,candidate) {
            const auto name=text(candidate,"name");const auto found=originals.find(name);
            if(found!=originals.end()&&found->second==source){
                check(root_name.empty(),RSF_PATCH_HASH,"ambiguous root");root_name=name;
                auto* expected=yyjson_obj_get(candidate,"patched_sha256");
                if(yyjson_is_str(expected))check(sha256(patched.at(name))==yyjson_get_str(expected),RSF_PATCH_HASH,"patched root hash");
            }
        }
        check(!root_name.empty(),RSF_PATCH_HASH,"root not allowlisted/acquired from engine");
        const auto proof=compile(*s,*r,originals,source);check(proof==stock,RSF_PATCH_ORIGINAL,"original does not reproduce engine DXBC");
        // Increment the cache revision if transform semantics change within one product version.
        std::string key="rsf-source-cache-1:" RSF_VERSION_STRING ":"+s->pack.hash+s->compiler_hash+sha256(stock);
        const auto field=[&](std::string_view value){key+=std::to_string(value.size())+":";key+=value;};
        field(root_name);field(sha256(source));field(r->entry);field(r->target);field(std::to_string(r->flags));
        for(size_t j=0;j<r->define_count;++j){field(r->defines[j].name);field(r->defines[j].value);}
        const auto path=s->cache/(sha256(key)+".dxbc");
        s->output=s->cache.empty()?std::string{}:read(path);
        if(!s->output.empty())try{reflection(*s,stock,s->output);out->cache_hit=1;++cache_hits;}catch(...){s->output.clear();}
        if(s->output.empty()){
            s->output=compile(*s,*r,patched,patched.at(root_name));reflection(*s,stock,s->output);++built;
            if(!s->cache.empty()){
                std::error_code ec;std::filesystem::create_directories(s->cache,ec);
                auto temp=path;temp+=".tmp";{std::ofstream f(temp,std::ios::binary|std::ios::trunc);f.write(s->output.data(),static_cast<std::streamsize>(s->output.size()));}
                std::filesystem::rename(temp,path,ec);if(ec)std::filesystem::remove(temp,ec);
            }
        }
        out->bytecode={s->output.data(),s->output.size()};return RSF_PATCH_OK;
    }catch(const Error& e){++refused;if(e.code<0&&e.code>=-10)++refusal_reasons[static_cast<size_t>(-e.code)];
        strncpy_s(out->reason,e.reason.c_str(),_TRUNCATE);return e.code;}
    catch(...){++refused;++refusal_reasons[10];strncpy_s(out->reason,"shader patch exception",_TRUNCATE);return RSF_PATCH_INTERNAL;}
}
void close(rsf_patch_session* s) noexcept{if(s){unclaim(*s);delete s;}}
}
extern "C" int32_t rsf_patch_query(rsf_shader_patch_api* api) {
    if(!api||api->struct_size<sizeof(*api))return RSF_PATCH_ARGUMENT;
    if(api->abi_version!=RSF_SHADER_PATCH_VERSION)return RSF_PATCH_ABI;
    *api={sizeof(*api),RSF_SHADER_PATCH_VERSION,open,identity,root_count,root_info,prepare,close,
        attach_d3d11,create_d3d11,constant_buffer,upload_constant,bind_constant,counters,dump};return RSF_PATCH_OK;
}
