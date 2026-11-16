// SPDX-License-Identifier: GPL-3.0-only
#include "screen.h"
#include <windows.h>
#include <array>
#include <atomic>
#include <cstring>

namespace rsf::fallout4 {
namespace {
std::atomic<bool> verified{};
uintptr_t image() noexcept { return uintptr_t(GetModuleHandleW(nullptr)); }
bool copy(uintptr_t address, void* destination, size_t size) noexcept {
    SIZE_T copied{};
    return address && ReadProcessMemory(GetCurrentProcess(), reinterpret_cast<const void*>(address),
        destination, size, &copied) && copied==size;
}
template<class T> bool read(uintptr_t address, T& value) noexcept { return copy(address,&value,sizeof(value)); }
bool guard(uintptr_t rva, const char* expected, size_t size) noexcept {
    std::array<char,32> actual{};
    return size<=actual.size() && copy(image()+rva,actual.data(),size) && !std::memcmp(actual.data(),expected,size);
}
struct MenuQuery {
    uintptr_t entry,literal_lea,return_lea,literal,object;
    const char *name,*literal_bytes,*return_bytes;
    uint32_t fact;
};
const MenuQuery menus[]{
    {0x1a87520, 0x1a87575, 0x1a87549, 0x25b0548, 0x3dbd830, "LoadingMenu", "\x48\x8d\x15\xcc\x8f\xb2\x00", "\x48\x8d\x05\xe0\x62\x33\x02", ScreenLoading},
    {0x1a879a0, 0x1a879f5, 0x1a879c9, 0x253f640, 0x3dbd8b0, "MainMenu", "\x48\x8d\x15\x44\x7c\xab\x00", "\x48\x8d\x05\xe0\x5e\x33\x02", ScreenMainMenu},
    {0x1a87e20, 0x1a87e75, 0x1a87e49, 0x271de40, 0x3dbd930, "PauseMenu", "\x48\x8d\x15\xc4\x5f\xc9\x00", "\x48\x8d\x05\xe0\x5a\x33\x02", ScreenPause},
    {0x1a87fd0, 0x1a88025, 0x1a87ff9, 0x253cde8, 0x3dbd960, "PipboyMenu", "\x48\x8d\x15\xbc\x4d\xab\x00", "\x48\x8d\x05\x60\x59\x33\x02", ScreenPipboy},
    {0x1a87a30, 0x1a87a85, 0x1a87a59, 0x271ddf0, 0x3dbd8d0, "MapMenu", "\x48\x8d\x15\x64\x63\xc9\x00", "\x48\x8d\x05\x70\x5e\x33\x02", ScreenMap},
    {0x1a890b0, 0x1a89105, 0x1a890d9, 0x2541f48, 0x3dbdb40, "VATSMenu", "\x48\x8d\x15\x3c\x8e\xab\x00", "\x48\x8d\x05\x60\x4a\x33\x02", ScreenVatsMenu},
};
using MenuName = const void* (*)();
using MenuOpen = bool (*)(void*,const void*);
using Freeze = bool (*)(void*);
using CameraEquals = bool (*)(void*,uint32_t);
struct Observation {
    uintptr_t ui{},main{},player_camera{},camera_state{},camera_vtable{},world_graph{},world_camera{};
    std::array<unsigned char,8> main_flags{};
    unsigned char in_menu{};
    uint32_t freeze_background{},freeze_pause{},native_frame{};
    bool operator==(const Observation&) const = default;
};
bool observe(Observation& value) noexcept {
    const auto base=image();
    if (!read(base+0x30e8930,value.ui) || !value.ui ||
        !read(value.ui+0x1ec,value.freeze_background) || !read(value.ui+0x1f0,value.freeze_pause) ||
        !read(base+0x32dd130,value.main) ||
        !read(base+0x30e6e58,value.player_camera) ||
        !read(base+0x32dd338,value.world_graph) || !read(base+0x3d871a0+0x98,value.native_frame)) return false;
    if (value.main && (!read(value.main+0x24,value.main_flags) || !read(value.main+0x1d0,value.in_menu))) return false;
    if (value.player_camera && (!read(value.player_camera,value.camera_vtable) ||
        !read(value.player_camera+0x28,value.camera_state))) return false;
    return !value.world_graph || read(value.world_graph+0x140,value.world_camera);
}
bool main_eligible(const Observation& value) noexcept {
    const auto& f=value.main_flags;
    return value.main && !value.in_menu && !f[0] && !f[1] && !f[2] && f[3] && !f[5] && !f[6] && !f[7];
}
}
bool verify_screen_queries() noexcept {
    verified.store(false,std::memory_order_release);
    for (const auto& menu:menus) {
        std::array<char,20> name{};
        const auto size=std::strlen(menu.name)+1;
        if (size>name.size() || !guard(menu.entry,"\x40\x53\x48\x83\xec\x20",6) ||
            !guard(menu.literal_lea,menu.literal_bytes,7) || !guard(menu.return_lea,menu.return_bytes,7) ||
            !copy(image()+menu.literal,name.data(),size) || std::memcmp(name.data(),menu.name,size)) return false;
    }
    const bool valid=guard(0x1a80320,"\x48\x89\x5c\x24\x08\x57\x48\x83\xec\x40",10) &&
        guard(0x1a80377,"\xf6\x41\x58\x40",4) &&
        guard(0x1a80480,"\x83\xb9\xec\x01\x00\x00\x00\x0f\x97\xc0\xc3",11) &&
        guard(0x1a804a0,"\x83\xb9\xf0\x01\x00\x00\x00\x0f\x97\xc0\xc3",11) &&
        guard(0x102b5c0,"\x4c\x8b\x41\x28\x4d\x85\xc0\x74\x0f\x8b\xc2\x4c\x3b\x84\xc1\xe0\x00\x00\x00\x75\x03\xb0\x01\xc3\x32\xc0\xc3",27);
    verified.store(valid,std::memory_order_release);
    return valid;
}
NativeScreen classify_native_screen(const CpuIdentity& source, const rsf_game_render_pass& pass,
    bool ordinary_world) noexcept try {
    NativeScreen result;
    const auto error=GetLastError();
    struct Restore { DWORD value; ~Restore() { SetLastError(value); } } restore{error};
    if (!verified.load(std::memory_order_acquire)) { result.refusals=ScreenQueryUnavailable; return result; }
    if (!render_source_is_current(source)) { result.refusals=ScreenSourceChanged; return result; }
    Observation before{},after{};
    if (!observe(before)) { result.refusals=ScreenQueryUnavailable; return result; }
    const auto base=image();
    for (const auto& menu:menus) {
        const auto name=reinterpret_cast<MenuName>(base+menu.entry)();
        if (uintptr_t(name)!=base+menu.object) { result.refusals=ScreenQueryUnavailable; return result; }
        if (reinterpret_cast<MenuOpen>(base+0x1a80320)(reinterpret_cast<void*>(before.ui),name)) result.facts|=menu.fact;
    }
    if (reinterpret_cast<Freeze>(base+0x1a80480)(reinterpret_cast<void*>(before.ui))) result.facts|=ScreenFrozenBackground;
    if (reinterpret_cast<Freeze>(base+0x1a804a0)(reinterpret_cast<void*>(before.ui))) result.facts|=ScreenFrozenPause;
    if (before.player_camera && before.camera_state && before.camera_vtable==base+0x25aa6d8) {
        unsigned matches=0;
        for (uint32_t i=0;i<13;++i) if (reinterpret_cast<CameraEquals>(base+0x102b5c0)(reinterpret_cast<void*>(before.player_camera),i)) {
            result.camera_index=i; ++matches;
            if (i==2) result.facts|=ScreenVatsCamera;
            if (i==3) result.facts|=ScreenFreeCamera;
        }
        if (matches!=1) result.camera_index=UINT32_MAX;
    }
    result.camera_state=before.camera_state;
    if (!observe(after) || before!=after || !render_source_is_current(source)) {
        result.refusals=ScreenObservationChanged; return result;
    }
    if (bool(result.facts&ScreenFrozenBackground)!=(before.freeze_background!=0) ||
        bool(result.facts&ScreenFrozenPause)!=(before.freeze_pause!=0)) {
        result.refusals=ScreenObservationChanged; return result;
    }
    if (!ordinary_world) result.refusals|=ScreenNotOrdinaryWorld;
    if (!main_eligible(before)) result.refusals|=ScreenMainIneligible;
    const auto identity_flags=RSF_GAME_RENDER_PRIMARY|RSF_GAME_RENDER_AFTER_SIMULATION;
    if (pass.struct_size<sizeof(pass) || pass.session_id!=source.session || pass.source_frame_id!=source.source ||
        pass.submission_id!=source.submission || pass.scope_id!=source.submission ||
        (pass.flags&identity_flags)!=identity_flags || !pass.camera_valid ||
        pass.camera.struct_size<sizeof(pass.camera) || pass.camera.abi_version!=RSF_GAME_CAMERA_ABI_VERSION ||
        !pass.color_input || !pass.depth || !pass.motion || !pass.swapchain ||
        !pass.camera.render_width || !pass.camera.render_height || !pass.camera.output_width || !pass.camera.output_height ||
        !before.world_camera || pass.view_key!=before.world_camera || pass.family_key!=pass.view_key ||
        pass.history_key!=pass.view_key || pass.native_frame!=before.native_frame) result.refusals|=ScreenWorldLeaseInvalid;
    if (result.camera_index!=0 && result.camera_index!=4 && result.camera_index!=8) result.refusals|=ScreenCameraIneligible;
    if (result.facts) result.refusals|=ScreenMenuOrFreeze;
    if (result.facts&ScreenLoading) result.screen=RSF_SCREEN_LOADING;
    else if (result.facts&(ScreenMainMenu|ScreenPause|ScreenPipboy|ScreenMap|ScreenVatsMenu)) result.screen=RSF_SCREEN_MENU;
    else if (!result.refusals) result.screen=RSF_SCREEN_GAMEPLAY;
    return result;
} catch (...) { NativeScreen result; result.refusals=ScreenQueryUnavailable; return result; }
}
