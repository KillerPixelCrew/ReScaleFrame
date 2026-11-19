// SPDX-License-Identifier: MIT
#include "../runtime/orchestrator/src/render_settings_store.h"
#include "../runtime/backends/common/sharpening.h"
#include <windows.h>
#include <cwchar>
#include <iterator>
#include <string>
#include <rescaleframe/overlay.h>
#include <rescaleframe/d3d11_sr_api.h>
#include <cstddef>
#include <cstdio>
#include <cmath>
#include <limits>
#include "../runtime/orchestrator/src/sharpening_overlay.h"
#include <cstring>
#ifdef RSF_SHARPENING_FFX_HEADER
#include <ffx_upscale.h>
#else
struct ffxDispatchDescUpscale { bool enableSharpening; float sharpness; };
#endif
static_assert(RSF_RENDER_FRONTEND_ABI_VERSION == 3);
static_assert(sizeof(rsf_render_settings) == 96);
static_assert(offsetof(rsf_render_settings, sharpening_mode) == 48);
static_assert(sizeof(rsf_sharpening_options) == 56);
static_assert(RSF_OVERLAY_ABI_VERSION == 18);
static_assert(sizeof(rsf_overlay_stats) == 416);
static_assert(offsetof(rsf_overlay_stats, ui_preferences) == 340);
static_assert(offsetof(rsf_overlay_stats, latency_paced_frames) == 352);
static_assert(sizeof(rsf_overlay_intent) == 200);
static_assert(offsetof(rsf_overlay_intent, close_requested) == 188);
static_assert(offsetof(rsf_overlay_intent, ui_preferences) == 196);
static_assert(offsetof(rsf_overlay_stats, host_options) == 280);
static_assert(offsetof(rsf_overlay_stats, sharpening_available) == 288);
static_assert(offsetof(rsf_overlay_intent, sharpening_changed) == 136);
static_assert(offsetof(rsf_overlay_stats, sharpening_mode) == 292);
static_assert(offsetof(rsf_overlay_intent, sharpening_mode) == 140);
static_assert(offsetof(rsf_render_settings, sharpening_mode) == 48);
static_assert(offsetof(rsf_sharpening_options, mode) == 8);
static_assert(offsetof(rsf_overlay_stats, sharpening_sharpness) == 296);
static_assert(offsetof(rsf_overlay_intent, sharpening_sharpness) == 144);
static_assert(offsetof(rsf_render_settings, sharpening_sharpness) == 52);
static_assert(offsetof(rsf_sharpening_options, sharpness) == 12);
static_assert(offsetof(rsf_overlay_stats, sharpening_mas) == 300);
static_assert(offsetof(rsf_overlay_intent, sharpening_mas) == 148);
static_assert(offsetof(rsf_render_settings, sharpening_mas) == 56);
static_assert(offsetof(rsf_sharpening_options, mas) == 16);
static_assert(offsetof(rsf_overlay_stats, sharpening_motion_sharpness) == 304);
static_assert(offsetof(rsf_overlay_intent, sharpening_motion_sharpness) == 152);
static_assert(offsetof(rsf_render_settings, sharpening_motion_sharpness) == 60);
static_assert(offsetof(rsf_sharpening_options, motion_sharpness) == 20);
static_assert(offsetof(rsf_overlay_stats, sharpening_motion_threshold) == 308);
static_assert(offsetof(rsf_overlay_intent, sharpening_motion_threshold) == 156);
static_assert(offsetof(rsf_render_settings, sharpening_motion_threshold) == 64);
static_assert(offsetof(rsf_sharpening_options, motion_threshold) == 24);
static_assert(offsetof(rsf_overlay_stats, sharpening_motion_scale_limit) == 312);
static_assert(offsetof(rsf_overlay_intent, sharpening_motion_scale_limit) == 160);
static_assert(offsetof(rsf_render_settings, sharpening_motion_scale_limit) == 68);
static_assert(offsetof(rsf_sharpening_options, motion_scale_limit) == 28);
static_assert(offsetof(rsf_overlay_stats, sharpening_contrast_enabled) == 316);
static_assert(offsetof(rsf_overlay_intent, sharpening_contrast_enabled) == 164);
static_assert(offsetof(rsf_render_settings, sharpening_contrast_enabled) == 72);
static_assert(offsetof(rsf_sharpening_options, contrast_enabled) == 32);
static_assert(offsetof(rsf_overlay_stats, sharpening_contrast) == 320);
static_assert(offsetof(rsf_overlay_intent, sharpening_contrast) == 168);
static_assert(offsetof(rsf_render_settings, sharpening_contrast) == 76);
static_assert(offsetof(rsf_sharpening_options, contrast) == 36);
static_assert(offsetof(rsf_overlay_stats, sharpening_depth_scale) == 324);
static_assert(offsetof(rsf_overlay_intent, sharpening_depth_scale) == 172);
static_assert(offsetof(rsf_render_settings, sharpening_depth_scale) == 80);
static_assert(offsetof(rsf_sharpening_options, depth_scale) == 40);
static_assert(offsetof(rsf_overlay_stats, sharpening_depth_bias) == 328);
static_assert(offsetof(rsf_overlay_intent, sharpening_depth_bias) == 176);
static_assert(offsetof(rsf_render_settings, sharpening_depth_bias) == 84);
static_assert(offsetof(rsf_sharpening_options, depth_bias) == 44);
static_assert(offsetof(rsf_overlay_stats, sharpening_clamp_output) == 332);
static_assert(offsetof(rsf_overlay_intent, sharpening_clamp_output) == 180);
static_assert(offsetof(rsf_render_settings, sharpening_clamp_output) == 88);
static_assert(offsetof(rsf_sharpening_options, clamp_output) == 48);
static_assert(offsetof(rsf_overlay_stats, sharpening_debug) == 336);
static_assert(offsetof(rsf_overlay_intent, sharpening_debug) == 184);
static_assert(offsetof(rsf_render_settings, sharpening_debug) == 92);
static_assert(offsetof(rsf_sharpening_options, debug) == 52);
static_assert(offsetof(rsf_d3d11_sr_api, request_dump) == 96);
static_assert(offsetof(rsf_d3d11_sr_api, set_sharpening) == 104);
namespace { int failures=0;
void check(bool result,const char* label) { if (!result) { ++failures; std::printf("FAIL: %s\n",label); } }
std::wstring raw(const std::wstring& ini,const wchar_t* section,const wchar_t* key) {
    wchar_t text[128]{}; GetPrivateProfileStringW(section,key,L"<missing>",text,128,ini.c_str()); return text;
}
// The store's file format: every owned key name and the CRT's %.9g text for decimals, which
// earlier releases wrote and their users' INI files contain.
void store_format(const std::wstring& ini) {
    const wchar_t* section=L"Format";
    WritePrivateProfileStringW(section,L"OverlayLayout",L"9",ini.c_str());
    WritePrivateProfileStringW(section,L"sharpness",L"0.1",ini.c_str());
    rsf_render_settings s{sizeof(s),RSF_RENDER_FRONTEND_ABI_VERSION,1,3,2,1,4,3,2,16667,1,4};
    rsf::render_settings::initialize_sharpening(s);
    s.sharpening_mode=2; s.sharpening_mas=1; s.sharpening_contrast_enabled=1; s.sharpening_clamp_output=1; s.sharpening_debug=1;
    check(rsf::render_settings::write(ini,section,s)==RSF_OK,"format write");
    const wchar_t* expected[][2]={{L"SR",L"1"},{L"SRBackend",L"3"},{L"DLSSPreset",L"2"},{L"FG",L"1"},{L"FGBackend",L"4"},
        {L"FGGeneratedFrames",L"3"},{L"Latency",L"2"},{L"FrameLimitUs",L"16667"},{L"StatusOverlay",L"1"},{L"SRQuality",L"4"},
        {L"SharpeningMode",L"2"},{L"Sharpness",L"0.400000006"},{L"MAS",L"1"},{L"MotionSharpness",L"0.200000003"},
        {L"MotionThreshold",L"0"},{L"MotionScaleLimit",L"10"},{L"ContrastEnabled",L"1"},{L"Contrast",L"0"},
        {L"DADepthScale",L"35"},{L"DADepthBias",L"0.00100000005"},{L"DAClampOutput",L"1"},{L"SharpeningDebug",L"1"}};
    for (const auto& entry:expected) check(raw(ini,section,entry[0])==entry[1],"owned key name and text");
    check(raw(ini,section,L"OverlayLayout")==L"9" && rsf::render_settings::read_overlay_layout(ini,section)==9,"layout survives a settings write");
    wchar_t lines[4096]{}; const auto length=GetPrivateProfileSectionW(section,lines,4096,ini.c_str());
    int sharpness_lines=0;
    for (const wchar_t* line=lines; line<lines+length && *line; line+=std::wcslen(line)+1)
        if (!_wcsnicmp(line,L"Sharpness=",10)) ++sharpness_lines;
    check(sharpness_lines==1,"differently cased owned key replaced");
    // Every decimal is written as the CRT formats it and reads back to the same bits.
    uint32_t state=0x1234567u;
    for (int i=0;i<1600;++i) {
        float value;
        if (i<512) value=1.0f+static_cast<float>(i)/512.0f;           // exact ties at the tenth digit
        else if (i<520) { const float edge[]={0.0f,-0.0f,1e-5f,1e-4f,123456792.0f,1234567936.0f,3.4028235e38f,1.17549435e-38f}; value=edge[i-512]; }
        else { state=state*1664525u+1013904223u; uint32_t bits=state; if (((bits>>23)&0xff)==0xff) bits^=1u<<23; std::memcpy(&value,&bits,4); }
        s.sharpening_contrast=value;
        if (rsf::render_settings::write(ini,section,s)!=RSF_OK) { check(false,"decimal write"); break; }
        wchar_t crt[64]{}; std::swprintf(crt,std::size(crt),L"%.9g",static_cast<double>(value));
        const auto text=raw(ini,section,L"Contrast");
        const auto back=rsf::render_settings::read(ini,section,s).sharpening_contrast;
        if (text!=crt || std::memcmp(&back,&value,4)) {
            check(false,"decimal text matches CRT %.9g and round-trips");
            std::printf("  value %a: store '%ls' CRT '%ls'\n",static_cast<double>(value),text.c_str(),crt);
            break;
        }
    }
    for (float special:{std::numeric_limits<float>::infinity(),-std::numeric_limits<float>::infinity(),
             std::numeric_limits<float>::quiet_NaN(),-std::numeric_limits<float>::quiet_NaN()}) {
        s.sharpening_contrast=special;
        check(rsf::render_settings::write(ini,section,s)==RSF_OK,"special write");
        wchar_t crt[64]{}; std::swprintf(crt,std::size(crt),L"%.9g",static_cast<double>(special));
        if (raw(ini,section,L"Contrast")!=crt) { check(false,"special decimal text matches CRT"); std::printf("  CRT '%ls' store '%ls'\n",crt,raw(ini,section,L"Contrast").c_str()); }
    }
    // Missing post-pass keys take the C defaults, whatever the host aggregate holds.
    rsf_render_settings zero{sizeof(zero),RSF_RENDER_FRONTEND_ABI_VERSION};
    const auto unset=rsf::render_settings::sharpening(rsf::render_settings::read(ini,L"Empty",zero));
    const auto c_defaults=rsf_sharpening_defaults();
    check(!std::memcmp(&unset,&c_defaults,sizeof(unset)),"missing post-pass keys read as rsf_sharpening_defaults");
    // A section beyond the store's buffer is refused rather than truncated.
    const std::wstring large(40000,L'x');
    WritePrivateProfileStringW(L"Large",L"Padding",large.c_str(),ini.c_str());
    check(rsf::render_settings::write(ini,L"Large",s)==RSF_ERROR_INVALID_ARGUMENT && raw(ini,L"Large",L"SR")==L"<missing>","oversized section refused unchanged");
    auto bad=rsf_sharpening_defaults(); bad.depth_scale=601;
    check(rsf::render_settings::write_sharpening(ini,section,bad)==RSF_ERROR_INVALID_ARGUMENT && raw(ini,section,L"DADepthScale")==L"35","invalid sharpening write refused unchanged");
    auto short_options=rsf_sharpening_defaults(); short_options.struct_size-=4;
    check(rsf_settings_store_write_sharpening(ini.c_str(),section,&short_options)==RSF_ERROR_INVALID_ARGUMENT,"store refuses short sharpening options");
    check(rsf_settings_store_write(ini.c_str(),nullptr,&s)==RSF_ERROR_INVALID_ARGUMENT && rsf_settings_store_write(ini.c_str(),section,nullptr)==RSF_ERROR_INVALID_ARGUMENT,"store refuses missing arguments");
}
}
int main() {
    auto defaults=rsf_sharpening_defaults();
    rsf_d3d11_sr_api table{sizeof(table),RSF_D3D11_SR_API_VERSION};
    check(rsf_get_d3d11_sr_api(&table)==RSF_OK && table.set_sharpening,"appended D3D11 setter is populated");
    auto short_options=defaults; short_options.struct_size=sizeof(short_options)-1;
    check(table.set_sharpening(&short_options)==RSF_DLSS_PIPELINE_ERROR_INVALID_ARGUMENT,"short sharpening options refused");
    short_options=defaults; ++short_options.abi_version;
    check(table.set_sharpening(&short_options)==RSF_DLSS_PIPELINE_ERROR_ABI_MISMATCH,"sharpening ABI mismatch refused");
    check(table.set_sharpening(&defaults)==RSF_DLSS_PIPELINE_OK,"sharpening setter accepts before graphics startup");
    check(rsf::sharpening_settings::valid(defaults),"defaults valid");
    wchar_t temp[MAX_PATH]{}, path[MAX_PATH]{}; GetTempPathW(MAX_PATH,temp);
    if (!GetTempFileNameW(temp,L"rsf",0,path)) return 1;
    const std::wstring ini=path, section=L"Sharpening";
    rsf_render_settings settings{sizeof(settings),RSF_RENDER_FRONTEND_ABI_VERSION};
    rsf::render_settings::initialize_sharpening(settings);
    WritePrivateProfileStringW(section.c_str(),L"Foreign",L"kept",path);
    for (uint32_t mode=0;mode<=4;++mode) {
        auto options=defaults; options.mode=mode; options.sharpness=0.73f; options.mas=1;
        options.motion_sharpness=-0.17f; options.motion_threshold=2; options.motion_scale_limit=17;
        options.contrast_enabled=1; options.contrast=1.3f; options.depth_scale=105;
        options.depth_bias=0.0037f; options.clamp_output=1; options.debug=1;
        rsf::render_settings::sharpening(settings,options);
        check(rsf::render_settings::write(ini,section,settings)==RSF_OK,"settings write");
        auto read=rsf::render_settings::read(ini,section,settings);
        check(read.sharpening_mode==options.mode,"round trip mode");
        check(std::fabs(read.sharpening_sharpness-options.sharpness)<1e-6f,"round trip sharpness");
        check(read.sharpening_mas==options.mas,"round trip mas");
        check(std::fabs(read.sharpening_motion_sharpness-options.motion_sharpness)<1e-6f,"round trip motion_sharpness");
        check(std::fabs(read.sharpening_motion_threshold-options.motion_threshold)<1e-6f,"round trip motion_threshold");
        check(std::fabs(read.sharpening_motion_scale_limit-options.motion_scale_limit)<1e-6f,"round trip motion_scale_limit");
        check(read.sharpening_contrast_enabled==options.contrast_enabled,"round trip contrast_enabled");
        check(std::fabs(read.sharpening_contrast-options.contrast)<1e-6f,"round trip contrast");
        check(std::fabs(read.sharpening_depth_scale-options.depth_scale)<1e-6f,"round trip depth_scale");
        check(std::fabs(read.sharpening_depth_bias-options.depth_bias)<1e-6f,"round trip depth_bias");
        check(read.sharpening_clamp_output==options.clamp_output,"round trip clamp_output");
        check(read.sharpening_debug==options.debug,"round trip debug");
        for (uint32_t backend=0;backend<=6;++backend) {
            ffxDispatchDescUpscale dispatch{};
            dispatch.enableSharpening=true; dispatch.sharpness=1;
            rsf::ffx_sharpening(dispatch,rsf::sharpening_settings::vendor_sharpness(options,backend));
            const bool native=mode==4 && backend>=2 && backend<=4;
            check(dispatch.enableSharpening==native && dispatch.sharpness==(native?0.73f:0),"FFX descriptor fields per mode/backend");
        }
    }
    auto bad=defaults; bad.mode=99; bad.sharpness=2; bad.mas=7; bad.motion_sharpness=-5;
    bad.motion_threshold=std::numeric_limits<float>::quiet_NaN(); bad.motion_scale_limit=-1;
    bad.contrast_enabled=4; bad.contrast=8; bad.depth_scale=900; bad.depth_bias=-1; bad.clamp_output=2; bad.debug=3;
    check(!rsf::sharpening_settings::valid(bad),"invalid options refused");
    const auto clean=rsf::sharpening_settings::sanitize(bad);
    check(rsf::sharpening_settings::valid(clean) && clean.mode==0 && clean.sharpness==1 && !clean.mas &&
        clean.motion_sharpness==-1 && clean.motion_threshold==0 && clean.motion_scale_limit==0.01f &&
        !clean.contrast_enabled && clean.contrast==2 && clean.depth_scale==600 && clean.depth_bias==0 &&
        !clean.clamp_output && !clean.debug,"sanitize every option");
    wchar_t foreign[16]{}; GetPrivateProfileStringW(section.c_str(),L"Foreign",L"",foreign,16,path);
    check(std::wstring(foreign)==L"kept","foreign setting preserved");
    WritePrivateProfileStringW(section.c_str(),L"Sharpness",L"nan",path);
    WritePrivateProfileStringW(section.c_str(),L"MotionScaleLimit",L"-8",path);
    auto invalid=rsf::render_settings::read(ini,section,settings);
    auto normalized=rsf::sharpening_settings::sanitize(rsf::render_settings::sharpening(invalid));
    check(normalized.sharpness==0.4f && normalized.motion_scale_limit==0.01f,"INI invalid values sanitized");
    // Callback hosts save only sharpening, preserving engine-owned base keys.
    WritePrivateProfileStringW(L"Rendering",L"EngineQuality",L"untouched",path);
    for (uint32_t mode=0;mode<=4;++mode) {
        auto o=defaults; o.mode=mode; o.sharpness=0.64f; o.motion_sharpness=-0.1f;
        o.depth_bias=0.0000003f; o.mas=o.debug=1;
        check(rsf::render_settings::write_sharpening(ini,L"Rendering",o)==RSF_OK,"callback host writes shared keys");
        auto read=rsf::render_settings::read_sharpening(ini,L"Rendering");
        check(!std::memcmp(&o,&read,sizeof(o)),"callback host round trip with float precision");
        rsf_overlay_stats stats{}; rsf::sharpening_overlay::fill(stats,o);
        rsf_overlay_intent intent{};
        intent.sharpening_mode=stats.sharpening_mode;
        intent.sharpening_sharpness=stats.sharpening_sharpness;
        intent.sharpening_mas=stats.sharpening_mas;
        intent.sharpening_motion_sharpness=stats.sharpening_motion_sharpness;
        intent.sharpening_motion_threshold=stats.sharpening_motion_threshold;
        intent.sharpening_motion_scale_limit=stats.sharpening_motion_scale_limit;
        intent.sharpening_contrast_enabled=stats.sharpening_contrast_enabled;
        intent.sharpening_contrast=stats.sharpening_contrast;
        intent.sharpening_depth_scale=stats.sharpening_depth_scale;
        intent.sharpening_depth_bias=stats.sharpening_depth_bias;
        intent.sharpening_clamp_output=stats.sharpening_clamp_output;
        intent.sharpening_debug=stats.sharpening_debug;
        const auto mapped=rsf::sharpening_overlay::requested(intent);
        check(stats.sharpening_available && !std::memcmp(&o,&mapped,sizeof(o)),"D3D12 host stats/intent mapping");
    }
    wchar_t engine[32]{}; GetPrivateProfileStringW(L"Rendering",L"EngineQuality",L"",engine,32,path);
    check(std::wstring(engine)==L"untouched","callback persistence preserves engine settings");
    store_format(ini);
    DeleteFileW(path);
    std::printf("sharpening_settings: %d failures\n",failures);
    return failures?1:0;
}
