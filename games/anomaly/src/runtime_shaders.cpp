// SPDX-License-Identifier: GPL-3.0-only
#include "runtime_shaders.h"
#include <d3dcompiler.h>
#include <fstream>
#include <cstring>
namespace rsf::anomaly::motion {
namespace {
bool read(void* out,const void* p,size_t size) noexcept {
    __try {std::memcpy(out,p,size);return true;} __except(EXCEPTION_EXECUTE_HANDLER){return false;}
}
HRESULT include_open(void* table,const char* name,const void** data,UINT* size) noexcept {
    __try {
        auto* object=reinterpret_cast<ID3DInclude*>(&table);
        return object->Open(D3D_INCLUDE_LOCAL,name,nullptr,data,size);
    }__except(EXCEPTION_EXECUTE_HANDLER){return E_FAIL;}
}
void include_close(void* table,const void* data) noexcept {
    __try {reinterpret_cast<ID3DInclude*>(&table)->Close(data);}__except(EXCEPTION_EXECUTE_HANDLER){}
}
}
RuntimeShaders::~RuntimeShaders(){if(session_)api_.close(session_);}
bool RuntimeShaders::open(const std::filesystem::path& pack,const std::filesystem::path& cache,
    std::byte* image,const MotionProfile& profile,std::string& error) {
    // Already guarded D3DCompile setup: lea rax,[rip+disp32] loads the stateless includer's vtable.
    const auto* lea=image+profile.d3dcompile_setup_rva+6;unsigned char opcode[3]{};int32_t displacement{};
    if(!read(opcode,lea,3)||std::memcmp(opcode,"\x48\x8d\x05",3)!=0||!read(&displacement,lea+3,4)){
        error="include table instruction";return false;}
    include_table_=const_cast<std::byte*>(lea+7+displacement);
    void* functions[2]{};unsigned char open_bytes[9]{},close_bytes[8]{};
    if(!read(functions,include_table_,sizeof(functions))||!read(open_bytes,functions[0],sizeof(open_bytes))||
        !read(close_bytes,functions[1],sizeof(close_bytes))||std::memcmp(open_bytes,"\x40\x53\x48\x81\xec\x40\x02\x00\x00",9)||
        std::memcmp(close_bytes,"\x48\x83\xec\x28\x48\x85\xd2\x74",8)){
        error="engine includer entries";return false;}
    const auto runtime=GetModuleHandleW(L"ReScaleFrame.Runtime.dll");
    const auto query=runtime?reinterpret_cast<rsf_get_shader_patch_api_fn>(GetProcAddress(runtime,"rsf_get_shader_patch_api")):nullptr;
    api_.struct_size=sizeof(api_);api_.abi_version=RSF_SHADER_PATCH_VERSION;
    if(!query||query(&api_)!=0){error="Runtime shader service unavailable";return false;}
    std::ifstream f(pack,std::ios::binary);const std::string data((std::istreambuf_iterator<char>(f)),{});
    const auto compiler=GetModuleHandleW(engine_compiler_module);
    const auto fn=compiler?GetProcAddress(compiler,"D3DCompile"):nullptr;
    const auto result=api_.open({data.data(),data.size()},cache.c_str(),reinterpret_cast<void*>(fn),&session_);
    if(result!=0){error="Runtime pack refused: "+std::to_string(result);return false;}
    manifest.name_space=api_.hash(session_);
    for(uint32_t i=0;i<api_.root_count(session_);++i) {
        rsf_patch_root root{sizeof(root),RSF_SHADER_PATCH_VERSION};
        if(api_.root(session_,i,&root)!=0){error="pack root metadata";return false;}
        Family family=Family::None;
        for(uint32_t j=1;j<static_cast<uint32_t>(Family::Count);++j)
            if(std::strcmp(family_name(static_cast<Family>(j)),root.tag)==0)family=static_cast<Family>(j);
        if(family==Family::None||static_cast<uint32_t>(family_stage(family))!=root.stage){error="unknown adapter root family/stage";return false;}
        ManifestEntry entry{static_cast<char>(root.stage),family,root.name,root.source_sha256,root.patched_sha256};
        if(!manifest.by_original.emplace(root.source_sha256,std::move(entry)).second){error="ambiguous root source";return false;}
    }
    return !manifest.by_original.empty();
}
int32_t RuntimeShaders::resolve(void* user,const char* name,rsf_patch_bytes* out) noexcept try {
    auto& self=*static_cast<RuntimeShaders*>(user);const void* data{};UINT size{};
    std::string engine_name(name);
    for (auto& c : engine_name) if (c == '/') c = '\\';
    if(FAILED(include_open(self.include_table_,engine_name.c_str(),&data,&size))||!data)return RSF_PATCH_SOURCE;
    try { self.source_.resize(size); }
    catch (...) { include_close(self.include_table_,data); return RSF_PATCH_SOURCE; }
    const bool ok=read(self.source_.data(),data,size);include_close(self.include_table_,data);
    if(!ok)return RSF_PATCH_SOURCE;*out={self.source_.data(),self.source_.size()};return RSF_PATCH_OK;
} catch (...) { return RSF_PATCH_SOURCE; }
Prepared RuntimeShaders::prepare(const ShaderRequest& r,const ManifestEntry& entry,D3DShaderApi& reflector) {
    Prepared p;p.entry=&entry;p.skin=skin_mode(r.defines);std::vector<rsf_patch_define> defines;
    for(const auto& d:r.defines)defines.push_back({d.name.c_str(),d.value.c_str()});
    rsf_patch_request request{sizeof(request),RSF_SHADER_PATCH_VERSION,{r.source.data(),r.source.size()},
        {r.bytecode.data(),r.bytecode.size()},r.entry.c_str(),r.target.c_str(),r.flags,defines.data(),defines.size(),resolve,this};
    rsf_patch_output output{};output.struct_size=sizeof(output);output.abi_version=RSF_SHADER_PATCH_VERSION;
    const auto result=api_.prepare(session_,&request,&output);
    p.detail=output.reason;
    if(result!=0){p.outcome=result==RSF_PATCH_ORIGINAL?Outcome::OriginalMismatch:
        result==RSF_PATCH_REFLECTION?Outcome::ReflectionMismatch:Outcome::MotionCompileFailed;return p;}
    const auto* begin=static_cast<const std::byte*>(output.bytecode.data);p.variant.assign(begin,begin+output.bytecode.size);
    if(!reflector.reflect(r.bytecode,p.stock)||!reflector.reflect(p.variant,p.reflection)){
        p.outcome=Outcome::ReflectionMismatch;return p;}
    p.compatibility=compatible(entry.stage,p.stock,p.reflection);
    p.outcome=p.compatibility==Compatibility::Ok?(output.cache_hit?Outcome::ReplacedFromCache:Outcome::Replaced):Outcome::ReflectionMismatch;
    return p;
}
}
