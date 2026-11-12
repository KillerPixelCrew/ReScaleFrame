// SPDX-License-Identifier: GPL-3.0-only
#include "patch.h"
#include <filesystem>
#include <fstream>
#include <iostream>
#include <d3dcompiler.h>
#include <windows.h>
#include <wrl/client.h>
using namespace rsf::patch;
extern "C" int32_t rsf_patch_query(rsf_shader_patch_api*);
class Includes final : public ID3DInclude {
    const Files& files_;
public:
    explicit Includes(const Files& files) : files_(files) {}
    HRESULT __stdcall Open(D3D_INCLUDE_TYPE,LPCSTR name,LPCVOID,LPCVOID* data,UINT* size) override {
        std::string key(name);for(auto& c:key){if(c=='\\')c='/';else if(c>='A'&&c<='Z')c=char(c-'A'+'a');}
        const auto it=files_.find(key);if(it==files_.end())return E_FAIL;
        *data=it->second.data();*size=static_cast<UINT>(it->second.size());return S_OK;
    }
    HRESULT __stdcall Close(LPCVOID) override {return S_OK;}
};
int32_t resolve(void* p,const char* name,rsf_patch_bytes* out){const auto& files=*static_cast<Files*>(p);const auto i=files.find(name);
    if(i==files.end())return RSF_PATCH_SOURCE;*out={i->second.data(),i->second.size()};return 0;}
std::string read(const std::filesystem::path& p){std::ifstream f(p,std::ios::binary);return {std::istreambuf_iterator<char>(f),{}};}
int main(int argc,char** argv) {
    try {
        if(argc!=4&&argc!=5)throw Error{-1,"expected pack, corpus, oracle, optional compile"};
        const auto pack_data=read(argv[1]);Pack pack;pack.load(pack_data);Files original;
        for(const auto& e:std::filesystem::recursive_directory_iterator(argv[2]))if(e.is_regular_file())
            original.emplace(std::filesystem::relative(e.path(),argv[2]).generic_string(),read(e.path()));
        const auto out=pack.transform(original);size_t count=0;
        for(const auto& [name,data]:out){
            if(name=="metadata.txt")continue;
            const auto expected=read(std::filesystem::path(argv[3])/"motion"/"r3"/name);
            if(data!=expected)throw Error{-1,"parity mismatch: "+name};++count;
        }
        std::cout<<count<<" files byte-identical to Python\n";
        if(argc==5){
            const auto module=LoadLibraryExW(L"d3dcompiler_43.dll",nullptr,LOAD_LIBRARY_SEARCH_SYSTEM32);
            const auto compiler=module?reinterpret_cast<decltype(&D3DCompile)>(GetProcAddress(module,"D3DCompile")):nullptr;
            check(compiler!=nullptr,RSF_PATCH_COMPILE,"engine compiler unavailable");
            rsf_shader_patch_api api{sizeof(api),1};check(rsf_patch_query(&api)==0,RSF_PATCH_ABI,"query");
            rsf_patch_session* session{};check(api.open({pack_data.data(),pack_data.size()},nullptr,reinterpret_cast<void*>(compiler),&session)==0,RSF_PATCH_FORMAT,"session");
            auto* roots=yyjson_obj_get(yyjson_doc_get_root(pack.rules),"roots");size_t i,n;yyjson_val* root;
            size_t compiled=0;
            yyjson_arr_foreach(roots,i,n,root){
                const auto name=text(root,"name"),stage=text(root,"stage"),family=text(root,"tag");
                const auto target=stage+"_5_0";const auto& source=original.at(name);
                const bool model=family=="core_model";
                const D3D_SHADER_MACRO macros[]={{"SM_5","1"},{"SKIN_1","1"},{nullptr,nullptr}};
                const rsf_patch_define defs[]={{"SM_5","1"},{"SKIN_1","1"}};
                D3D_SHADER_MACRO selected[]={{"SM_5","1"},{nullptr,nullptr},{nullptr,nullptr}};
                if(model)selected[1]=macros[1];Includes includes(original);Microsoft::WRL::ComPtr<ID3DBlob> code,errors;
                const auto hr=compiler(source.data(),source.size(),"",selected,&includes,"main",target.c_str(),0,0,&code,&errors);
                check(SUCCEEDED(hr)&&code,RSF_PATCH_COMPILE,("original compile: "+name).c_str());
                rsf_patch_request request{sizeof(request),1,{source.data(),source.size()},
                    {code->GetBufferPointer(),code->GetBufferSize()},"main",target.c_str(),0,defs,model?2u:1u,resolve,&original};
                rsf_patch_output output{sizeof(output),1};const auto result=api.prepare(session,&request,&output);
                if(result!=0)throw Error{result,name+": "+output.reason};++compiled;
            }
            api.close(session);FreeLibrary(module);std::cout<<compiled<<" engine-compiler roots source-proven, runtime-compiled and reflection-checked\n";
        }
        auto changed=original;changed.begin()->second+='\n';
        try{(void)pack.transform(changed);return 2;}catch(const Error& e){if(e.code!=RSF_PATCH_HASH)return 3;}
        for(size_t i=0;i<24;++i)try{Pack bad;bad.load(std::string(i,'x'));return 4;}catch(const Error&){}
        return 0;
    }catch(const Error& e){std::cerr<<e.reason<<'\n';return 1;}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
}
