// SPDX-License-Identifier: MIT
#include <rescaleframe/overlay_api.h>
#include <rescaleframe/native_fg.h>
#include <windows.h>
#include <string.h>

#ifdef RSF_RUNTIME_BUILD
#error The contract client must import Runtime.
#endif

typedef int32_t (*apply_fn)(const rsf_render_settings*);
typedef int32_t (*guarded_fn)(apply_fn);
typedef int32_t (*diagnostics_fn)(rsf_overlay_diagnostics*);
typedef int (*fg_status_fn)(rsf_native_fg_status*);

int rsf_test_overlay_contract(rsf_get_overlay_api_fn, uint32_t name_resolves,
    guarded_fn, diagnostics_fn, fg_status_fn);

static int32_t apply_at_page_boundary(apply_fn apply)
{
    SYSTEM_INFO system = {0};
    GetSystemInfo(&system);
    const size_t length = system.dwPageSize;
    unsigned char* pages = VirtualAlloc(NULL, length * 2, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    if (!pages) return RSF_ERROR_NOT_READY;
    int32_t result = RSF_ERROR_NOT_READY;
    DWORD previous = 0;
    if (VirtualProtect(pages + length, length, PAGE_NOACCESS, &previous)) {
        uint32_t* prefix = (uint32_t*)(pages + length - sizeof(uint32_t));
        *prefix = sizeof(uint32_t);
        result = apply((const rsf_render_settings*)prefix);
    }
    VirtualFree(pages, 0, MEM_RELEASE);
    return result;
}

int main(void)
{
    const FARPROC entry = GetProcAddress(GetModuleHandleW(L"ReScaleFrame.Runtime.dll"),
        RSF_OVERLAY_API_ENTRY_POINT);
    rsf_get_overlay_api_fn resolved = NULL;
    _Static_assert(sizeof(entry) == sizeof(resolved), "Windows function pointer size");
    memcpy(&resolved, &entry, sizeof(resolved));
    return rsf_test_overlay_contract(rsf_get_overlay_api, resolved == rsf_get_overlay_api ? 1u : 0u,
        apply_at_page_boundary, rsf_get_overlay_diagnostics, rsf_native_fg_status_get);
}
