// SPDX-License-Identifier: MIT
#include <rescaleframe/sha256_file.h>
#include <windows.h>
#include <cstdint>
#include <cstring>
#include <cwchar>
#include <string>

extern "C" bool rsf_wingman_matches_executable(const wchar_t*) noexcept;
extern "C" uint32_t rsf_wingman_carrier_start(HMODULE, bool (*)(const wchar_t*, char*)) noexcept;

namespace {
bool fingerprint(const wchar_t* executable, char* digest) noexcept try
{
    std::string value;
    if (!rsf::sha256_file(executable, value) || value.size() != 64) return false;
    std::memcpy(digest, value.c_str(), 65);
    return true;
}
catch (...) { return false; }

DWORD WINAPI startup(void* module) noexcept
{
    return rsf_wingman_carrier_start(static_cast<HMODULE>(module), fingerprint);
}
}

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, void*)
{
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(instance);
        static wchar_t executable[32768]{};
        const auto length = GetModuleFileNameW(nullptr, executable, 32768);
        if (length && length < 32768) {
            const auto* leaf = std::wcsrchr(executable, L'\\');
            if (rsf_wingman_matches_executable(leaf ? leaf + 1 : executable)) {
                if (auto worker = CreateThread(nullptr, 0, startup, instance, 0, nullptr)) CloseHandle(worker);
            }
        }
    }
    return TRUE;
}
