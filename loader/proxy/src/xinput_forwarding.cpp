// SPDX-License-Identifier: MIT
#include <windows.h>
#include <cstdint>
#include <string>

namespace {
INIT_ONCE forwarding = INIT_ONCE_STATIC_INIT;
constexpr WORD ordinals[] = {1, 2, 3, 4, 5, 6, 7, 8, 100, 101, 102, 103};
FARPROC exports[sizeof(ordinals) / sizeof(*ordinals)]{};

BOOL CALLBACK initialize(PINIT_ONCE, void*, void**) noexcept try
{
    wchar_t directory[MAX_PATH]{};
    const auto length = GetSystemDirectoryW(directory, MAX_PATH);
    if (!length || length >= MAX_PATH) return FALSE;
    const auto path = std::wstring(directory, length) + L"\\xinput1_3.dll";
    const auto module = LoadLibraryExW(path.c_str(), nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module) return FALSE;
    for (size_t i = 0; i < sizeof(exports) / sizeof(*exports); ++i)
        exports[i] = GetProcAddress(module, MAKEINTRESOURCEA(ordinals[i]));
    return TRUE;
}
catch (...) { return FALSE; }
}

extern "C" FARPROC rsf_xinput_resolve(uint32_t index)
{
    if (index >= sizeof(exports) / sizeof(*exports) ||
        !InitOnceExecuteOnce(&forwarding, initialize, nullptr, nullptr) || !exports[index]) {
        SetLastError(ERROR_PROC_NOT_FOUND);
        RaiseException(EXCEPTION_ILLEGAL_INSTRUCTION, EXCEPTION_NONCONTINUABLE, 0, nullptr);
        return nullptr;
    }
    return exports[index];
}
