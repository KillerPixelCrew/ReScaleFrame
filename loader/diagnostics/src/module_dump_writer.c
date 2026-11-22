/* SPDX-License-Identifier: MIT */
/* Native ABI entry and CRT stream ownership for compiler-specific RTTI. */

#include "module_dump_internal.h"

_Static_assert(RSF_DUMP_MODULE_PATH_UNITS == MAX_PATH, "module path must match the Windows SDK");
_Static_assert(sizeof(RUNTIME_FUNCTION) == 12, "RUNTIME_FUNCTION must use the Windows x64 layout");

void rsf_dump_sidecar_impl(void* stream, rsf_dump_stream_write write,
                           const rsf_dump_sidecar_info* info, rsf_dump_sidecar_stats* stats);
rsf_dump_result rsf_dump_module_impl(const void* module, const rsf_dump_options* options,
                                    rsf_dump_report* report, const rsf_dump_module_platform* platform);

static void write_bytes(void* stream, const unsigned char* bytes, size_t count)
{
    fwrite(bytes, 1, count, (FILE*)stream);
}

static int32_t write_sidecar(const wchar_t* path, const rsf_dump_sidecar_info* info,
                             rsf_dump_sidecar_stats* stats)
{
    FILE* stream = _wfopen(path, L"wb");
    if (!stream) {
        return RSF_DUMP_ERROR_WRITE_FAILED;
    }
    rsf_dump_sidecar_impl(stream, write_bytes, info, stats);
    const int failed = ferror(stream);
    fclose(stream);
    return failed ? RSF_DUMP_ERROR_WRITE_FAILED : RSF_DUMP_OK;
}

rsf_dump_result rsf_dump_module(const void* module, const rsf_dump_options* options,
                                rsf_dump_report* report)
{
    static const rsf_dump_module_platform platform = {write_sidecar};
    return rsf_dump_module_impl(module, options, report, &platform);
}
