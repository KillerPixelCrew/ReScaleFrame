// SPDX-License-Identifier: GPL-3.0-only
#include "../integrations/godot/native/api.h"
#include "../integrations/unity/native/package.h"
#include <windows.h>
#include <cstddef>
#include <cstdio>
#include <cstring>
#include <limits>
static_assert(sizeof(rsf_godot_settings)==100 && offsetof(rsf_godot_settings,sharpening)==44);
static_assert(sizeof(rsf_unity_package_settings)==100 && offsetof(rsf_unity_package_settings,sharpening)==44);
static_assert(sizeof(rsf_srvk_frame)==384 && offsetof(rsf_srvk_frame,exposure)==272 && offsetof(rsf_srvk_frame,sharpening)==312);
static_assert(RSF_SRVK_ABI_VERSION==2 && RSF_GODOT_SETTINGS_ABI_VERSION==2 && RSF_UNITY_SETTINGS_ABI_VERSION==2);
template<class Settings> bool verify(const char* path,const char* configure_name,const char* get_name) {
    auto module=LoadLibraryA(path);if(!module){std::printf("LoadLibrary failed %lu: %s\n",GetLastError(),path);return false;}
    auto configure=reinterpret_cast<int(*)(const Settings*)>(GetProcAddress(module,configure_name));
    auto get=reinterpret_cast<int(*)(Settings*)>(GetProcAddress(module,get_name));
    if(!configure||!get)return false;
    Settings value{};value.struct_size=sizeof(value);value.abi_version=2;value.sr_backend=7;value.quality=1;value.fg_backend=7;value.generated_frames=1;value.gameplay=1;
    value.sharpening=rsf_sharpening_defaults();
    for(uint32_t mode=0;mode<=4;++mode) {
        value.sharpening.mode=mode;value.sharpening.sharpness=.713f;value.sharpening.mas=1;value.sharpening.motion_sharpness=-.31f;
        value.sharpening.motion_threshold=4;value.sharpening.motion_scale_limit=19;value.sharpening.contrast_enabled=1;value.sharpening.contrast=1.6f;
        value.sharpening.depth_scale=128;value.sharpening.depth_bias=.0037f;value.sharpening.clamp_output=1;value.sharpening.debug=1;
        Settings actual{};actual.struct_size=sizeof(actual);actual.abi_version=2;
        if(!configure(&value)||!get(&actual)||std::memcmp(&value,&actual,sizeof(value)))return false;
    }
    auto invalid=value;invalid.abi_version=1;if(configure(&invalid))return false;
    invalid=value;--invalid.struct_size;if(configure(&invalid))return false;
    invalid=value;invalid.sharpening.sharpness=std::numeric_limits<float>::quiet_NaN();if(configure(&invalid))return false;
    invalid=value;invalid.sharpening.mode=5;if(configure(&invalid))return false;
    invalid=value;invalid.sharpening.depth_scale=601;if(configure(&invalid))return false;
    Settings actual{};actual.struct_size=sizeof(actual);actual.abi_version=2;
    if(!get(&actual)||std::memcmp(&value,&actual,sizeof(value)))return false;
    FreeLibrary(module);return true;
}
int main(int argc,char** argv) {
    if(argc!=3)return 1;
    if(!verify<rsf_godot_settings>(argv[1],"rsfg_configure","rsfg_settings")||!verify<rsf_unity_package_settings>(argv[2],"rsfu_configure","rsfu_settings"))return 1;
    std::puts("Godot and Unity settings: all 12 options round trip in five modes; ABI/range refusal preserves settings.");return 0;
}
