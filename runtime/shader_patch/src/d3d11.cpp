// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/shader_patch.h>
#include <d3d11.h>
#include <MinHook.h>
#include <array>
#include <atomic>
#include <cstring>
#include <mutex>
#include <map>
#include <fstream>
#include <filesystem>
#include <d3dcompiler.h>
#include <wrl/client.h>
#include "patch.h"
namespace rsf::patch {
std::atomic<uint64_t> intercepted{},built{},cache_hits{},refused{};
std::array<std::atomic<uint64_t>,11> refusal_reasons{};
namespace {
using Create=HRESULT(STDMETHODCALLTYPE*)(ID3D11Device*,const void*,SIZE_T,ID3D11ClassLinkage*,void**);
std::array<Create,6> originals{};
std::array<void*,6> targets{};
std::mutex guard;
std::mutex inventory_guard;
std::map<std::string,std::string> inventory;
thread_local bool internal=false;
template<size_t I> HRESULT STDMETHODCALLTYPE create(ID3D11Device* d,const void* p,SIZE_T n,ID3D11ClassLinkage* l,void** out) {
    if(!internal) {
        ++intercepted;
        try { if(p&&n) { const std::string_view code(static_cast<const char*>(p),n);
            const auto hash=sha256(code);std::lock_guard lock(inventory_guard);inventory.try_emplace(hash,code); } }catch(...){}
    }
    return originals[I](d,p,n,l,out);
}
}
int32_t attach_d3d11(void* pointer) noexcept {
    if(!pointer)return RSF_PATCH_ARGUMENT;
    std::lock_guard lock(guard);auto** vtable=*static_cast<void***>(pointer);
    constexpr size_t slots[]{12,15,13,16,17,18}; // VS, PS, GS, HS, DS, CS (d3d11.h).
    const std::array<void*,6> hooks{reinterpret_cast<void*>(&create<0>),reinterpret_cast<void*>(&create<1>),
        reinterpret_cast<void*>(&create<2>),reinterpret_cast<void*>(&create<3>),reinterpret_cast<void*>(&create<4>),reinterpret_cast<void*>(&create<5>)};
    if(targets[0]){
        for(size_t i=0;i<6;++i)if(targets[i]!=vtable[slots[i]])return RSF_PATCH_ARGUMENT;
        return RSF_PATCH_OK;
    }
    const auto initialized=MH_Initialize();if(initialized!=MH_OK&&initialized!=MH_ERROR_ALREADY_INITIALIZED)return RSF_PATCH_INTERNAL;
    size_t count=0;
    for(;count<6;++count){
        if(MH_CreateHook(vtable[slots[count]],hooks[count],reinterpret_cast<void**>(&originals[count]))!=MH_OK)break;
        targets[count]=vtable[slots[count]];
    }
    if(count==6)for(size_t i=0;i<6;++i)if(MH_EnableHook(targets[i])!=MH_OK){count=i;break;}
    if(count!=6){for(auto& t:targets)if(t){MH_DisableHook(t);MH_RemoveHook(t);t=nullptr;}return RSF_PATCH_INTERNAL;}
    return RSF_PATCH_OK;
}
int32_t create_d3d11(void* pointer,const char* target,rsf_patch_bytes code,void** shader) noexcept {
    if(shader)*shader=nullptr;
    if(!pointer||!target||!code.data||!code.size||!shader)return RSF_PATCH_ARGUMENT;
    auto* d=static_cast<ID3D11Device*>(pointer);HRESULT result=E_INVALIDARG;internal=true;
    switch(target[0]){
    case 'v':result=d->CreateVertexShader(code.data,code.size,nullptr,reinterpret_cast<ID3D11VertexShader**>(shader));break;
    case 'p':result=d->CreatePixelShader(code.data,code.size,nullptr,reinterpret_cast<ID3D11PixelShader**>(shader));break;
    case 'g':result=d->CreateGeometryShader(code.data,code.size,nullptr,reinterpret_cast<ID3D11GeometryShader**>(shader));break;
    case 'h':result=d->CreateHullShader(code.data,code.size,nullptr,reinterpret_cast<ID3D11HullShader**>(shader));break;
    case 'd':result=d->CreateDomainShader(code.data,code.size,nullptr,reinterpret_cast<ID3D11DomainShader**>(shader));break;
    case 'c':result=d->CreateComputeShader(code.data,code.size,nullptr,reinterpret_cast<ID3D11ComputeShader**>(shader));break;
    }
    internal=false;return SUCCEEDED(result)?RSF_PATCH_OK:RSF_PATCH_COMPILE;
}
int32_t constant_buffer(void* pointer,uint32_t size,void** out) noexcept {
    if(out)*out=nullptr;
    if(!pointer||!out||!size||size%16||size>65536)return RSF_PATCH_ARGUMENT;
    D3D11_BUFFER_DESC desc{size,D3D11_USAGE_DYNAMIC,D3D11_BIND_CONSTANT_BUFFER,D3D11_CPU_ACCESS_WRITE,0,0};
    return SUCCEEDED(static_cast<ID3D11Device*>(pointer)->CreateBuffer(&desc,nullptr,reinterpret_cast<ID3D11Buffer**>(out)))?0:RSF_PATCH_INTERNAL;
}
int32_t upload_constant(void* pointer,void* resource,rsf_patch_bytes bytes) noexcept {
    if(!pointer||!resource||!bytes.data)return RSF_PATCH_ARGUMENT;
    auto* c=static_cast<ID3D11DeviceContext*>(pointer);auto* b=static_cast<ID3D11Buffer*>(resource);D3D11_BUFFER_DESC desc{};b->GetDesc(&desc);
    if(bytes.size!=desc.ByteWidth||desc.Usage!=D3D11_USAGE_DYNAMIC||!(desc.CPUAccessFlags&D3D11_CPU_ACCESS_WRITE))return RSF_PATCH_ARGUMENT;
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if(FAILED(c->Map(b,0,D3D11_MAP_WRITE_DISCARD,0,&mapped))||!mapped.pData)return RSF_PATCH_INTERNAL;
    std::memcpy(mapped.pData,bytes.data,bytes.size);c->Unmap(b,0);return RSF_PATCH_OK;
}
int32_t bind_constant(void* pointer,uint32_t stage,uint32_t slot,void* resource) noexcept {
    if(!pointer||slot>=D3D11_COMMONSHADER_CONSTANT_BUFFER_API_SLOT_COUNT)return RSF_PATCH_ARGUMENT;
    auto* c=static_cast<ID3D11DeviceContext*>(pointer);auto* b=static_cast<ID3D11Buffer*>(resource);
    switch(stage){
    case 'v':c->VSSetConstantBuffers(slot,1,&b);break;
    case 'p':c->PSSetConstantBuffers(slot,1,&b);break;
    case 'g':c->GSSetConstantBuffers(slot,1,&b);break;
    case 'h':c->HSSetConstantBuffers(slot,1,&b);break;
    case 'd':c->DSSetConstantBuffers(slot,1,&b);break;
    case 'c':c->CSSetConstantBuffers(slot,1,&b);break;
    default:return RSF_PATCH_ARGUMENT;
    }
    return 0;
}
int32_t counters(rsf_patch_counters* out) noexcept{
    if(!out||out->struct_size<sizeof(*out))return RSF_PATCH_ARGUMENT;
    if(out->abi_version!=RSF_SHADER_PATCH_VERSION)return RSF_PATCH_ABI;
    *out={sizeof(*out),RSF_SHADER_PATCH_VERSION,intercepted.load(),built.load(),cache_hits.load(),refused.load()};
    for(size_t i=0;i<11;++i)out->refusals[i]=refusal_reasons[i].load();
    return 0;
}
int32_t dump(const wchar_t* directory) noexcept {
    try {
        if(!directory)return RSF_PATCH_ARGUMENT;
        const std::filesystem::path root(directory);std::filesystem::create_directories(root);
        std::map<std::string,std::string> snapshot;
        {std::lock_guard lock(inventory_guard);snapshot=inventory;}
        for(const auto& [hash,code]:snapshot){
            std::ofstream binary(root/(hash+".dxbc"),std::ios::binary);binary.write(code.data(),static_cast<std::streamsize>(code.size()));
            Microsoft::WRL::ComPtr<ID3DBlob> assembly;
            if(SUCCEEDED(D3DDisassemble(code.data(),code.size(),D3D_DISASM_ENABLE_INSTRUCTION_NUMBERING,nullptr,&assembly))){
                std::ofstream text(root/(hash+".asm"),std::ios::binary);
                text.write(static_cast<const char*>(assembly->GetBufferPointer()),static_cast<std::streamsize>(assembly->GetBufferSize()));
            }
        }
        return RSF_PATCH_OK;
    }catch(...){return RSF_PATCH_INTERNAL;}
}
}
