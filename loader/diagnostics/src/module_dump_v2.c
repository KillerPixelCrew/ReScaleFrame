/* SPDX-License-Identifier: MIT */
/* Native stream bridge for the structured module dump sections. */

#include "module_dump_internal.h"

#include <wchar.h>

_Static_assert(RSF_DUMP_MODULE_PATH_UNITS == MAX_PATH, "module path must match the Windows SDK");
_Static_assert(sizeof(RUNTIME_FUNCTION) == 12, "RUNTIME_FUNCTION must use the Windows x64 layout");

/* ---- JSON helpers ---- */

void rsf_json_wide(FILE* stream, const wchar_t* text)
{
    for (const wchar_t* cursor = text; *cursor; ++cursor) {
        if (*cursor == L'"' || *cursor == L'\\') {
            fprintf(stream, "\\%c", (char)*cursor);
        } else if (*cursor < 0x20 || *cursor > 0x7e) {
            fprintf(stream, "\\u%04x", (unsigned)*cursor);
        } else {
            fputc((char)*cursor, stream);
        }
    }
}

void rsf_json_ascii(FILE* stream, const char* text, size_t limit)
{
    for (size_t index = 0; index < limit && text[index]; ++index) {
        const unsigned char value = (unsigned char)text[index];
        if (value == '"' || value == '\\') {
            fprintf(stream, "\\%c", (char)value);
        } else if (value < 0x20 || value > 0x7e) {
            fprintf(stream, "\\u%04x", (unsigned)value);
        } else {
            fputc((char)value, stream);
        }
    }
}

const wchar_t* rsf_dump_basename(const wchar_t* path)
{
    const wchar_t* leaf = wcsrchr(path, L'\\');
    return leaf ? leaf + 1 : path;
}

uint32_t rsf_dump_threads_impl(void* stream, rsf_dump_stream_write write,
                               const rsf_module_entry* modules, size_t count);
uint32_t rsf_dump_functions_impl(void* stream, rsf_dump_stream_write write,
                                 const unsigned char* base, uint32_t image_size,
                                 uint32_t directory, uint32_t size);
void rsf_dump_names_impl(void* stream, const char* path_utf8, rsf_dump_stream_write write);

static void write_bytes(void* stream, const unsigned char* data, size_t size)
{
    fwrite(data, 1, size, (FILE*)stream);
}

uint32_t rsf_dump_write_threads(FILE* stream, const rsf_module_entry* modules,
                                size_t module_count)
{
    return rsf_dump_threads_impl(stream, write_bytes, modules, module_count);
}

void rsf_dump_write_functions(FILE* stream, const unsigned char* base,
                              const IMAGE_NT_HEADERS64* headers, uint32_t* function_count)
{
    const IMAGE_DATA_DIRECTORY* directory =
        &headers->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_EXCEPTION];
    const uint32_t written = rsf_dump_functions_impl(
        stream, write_bytes, base, headers->OptionalHeader.SizeOfImage,
        directory->VirtualAddress, directory->Size);
    if (function_count) {
        *function_count = written;
    }
}

void rsf_dump_write_names(FILE* stream, const char* path_utf8)
{
    rsf_dump_names_impl(stream, path_utf8, write_bytes);
}
