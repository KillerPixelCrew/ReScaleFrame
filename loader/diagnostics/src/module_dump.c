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

/* Reach a console variable object the way the engine's own code does. The float path above
   predates this and does the same thing inline; it is left alone rather than reworked, because it
   is the one console path that has been confirmed in the running game. */
static rsf_dump_result find_console_variable(const char* name_utf8, uint32_t singleton_rva,
                                             uint32_t find_slot, void** variable_out)
{
    if (!name_utf8 || singleton_rva == 0 || !variable_out) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }
    unsigned char* base = (unsigned char*)rsf_dump_resolve_base(NULL);
    const IMAGE_NT_HEADERS64* headers = rsf_dump_nt_headers(base);
    if (!headers || singleton_rva + sizeof(void*) > headers->OptionalHeader.SizeOfImage) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }

    void* manager = *(void**)(base + singleton_rva);
    if (!manager) {
        return RSF_DUMP_ERROR_ABI_MISMATCH;
    }

    wchar_t name[256];
    if (MultiByteToWideChar(CP_UTF8, 0, name_utf8, -1, name, 256) == 0) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }

    typedef void*(*find_console_variable_fn)(void*, const wchar_t*, int);
    void** vtable = *(void***)manager;
    find_console_variable_fn find = (find_console_variable_fn)vtable[find_slot / sizeof(void*)];
    void* variable = find(manager, name, 1);
    if (!variable) {
        return RSF_DUMP_ERROR_NOT_A_PE;
    }
    *variable_out = variable;
    return RSF_DUMP_OK;
}

rsf_dump_result rsf_console_set_int(const char* name_utf8, int32_t expected_current,
                                    int32_t new_value, uint32_t value_offset,
                                    uint32_t singleton_rva, uint32_t find_slot)
{
    void* variable = NULL;
    const rsf_dump_result found =
        find_console_variable(name_utf8, singleton_rva, find_slot, &variable);
    if (found != RSF_DUMP_OK) {
        return found;
    }

    /* Both thread copies are checked before either is written. An offset that is wrong for this
       build almost certainly fails the check, and a half-written pair would leave the game thread
       and the render thread disagreeing about the value, which is worse than not setting it. */
    unsigned char* slots = (unsigned char*)variable + value_offset;
    int32_t current[2];
    memcpy(current, slots, sizeof(current));
    if (current[0] != expected_current || current[1] != expected_current) {
        return RSF_DUMP_ERROR_STILL_ENCRYPTED;
    }

    DWORD protection = 0;
    if (!VirtualProtect(slots, sizeof(current), PAGE_READWRITE, &protection)) {
        return RSF_DUMP_ERROR_WRITE_FAILED;
    }
    const int32_t replacement[2] = {new_value, new_value};
    memcpy(slots, replacement, sizeof(replacement));
    DWORD restored = 0;
    VirtualProtect(slots, sizeof(current), protection, &restored);
    return RSF_DUMP_OK;
}

rsf_dump_result rsf_console_set_float(const char* name_utf8, float expected_current,
                                      float new_value, uint32_t singleton_rva, uint32_t find_slot,
                                      uint32_t* found_offset)
{
    if (!name_utf8 || singleton_rva == 0) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }
    unsigned char* base = (unsigned char*)rsf_dump_resolve_base(NULL);
    const IMAGE_NT_HEADERS64* headers = rsf_dump_nt_headers(base);
    if (!headers || singleton_rva + sizeof(void*) > headers->OptionalHeader.SizeOfImage) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }

    void* manager = *(void**)(base + singleton_rva);
    if (!manager) {
        /* Nothing has asked the engine for a console variable yet, so the manager does not exist.
           Creating it here would run engine code at a moment of our choosing, which is worse than
           waiting for the game to do it. */
        return RSF_DUMP_ERROR_ABI_MISMATCH;
    }

    wchar_t name[256];
    if (MultiByteToWideChar(CP_UTF8, 0, name_utf8, -1, name, 256) == 0) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }

    /* An extra register argument is harmless in this calling convention, so passing the tracking
       flag works whether or not this build's signature declares it. */
    typedef void*(*find_console_variable_fn)(void*, const wchar_t*, int);
    void** vtable = *(void***)manager;
    find_console_variable_fn find = (find_console_variable_fn)vtable[find_slot / sizeof(void*)];
    void* variable = find(manager, name, 1);
    if (!variable) {
        return RSF_DUMP_ERROR_NOT_A_PE;
    }

    /* Search the object for the value rather than trusting an offset, and replace every copy.
       Unreal keeps TConsoleVariableData<T>::Values[2], one read on the game thread and one on the
       render thread, so setting only the first leaves the renderer using the old number. The
       value sits well past the help string, flags and delegate that precede it, which is why the
       window has to be generous. */
    unsigned char* bytes = (unsigned char*)variable;
    uint32_t replaced = 0;
    for (uint32_t offset = 0; offset <= 0x100; offset += 4) {
        float current;
        memcpy(&current, bytes + offset, sizeof(current));
        const float difference = current > expected_current ? current - expected_current
                                                            : expected_current - current;
        if (difference > 0.0001f) {
            continue;
        }
        DWORD protection = 0;
        if (!VirtualProtect(bytes + offset, sizeof(float), PAGE_READWRITE, &protection)) {
            continue;
        }
        memcpy(bytes + offset, &new_value, sizeof(new_value));
        DWORD restored = 0;
        VirtualProtect(bytes + offset, sizeof(float), protection, &restored);
        if (found_offset && replaced == 0) {
            *found_offset = offset;
        }
        ++replaced;
    }
    if (replaced) {
        return RSF_DUMP_OK;
    }
    return RSF_DUMP_ERROR_STILL_ENCRYPTED;
}

