// SPDX-License-Identifier: GPL-3.0-only
#include "input_hooks.h"
#include <windows.h>
#include <cstdio>
#include <cstring>
#include <string_view>
using namespace rsf::anomaly;
namespace rsf::anomaly::fg { void input_begin(uint32_t) noexcept {} void input_received(uint32_t,uint32_t) noexcept {} }
namespace {
int failures{};
#define CHECK(x) do { if (!(x)) { std::printf("FAIL %d: %s\n",__LINE__,#x); ++failures; } } while(false)
void fill(std::byte* image,const InputProfile& p) {
    for(const auto* site : {&p.frame,&p.keyboard,&p.mouse}) std::memcpy(image+site->rva,site->expected.data(),site->expected.size());
    for(size_t i=0;p.frame_code[i*2];++i) { unsigned value{}; sscanf_s(p.frame_code+i*2,"%2x",&value); image[p.frame.rva+i]=std::byte(value); }
    *reinterpret_cast<void**>(image+p.frame_vtable_rva)=image+p.frame.rva;
}
}
int main(int argc,char** argv) {
    const auto& build=argc>1 && std::string_view(argv[1])=="avx" ? avx_build : standard_build;
    const auto& p=*build.input;
    HMODULE mapped{};
    auto* image=argc>2 ? reinterpret_cast<std::byte*>(mapped=LoadLibraryExW(build.executable_wide ?
        (build.label==std::string_view("avx") ? L"D:/Anomaly 1.5.3/bin/AnomalyDX11AVX.exe" : L"D:/Anomaly 1.5.3/bin/AnomalyDX11.exe") : L"",nullptr,DONT_RESOLVE_DLL_REFERENCES)) :
        static_cast<std::byte*>(VirtualAlloc(nullptr,build.image_size(),MEM_COMMIT|MEM_RESERVE,PAGE_READWRITE));
    CHECK(image); if(!image) return 1;
    const char* reason{};
    if(!mapped) {
        for(const auto* site : {&p.frame,&p.keyboard,&p.mouse}) {
            fill(image,p); image[site->rva]^=std::byte{1}; CHECK(!input::install(image,build.image_size(),build,&reason));
        }
        fill(image,p); image[p.frame.rva+std::strlen(p.frame_code)/2-1]^=std::byte{1};
        CHECK(!input::install(image,build.image_size(),build,&reason));
        fill(image,p); *reinterpret_cast<void**>(image+p.frame_vtable_rva)=image+p.mouse.rva;
        CHECK(!input::install(image,build.image_size(),build,&reason)); fill(image,p);
    }
    CHECK(input::install(image,build.image_size(),build,&reason));
    CHECK(*reinterpret_cast<void**>(image+p.frame_vtable_rva)!=image+p.frame.rva);
    CHECK(input::disable()); CHECK(*reinterpret_cast<void**>(image+p.frame_vtable_rva)==image+p.frame.rva);
    CHECK(input::enable()); CHECK(input::disable());
    if(mapped) FreeLibrary(mapped); else VirtualFree(image,0,MEM_RELEASE);
    std::printf("%s: %d failures\n",build.label,failures);return failures ? 1 : 0;
}
