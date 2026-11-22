// SPDX-License-Identifier: MIT
#include "rdr2_camera_abi.h"
#include <array>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <vector>

namespace {
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
    if (argc != 2) return 1;
    // RSFCAMF1: the copied RAGE view, Windows x64 camera inputs, then expected jitter status/FOV.
    std::ifstream file(argv[1], std::ios::binary);
    std::array<char, 8> magic{};
    std::array<char, 0x800> view{};
    rsf_rdr2_camera_inputs input{};
    uint32_t jitter_matches{};
    float fov{};
    if (!file.read(magic.data(), magic.size()) || std::memcmp(magic.data(), "RSFCAMF1", magic.size()) != 0 ||
        !file.read(view.data(), view.size()) ||
        !file.read(reinterpret_cast<char*>(&input), sizeof(input)) ||
        !file.read(reinterpret_cast<char*>(&jitter_matches), sizeof(jitter_matches)) ||
        !file.read(reinterpret_cast<char*>(&fov), sizeof(fov))) return 1;
    rsf_rdr2_camera_result output{};
    return rsf_rdr2_decode_render_camera(view.data(), view.size(), input, output) &&
        output.camera.struct_size == sizeof(rsf_camera_frame) &&
        output.projection_jitter_matches == (jitter_matches != 0) &&
        std::abs(output.camera.vertical_fov_radians - fov) < 1e-5f ? 0 : 1;
}