rsf_dump_result rsf_console_probe(const char* name_utf8, uint32_t singleton_rva,
                                  uint32_t find_slot, uint64_t* manager_out,
                                  uint64_t* variable_out, float* floats, uint32_t float_count)
{
    if (!name_utf8 || singleton_rva == 0) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }
    unsigned char* base = (unsigned char*)rsf_dump_resolve_base(NULL);
    const IMAGE_NT_HEADERS64* headers = rsf_dump_nt_headers(base);
    if (!headers || singleton_rva + sizeof(void*) > headers->OptionalHeader.SizeOfImage) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }

    void* manager = *(void**)(base + singleton_rva);
    if (manager_out) {
        *manager_out = (uint64_t)(uintptr_t)manager;
    }
    if (!manager) {
        return RSF_DUMP_ERROR_ABI_MISMATCH;
    }

    wchar_t name[256];
    if (MultiByteToWideChar(CP_UTF8, 0, name_utf8, -1, name, 256) == 0) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }
    typedef void*(*find_console_variable_fn)(void*, const wchar_t*, int);
    void** vtable = *(void***)manager;
    find_console_variable_fn find = (find_console_variable_fn)vtable[find_slot / sizeof(void*)];
    void* variable = find(manager, name, 1);
    if (variable_out) {
        *variable_out = (uint64_t)(uintptr_t)variable;
    }
    if (!variable) {
        return RSF_DUMP_ERROR_NOT_A_PE;
    }
    if (floats && float_count) {
        memcpy(floats, variable, (size_t)float_count * sizeof(float));
    }
    return RSF_DUMP_OK;
}

rsf_dump_result rsf_write_module_list(const char* path_utf8, const char* label)
{
    if (!path_utf8) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }
    wchar_t path[MAX_PATH * 2];
    if (MultiByteToWideChar(CP_UTF8, 0, path_utf8, -1, path, MAX_PATH * 2) == 0) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }
    FILE* stream = _wfopen(path, L"ab");
    if (!stream) {
        return RSF_DUMP_ERROR_WRITE_FAILED;
    }

    rsf_module_entry* modules =
        (rsf_module_entry*)calloc(RSF_MAX_MODULES, sizeof(rsf_module_entry));
    if (!modules) {
        fclose(stream);
        return RSF_DUMP_ERROR_WRITE_FAILED;
    }
    size_t count = 0;
    rsf_dump_collect_modules(modules, RSF_MAX_MODULES, &count);
    fprintf(stream, "[%s] %llu modules\n", label ? label : "sample", (unsigned long long)count);
    for (size_t index = 0; index < count; ++index) {
        const wchar_t* leaf = wcsrchr(modules[index].path, L'\\');
        fprintf(stream, "  %ls\n", leaf ? leaf + 1 : modules[index].path);
    }
    free(modules);
    const int failed = ferror(stream);
    fclose(stream);
    return failed ? RSF_DUMP_ERROR_WRITE_FAILED : RSF_DUMP_OK;
}
