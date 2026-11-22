/* SPDX-License-Identifier: MIT */
/* The module-list writer's C stdio stream for the Rust lister. */

#include "module_dump_internal.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

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
