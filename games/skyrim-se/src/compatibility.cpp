// SPDX-License-Identifier: MIT
#include "postprocess_platform.h"
#include <windows.h>
#include <cstddef>
#if defined(_MSC_VER)
#include <intrin.h>
#endif

extern "C" {
struct rsf_skyrim_cs_hdr_scope {
    rsf_skyrim_cs_hdr_call call;
    bool primary, active;
};
uintptr_t rsf_skyrim_cs_original(uint32_t slot) noexcept;
uintptr_t rsf_skyrim_cs_resolved(uintptr_t module, const char* name, uintptr_t result) noexcept;
uintptr_t rsf_skyrim_cs_excluded_byte(uintptr_t feature) noexcept;
void rsf_skyrim_cs_hdr_enter(uintptr_t game, const rsf_skyrim_cs_hdr_call*, rsf_skyrim_cs_hdr_scope*) noexcept;
void rsf_skyrim_cs_hdr_leave(const rsf_skyrim_cs_hdr_scope*) noexcept;
}

static_assert(sizeof(wchar_t) == 2 && sizeof(rsf_skyrim_cs_hdr_scope) == 48);
static_assert(sizeof(IMAGE_DOS_HEADER) == 64 && offsetof(IMAGE_DOS_HEADER, e_lfanew) == 0x3c);
static_assert(sizeof(IMAGE_NT_HEADERS64) == 264 && sizeof(IMAGE_OPTIONAL_HEADER64) == 240);
static_assert(offsetof(IMAGE_NT_HEADERS64, OptionalHeader) == 24 &&
    offsetof(IMAGE_OPTIONAL_HEADER64, SizeOfImage) == 56);
static_assert(sizeof(IMAGE_SECTION_HEADER) == 40 && offsetof(IMAGE_SECTION_HEADER, Characteristics) == 36);

extern "C" FARPROC WINAPI rsf_skyrim_cs_resolve(HMODULE module, LPCSTR name)
{
    const auto original = reinterpret_cast<decltype(&GetProcAddress)>(rsf_skyrim_cs_original(0));
    if (!original) return nullptr;
    const auto found = original(module, name);
    const auto error = GetLastError();
    const auto resolved = rsf_skyrim_cs_resolved(uintptr_t(module), name, uintptr_t(found));
    SetLastError(error);
    return reinterpret_cast<FARPROC>(resolved);
}

extern "C" uint32_t rsf_skyrim_cs_call_initializer(uintptr_t original, const void* skse) noexcept try
{
    using Initialize = bool (*)(const void*);
    return original && reinterpret_cast<Initialize>(original)(skse) ? 1u : 0u;
} catch (...) { return 2u; }

extern "C" void rsf_skyrim_cs_configure(void* feature, void* json)
{
    using Configure = void (*)(void*, void*);
    // The original keeps parsing and validating desired settings. The guarded byte affects only lifecycle participation.
    const auto original = reinterpret_cast<Configure>(rsf_skyrim_cs_original(1));
    struct Exclude {
        unsigned char* loaded;
        ~Exclude() { if (loaded) *loaded = 0; }
    } exclusion{reinterpret_cast<unsigned char*>(rsf_skyrim_cs_excluded_byte(uintptr_t(feature)))};
    if (original) original(feature, json);
}

extern "C" void rsf_skyrim_cs_hdr_process(
    void* manager, uint32_t input, uint32_t output, uintptr_t argument4, uint8_t argument5)
{
    using HdrProcess = void (*)(void*, uint32_t, uint32_t, uintptr_t, uint8_t);
#if defined(_MSC_VER)
    const auto caller = uintptr_t(_ReturnAddress());
#else
    const auto caller = uintptr_t(__builtin_return_address(0));
#endif
    const auto original = reinterpret_cast<HdrProcess>(rsf_skyrim_cs_original(2));
    const rsf_skyrim_cs_hdr_call call{caller, uintptr_t(manager), argument4, input, output, argument5};
    struct Restore {
        rsf_skyrim_cs_hdr_scope saved{};
        ~Restore() { rsf_skyrim_cs_hdr_leave(&saved); }
    } restore;
    rsf_skyrim_cs_hdr_enter(uintptr_t(GetModuleHandleW(nullptr)), &call, &restore.saved);
    // Preserve CS's RedirectFramebuffer -> ProcessMain -> RestoreFramebuffer sequence.
    if (original) original(manager, input, output, argument4, argument5);
}
