#pragma once
#include <windows.h>

// Library-local loader UI policy. Never alter the host process's error mode and
// restore the caller thread before running any adapter code. Preserve the actual
// LoadLibraryEx error across restoration so diagnostics do not report a stale code.
inline HMODULE thin_load_library(const wchar_t* path, DWORD flags) noexcept
{
    DWORD previous = 0;
    constexpr DWORD quiet = SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX;
    if (!SetThreadErrorMode(GetThreadErrorMode() | quiet, &previous)) return nullptr;
    HMODULE module = LoadLibraryExW(path, nullptr, flags);
    const DWORD error = GetLastError();
    if (!SetThreadErrorMode(previous, nullptr)) {
        // Refuse activation if restoration fails. Never release a successfully
        // loaded DLL here, since its DllMain may already have established state.
        return nullptr;
    }
    SetLastError(error);
    return module;
}
