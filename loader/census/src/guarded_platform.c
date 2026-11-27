/* SPDX-License-Identifier: MIT */
/* SEH leaves for pointers and calls borrowed from native graphics code. */
#include <string.h>

#include "census_internal.h"

/* ---- Safe memory ------------------------------------------------------------------------------ */
bool rsf_census_safe_read(const void* src, void* dst, size_t n)
{
    RSF_FOREIGN_TRY {
        memcpy(dst, src, n);
        return true;
    } RSF_FAULT_CATCH {
        return false;
    }
}

bool rsf_census_safe_read_cstr(const char* src, char* dst, size_t cap)
{
    if (!cap) return false;
    RSF_FOREIGN_TRY {
        size_t i = 0;
        for (; i + 1 < cap; ++i) {
            char c = src[i];
            dst[i] = c;
            if (!c) return true;
        }
        dst[i] = 0;
        return true;
    } RSF_FAULT_CATCH {
        dst[0] = 0;
        return false;
    }
}

bool rsf_census_safe_read_wstr(const wchar_t* src, wchar_t* dst, size_t cap)
{
    if (!cap) return false;
    RSF_FOREIGN_TRY {
        size_t i = 0;
        for (; i + 1 < cap; ++i) {
            wchar_t c = src[i];
            dst[i] = c;
            if (!c) return true;
        }
        dst[i] = 0;
        return true;
    } RSF_FAULT_CATCH {
        dst[0] = 0;
        return false;
    }
}

bool rsf_census_plausible_ptr(const void* p)
{
    const uintptr_t v = (uintptr_t)p;
    return v >= 0x10000 && v < 0x00007FFFFFFF0000ull;
}

bool rsf_census_is_code_ptr(const void* p)
{
    if (!rsf_census_plausible_ptr(p)) return false;
    MEMORY_BASIC_INFORMATION mbi;
    if (VirtualQuery(p, &mbi, sizeof(mbi)) != sizeof(mbi)) return false;
    const DWORD exec = PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE |
                       PAGE_EXECUTE_WRITECOPY;
    return mbi.State == MEM_COMMIT && (mbi.Protect & exec) != 0 && (mbi.Protect & PAGE_GUARD) == 0;
}

void* rsf_census_vtable_slot(const void* obj, size_t slot)
{
    void* const* vt = NULL;
    if (!rsf_census_plausible_ptr(obj) || !rsf_census_safe_read(obj, &vt, sizeof(vt)) ||
        !rsf_census_plausible_ptr(vt)) {
        return NULL;
    }
    void* fn = NULL;
    return rsf_census_safe_read(vt + slot, &fn, sizeof(fn)) && rsf_census_is_code_ptr(fn) ? fn
                                                                                         : NULL;
}

bool rsf_census_read_arg_string(const void* p, char* out, size_t cap)
{
    if (!cap) return false;
    out[0] = 0;
    return rsf_census_plausible_ptr(p) && rsf_census_safe_read_cstr((const char*)p, out, cap);
}

bool rsf_census_read_arg_wstring(const void* p, wchar_t* out, size_t cap)
{
    if (!cap) return false;
    out[0] = 0;
    return rsf_census_plausible_ptr(p) && rsf_census_safe_read_wstr((const wchar_t*)p, out, cap);
}
