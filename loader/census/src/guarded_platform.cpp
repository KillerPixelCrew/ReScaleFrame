// SPDX-License-Identifier: MIT
// SEH leaves for pointers and calls borrowed from native graphics code.
#include "census_internal.h"

namespace rsf_census {
// ---- Safe memory ------------------------------------------------------------------------------
bool safe_read(const void* src, void* dst, size_t n) noexcept {
    __try {
        std::memcpy(dst, src, n);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

bool safe_read_cstr(const char* src, char* dst, size_t cap) noexcept {
    if (!cap) return false;
    __try {
        size_t i = 0;
        for (; i + 1 < cap; ++i) {
            char c = src[i];
            dst[i] = c;
            if (!c) return true;
        }
        dst[i] = 0;
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        dst[0] = 0;
        return false;
    }
}

bool safe_read_wstr(const wchar_t* src, wchar_t* dst, size_t cap) noexcept {
    if (!cap) return false;
    __try {
        size_t i = 0;
        for (; i + 1 < cap; ++i) {
            wchar_t c = src[i];
            dst[i] = c;
            if (!c) return true;
        }
        dst[i] = 0;
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        dst[0] = 0;
        return false;
    }
}

bool plausible_ptr(const void* p) noexcept {
    const uintptr_t v = reinterpret_cast<uintptr_t>(p);
    return v >= 0x10000 && v < 0x00007FFFFFFF0000ull;
}

bool is_code_ptr(const void* p) noexcept {
    if (!plausible_ptr(p)) return false;
    MEMORY_BASIC_INFORMATION mbi;
    if (VirtualQuery(p, &mbi, sizeof(mbi)) != sizeof(mbi)) return false;
    constexpr DWORD kExec = PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY;
    return mbi.State == MEM_COMMIT && (mbi.Protect & kExec) != 0 && (mbi.Protect & PAGE_GUARD) == 0;
}

void* vtable_slot(const void* obj, size_t slot) noexcept {
    void* const* vt = nullptr;
    if (!plausible_ptr(obj) || !rd(obj, vt) || !plausible_ptr(vt)) return nullptr;
    void* fn = nullptr;
    return rd(vt + slot, fn) && is_code_ptr(fn) ? fn : nullptr;
}

bool guarded(void (*fn)(void*), void* ctx) noexcept {
    __try {
        fn(ctx);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

} // namespace rsf_census
