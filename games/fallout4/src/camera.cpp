// SPDX-License-Identifier: MIT
#include "camera.h"
#include <DirectXMath.h>
#include <algorithm>
#include <cmath>
#include <cstring>
#include <initializer_list>

namespace rsf::fallout4 {
namespace {
using namespace DirectX;
XMMATRIX load(const float* data) noexcept {
    XMFLOAT4X4 matrix{}; std::memcpy(&matrix,data,sizeof(matrix)); return XMLoadFloat4x4(&matrix);
}
bool store(FXMMATRIX matrix, float* data) noexcept {
    XMFLOAT4X4 values{}; XMStoreFloat4x4(&values,matrix);
    for (const auto& row : values.m) for (float value : row) if (!std::isfinite(value)) return false;
    std::memcpy(data,&values,sizeof(values)); return true;
}
bool invert(FXMMATRIX matrix, XMMATRIX& result) noexcept {
    XMVECTOR determinant{}; result = XMMatrixInverse(&determinant,matrix);
    const float value = XMVectorGetX(determinant);
    float checked[16]{};
    return std::isfinite(value) && std::abs(value) > 1e-8f && store(result,checked);
}
bool near(float a, float b) noexcept {
    return std::isfinite(a) && std::isfinite(b) && std::abs(a-b) <= 0.0005f*std::max({1.0f,std::abs(a),std::abs(b)});
}
}
bool camera_raster_transform(const CameraRaster& raster, float* output) noexcept {
    if (!output || raster.crop[0]<0 || raster.crop[1]<0 || raster.crop[2]<=raster.crop[0] || raster.crop[3]<=raster.crop[1]) return false;
    for (float v : raster.xywh) if (!std::isfinite(v)) return false;
    if (raster.xywh[0]<0 || raster.xywh[1]<0 || !(raster.xywh[2]>0) || !(raster.xywh[3]>0)) return false;
    const float w=float(raster.crop[2]-raster.crop[0]), h=float(raster.crop[3]-raster.crop[1]);
    const float sx=raster.xywh[2]/w, sy=raster.xywh[3]/h;
    const float ox=(2*(raster.xywh[0]-float(raster.crop[0]))+raster.xywh[2])/w-1;
    const float oy=1-(2*(raster.xywh[1]-float(raster.crop[1]))+raster.xywh[3])/h;
    return store(XMMatrixSet(sx,0,0,0, 0,sy,0,0, 0,0,1,0, ox,oy,0,1),output);
}
bool camera_motion_to_uv(const CameraRaster& raster, float* output) noexcept {
    if (!output) return false;
    output[0]=output[1]=0;
    float transform[16]{};
    if (!camera_raster_transform(raster,transform)) return false;
    output[0]=-transform[0]; output[1]=-transform[5]; return true;
}
bool camera_depth_domain_forward(const float* m, float range, CameraDepthBound& output) noexcept {
    output={};
    if (!m || !std::isfinite(range) || !(range>0)) return false;
    for (unsigned i=0;i<16;++i) if (!std::isfinite(m[i])) return false;
    const double epsilon=1.0/(16777215.0*double(range));
    output.coefficients[0]=m[3]; output.coefficients[1]=m[7];
    output.coefficients[2]=m[11]; output.coefficients[3]=m[15]; output.range=range;
    output.minimum=double(m[15])-std::abs(double(m[3]))-std::abs(double(m[7]))+
        std::min(-epsilon*double(m[11]),(1.0+epsilon)*double(m[11]));
    return std::isfinite(output.minimum) && output.minimum>1e-6;
}
bool camera_composed_view_depth(const float* source, const float* target, float* output) noexcept {
    using namespace DirectX;
    if (!output) return false;
    std::memset(output,0,16*sizeof(float));
    if (!source || !target) return false;
    // This convention requires the native forward perspective projection form.
    for (const float* p : {source,target}) {
        for (unsigned i=0;i<16;++i) if (!std::isfinite(p[i])) return false;
        if (!(p[0]>0) || !(p[5]>0) || !(p[10]>1) || !(p[14]<0) ||
            p[11]!=1 || p[15]!=0) return false;
        for (unsigned i : {1u,2u,3u,4u,6u,7u,12u,13u}) if (p[i]!=0) return false;
    }
    XMMATRIX source_inverse{},target_inverse{};
    if (!invert(load(source),source_inverse) || !invert(load(target),target_inverse)) return false;
    return store(XMMatrixMultiply(source_inverse,load(target)),output);
}
bool camera_frame(const NativeCamera& native, uint32_t width, uint32_t height,
    rsf_camera_frame& output, float* previous_clip_to_clip, bool& history_valid,
    const CameraRaster* current, const CameraRaster* previous) noexcept {
    using namespace DirectX;
    output = {}; history_valid = false;
    if (!width || !height || !previous_clip_to_clip) return false;
    for (float value : native.position_adjust) if (!std::isfinite(value)) return false;
    for (float value : native.previous_position_adjust) if (!std::isfinite(value)) return false;
    for (float value : native.jitter_ndc) if (!std::isfinite(value)) return false;
    XMMATRIX inverse_view{}, inverse_vp{}, inverse_projection{};
    const auto view = load(native.view);
    auto vp = load(native.view_projection_unjittered);
    if (!invert(view,inverse_view) || !invert(vp,inverse_vp)) return false;
    // Native row-vector view*projection is cached independently before jitter.
    auto projection = XMMatrixMultiply(inverse_view,vp);
    float p[16]{};
    if (!store(projection,p) || !invert(projection,inverse_projection) ||
        !(p[0]>0) || !(p[5]>0) || !near(p[11],1) || !near(p[15],0) || !(p[10]>1) || !(p[14]<0)) return false;
    const float near_plane = -p[14]/p[10], far_plane = -p[14]/(p[10]-1);
    if (!std::isfinite(near_plane) || !std::isfinite(far_plane) || !(near_plane>0) || !(far_plane>near_plane)) return false;
    // The native builder postmultiplies by an NDC translation. Its actual cached
    // projection, rather than a later cleared UI jitter global, supplies displacement.
    for (unsigned i=0;i<16;++i) if (i!=8 && i!=9 && !near(native.projection[i],p[i])) return false;
    float raster_matrix[16]{}, previous_raster_matrix[16]{};
    if (current && (current->crop[2]-current->crop[0]!=int32_t(width) || current->crop[3]-current->crop[1]!=int32_t(height) ||
        !camera_raster_transform(*current,raster_matrix))) return false;
    if (previous && !camera_raster_transform(*previous,previous_raster_matrix)) return false;
    output.jitter_pixels[0] = (native.projection[8]-p[8])*(current?current->xywh[2]:float(width))*0.5f;
    output.jitter_pixels[1] = -(native.projection[9]-p[9])*(current?current->xywh[3]:float(height))*0.5f;
    if (!std::isfinite(output.jitter_pixels[0]) || !std::isfinite(output.jitter_pixels[1]) ||
        std::abs(output.jitter_pixels[0])>2 || std::abs(output.jitter_pixels[1])>2) return false;
    if (current) {
        projection=XMMatrixMultiply(projection,load(raster_matrix));
        vp=XMMatrixMultiply(vp,load(raster_matrix));
        if (!invert(projection,inverse_projection) || !invert(vp,inverse_vp) || !store(projection,p)) return false;
    }
    const auto world_view = XMMatrixMultiply(XMMatrixTranslation(-native.position_adjust[0],
        -native.position_adjust[1],-native.position_adjust[2]),view);
    XMMATRIX view_world{};
    if (!invert(world_view,view_world) || !store(world_view,output.world_to_view) ||
        !store(view_world,output.view_to_world) || !store(projection,output.view_to_clip) ||
        !store(inverse_projection,output.clip_to_view)) return false;
    const auto delta = XMMatrixTranslation(native.position_adjust[0]-native.previous_position_adjust[0],
        native.position_adjust[1]-native.previous_position_adjust[1],native.position_adjust[2]-native.previous_position_adjust[2]);
    auto previous_vp=load(native.previous_view_projection_unjittered);
    if (previous) previous_vp=XMMatrixMultiply(previous_vp,load(previous_raster_matrix));
    const auto reproject = XMMatrixMultiply(XMMatrixMultiply(inverse_vp,delta),previous_vp);
    XMMATRIX previous_to_current{};
    history_valid = (!current || (previous && !std::memcmp(current,previous,sizeof(*current)))) &&
        invert(reproject,previous_to_current) && store(reproject,output.clip_to_previous_clip) &&
        store(previous_to_current,previous_clip_to_clip);
    if (!history_valid) {
        (void)store(XMMatrixIdentity(),output.clip_to_previous_clip);
        (void)store(XMMatrixIdentity(),previous_clip_to_clip);
    }
    output.struct_size = sizeof(output); output.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    output.render_width = output.output_width = width; output.render_height = output.output_height = height;
    output.near_plane = near_plane; output.far_plane = far_plane;
    output.vertical_fov_radians = std::atan((1-p[9])/p[5])-std::atan((-1-p[9])/p[5]);
    output.depth_inverted = 0; output.motion_jittered = 0;
    return std::isfinite(output.vertical_fov_radians) && output.vertical_fov_radians>0 && output.vertical_fov_radians<3.14159265f;
}
}
