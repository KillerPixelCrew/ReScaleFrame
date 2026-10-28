// SPDX-License-Identifier: GPL-3.0-only
#include <windows.h>
#include <string>
#include <cstdio>

int main()
{
    wchar_t executable[32768]{};
    const auto length = GetModuleFileNameW(nullptr, executable, 32768);
    if (!length || length >= 32768) return 1;
    std::wstring root(executable, length); root.resize(root.find_last_of(L"/\\") + 1);
    const HMODULE runtime = LoadLibraryExW((root + L"ReScaleFrame.Runtime.dll").c_str(), nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!runtime) return 1;
    for (const auto* name : {"rsf_render_host_create", "rsf_callback_host_start", "rsf_render_settings_get"})
        if (!GetProcAddress(runtime, name)) return 1;
    for (const auto* name : {"rsf_unity_sr_start", "rsf_unity_fg_start", "rsf_unity_sr_stop",
            "rsf_rdr2_sr_host_create", "rsf_rdr2_settings_get", "rsf_rage_sr_host_create"})
        if (GetProcAddress(runtime, name)) return 1;
    const HMODULE adapter = LoadLibraryExW((root + L"ReScaleFrame.Game.UnityMono.dll").c_str(), nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!adapter) return 1;
    for (const auto* name : {"rsf_unity_start", "rsf_unity_sr_start", "rsf_unity_fg_start", "rsf_unity_sr_stop"}) {
        const auto entry = GetProcAddress(adapter, name);
        HMODULE owner{};
        if (!entry || !GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                reinterpret_cast<const wchar_t*>(entry), &owner) || owner != adapter) return 1;
    }
    FreeLibrary(adapter); FreeLibrary(runtime);
    std::puts("Generic runtime exports and integration-owned engine entry points verified.");
    return 0;
}
