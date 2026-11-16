// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/input_dump.h>
#include <windows.h>
#include <filesystem>
#include <fstream>
#include <sstream>
#include <string>
#include <cstdio>
namespace fs=std::filesystem;
std::string contents(const fs::path& path) {
    std::ifstream stream(path); std::ostringstream text; text<<stream.rdbuf(); return text.str();
}
int main(int argc,char** argv) {
    if (argc!=2) return 1;
    const auto root=fs::path(argv[1])/("capture-metadata-"+std::to_string(GetCurrentProcessId()));
    fs::create_directories(root);
    const auto utf8=root.u8string();
    rsf_input_dump_configure(reinterpret_cast<const char*>(utf8.c_str()),"contract","test",nullptr,nullptr);
    rsf_input_dump_request();
    rsf_frame_record frame{}; frame.struct_size=sizeof(frame); frame.abi_version=RSF_GAME_FRAME_ABI_VERSION;
    frame.session_id=7;frame.frame_id=11;frame.view_id=13;
    frame.render_width=frame.render_height=32;frame.output_width=frame.output_height=64;
    if (!rsf_input_dump_begin_record(&frame,1)) return 2;
    rsf_input_dump_result(&frame,"sr",1,-3);
    rsf_input_dump_result(&frame,"sequence_capture",1,0);
    rsf_input_dump_result(&frame,"automatic_sr_failure",1,-3);
    rsf_input_dump_result(&frame,"spatial_recovery",0,0);
    rsf_input_dump_result(&frame,"../not_allowed",0,0);
    rsf_game_render_pass pass{}; pass.struct_size=sizeof(pass); pass.flags=RSF_GAME_RENDER_PRIMARY;
    pass.session_id=7; pass.source_frame_id=11; pass.view_key=13; pass.native_frame=17;
    rsf_input_dump_pass_metadata(&pass);
    auto generation_record=frame; generation_record.resource_generation=99;
    rsf_fg_frame generation{}; generation.struct_size=sizeof(generation); generation.record=&generation_record;
    rsf_input_dump_fg_prepare(&pass,&generation,0);
    generation_record.view_id=14; generation_record.resource_generation=100;
    rsf_input_dump_fg_prepare(&pass,&generation,-1); // different view must not overwrite the valid join
    rsf_input_dump_complete(&frame,0);
    fs::path bundle;
    for (const auto& entry:fs::recursive_directory_iterator(root))
        if (entry.path().filename()=="manifest.json") bundle=entry.path().parent_path();
    if (bundle.empty()) return 3;
    for (const char* name:{"sr","sequence_capture","automatic_sr_failure","spatial_recovery"}) {
        const auto text=contents(bundle/(std::string(name)+"_status.json"));
        if (text.find("\"source_frame_id\":11")==std::string::npos) return 4;
    }
    const auto manifest=contents(bundle/"manifest.json");
    if (manifest.find("\"sr_result\":-3")==std::string::npos || fs::exists(bundle.parent_path()/"not_allowed_status.json")) return 5;
    const auto fg=contents(bundle/"fg_constants.json");
    if (fg.find("\"resource_generation\":99")==std::string::npos ||
        fg.find("\"prepare_result\":0")==std::string::npos) return 6;
    std::puts("PASS capture metadata and preserved temporal failure without a graphics device");
    return 0;
}
