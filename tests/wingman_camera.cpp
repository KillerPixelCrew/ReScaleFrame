// SPDX-License-Identifier: GPL-3.0-only
#include "../games/project-wingman/src/native_camera.h"
#include <array>
#include <cmath>
#include <cstring>
#include <cstdio>

namespace {
template<class T> void store(void* base, size_t offset, const T& value)
{ std::memcpy(static_cast<unsigned char*>(base)+offset,&value,sizeof(value)); }
bool close(float left, float right) { return std::abs(left-right)<0.0001f; }
}
int main()
{
    std::array<unsigned char,0x5700> view{};
    std::array<unsigned char,0xd0> family{};
    auto* family_pointer=family.data(); store(view.data(),0,family_pointer);
    const uint32_t frame=37; const float delta=1.f/60.f;
    store(family.data(),0x5c,frame); store(family.data(),0x54,delta);
    const float identity[16]={1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1};
    const float projection[16]={1,0,0,0,0,2,0,0,0,0,0,1,0,0,10,0};
    store(view.data(),0x2c0,projection); store(view.data(),0x5010,projection);
    store(view.data(),0x340,identity); store(view.data(),0x380,identity);
    store(view.data(),0x480,identity); store(view.data(),0x51d0,identity);
    rsf_game_render_pass pass{};
    pass.render_rect[2]=100; pass.render_rect[3]=80;
    pass.output_rect[2]=200; pass.output_rect[3]=160;
    if (!rsf::wingman::read_camera(view.data(),pass) || pass.native_frame!=frame ||
        !close(pass.camera.near_plane,10) || !close(pass.camera.vertical_fov_radians,2.f*std::atan(0.5f))) return 1;
    for (uint32_t i=0;i<16;++i) if (!close(pass.camera.clip_to_previous_clip[i],identity[i])) return 1;
    // Reprojection follows translated coordinates, not a current/previous world-position subtraction
    // performed after projecting. A one-unit camera translation is 0.1 NDC at this near plane.
    const float current_translation[3]={-1,0,0};
    store(view.data(),0x600,current_translation);
    if (!rsf::wingman::read_camera(view.data(),pass) || !close(pass.camera.clip_to_previous_clip[8],0.1f)) return 1;
    const uint32_t unknown=UINT32_MAX; store(family.data(),0x5c,unknown);
    if (rsf::wingman::read_camera(view.data(),pass) || pass.camera_valid) return 1;
    store(family.data(),0x5c,frame);
    const float paused=0; store(family.data(),0x54,paused);
    if (rsf::wingman::read_camera(view.data(),pass)) return 1;
    std::puts("PASS: Wingman projection, translated camera reprojection, frame identity and pause refusal.");
    return 0;
}
