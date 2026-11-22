/* SPDX-License-Identifier: MIT */
/* Read a loaded module out of its own address space and write it as a file whose raw offsets
   equal its virtual addresses, so the result maps one to one onto runtime addresses.

   The dump is for analysis and is never meant to run again. Nothing here repairs the entry point
   or rebuilds an import address table. */

#include "module_dump_internal.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

_Static_assert(RSF_DUMP_MODULE_PATH_UNITS == MAX_PATH, "module path must match the Windows SDK");
_Static_assert(sizeof(RUNTIME_FUNCTION) == 12, "RUNTIME_FUNCTION must use the Windows x64 layout");

rsf_dump_result rsf_dump_image_write(const rsf_dump_image_info* info, rsf_section_digest* digests,
                                    rsf_dump_image_stats* stats);
typedef void (*rsf_dump_rtti_write)(void* stream, const unsigned char* base,
                                    const void* headers, uint32_t budget, uint32_t* count);
void rsf_dump_sidecar_impl(void* stream, rsf_dump_stream_write write, rsf_dump_rtti_write rtti,
                           const rsf_dump_sidecar_info* info, rsf_dump_sidecar_stats* stats);

static void write_bytes(void* stream, const unsigned char* bytes, size_t count)
{
    fwrite(bytes, 1, count, (FILE*)stream);
}

static void write_rtti(void* stream, const unsigned char* base, const void* headers,
                       uint32_t budget, uint32_t* count)
{
    rsf_dump_write_rtti((FILE*)stream, base, (const IMAGE_NT_HEADERS64*)headers, budget, count);
}

static rsf_dump_result write_sidecar(const wchar_t* path, const rsf_dump_sidecar_info* info,
                                     rsf_dump_report* report)
{
    FILE* stream = _wfopen(path, L"wb");
    if (!stream) {
        return RSF_DUMP_ERROR_WRITE_FAILED;
    }
    rsf_dump_sidecar_stats stats = {0};
    rsf_dump_sidecar_impl(stream, write_bytes, write_rtti, info, &stats);
    report->imports_described = stats.imports_described;
    report->threads_listed = stats.threads_listed;
    report->rtti_classes = stats.rtti_classes;
    report->function_starts = stats.function_starts;
    const int failed = ferror(stream);
    fclose(stream);
    return failed ? RSF_DUMP_ERROR_WRITE_FAILED : RSF_DUMP_OK;
}

/* Copy no more than the caller's structure can hold. */
static void copy_report(rsf_dump_report* report, const rsf_dump_report* local)
{
    if (!report) {
        return;
    }
    const uint32_t size = report->struct_size;
    memcpy(report, local, size < sizeof(*local) ? size : sizeof(*local));
    report->struct_size = size;
}

