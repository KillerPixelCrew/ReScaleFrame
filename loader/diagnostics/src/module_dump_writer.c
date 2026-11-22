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

rsf_dump_result rsf_dump_image_write(const rsf_dump_image_info* info, rsf_section_digest* digests,
                                    rsf_dump_image_stats* stats);
uint32_t rsf_dump_imports_impl(void* stream, rsf_dump_stream_write write,
                               const unsigned char* base, uint32_t image_size,
                               uint32_t directory, const rsf_module_entry* modules, size_t count);

static void write_bytes(void* stream, const unsigned char* bytes, size_t count)
{
    fwrite(bytes, 1, count, (FILE*)stream);
}

/* Format a fixed-point number without going through the CRT's float formatting, which follows
   the process locale. A German locale writes 7,9971, and that is not JSON. */
static void write_json_number(FILE* stream, double value)
{
    if (value < 0.0) {
        fputc('-', stream);
        value = -value;
    }
    const long long scaled = (long long)(value * 10000.0 + 0.5);
    fprintf(stream, "%lld.%04lld", scaled / 10000, scaled % 10000);
}

static rsf_dump_result write_sidecar(const wchar_t* path, const unsigned char* base,
                                     const IMAGE_NT_HEADERS64* headers, const wchar_t* module_path,
                                     const rsf_module_entry* modules, size_t module_count,
                                     const rsf_dump_options* options,
                                     const rsf_section_digest* digests, double hash_ms,
                                     double started_ms, rsf_dump_report* report)
{
    FILE* stream = _wfopen(path, L"wb");
    if (!stream) {
        return RSF_DUMP_ERROR_WRITE_FAILED;
    }

    fputs("{\n  \"tool\": \"rescaleframe-module-dump\",\n", stream);
    fprintf(stream, "  \"abi_version\": %u,\n", RSF_MODULE_DUMP_ABI_VERSION);
    fprintf(stream, "  \"schema_version\": %u,\n", RSF_MODULE_DUMP_SCHEMA_VERSION);
    fputs("  \"module\": {\"path\": \"", stream);
    rsf_json_wide(stream, module_path);
    fprintf(stream,
            "\", \"load_base\": \"0x%llx\", \"preferred_base\": \"0x%llx\", "
            "\"size_of_image\": %lu, \"entry_point_rva\": \"0x%lx\"},\n",
            (unsigned long long)(uintptr_t)base,
            (unsigned long long)headers->OptionalHeader.ImageBase,
            (unsigned long)headers->OptionalHeader.SizeOfImage,
            (unsigned long)headers->OptionalHeader.AddressOfEntryPoint);
    fputs("  \"code_entropy\": ", stream);
    write_json_number(stream, report->code_entropy);
    fprintf(stream, ",\n  \"iat_references\": %lu,\n", (unsigned long)report->iat_references);

    fputs("  \"sections\": [", stream);
    const IMAGE_SECTION_HEADER* sections = IMAGE_FIRST_SECTION(headers);
    for (WORD index = 0; index < headers->FileHeader.NumberOfSections; ++index) {
        fprintf(stream, "%s\n    {\"name\": \"", index ? "," : "");
        rsf_json_ascii(stream, (const char*)sections[index].Name, IMAGE_SIZEOF_SHORT_NAME);
        fprintf(stream,
                "\", \"rva\": \"0x%lx\", \"virtual_size\": %lu, \"characteristics\": \"0x%lx\"",
                (unsigned long)sections[index].VirtualAddress,
                (unsigned long)sections[index].Misc.VirtualSize,
                (unsigned long)sections[index].Characteristics);
        if (digests && digests[index].valid) {
            fprintf(stream, ", \"dumped_size\": %llu, \"readable_bytes\": %llu, \"entropy\": ",
                    (unsigned long long)digests[index].dumped_size,
                    (unsigned long long)digests[index].readable_bytes);
            write_json_number(stream, digests[index].entropy);
            fprintf(stream, ", \"sha256\": \"%s\"", digests[index].sha256);
        }
        fputc('}', stream);
    }
    fputs("\n  ],\n", stream);

    fputs("  \"modules\": [", stream);
    for (size_t index = 0; index < module_count; ++index) {
        fprintf(stream, "%s\n    {\"path\": \"", index ? "," : "");
        rsf_json_wide(stream, modules[index].path);
        fprintf(stream, "\", \"base\": \"0x%llx\", \"size\": %llu}",
                (unsigned long long)(uintptr_t)modules[index].base,
                (unsigned long long)modules[index].size);
    }
    fputs("\n  ],\n", stream);

    fputs("  \"imports\": [", stream);
    const IMAGE_DATA_DIRECTORY* imports =
        &headers->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT];
    if (imports->Size != 0) {
        report->imports_described = rsf_dump_imports_impl(
            stream, write_bytes, base, headers->OptionalHeader.SizeOfImage,
            imports->VirtualAddress, modules, module_count);
    }
    fputs("\n  ],\n", stream);

    const double structured = rsf_dump_now_ms();
    report->threads_listed = rsf_dump_write_threads(stream, modules, module_count);
    rsf_dump_write_rtti(stream, base, headers, options->rtti_budget_ms, &report->rtti_classes);
    rsf_dump_write_functions(stream, base, headers, &report->function_starts);
    rsf_dump_write_names(stream, options->name_list_path_utf8);
    fprintf(stream,
            "  \"timing_ms\": {\"section_hash_and_entropy\": %lu, \"structured_sections\": %lu, "
            "\"total\": %lu},\n",
            (unsigned long)(hash_ms + 0.5), (unsigned long)(rsf_dump_now_ms() - structured + 0.5),
            (unsigned long)(rsf_dump_now_ms() - started_ms + 0.5));

    fputs("  \"limits\": \"Section raw offsets equal virtual addresses, so the dump maps one to "
          "one onto runtime addresses. It is not runnable: the entry point is unchanged and no "
          "import table was rebuilt. Import values are the addresses resolved in this process at "
          "dump time. Threads, RTTI and function starts describe the process at dump time.\"\n}\n", stream);

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
    const rsf_dump_result sidecar =
        write_sidecar(sidecar_path, base, headers, module_path, modules, module_count, options,
                      digests, stats.hash_ms, started_ms, &local);
    free(modules);
    free(digests);

    copy_report(report, &local);
    return sidecar;
}
