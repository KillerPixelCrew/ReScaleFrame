// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/render_frontend.h>
#include <rescaleframe/render_host.h>
#include <rescaleframe/game_api.h>
#include <windows.h>
#include <cstdio>
#include <string>

int main()
{
    rsf_render_status status{}; status.struct_size=sizeof(status); status.abi_version=RSF_RENDER_FRONTEND_ABI_VERSION;
    if(rsf_render_status_get(nullptr)!=RSF_ERROR_INVALID_ARGUMENT || rsf_render_status_get(&status)!=RSF_ERROR_NOT_READY)return 1;
    wchar_t temp[MAX_PATH]{}; if(!GetTempPathW(MAX_PATH,temp))return 1;
    const auto directory=std::wstring(temp)+L"rsf-rdr2-frontend-"+std::to_wstring(GetCurrentProcessId());
    if(!CreateDirectoryW(directory.c_str(),nullptr))return 1;
    const auto ini=directory+L"\\ReScaleFrame.ini";
    if(!WritePrivateProfileStringW(L"RDR2",L"ForceD3D12",L"1",ini.c_str()) ||
        !WritePrivateProfileStringW(L"RDR2",L"UnrelatedSetting",L"preserved",ini.c_str()))return 1;
    rsf_game_host_services host{};host.struct_size=sizeof(host);host.abi_version=RSF_GAME_ABI_VERSION;host.session_id=7;
    rsf_render_host_setup setup{};setup.struct_size=sizeof(setup);setup.abi_version=RSF_RENDER_HOST_ABI_VERSION;
    setup.configuration_path=ini.c_str();setup.runtime_directory=directory.c_str();setup.configuration_section=L"RDR2";
    setup.engine_version_utf8="Fixture";setup.units_to_meters=1;setup.depth_inverted=1;
    setup.sr_backend_choices=(1u<<1)|(1u<<3)|(1u<<4)|(1u<<5);
    if(rsf_render_host_create(&setup,&host)!=RSF_OK || !host.render_feedback)return 1;
    const rsf_game_latency_event native_mode{sizeof(native_mode),RSF_GAME_LATENCY_MODE,7,0,0,0};
    if(!host.process_latency || host.process_latency(host.user,&native_mode)!=RSF_OK)return 1;
    rsf_render_settings settings{};settings.struct_size=sizeof(settings);settings.abi_version=RSF_RENDER_FRONTEND_ABI_VERSION;
    if(rsf_render_settings_get(&settings)!=RSF_OK || settings.sr_backend!=1 || !settings.status_overlay)return 1;
    settings.sr_backend=3;settings.fg_enabled=1;settings.fg_backend=1;settings.generated_frames=3;settings.latency=2;settings.sr_quality=4;
    if(rsf_render_settings_apply(&settings)!=RSF_OK)return 1;
    rsf_render_settings saved{};saved.struct_size=sizeof(saved);saved.abi_version=RSF_RENDER_FRONTEND_ABI_VERSION;
    if(rsf_render_settings_get(&saved)!=RSF_OK || saved.sr_backend!=3 || saved.generated_frames!=3 || saved.latency!=2 || saved.sr_quality!=4)return 1;
    auto disabled=saved;disabled.fg_enabled=0;
    if(rsf_render_settings_apply(&disabled)!=RSF_OK || rsf_render_status_get(&status)!=RSF_OK ||
        status.fg_requested || status.fg_requested_backend || status.fg_effective_backend)return 1;
    rsf_render_settings remembered{sizeof(remembered),RSF_RENDER_FRONTEND_ABI_VERSION};
    if(rsf_render_settings_get(&remembered)!=RSF_OK || remembered.fg_backend!=saved.fg_backend || remembered.fg_enabled)return 1;
    if(rsf_render_settings_apply(&saved)!=RSF_OK)return 1;
    auto invalid_quality=saved;invalid_quality.sr_quality=5;
    if(rsf_render_settings_apply(&invalid_quality)!=RSF_ERROR_INVALID_ARGUMENT ||
        GetPrivateProfileIntW(L"RDR2",L"SRQuality",0,ini.c_str())!=4)return 1;
    wchar_t untouched[64]{};GetPrivateProfileStringW(L"RDR2",L"UnrelatedSetting",L"",untouched,64,ini.c_str());
    if(std::wstring(untouched)!=L"preserved" || GetPrivateProfileIntW(L"RDR2",L"ForceD3D12",0,ini.c_str())!=1)return 1;
    settings.generated_frames=0;
    if(rsf_render_settings_apply(&settings)!=RSF_ERROR_INVALID_ARGUMENT ||
        GetPrivateProfileIntW(L"RDR2",L"FGGeneratedFrames",0,ini.c_str())!=3)return 1;
    const rsf_game_render_feedback event{sizeof(event),RSF_GAME_RENDER_FEEDBACK_CAMERA_JITTER_INVALID,7,123};
    host.render_feedback(host.user,&event);
    if(rsf_render_status_get(&status)!=RSF_OK || status.native_frame!=123 ||
        std::string(status.reason)!="Invalid camera or frozen jitter" || status.output_fps_valid)return 1;
    status.abi_version++;
    if(rsf_render_status_get(&status)!=RSF_ERROR_ABI_MISMATCH)return 1;
    if(!DeleteFileW(ini.c_str()) || !RemoveDirectoryW(directory.c_str()))return 1;
    std::puts("RDR2 frontend: settings transaction, unrelated keys, refusal visibility, unknown output FPS and ABI guards passed.");
}
