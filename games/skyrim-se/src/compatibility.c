/* SPDX-License-Identifier: MIT */
#include "postprocess_platform.h"
#include <stdbool.h>
#include <stddef.h>
#include <windows.h>
#if defined(_MSC_VER)
#include <intrin.h>
#endif

typedef struct rsf_skyrim_cs_hdr_scope {
    rsf_skyrim_cs_hdr_call call;
    bool primary, active;
} rsf_skyrim_cs_hdr_scope;

uintptr_t rsf_skyrim_cs_original(uint32_t slot);
uintptr_t rsf_skyrim_cs_resolved(uintptr_t module, const char* name, uintptr_t result);
uintptr_t rsf_skyrim_cs_excluded_byte(uintptr_t feature);
void rsf_skyrim_cs_hdr_enter(uintptr_t game, const rsf_skyrim_cs_hdr_call*, rsf_skyrim_cs_hdr_scope*);
void rsf_skyrim_cs_hdr_leave(const rsf_skyrim_cs_hdr_scope*);

_Static_assert(sizeof(wchar_t) == 2 && sizeof(rsf_skyrim_cs_hdr_scope) == 48, "Skyrim HDR scope layout");
_Static_assert(sizeof(IMAGE_DOS_HEADER) == 64 && offsetof(IMAGE_DOS_HEADER, e_lfanew) == 0x3c, "DOS header");
_Static_assert(sizeof(IMAGE_NT_HEADERS64) == 264 && sizeof(IMAGE_OPTIONAL_HEADER64) == 240, "NT headers");
_Static_assert(offsetof(IMAGE_NT_HEADERS64, OptionalHeader) == 24 &&
    offsetof(IMAGE_OPTIONAL_HEADER64, SizeOfImage) == 56, "Optional header offsets");
_Static_assert(sizeof(IMAGE_SECTION_HEADER) == 40 && offsetof(IMAGE_SECTION_HEADER, Characteristics) == 36,
    "Section header");

/* MSVC unwinds through these wrappers when the original raises. Their cleanup must still run. */
#if defined(_MSC_VER)
#define RSF_TRY __try {
#define RSF_FINALLY(cleanup) } __finally { cleanup; }
#else
#define RSF_TRY {
#define RSF_FINALLY(cleanup) } cleanup;
#endif

FARPROC WINAPI rsf_skyrim_cs_resolve(HMODULE module, LPCSTR name)
{
    FARPROC (WINAPI* original)(HMODULE, LPCSTR) =
        (FARPROC (WINAPI*)(HMODULE, LPCSTR))rsf_skyrim_cs_original(0);
    FARPROC found;
    DWORD error;
    uintptr_t resolved;
    if (!original) return NULL;
    found = original(module, name);
    error = GetLastError();
    resolved = rsf_skyrim_cs_resolved((uintptr_t)module, name, (uintptr_t)found);
    SetLastError(error);
    return (FARPROC)resolved;
}

/* The initializer is foreign C++. SEH also contains its C++ exceptions. */
uint32_t rsf_skyrim_cs_call_initializer(uintptr_t original, const void* skse)
{
    bool (*initialize)(const void*) = (bool (*)(const void*))original;
#if defined(_MSC_VER)
    __try {
        return original && initialize(skse) ? 1u : 0u;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return 2u;
    }
#else
    return original && initialize(skse) ? 1u : 0u;
#endif
}

void rsf_skyrim_cs_configure(void* feature, void* json)
{
    /* The original keeps parsing and validating desired settings. The guarded byte affects only
       lifecycle participation. */
    void (*original)(void*, void*) = (void (*)(void*, void*))rsf_skyrim_cs_original(1);
    unsigned char* loaded = (unsigned char*)rsf_skyrim_cs_excluded_byte((uintptr_t)feature);
    RSF_TRY
        if (original) original(feature, json);
    RSF_FINALLY(if (loaded) *loaded = 0)
}

void rsf_skyrim_cs_hdr_process(
    void* manager, uint32_t input, uint32_t output, uintptr_t argument4, uint8_t argument5)
{
#if defined(_MSC_VER)
    const uintptr_t caller = (uintptr_t)_ReturnAddress();
#else
    const uintptr_t caller = (uintptr_t)__builtin_return_address(0);
#endif
    void (*original)(void*, uint32_t, uint32_t, uintptr_t, uint8_t) =
        (void (*)(void*, uint32_t, uint32_t, uintptr_t, uint8_t))rsf_skyrim_cs_original(2);
    const rsf_skyrim_cs_hdr_call call = {caller, (uintptr_t)manager, argument4, input, output, argument5};
    rsf_skyrim_cs_hdr_scope saved = {0};
    rsf_skyrim_cs_hdr_enter((uintptr_t)GetModuleHandleW(NULL), &call, &saved);
    RSF_TRY
        /* Preserve CS's RedirectFramebuffer -> ProcessMain -> RestoreFramebuffer sequence. */
        if (original) original(manager, input, output, argument4, argument5);
    RSF_FINALLY(rsf_skyrim_cs_hdr_leave(&saved))
}