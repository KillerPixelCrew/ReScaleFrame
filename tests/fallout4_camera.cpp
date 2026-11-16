// SPDX-License-Identifier: GPL-3.0-only
#include "../games/fallout4/src/camera.h"
#include <cmath>
#include <cstdio>
#include <cstring>
#include <limits>
#include <initializer_list>
#include <DirectXMath.h>

namespace {
int failures{};
void check(bool value, const char* name) { if (!value) { ++failures; std::printf("FAIL %s\n",name); } }
bool close(float a, float b, float tolerance=0.0001f) { return std::isfinite(a) && std::isfinite(b) && std::abs(a-b)<tolerance; }
void transform(const float* p, const float* m, float* output) {
    for (unsigned j=0;j<4;++j) { output[j]=0; for (unsigned i=0;i<4;++i) output[j]+=p[i]*m[i*4+j]; }
}
rsf::fallout4::NativeCamera camera() {
    rsf::fallout4::NativeCamera n{};
    for (unsigned i=0;i<4;++i) n.view[i*5]=1;
    n.projection[0]=2; n.projection[5]=3; n.projection[8]=0.2f; n.projection[9]=-0.1f;
    n.projection[10]=1.01f; n.projection[11]=1; n.projection[14]=-1.01f;
    std::memcpy(n.view_projection_unjittered,n.projection,sizeof(n.projection));
    std::memcpy(n.previous_view_projection_unjittered,n.projection,sizeof(n.projection));
    n.projection[8]+=0.0007f; n.projection[9]-=0.0011f;
    return n;
}
}
int main() {
    using namespace rsf::fallout4;
    auto native=camera();
    CameraRaster raster{{0.75f,1.25f,99.2f,79.6f},{1,1,100,81}};
    rsf_camera_frame frame{}; float backward[16]{}; bool history{};
    check(camera_frame(native,99,80,frame,backward,history,&raster,&raster) && history,"fractional raster camera and steady history");
    // Independent geometric expectation: ray through a source allocation pixel
    // center must project to that exact center in the integer cropped grid.
    constexpr float px=42.5f, py=25.5f, z=5;
    const float nx=2*(px-0.75f)/99.2f-1, ny=1-2*(py-1.25f)/79.6f;
    const float point[]{(nx-0.2f)*z/2,(ny+0.1f)*z/3,z,1};
    float clip[4]{}, restored[4]{};
    transform(point,frame.view_to_clip,clip);
    check(close(clip[0]/clip[3],2*(px-1)/99-1) && close(clip[1]/clip[3],1-2*(py-1)/80),"asymmetric non-origin pixel-center projection");
    transform(clip,frame.clip_to_view,restored);
    for (unsigned i=0;i<3;++i) check(close(restored[i]/restored[3],point[i]),"canonical projection inverse roundtrip");
    check(close(frame.jitter_pixels[0],0.0007f*99.2f/2) && close(frame.jitter_pixels[1],0.0011f*79.6f/2),"jitter uses actual raster not rounded crop");
    check(close(frame.near_plane,1) && close(frame.far_plane,101,0.002f) && frame.render_width==99 && frame.render_height==80,"projection planes and integer extents");
    for (float depth : {1.0f,101.0f}) {
        const float ray[]{0,0,depth,1}; float q[4]{}; transform(ray,frame.view_to_clip,q);
        check(close(q[2]/q[3],depth==1?0.0f:1.0f),"crop affine preserves near/far depth");
    }
    float motion[2]{};
    check(camera_motion_to_uv(raster,motion),"native motion crop conversion available");
    // Raw native motion is previous-current UV. A point moving +4/-3 pixels
    // has current-previous crop UV +4/99,-3/80, independently of origin/FOV.
    check(close((-4/99.2f)*motion[0],4/99.0f) && close((3/79.6f)*motion[1],-3/80.0f),"native motion pixel displacement roundtrip");
    native.position_adjust[0]=10; native.previous_position_adjust[0]=9;
    check(camera_frame(native,99,80,frame,backward,history,&raster,&raster) && history,"origin delta with canonical history");
    transform(point,frame.view_to_clip,clip);
    float previous[4]{}; transform(clip,frame.clip_to_previous_clip,previous);
    const float expected_previous_pixel=0.75f+99.2f*((2*(point[0]+1)/z+0.2f)+1)/2;
    check(close(previous[0]/previous[3],2*(expected_previous_pixel-1)/99-1),"previous projection follows origin-relative geometry");
    CameraRaster old{{2.25f,0.5f,94.8f,78.1f},{2,0,97,79}};
    check(camera_frame(native,99,80,frame,backward,history,&raster,&old) && !history,"changed previous raster resets history");
    check(camera_frame(native,99,80,frame,backward,history,&raster,nullptr) && !history,"missing previous raster resets history");
    auto bad=raster; bad.crop[2]=bad.crop[0];
    check(!camera_frame(native,99,80,frame,backward,history,&bad,&raster),"empty crop refused");
    bad=raster; bad.xywh[2]=std::numeric_limits<float>::quiet_NaN();
    check(!camera_frame(native,99,80,frame,backward,history,&bad,&raster) && !camera_motion_to_uv(bad,motion),"nonfinite raster refused");
    bad=raster; bad.xywh[3]=0;
    check(!camera_motion_to_uv(bad,motion),"degenerate native viewport refused");
    native=camera(); std::memset(native.view,0,sizeof(native.view));
    check(!camera_frame(native,99,80,frame,backward,history,&raster,&raster),"singular camera refused");
    native=camera(); native.projection[0]=std::numeric_limits<float>::infinity();
    check(!camera_frame(native,99,80,frame,backward,history,&raster,&raster),"nonfinite projection refused");
    // Observed world origin is separate from the native rotation-only cache V.
    // Stress the actual projection recovery/history implementation with the
    // observed near/far ratio and all eight jitter phases, without inserting
    // that world origin into V a second time.
    constexpr float angle=0.83f;
    const float c=std::cos(angle), s=std::sin(angle);
    constexpr float phases[8][2]{{0,-0.3333333f},{-0.5f,0.3333333f},{0.5f,-0.7777778f},{-0.75f,-0.1111111f},
        {0.25f,0.5555556f},{-0.25f,-0.5555556f},{0.75f,0.1111111f},{-0.875f,0.7777778f}};
    CameraRaster observed{{0,0,2560,1600},{0,0,2560,1600}};
    for (const auto& phase : phases) {
        NativeCamera stress{};
        const float view[]{c,0,-s,0, 0,1,0,0, s,0,c,0, 0,0,0,1};
        std::memcpy(stress.view,view,sizeof(view));
        float projection[16]{};
        projection[0]=1.32417f; projection[5]=2.11867f;
        projection[10]=353468.0f/(353468.0f-15); projection[11]=1;
        projection[14]=-15*353468.0f/(353468.0f-15);
        std::memcpy(stress.projection,projection,sizeof(projection));
        stress.projection[8]+=phase[0]/2560; stress.projection[9]+=phase[1]/1600;
        for (unsigned i=0;i<4;++i) for (unsigned j=0;j<4;++j) {
            float value=0; for (unsigned k=0;k<4;++k) value+=view[i*4+k]*projection[k*4+j];
            stress.view_projection_unjittered[i*4+j]=stress.previous_view_projection_unjittered[i*4+j]=value;
        }
        stress.position_adjust[0]=-80300.7f; stress.position_adjust[1]=90804.2f; stress.position_adjust[2]=7973.14f;
        stress.previous_position_adjust[0]=stress.position_adjust[0]-0.25f;
        stress.previous_position_adjust[1]=stress.position_adjust[1]+0.125f;
        stress.previous_position_adjust[2]=stress.position_adjust[2]-0.0625f;
        check(camera_frame(stress,2560,1600,frame,backward,history,&observed,&observed) && history,"observed large origin and far/near jitter phase");
        for (unsigned i=0;i<16;++i) check(close(frame.view_to_clip[i],projection[i]),"large origin does not contaminate native projection recovery");
        check(close(frame.jitter_pixels[0],phase[0]/2) && close(frame.jitter_pixels[1],-phase[1]/2),"eight-phase actual jitter units");
        check(close(frame.near_plane,15,0.001f) && std::abs(frame.far_plane-353468)<1000,"observed finite near/far retained");
        const float ray[]{2,1,100,1}; float current_clip[4]{}, history_clip[4]{};
        transform(ray,frame.view_to_clip,current_clip); transform(current_clip,frame.clip_to_previous_clip,history_clip);
        // Direct analytic yaw of the origin difference into view coordinates.
        const float previous_x=2+0.25f*c+0.0625f*s;
        const float previous_y=1-0.125f;
        const float previous_z=100-0.25f*s+0.0625f*c;
        check(close(history_clip[0]/history_clip[3],1.32417f*previous_x/previous_z) &&
            close(history_clip[1]/history_clip[3],2.11867f*previous_y/previous_z),"large-origin history follows analytic relative displacement");
        stress.view_projection_unjittered[0]+=0.1f;
        check(!camera_frame(stress,2560,1600,frame,backward,history,&observed,&observed),"inconsistent source cache still refuses without tolerance widening");
    }
    float domain[16]{}; for (unsigned i=0;i<4;++i) domain[i*5]=1;
    CameraDepthBound bound{};
    check(camera_depth_domain_forward(domain,0.01f,bound) && bound.minimum==1,"identity depth mapping forward W");
    domain[3]=0.1f; domain[7]=-0.2f; domain[11]=-0.3f;
    check(camera_depth_domain_forward(domain,0.01f,bound),"off-axis positive depth mapping forward W");
    // Independent enumeration of affine-W clip corners, including native
    // D24 endpoint uncertainty, must agree with the production admission bound.
    const double epsilon=1.0/(16777215.0*double(0.01f));
    double corner_minimum=std::numeric_limits<double>::infinity();
    for (double x : {-1.0,1.0}) for (double y : {-1.0,1.0}) for (double zc : {-epsilon,1.0+epsilon}) {
        const double w=x*double(domain[3])+y*double(domain[7])+zc*double(domain[11])+double(domain[15]);
        if (w<corner_minimum) corner_minimum=w;
    }
    check(std::abs(bound.minimum-corner_minimum)<1e-12,"W bound agrees with independently enumerated corners");
    domain[3]=1.1f;
    check(!camera_depth_domain_forward(domain,0.01f,bound) && bound.minimum<0,"behind-target corner retains refusal and bound");
    domain[3]=domain[7]=domain[11]=0; domain[15]=0.0000005f;
    check(!camera_depth_domain_forward(domain,0.01f,bound) && bound.minimum>0,"small positive W retains existing admission threshold");
    domain[15]=std::numeric_limits<float>::infinity();
    check(!camera_depth_domain_forward(domain,0.01f,bound),"nonfinite depth map refused");
    check(!camera_depth_domain_forward(nullptr,0.01f,bound),"missing depth map refused");
    // Actual recorded origins belong to different composed coordinate domains.
    // A physical origin translation puts a local first-person point behind the
    // world camera; the adopted convention instead preserves its view-space Z.
    const float near_origin[]{-0.00769183971f,0.0319054201f,120.485939f};
    const float world_origin[]{-80300.6641f,90804.2344f,7973.14453f};
    float near_projection[16]{},world_projection[16]{},composed[16]{};
    near_projection[0]=2.3f; near_projection[5]=3.1f;
    near_projection[10]=1000.0f/999; near_projection[11]=1; near_projection[14]=-1000.0f/999;
    world_projection[0]=1.32417f; world_projection[5]=2.11867f;
    world_projection[10]=353468.0f/(353468-15); world_projection[11]=1;
    world_projection[14]=-15*353468.0f/(353468-15);
    for (float jitter : {-0.000341797f,0.0001953125f}) {
        near_projection[8]=jitter; near_projection[9]=0.000486111f;
        check(camera_composed_view_depth(near_projection,world_projection,composed),"virtual first-person depth matrix admitted");
        check(camera_depth_domain_forward(composed,0.01f,bound),"virtual depth retains positive W over quantized near interval");
        for (float zview : {20.0f,200.0f,800.0f}) {
            const float local[]{4,-3,zview,1}; float source_clip[4]{},target_clip[4]{};
            transform(local,near_projection,source_clip); transform(source_clip,composed,target_clip);
            const double expected_depth=(353468.0/(353468.0-15.0))*(1.0-15.0/double(zview));
            check(std::abs(double(target_clip[2]/target_clip[3])-expected_depth)<0.000002,
                "composed scalar depth agrees independent view-Z perspective equation");
            check(close(target_clip[3],zview,0.001f),"virtual convention preserves positive local view distance");
        }
    }
    using namespace DirectX;
    XMFLOAT4X4 source_matrix{},target_matrix{};
    std::memcpy(&source_matrix,near_projection,sizeof(source_matrix));
    std::memcpy(&target_matrix,world_projection,sizeof(target_matrix));
    const auto incorrectly_shared=XMMatrixMultiply(XMMatrixMultiply(XMMatrixInverse(nullptr,XMLoadFloat4x4(&source_matrix)),
        XMMatrixTranslation(near_origin[0]-world_origin[0],near_origin[1]-world_origin[1],near_origin[2]-world_origin[2])),
        XMLoadFloat4x4(&target_matrix));
    XMFLOAT4X4 physical{}; XMStoreFloat4x4(&physical,incorrectly_shared);
    check(!camera_depth_domain_forward(&physical._11,0.01f,bound) && bound.minimum<0,
        "recorded origins still refuse the inappropriate shared-world model");
    const float point40[]{4,-3,40,1}; float local_clip[4]{},physical_clip[4]{};
    transform(point40,near_projection,local_clip); transform(local_clip,&physical._11,physical_clip);
    check(close(physical_clip[3],40+near_origin[2]-world_origin[2],0.01f) && physical_clip[3]<0,
        "recorded origin translation has independently predicted behind-camera W");
    float invalid_projection[16]{};
    check(!camera_composed_view_depth(invalid_projection,world_projection,composed),"singular virtual projection refused");
    for (float value : composed) check(value==0,"failed composed map is zero");
    std::memcpy(invalid_projection,near_projection,sizeof(invalid_projection)); invalid_projection[3]=0.1f;
    check(!camera_composed_view_depth(invalid_projection,world_projection,composed),"non-perspective composed model refused");
    invalid_projection[3]=0; invalid_projection[0]=std::numeric_limits<float>::quiet_NaN();
    check(!camera_composed_view_depth(invalid_projection,world_projection,composed),"nonfinite virtual projection refused");
    check(!camera_composed_view_depth(nullptr,world_projection,composed),"missing virtual projection refused");
    if (!failures) std::puts("PASS: Fallout4 fractional crop geometry, jitter, motion units, history and invalid camera refusals.");
    return failures?1:0;
}
