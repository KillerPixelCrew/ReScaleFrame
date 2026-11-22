/* SPDX-License-Identifier: MIT */
/* Loaded-module access and diagnostic entry points shared with the image writer. */

#include "module_dump_internal.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

static const IMAGE_NT_HEADERS64* rsf_dump_nt_headers(const void* base)
{
    const IMAGE_DOS_HEADER* dos = (const IMAGE_DOS_HEADER*)base;
    if (!dos || dos->e_magic != IMAGE_DOS_SIGNATURE) {
        return NULL;
    }
    const IMAGE_NT_HEADERS64* headers =
        (const IMAGE_NT_HEADERS64*)((const unsigned char*)base + dos->e_lfanew);
    if (headers->Signature != IMAGE_NT_SIGNATURE ||
        headers->OptionalHeader.Magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC) {
        return NULL;
    }
    return headers;
}

static const void* rsf_dump_resolve_base(const void* module_base)
{
    return module_base ? module_base : (const void*)GetModuleHandleW(NULL);
}

rsf_dump_result rsf_patch_code(uint32_t rva, const uint8_t* bytes, uint32_t count,
                               const uint8_t* expected, uint32_t expected_count,
                               uint8_t* previous)
{
    if (!bytes || count == 0) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }
    unsigned char* base = (unsigned char*)rsf_dump_resolve_base(NULL);
    const IMAGE_NT_HEADERS64* headers = rsf_dump_nt_headers(base);
    if (!headers) {
        return RSF_DUMP_ERROR_NOT_A_PE;
    }
    if (rva + count > headers->OptionalHeader.SizeOfImage) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }

    unsigned char* target = base + rva;
    if (expected && expected_count) {
        if (expected_count != count) {
            return RSF_DUMP_ERROR_INVALID_ARGUMENT;
        }
        /* Refusing on a mismatch is the whole point. A patch aimed at the wrong address is far
           worse than no patch, and a build that shifted the code would land exactly there. */
        if (memcmp(target, expected, expected_count) != 0) {
            return RSF_DUMP_ERROR_INVALID_ARGUMENT;
        }
    }

    DWORD protection = 0;
    if (!VirtualProtect(target, count, PAGE_EXECUTE_READWRITE, &protection)) {
        return RSF_DUMP_ERROR_WRITE_FAILED;
    }
    if (previous) {
        memcpy(previous, target, count);
    }
    memcpy(target, bytes, count);
    DWORD restored = 0;
    VirtualProtect(target, count, protection, &restored);
    FlushInstructionCache(GetCurrentProcess(), target, count);
    return RSF_DUMP_OK;
}

rsf_dump_result rsf_write_module_list_impl(const char* path, const char* label,
                                          const rsf_dump_list_stream* native);

static void* module_list_open(const wchar_t* path)
{
    return _wfopen(path, L"ab");
}

static void module_list_title(void* stream, const char* label, size_t count)
{
    fprintf((FILE*)stream, "[%s] %llu modules\n", label, (unsigned long long)count);
}

static void module_list_leaf(void* stream, const wchar_t* basename)
{
    fprintf((FILE*)stream, "  %ls\n", basename);
}

static int32_t module_list_close(void* stream)
{
    const int failed = ferror((FILE*)stream);
    fclose((FILE*)stream);
    return failed ? RSF_DUMP_ERROR_WRITE_FAILED : RSF_DUMP_OK;
}

rsf_dump_result rsf_write_module_list(const char* path_utf8, const char* label)
{
    static const rsf_dump_list_stream native = {
        module_list_open, module_list_title, module_list_leaf, module_list_close,
    };
    return rsf_write_module_list_impl(path_utf8, label, &native);
}
