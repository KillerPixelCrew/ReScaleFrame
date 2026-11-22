// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/input_dump.h>
#include <dxgiformat.h>
#include <windows.h>
#include <array>
#include <filesystem>
#include <fstream>
#include <sstream>
#include <string>
#include <cstdio>
namespace fs=std::filesystem;
std::string contents(const fs::path& path) {
    std::ifstream stream(path, std::ios::binary); std::ostringstream text; text<<stream.rdbuf(); return text.str();
}
void count_messages(void* user, const char*) {
    // The encoder and capture status messages are synchronous, even if the sink
    // queries capture state before logging a progress line.
    (void)rsf_input_dump_requested();
    ++*static_cast<unsigned*>(user);
}
int main(int argc,char** argv) {
    if (argc!=2) return 1;
    const auto root=fs::path(argv[1])/("capture-metadata-"+std::to_string(GetCurrentProcessId())+"-"+std::to_string(GetTickCount64()));
    fs::create_directories(root);
    const auto utf8=root.u8string();
    unsigned messages = 0;
    rsf_input_dump_configure(reinterpret_cast<const char*>(utf8.c_str()),"contract\n\"test\"","test",count_messages,&messages);
    rsf_input_dump_request();
    rsf_frame_record frame{}; frame.struct_size=sizeof(frame); frame.abi_version=RSF_GAME_FRAME_ABI_VERSION;
    frame.session_id=7;frame.frame_id=11;frame.view_id=13;
    frame.resource_generation=2;
    frame.camera.struct_size=sizeof(frame.camera);
    frame.camera.abi_version=RSF_GAME_CAMERA_ABI_VERSION+1;
    frame.render_width=frame.render_height=32;frame.output_width=frame.output_height=64;
    if (!rsf_input_dump_begin_record(&frame,1)) return 2;
    rsf_input_dump_result(&frame,"sr",1,-3);
    rsf_input_dump_result(&frame,"sequence_capture",1,0);
    rsf_input_dump_result(&frame,"automatic_sr_failure",1,-3);
    rsf_input_dump_result(&frame,"spatial_recovery",0,0);
    rsf_input_dump_result(&frame,"../not_allowed",0,0);
    rsf_game_render_pass pass{}; pass.struct_size=sizeof(pass); pass.flags=RSF_GAME_RENDER_PRIMARY;
    pass.session_id=7; pass.source_frame_id=11; pass.view_key=13; pass.native_frame=17;
    auto wrong_pass=pass; ++wrong_pass.view_key;
    rsf_input_dump_pass_metadata(&wrong_pass);
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
    if (contents(bundle/"frame.json").find("\"camera_available\":false")==std::string::npos) return 26;
    const auto manifest=contents(bundle/"manifest.json");
    if (manifest.find("\"sr_result\":-3")==std::string::npos ||
        manifest.find("\"game\":\"contract\\u000a\\\"test\\\"\"")==std::string::npos ||
        fs::exists(bundle.parent_path()/"not_allowed_status.json")) return 5;
    const auto fg=contents(bundle/"fg_constants.json");
    if (fg.find("\"resource_generation\":99")==std::string::npos ||
        fg.find("\"prepare_result\":0")==std::string::npos) return 6;
    if (contents(bundle/"engine_pass.json").find("\"view_key\":13")==std::string::npos) return 7;

    rsf_input_dump_request();
    ++frame.frame_id;
    if (!rsf_input_dump_begin_record(&frame,2)) return 8;
    auto different=frame; ++different.resource_generation;
    if (rsf_input_dump_wants(&different) || rsf_input_dump_begin_record(&different,2) ||
        rsf_input_dump_reserve(&different,"other_generation","wrong",RSF_DUMP_VIEW_RAW,1)) return 9;
    auto ambiguous=frame; ambiguous.flags=RSF_FRAME_FLAG_AMBIGUOUS_ID|RSF_FRAME_FLAG_NO_FG;
    if (rsf_input_dump_wants(&ambiguous) || rsf_input_dump_begin_record(&ambiguous,2)) return 24;
    const auto ticket=rsf_input_dump_reserve(&frame,"final_color","padded CPU rows",RSF_DUMP_VIEW_RAW,1);
    if (!ticket || rsf_input_dump_reserve(&frame,"final_color","duplicate",RSF_DUMP_VIEW_RAW,1)) return 10;
    if (ticket!=rsf_input_dump_reserve(&frame,"later","delayed input",RSF_DUMP_VIEW_RAW,1)) return 12;
    if (ticket!=rsf_input_dump_reserve(&frame,"blocked_raw","occupied raw path",RSF_DUMP_VIEW_RAW,1) ||
        ticket!=rsf_input_dump_reserve(&frame,"blocked_metadata","occupied metadata path",RSF_DUMP_VIEW_RAW,1)) return 25;
    rsf_input_dump_complete(&different,0);
    if (!rsf_input_dump_requested()) return 13;
    rsf_input_dump_complete(&frame,0);
    if (rsf_input_dump_reserve(&frame,"after_present","too late",RSF_DUMP_VIEW_RAW,1)) return 14;
    fs::path later_bundle;
    for (const auto& entry:fs::directory_iterator(root/"website"))
        if (entry.path()!=bundle) later_bundle=entry.path();
    if (later_bundle.empty() ||
        contents(later_bundle/"manifest.json").find("\"status\":\"waiting_for_gpu\"")==std::string::npos) return 15;

    const std::array<uint8_t,12> source{1,2,3,4,0,0,0,0,5,6,7,8};
    const rsf_dump_pixels pixels{sizeof(pixels),1,2,DXGI_FORMAT_R8G8B8A8_UNORM,8,source.size(),source.data()};
    fs::create_directory(later_bundle/"blocked_raw.bin");
    if (rsf_input_dump_pixels(ticket,"blocked_raw",&pixels,3,37) ||
        fs::exists(later_bundle/"blocked_raw_raw.json") ||
        !fs::is_directory(later_bundle/"blocked_raw.bin") ||
        !fs::exists(later_bundle/"blocked_raw.png")) return 26;
    fs::create_directory(later_bundle/"blocked_metadata_raw.json");
    if (rsf_input_dump_pixels(ticket,"blocked_metadata",&pixels,3,37) ||
        fs::exists(later_bundle/"blocked_metadata.bin") ||
        !fs::is_directory(later_bundle/"blocked_metadata_raw.json") ||
        !fs::exists(later_bundle/"blocked_metadata.png")) return 27;
    if (!rsf_input_dump_pixels(ticket,"final_color",&pixels,3,37) ||
        rsf_input_dump_pixels(ticket,"final_color",&pixels,3,37) || !rsf_input_dump_requested()) return 16;
    rsf_input_dump_failed(ticket,"later",RSF_TEXTURE_ERROR_STAGING_FAILED);
    const auto preview=contents(later_bundle/"final_color.png");
    if (rsf_input_dump_requested() ||
        contents(later_bundle/"manifest.json").find("\"status\":\"partial\"")==std::string::npos ||
        contents(later_bundle/"final_color.bin")!=std::string("\x01\x02\x03\x04\x05\x06\x07\x08",8) ||
        contents(later_bundle/"final_color_raw.json")!="{\"width\":1,\"height\":2,\"format\":28,\"native_api\":3,"
            "\"native_format\":37,\"row_pitch\":4,\"source_row_pitch\":8,\"bytes\":8}\n" ||
        preview.size()<8 || preview.substr(1,3)!="PNG") return 17;

    rsf_input_dump_request();
    ++frame.frame_id;
    if (!rsf_input_dump_begin_record(&frame,2)) return 18;
    const auto later_ticket=rsf_input_dump_reserve(&frame,"final_color","new request",RSF_DUMP_VIEW_RAW,1);
    if (!later_ticket || later_ticket==ticket || rsf_input_dump_pixels(ticket,"final_color",&pixels,3,37)) return 18;
    rsf_input_dump_cancel(frame.session_id+1);
    if (!rsf_input_dump_requested()) return 19;
    rsf_input_dump_cancel(frame.session_id);
    if (rsf_input_dump_requested() || messages==0) return 20;

    rsf_input_dump_request();
    pass.role=RSF_GAME_RENDER_SR;
    rsf_input_dump_pass(nullptr,&pass,1);
    if (!rsf_input_dump_active()) return 21;
    auto window=pass;
    window.role=RSF_GAME_RENDER_WINDOW;
    window.scope_id=45; window.window_key=46; window.viewport_key=pass.viewport_key; window.rhi_viewport_key=47;
    rsf_input_dump_present(nullptr,nullptr,&window);
    auto other_window=window; ++other_window.rhi_viewport_key;
    rsf_input_dump_presented(&other_window,0);
    if (!rsf_input_dump_requested()) return 22;
    rsf_input_dump_presented(&window,0);
    if (rsf_input_dump_requested() || rsf_input_dump_active()) return 23;
    std::puts("PASS capture metadata, ticket retirement and CPU row encoding without a graphics device");
    return 0;
}