rsf_dump_result rsf_dump_module(const void* module_base, const rsf_dump_options* options,
                                rsf_dump_report* report)
{
    if (!options || options->struct_size < RSF_DUMP_OPTIONS_V1_SIZE ||
        !options->output_directory_utf8) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }
    if (options->abi_version != RSF_MODULE_DUMP_ABI_VERSION) {
        return RSF_DUMP_ERROR_ABI_MISMATCH;
    }
    if (report && report->struct_size < RSF_DUMP_REPORT_V1_SIZE) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }
    const double started_ms = rsf_dump_now_ms();

    /* A caller built before the schema 2 fields existed supplies a shorter structure. */
    rsf_dump_options effective;
    memset(&effective, 0, sizeof(effective));
    memcpy(&effective, options,
           options->struct_size < sizeof(effective) ? options->struct_size : sizeof(effective));
    options = &effective;

    const unsigned char* base = (const unsigned char*)rsf_dump_resolve_base(module_base);
    const IMAGE_NT_HEADERS64* headers = rsf_dump_nt_headers(base);
    if (!headers) {
        return RSF_DUMP_ERROR_NOT_A_PE;
    }

    rsf_dump_report local;
    memset(&local, 0, sizeof(local));
    local.struct_size = sizeof(local);
    local.load_base = (uint64_t)(uintptr_t)base;
    local.preferred_base = (uint64_t)headers->OptionalHeader.ImageBase;

    if (rsf_measure_module_code(base, &local.code_entropy) != RSF_DUMP_OK) {
        return RSF_DUMP_ERROR_NOT_A_PE;
    }
    if (options->require_decrypted && local.code_entropy >= options->entropy_threshold) {
        copy_report(report, &local);
        return RSF_DUMP_ERROR_STILL_ENCRYPTED;
    }

    wchar_t module_path[MAX_PATH];
    const DWORD path_length = GetModuleFileNameW((HMODULE)(void*)base, module_path, MAX_PATH);
    if (path_length == 0 || path_length >= MAX_PATH) {
        wcscpy(module_path, L"<unknown>");
    }
    const wchar_t* leaf = wcsrchr(module_path, L'\\');
    leaf = leaf ? leaf + 1 : module_path;

    wchar_t directory[MAX_PATH];
    if (MultiByteToWideChar(CP_UTF8, 0, options->output_directory_utf8, -1, directory, MAX_PATH) == 0) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }
    wchar_t name[MAX_PATH];
    if (options->output_name_utf8) {
        if (MultiByteToWideChar(CP_UTF8, 0, options->output_name_utf8, -1, name, MAX_PATH) == 0) {
            return RSF_DUMP_ERROR_INVALID_ARGUMENT;
        }
    } else {
        wcsncpy(name, leaf, MAX_PATH - 1);
        name[MAX_PATH - 1] = L'\0';
    }

    wchar_t dump_path[MAX_PATH * 2];
    wchar_t sidecar_path[MAX_PATH * 2];
    const int dump_length = _snwprintf(dump_path, MAX_PATH * 2, L"%s\\%s.dump", directory, name);
    const int sidecar_length =
        _snwprintf(sidecar_path, MAX_PATH * 2, L"%s\\%s.dump.json", directory, name);
    if (dump_length < 0 || sidecar_length < 0 ||
        dump_length >= MAX_PATH * 2 || sidecar_length >= MAX_PATH * 2) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }

    const IMAGE_SECTION_HEADER* sections = IMAGE_FIRST_SECTION(headers);
    const rsf_dump_image_info info = {
        base, dump_path, headers->OptionalHeader.SizeOfImage, headers->OptionalHeader.SizeOfHeaders,
        headers->OptionalHeader.SectionAlignment,
        (uint32_t)((const unsigned char*)headers - base),
        (uint32_t)((const unsigned char*)sections - base),
        headers->FileHeader.NumberOfSections,
        headers->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT].VirtualAddress,
    };
    rsf_section_digest* digests =
        (rsf_section_digest*)calloc(info.sections ? info.sections : 1u, sizeof(rsf_section_digest));
    rsf_dump_image_stats stats = {0};
    const rsf_dump_result image = rsf_dump_image_write(&info, digests, &stats);
    if (image != RSF_DUMP_OK) {
        free(digests);
        return image;
    }
    local.sections_written = stats.sections_written;
    local.iat_references = stats.iat_references;
    local.bytes_written = stats.bytes_written;

    rsf_module_entry* modules =
        (rsf_module_entry*)calloc(RSF_MAX_MODULES, sizeof(rsf_module_entry));
    size_t module_count = 0;
    if (modules) {
        rsf_dump_collect_modules(modules, RSF_MAX_MODULES, &module_count);
    }
    local.modules_listed = (uint32_t)module_count;
    const IMAGE_DATA_DIRECTORY* directories = headers->OptionalHeader.DataDirectory;
    const rsf_dump_sidecar_info sidecar_info = {
        &info, headers, module_path, modules, module_count, digests, options->name_list_path_utf8,
        local.preferred_base, local.code_entropy, stats.hash_ms, started_ms,
        RSF_MODULE_DUMP_ABI_VERSION, RSF_MODULE_DUMP_SCHEMA_VERSION,
        headers->OptionalHeader.AddressOfEntryPoint, local.iat_references,
        directories[IMAGE_DIRECTORY_ENTRY_IMPORT].Size,
        directories[IMAGE_DIRECTORY_ENTRY_EXCEPTION].VirtualAddress,
        directories[IMAGE_DIRECTORY_ENTRY_EXCEPTION].Size, options->rtti_budget_ms,
    };
    const rsf_dump_result sidecar = write_sidecar(sidecar_path, &sidecar_info, &local);
    free(modules);
    free(digests);

    copy_report(report, &local);
    return sidecar;
}
