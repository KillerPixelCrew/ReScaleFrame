// SPDX-License-Identifier: MIT
#include <windows.h>
#include <string>
#include <cstdio>
#include <cstring>

// Optional overlay support must not require this export from a game's D3D11
// wrapper merely to load the runtime. Inspect the already loaded PE, without GPU use.
bool imports_on12(HMODULE module)
{
    const auto* base = reinterpret_cast<const unsigned char*>(module);
    const auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
    const auto* nt = reinterpret_cast<const IMAGE_NT_HEADERS*>(base + dos->e_lfanew);
    const auto rva = nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT].VirtualAddress;
    if (!rva) return false;
    const auto* descriptor = reinterpret_cast<const IMAGE_IMPORT_DESCRIPTOR*>(base + rva);
    for (; descriptor->Name; ++descriptor) {
        if (!descriptor->OriginalFirstThunk) continue;
        const auto* thunk = reinterpret_cast<const IMAGE_THUNK_DATA*>(base + descriptor->OriginalFirstThunk);
        for (; thunk->u1.AddressOfData; ++thunk) {
            if (IMAGE_SNAP_BY_ORDINAL(thunk->u1.Ordinal)) continue;
            const auto* name = reinterpret_cast<const IMAGE_IMPORT_BY_NAME*>(base + thunk->u1.AddressOfData);
            if (std::strcmp(reinterpret_cast<const char*>(name->Name), "D3D11On12CreateDevice") == 0) return true;
        }
    }
    return false;
}

int main()
{
    wchar_t executable[32768]{};
    const auto length = GetModuleFileNameW(nullptr, executable, 32768);
    if (!length || length >= 32768) return 1;
    std::wstring root(executable, length); root.resize(root.find_last_of(L"/\\") + 1);
    const HMODULE runtime = LoadLibraryExW((root + L"ReScaleFrame.Runtime.dll").c_str(), nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!runtime) return 1;
    if (imports_on12(runtime)) {
        std::fputs("Optional D3D11-on-12 must resolve lazily, not through the static import table.\n", stderr);
        FreeLibrary(runtime); return 1;
    }
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
