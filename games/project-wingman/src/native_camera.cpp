// SPDX-License-Identifier: GPL-3.0-only
#include "native_camera.h"
#include <windows.h>
#include <algorithm>
#include <cmath>
#include <cstring>
#include <limits>

namespace rsf::wingman {
namespace {
bool copy(void* output, const void* input, size_t bytes) noexcept
{
    if (!input) return false;
#if defined(_MSC_VER)
    __try { std::memcpy(output, input, bytes); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#else
    MEMORY_BASIC_INFORMATION page{};
    if (!VirtualQuery(input, &page, sizeof(page)) || page.State != MEM_COMMIT ||
        (page.Protect & (PAGE_GUARD | PAGE_NOACCESS)) ||
        uintptr_t(input) + bytes > uintptr_t(page.BaseAddress) + page.RegionSize) return false;
    std::memcpy(output, input, bytes); return true;
#endif
}
template<class T> bool read(const void* base, size_t offset, T& value) noexcept
{ return base && copy(&value, static_cast<const unsigned char*>(base) + offset, sizeof(value)); }
void multiply(const float* left, const float* right, float* result) noexcept
{
    float temporary[16]{};
    for (uint32_t row=0;row<4;++row) for (uint32_t col=0;col<4;++col)
        for (uint32_t inner=0;inner<4;++inner) temporary[row*4+col]+=left[row*4+inner]*right[inner*4+col];
    std::memcpy(result,temporary,sizeof(temporary));
}
bool inverse(const float* input, float* result) noexcept
{
    double augmented[4][8]{};
    for (uint32_t row=0;row<4;++row) {
        for (uint32_t col=0;col<4;++col) {
            if (!std::isfinite(input[row*4+col])) return false;
            augmented[row][col]=input[row*4+col];
        }
        augmented[row][row+4]=1;
    }
    for (uint32_t col=0;col<4;++col) {
        uint32_t pivot=col;
        for (uint32_t row=col+1;row<4;++row)
            if (std::abs(augmented[row][col])>std::abs(augmented[pivot][col])) pivot=row;
        if (std::abs(augmented[pivot][col])<1e-12) return false;
        for (uint32_t item=0;item<8;++item) std::swap(augmented[pivot][item],augmented[col][item]);
        const double divisor=augmented[col][col];
        for (double& item : augmented[col]) item/=divisor;
        for (uint32_t row=0;row<4;++row) if (row!=col) {
            const double factor=augmented[row][col];
            for (uint32_t item=0;item<8;++item) augmented[row][item]-=factor*augmented[col][item];
        }
    }
    for (uint32_t row=0;row<4;++row) for (uint32_t col=0;col<4;++col)
        result[row*4+col]=float(augmented[row][col+4]);
    return true;
}
}

bool read_camera(const void* view, rsf_game_render_pass& pass) noexcept
{
    pass.camera_valid=0;
    void* family=nullptr;
    uint32_t frame=0;
    float current_translated[16]{}, previous_translated[16]{}, previous_projection[16]{};
    float current_translation[3]{}, previous_translation[3]{};
    auto& camera=pass.camera;
    // UnscaledViewRect at 0x258 is instruction-backed by GetSecondaryViewRectSize.
    // The following rectangle/cascade fields place FViewMatrices at 0x280. The
    // previous block ends at its pre-exposure field 0x5380, before history 0x53d0.
    if (!read(view,0,family) || !read(family,0x5c,frame) ||
        !read(family,0x54,camera.frame_time_seconds) ||
        !read(view,0x2c0,camera.view_to_clip) || !read(view,0x340,camera.world_to_view) ||
        !read(view,0x380,camera.view_to_world) || !read(view,0x480,current_translated) ||
        !read(view,0x600,current_translation) || !read(view,0x5010,previous_projection) ||
        !read(view,0x51d0,previous_translated) || !read(view,0x5350,previous_translation) ||
        !read(view,0x4fa4,camera.jitter_pixels)) return false;
    if (frame==UINT32_MAX || !std::isfinite(camera.frame_time_seconds) ||
        camera.frame_time_seconds<=0 || camera.frame_time_seconds>1 ||
        std::abs(camera.view_to_clip[15])>1e-6f || camera.view_to_clip[0]<=0 || camera.view_to_clip[5]<=0 ||
        !inverse(camera.view_to_clip,camera.clip_to_view)) return false;
    for (uint32_t i=0;i<16;++i)
        if (!std::isfinite(camera.view_to_world[i]) || !std::isfinite(camera.world_to_view[i]) ||
            !std::isfinite(previous_projection[i])) return false;
    float identity[16]{};
    multiply(camera.world_to_view,camera.view_to_world,identity);
    for (uint32_t row=0;row<4;++row) for (uint32_t col=0;col<4;++col)
        if (std::abs(identity[row*4+col]-(row==col?1.f:0.f))>0.02f) return false;
    float transposed[16]{}, delta[16]{}, inv_current[16]{}, previous[16]{};
    for (uint32_t row=0;row<4;++row) for (uint32_t col=0;col<4;++col) {
        transposed[row*4+col]=current_translated[col*4+row];
        delta[row*4+col]=row==col?1.f:0.f;
    }
    for (uint32_t i=0;i<3;++i) delta[12+i]=previous_translation[i]-current_translation[i];
    multiply(camera.clip_to_view,transposed,inv_current);
    multiply(delta,previous_translated,previous);
    multiply(previous,previous_projection,previous);
    multiply(inv_current,previous,camera.clip_to_previous_clip);
    if (!inverse(camera.clip_to_previous_clip,pass.previous_clip_to_clip)) return false;
    camera.struct_size=sizeof(camera); camera.abi_version=RSF_GAME_FRAME_ABI_VERSION;
    camera.render_width=uint32_t(pass.render_rect[2]-pass.render_rect[0]);
    camera.render_height=uint32_t(pass.render_rect[3]-pass.render_rect[1]);
    camera.output_width=uint32_t(pass.output_rect[2]-pass.output_rect[0]);
    camera.output_height=uint32_t(pass.output_rect[3]-pass.output_rect[1]);
    camera.near_plane=camera.view_to_clip[14]/(1.f-camera.view_to_clip[10]);
    camera.far_plane=camera.view_to_clip[10]<0 ? -camera.view_to_clip[14]/camera.view_to_clip[10]
                                                : std::numeric_limits<float>::max();
    camera.vertical_fov_radians=2.f*std::atan(1.f/camera.view_to_clip[5]);
    camera.depth_inverted=1; camera.motion_jittered=0;
    pass.family_key=uintptr_t(family); pass.native_frame=frame;
    pass.motion_to_uv[0]=0.5f; pass.motion_to_uv[1]=-0.5f; pass.motion_camera_included=1;
    pass.camera_valid=std::isfinite(camera.near_plane) && camera.near_plane>0 &&
        camera.far_plane>camera.near_plane && camera.render_width && camera.render_height &&
        camera.output_width && camera.output_height ? 1u : 0u;
    return pass.camera_valid!=0;
}
}
