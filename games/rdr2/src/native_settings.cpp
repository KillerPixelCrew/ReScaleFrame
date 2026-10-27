// SPDX-License-Identifier: GPL-3.0-only
#include "native_settings.h"
#include <windows.h>
#include <MinHook.h>
#include <atomic>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <mutex>
#include <string>

namespace {
using Bind=void (*)(void*,const void*,const void*);
using Read=void (*)(const void*,void*,const void*,void*,int32_t);
Bind bind_original{};Read read_original{};
uintptr_t image{};
std::wstring output;
std::mutex guard;
uint32_t rows{},values{};
std::atomic<bool> armed{};
bool copy(uintptr_t address,void* target,size_t size)
{
    SIZE_T bytes{};return address&&ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<void*>(address),target,size,&bytes)&&bytes==size;
}
template<class T> T at(uintptr_t address) {T value{};copy(address,&value,sizeof(value));return value;}
void bytes(FILE* file,const void* pointer,size_t size)
{
    unsigned char buffer[256]{};
    if(size>sizeof(buffer)||!copy(reinterpret_cast<uintptr_t>(pointer),buffer,size)){std::fputs("null",file);return;}
    std::fputc('"',file);for(size_t i=0;i<size;++i)std::fprintf(file,"%02x",buffer[i]);std::fputc('"',file);
}
void bind(void* destination,const void* context,const void* metadata)
{
    const auto key=at<uint32_t>(reinterpret_cast<uintptr_t>(metadata)+8);
    if(armed.load()&&(key==0x4de87d5b||key==0x58e23159||key==0x3420e356||key==0x0cd9de94)) {
        std::lock_guard lock(guard);
        if(rows++<16)if(FILE* file=_wfopen(output.c_str(),L"ab")) {
            std::fprintf(file,"{\"event\":\"row\",\"thread\":%lu,\"key\":\"%08x\",\"destination\":\"%p\",\"vtable_rva\":\"%llx\",\"destination_bytes\":",
                GetCurrentThreadId(),key,destination,static_cast<unsigned long long>(at<uintptr_t>(reinterpret_cast<uintptr_t>(destination))-image));
            bytes(file,destination,128);std::fputs(",\"context\":",file);bytes(file,context,128);
            std::fputs(",\"metadata\":",file);bytes(file,metadata,128);
            std::fputs(",\"destination_vtable\":",file);bytes(file,reinterpret_cast<void*>(at<uintptr_t>(reinterpret_cast<uintptr_t>(destination))),256);
            std::fputs("}\n",file);std::fclose(file);
        }
    }
    bind_original(destination,context,metadata);
}
void read(const void* path,void* source,const void* type,void* destination,int32_t flags)
{
    read_original(path,source,type,destination,flags);
    if(!armed.load())return;
    std::lock_guard lock(guard);
    if(values++>=32)return;
    if(FILE* file=_wfopen(output.c_str(),L"ab")) {
        const auto table=at<uintptr_t>(reinterpret_cast<uintptr_t>(source));
        std::fprintf(file,"{\"event\":\"value\",\"thread\":%lu,\"source\":\"%p\",\"vtable_rva\":\"%llx\",\"getter_rva\":\"%llx\",\"flags\":%d,\"path\":",
            GetCurrentThreadId(),source,static_cast<unsigned long long>(table-image),static_cast<unsigned long long>(at<uintptr_t>(table+0x78)-image),flags);
        bytes(file,path,128);std::fputs(",\"source_bytes\":",file);bytes(file,source,128);
        std::fputs(",\"type\":",file);bytes(file,type,32);std::fputs(",\"value\":",file);bytes(file,destination,32);
        std::fputs("}\n",file);std::fclose(file);
    }
}
}
void rsf_rdr2_native_settings_probe_arm() noexcept { armed.store(true); }
bool rsf_rdr2_native_settings_probe(const wchar_t* path,rsf_game_log_fn log,void* user) noexcept
{
    if(!path||!*path||bind_original)return false;
    image=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    constexpr unsigned char bind_bytes[]={0x48,0x89,0x5c,0x24,0x10,0x48,0x89,0x7c,0x24,0x18,0x55,0x48,0x8d,0x6c,0x24,0xd0};
    constexpr unsigned char read_bytes[]={0x48,0x89,0x5c,0x24,0x08,0x57,0x48,0x83,0xec,0x50,0x48,0x8b,0x02,0x4c,0x8b,0xd2};
    if(std::memcmp(reinterpret_cast<void*>(image+0x2c626e0),bind_bytes,sizeof(bind_bytes))||
        std::memcmp(reinterpret_cast<void*>(image+0x2c627b8),read_bytes,sizeof(read_bytes)))return false;
    output=path;
    const auto initialized=MH_Initialize();if(initialized!=MH_OK&&initialized!=MH_ERROR_ALREADY_INITIALIZED)return false;
    if(MH_CreateHook(reinterpret_cast<void*>(image+0x2c626e0),reinterpret_cast<void*>(bind),reinterpret_cast<void**>(&bind_original))!=MH_OK)return false;
    if(MH_CreateHook(reinterpret_cast<void*>(image+0x2c627b8),reinterpret_cast<void*>(read),reinterpret_cast<void**>(&read_original))!=MH_OK) {
        MH_RemoveHook(reinterpret_cast<void*>(image+0x2c626e0));bind_original=nullptr;return false;
    }
    if(MH_EnableHook(reinterpret_cast<void*>(image+0x2c626e0))==MH_OK&&MH_EnableHook(reinterpret_cast<void*>(image+0x2c627b8))==MH_OK) {
        if(log)log(user,"RDR2 native settings read-only probe armed: max16 rows /32 values; no menu mutation.");return true;
    }
    MH_DisableHook(reinterpret_cast<void*>(image+0x2c626e0));MH_RemoveHook(reinterpret_cast<void*>(image+0x2c626e0));
    MH_DisableHook(reinterpret_cast<void*>(image+0x2c627b8));MH_RemoveHook(reinterpret_cast<void*>(image+0x2c627b8));
    bind_original=nullptr;read_original=nullptr;return false;
}
