// SPDX-License-Identifier: GPL-3.0-only
#include <windows.h>
#include <cstdio>
#include <vector>
int main(int argc, char** argv)
{
    if (argc != 2) return 2;
    HMODULE shim = LoadLibraryExA(argv[1], nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!shim) return 1;
    // Keep the carrier loaded for process lifetime because its startup worker can still run.
    auto direct_input = GetProcAddress(shim, "DirectInput8Create");
    if (!direct_input || direct_input != GetProcAddress(shim, MAKEINTRESOURCEA(1))) return 1;
    const char* names[] = {"GetFileVersionInfoA", "GetFileVersionInfoByHandle", "GetFileVersionInfoExA",
        "GetFileVersionInfoExW", "GetFileVersionInfoSizeA", "GetFileVersionInfoSizeExA", "GetFileVersionInfoSizeExW",
        "GetFileVersionInfoSizeW", "GetFileVersionInfoW", "VerFindFileA", "VerFindFileW", "VerInstallFileA",
        "VerInstallFileW", "VerLanguageNameA", "VerLanguageNameW", "VerQueryValueA", "VerQueryValueW"};
    for (const auto* name : names) if (!GetProcAddress(shim, name)) return 1;
    wchar_t directory[MAX_PATH]{};
    const auto length = GetSystemDirectoryW(directory, MAX_PATH);
    if (!length || length >= MAX_PATH - 14) return 1;
    wchar_t system_version[MAX_PATH]{}, system_kernel[MAX_PATH]{};
    swprintf_s(system_version, L"%s\\version.dll", directory);
    swprintf_s(system_kernel, L"%s\\kernel32.dll", directory);
    HMODULE original = LoadLibraryExW(system_version, nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!original) return 1;
    using Size = DWORD (WINAPI*)(LPCWSTR, LPDWORD);
    using Info = BOOL (WINAPI*)(LPCWSTR, DWORD, DWORD, LPVOID);
    using Query = BOOL (WINAPI*)(LPCVOID, LPCWSTR, LPVOID*, PUINT);
    auto size = reinterpret_cast<Size>(reinterpret_cast<void*>(GetProcAddress(shim, "GetFileVersionInfoSizeW")));
    auto original_size = reinterpret_cast<Size>(reinterpret_cast<void*>(GetProcAddress(original, "GetFileVersionInfoSizeW")));
    auto info = reinterpret_cast<Info>(reinterpret_cast<void*>(GetProcAddress(shim, "GetFileVersionInfoW")));
    auto query = reinterpret_cast<Query>(reinterpret_cast<void*>(GetProcAddress(shim, "VerQueryValueW")));
    DWORD ignored = 0, count = size(system_kernel, &ignored);
    if (!count || count != original_size(system_kernel, &ignored)) return 1;
    std::vector<unsigned char> data(count);
    if (!info(system_kernel, 0, count, data.data())) return 1;
    void* value = nullptr; UINT bytes = 0;
    if (!query(data.data(), L"\\", &value, &bytes) || !value || bytes < 52) return 1;
    std::puts("PASS: DirectInput ordinal 1, named version exports and real version-resource forwarding.");
    return 0;
}
