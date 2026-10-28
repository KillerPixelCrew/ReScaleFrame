// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/render_host.h>
#include <rescaleframe/frame_generation.h>
#include <windows.h>
#include <cwchar>

inline int rsf_rdr2_game_window(void*, void* pointer)
{
    DWORD process{}; wchar_t name[64]{};
    const auto window = static_cast<HWND>(pointer);
    return GetWindowThreadProcessId(window, &process) && process == GetCurrentProcessId() &&
        GetClassNameW(window, name, 64) && !std::wcscmp(name, L"sgaWindow");
}
inline rsf_render_host_setup rsf_rdr2_runtime_setup(const wchar_t* configuration, const wchar_t* directory)
{
    rsf_render_host_setup setup{};
    setup.struct_size = sizeof(setup); setup.abi_version = RSF_RENDER_HOST_ABI_VERSION;
    setup.configuration_path = configuration; setup.runtime_directory = directory;
    setup.configuration_section = L"RDR2";
    setup.presentation_owner = RSF_RENDER_HOST_OWNS_PRESENTATION;
    setup.sr_backend_choices = (1u << 1) | (1u << 3) | (1u << 4) | (1u << 5);
    setup.engine_version_utf8 = "RAGE 1.0.1491.50";
    setup.application_id = 0x60e8ab7;
    setup.project_id_utf8 = "6fd496e2-e0fd-48dc-9e99-0c37e87d30ca";
    setup.depth_inverted = 1; setup.units_to_meters = 1; setup.ui_mode = RSF_UI_MODE_BACKBUFFER_HUDLESS;
    setup.accept_window = rsf_rdr2_game_window;
    return setup;
}
