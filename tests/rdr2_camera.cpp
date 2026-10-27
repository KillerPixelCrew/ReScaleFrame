// SPDX-License-Identifier: GPL-3.0-only
#include "../games/rdr2/src/render_camera.h"
#include <array>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <limits>
#include <vector>

namespace {
int failures{};
void check(bool value,const char* message) { if(!value){std::fprintf(stderr,"%s\n",message);++failures;} }
void put(std::array<unsigned char,0x800>& view,size_t offset,float value) {std::memcpy(view.data()+offset,&value,4);}
void matrix(std::array<unsigned char,0x800>& view,size_t offset,const std::array<float,16>& value) {std::memcpy(view.data()+offset,value.data(),64);}
std::array<float,16> identity() {std::array<float,16> m{};for(size_t i=0;i<4;++i)m[5*i]=1;return m;}
void print_matrix(const char* name,const float* values) {
    std::printf("\"%s\":[",name);for(unsigned i=0;i<16;++i)std::printf("%s%.9g",i?",":"",values[i]);std::printf("]");
}
}
int main(int argc,char** argv) {
    if(argc==11) {
        std::ifstream file(argv[1],std::ios::binary);std::vector<char> bytes{std::istreambuf_iterator<char>(file),{}};
        rsf_rdr2_camera_inputs input{};
        input.render_width=static_cast<uint32_t>(std::strtoul(argv[2],nullptr,10));input.render_height=static_cast<uint32_t>(std::strtoul(argv[3],nullptr,10));
        input.output_width=static_cast<uint32_t>(std::strtoul(argv[4],nullptr,10));input.output_height=static_cast<uint32_t>(std::strtoul(argv[5],nullptr,10));
        input.jitter[0]=std::strtof(argv[6],nullptr);input.jitter[1]=std::strtof(argv[7],nullptr);
        input.previous_jitter[0]=std::strtof(argv[8],nullptr);input.previous_jitter[1]=std::strtof(argv[9],nullptr);
        input.reset=std::atoi(argv[10])!=0;
        rsf_rdr2_camera_result output{};if(!rsf_rdr2_decode_render_camera(bytes.data(),bytes.size(),input,output))return 1;
        std::printf("{\"jitter_matches\":%s,\"near\":%.9g,\"far\":%.9g,\"fov\":%.9g,",output.projection_jitter_matches?"true":"false",output.camera.near_plane,output.camera.far_plane,output.camera.vertical_fov_radians);
        print_matrix("projection",output.camera.view_to_clip);std::printf(",");print_matrix("inverse_projection",output.camera.clip_to_view);
        std::printf(",");print_matrix("clip_to_previous",output.camera.clip_to_previous_clip);std::printf("}\n");return 0;
    }
    std::array<unsigned char,0x800> view{};
    auto current=identity(),previous=identity(),world=identity();
    current[12]=-10;current[13]=20;current[14]=-30;world[12]=10;world[13]=-20;world[14]=30;
    previous=current;previous[12]=-9;
    constexpr float near=.15f,far=10000,fov=.66f;
    const float focal=1/std::tan(fov/2),jx=.25f,jy=-.3888888955f;
    std::array<float,16> projection{};projection[0]=focal/1.6f;projection[5]=focal;
    projection[8]=-2*jx/1706;projection[9]=2*jy/1066;projection[10]=near/(far-near);projection[11]=-1;
    projection[14]=near*far/(far-near);
    matrix(view,0x40,current);matrix(view,0x80,projection);matrix(view,0xc0,world);matrix(view,0x140,previous);matrix(view,0x180,projection);
    put(view,0x630,near);put(view,0x634,far);put(view,0x640,fov*180/3.14159265358979323846f);
    put(view,0x664,projection[8]);put(view,0x668,projection[9]);put(view,0x67c,1706);put(view,0x680,1066);view[0x671]=1;
    rsf_rdr2_camera_inputs input{1706,1066,2560,1600,{jx,jy},{-.25f,.16666667f},1.f/60,false,false};
    rsf_rdr2_camera_result output{};
    check(rsf_rdr2_decode_render_camera(view.data(),view.size(),input,output),"valid RAGE camera");
    check(output.projection_jitter_matches,"NGX jitter agrees with the producer");
    check(output.camera.view_to_clip[8]==0 && output.camera.view_to_clip[9]==0,"both jitter offsets removed");
    for(unsigned r=0;r<4;++r)for(unsigned c=0;c<4;++c){double sum{};for(unsigned k=0;k<4;++k)sum+=double(output.camera.view_to_clip[r*4+k])*output.camera.clip_to_view[k*4+c];
        check(std::abs(sum-(r==c?1.:0.))<1e-6,"projection times independently returned inverse");}
    // World point (10,-20,20): current view (0,0,-10), previous view (1,0,-10).
    const double clip[]{0,0,(-10.*projection[10]+projection[14])/10,1};double transformed[4]{};
    for(unsigned c=0;c<4;++c)for(unsigned r=0;r<4;++r)transformed[c]+=clip[r]*output.camera.clip_to_previous_clip[r*4+c];
    check(std::abs(transformed[0]/transformed[3]-projection[0]/10)<1e-6,"known static point reprojects to the previous camera");
    input.reset=true;check(rsf_rdr2_decode_render_camera(view.data(),view.size(),input,output),"reset camera");
    for(unsigned i=0;i<16;++i)check(output.camera.clip_to_previous_clip[i]==(i%5==0?1.f:0.f),"reset clears reprojection");
    check(output.camera.previous_jitter_pixels[0]==jx,"reset initializes jitter history");
    input.jitter[0]=-.25f;check(rsf_rdr2_decode_render_camera(view.data(),view.size(),input,output) && !output.projection_jitter_matches,"frozen pause projection remains explicit");
    input.jitter[0]=std::numeric_limits<float>::quiet_NaN();check(!rsf_rdr2_decode_render_camera(view.data(),view.size(),input,output),"non-finite vendor jitter refused");
    input.jitter[0]=jx;put(view,0x80,0);check(!rsf_rdr2_decode_render_camera(view.data(),view.size(),input,output),"singular projection refused");
    check(!rsf_rdr2_decode_render_camera(view.data(),16,input,output),"short copied view refused");
    return failures?1:0;
}
