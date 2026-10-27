// SPDX-License-Identifier: GPL-3.0-only
#include "render_camera.h"
#include <algorithm>
#include <array>
#include <cmath>
#include <cstring>

namespace {
using Matrix = std::array<double,16>;
Matrix identity()
{
    Matrix result{};
    for (size_t i=0;i<4;++i) result[i*4+i]=1;
    return result;
}
Matrix multiply(const Matrix& a,const Matrix& b)
{
    Matrix result{};
    for (size_t r=0;r<4;++r) for (size_t c=0;c<4;++c)
        for (size_t k=0;k<4;++k) result[r*4+c]+=a[r*4+k]*b[k*4+c];
    return result;
}
bool inverse(const Matrix& input,Matrix& output)
{
    double rows[4][8]{};
    for (size_t r=0;r<4;++r) {
        for (size_t c=0;c<4;++c) rows[r][c]=input[r*4+c];
        rows[r][r+4]=1;
    }
    for (size_t c=0;c<4;++c) {
        size_t pivot=c;
        for (size_t r=c+1;r<4;++r) if (std::abs(rows[r][c])>std::abs(rows[pivot][c])) pivot=r;
        if (!std::isfinite(rows[pivot][c]) || std::abs(rows[pivot][c])<1e-12) return false;
        if (pivot!=c) for (size_t k=0;k<8;++k) std::swap(rows[c][k],rows[pivot][k]);
        const auto divisor=rows[c][c];
        for (auto& value:rows[c]) value/=divisor;
        for (size_t r=0;r<4;++r) if (r!=c) {
            const auto scale=rows[r][c];
            for (size_t k=0;k<8;++k) rows[r][k]-=scale*rows[c][k];
        }
    }
    for (size_t r=0;r<4;++r) for (size_t c=0;c<4;++c) output[r*4+c]=rows[r][c+4];
    return true;
}
bool matrix(const unsigned char* bytes,size_t offset,Matrix& result)
{
    float source[16];std::memcpy(source,bytes+offset,sizeof(source));
    for (size_t i=0;i<16;++i) {
        if (!std::isfinite(source[i])) return false;
        result[i]=source[i];
    }
    return true;
}
float scalar(const unsigned char* bytes,size_t offset)
{
    float value{};std::memcpy(&value,bytes+offset,sizeof(value));return value;
}
bool store(const Matrix& source,float* destination)
{
    for (size_t i=0;i<16;++i) {
        destination[i]=static_cast<float>(source[i]);
        if (!std::isfinite(destination[i])) return false;
    }
    return true;
}
}
bool rsf_rdr2_decode_render_camera(const void* pointer,size_t bytes,
    const rsf_rdr2_camera_inputs& input,rsf_rdr2_camera_result& result) noexcept
{
    result={};
    if (!pointer || bytes<0x68c || !input.render_width || !input.render_height ||
        !input.output_width || !input.output_height || input.render_width>16384 || input.render_height>16384 ||
        input.output_width>16384 || input.output_height>16384 || !std::isfinite(input.frame_time_seconds) ||
        input.frame_time_seconds<0) return false;
    for (auto value:input.jitter) if (!std::isfinite(value)) return false;
    for (auto value:input.previous_jitter) if (!std::isfinite(value)) return false;
    const auto* raw=static_cast<const unsigned char*>(pointer);
    if (raw[0x671]!=1) return false;
    Matrix view{},projection{},world{},previous_view{},previous_projection{};
    if (!matrix(raw,0x40,view) || !matrix(raw,0x80,projection) || !matrix(raw,0xc0,world) ||
        !matrix(raw,0x140,previous_view) || !matrix(raw,0x180,previous_projection)) return false;
    const auto near_plane=scalar(raw,0x630),far_plane=scalar(raw,0x634),fov=scalar(raw,0x640);
    const auto jitter_x=scalar(raw,0x664),jitter_y=scalar(raw,0x668);
    const auto viewport_w=scalar(raw,0x67c),viewport_h=scalar(raw,0x680);
    if (!std::isfinite(near_plane) || !std::isfinite(far_plane) || !std::isfinite(fov) ||
        near_plane<=0 || far_plane<=near_plane || fov<=0 || fov>=180 ||
        !std::isfinite(jitter_x) || !std::isfinite(jitter_y) ||
        !std::isfinite(viewport_w) || !std::isfinite(viewport_h) || viewport_w<=0 || viewport_h<=0 ||
        projection[11]!=-1 || std::abs(projection[15])>1e-6) return false;
    rsf_rdr2_camera_result decoded{};
    decoded.projection_jitter_matches=
        std::abs(-jitter_x*std::floor(viewport_w+0.5f)/2-input.jitter[0])<1e-4f &&
        std::abs(jitter_y*std::floor(viewport_h+0.5f)/2-input.jitter[1])<1e-4f;
    // +0x180 is rebuilt with current jitter, not the preceding frame's jitter.
    projection[8]-=jitter_x;projection[9]-=jitter_y;
    previous_projection[8]-=jitter_x;previous_projection[9]-=jitter_y;
    Matrix clip_to_view{},current_inverse{},clip_to_previous=identity(),previous_to_clip=identity();
    if (!inverse(projection,clip_to_view) || !inverse(multiply(view,projection),current_inverse)) return false;
    if (!input.reset) {
        clip_to_previous=multiply(current_inverse,multiply(previous_view,previous_projection));
        if (!inverse(clip_to_previous,previous_to_clip)) return false;
    }
    auto& c=decoded.camera;
    c.struct_size=sizeof(c);c.abi_version=RSF_GAME_FRAME_ABI_VERSION;
    if (!store(view,c.world_to_view) || !store(world,c.view_to_world) || !store(projection,c.view_to_clip) ||
        !store(clip_to_view,c.clip_to_view) || !store(clip_to_previous,c.clip_to_previous_clip) ||
        !store(previous_to_clip,decoded.previous_clip_to_clip)) return false;
    std::copy(std::begin(input.jitter),std::end(input.jitter),c.jitter_pixels);
    const auto* previous=input.reset?input.jitter:input.previous_jitter;
    std::copy(previous,previous+2,c.previous_jitter_pixels);
    c.render_width=input.render_width;c.render_height=input.render_height;
    c.output_width=input.output_width;c.output_height=input.output_height;
    c.near_plane=near_plane;c.far_plane=far_plane;c.vertical_fov_radians=fov*0.01745329251994329577f;
    c.depth_inverted=1;c.motion_jittered=input.motion_jittered?1u:0u;c.frame_time_seconds=input.frame_time_seconds;
    result=decoded;return true;
}
