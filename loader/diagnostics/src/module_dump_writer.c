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

#define RSF_ENTROPY_WINDOW 65536u
#define RSF_ENTROPY_SAMPLES 16u

static DWORD align_up(DWORD value, DWORD alignment)
{
    if (alignment == 0) {
        return value;
    }
    return (value + alignment - 1u) & ~(alignment - 1u);
}

static int compare_rva(const void* left, const void* right);

/* Collect the address-table slot of every import, so code can be checked for references to them.
   The shipped AC7 file contains no such reference anywhere, because its code is ciphertext. A
   dump that contains them is decrypted; one that does not is not, whatever its entropy says. */
static DWORD* collect_iat_slots(const unsigned char* base, const IMAGE_NT_HEADERS64* headers,
                                size_t* count)
{
    *count = 0;
    const IMAGE_DATA_DIRECTORY* directory =
        &headers->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT];
    if (directory->VirtualAddress == 0) {
        return NULL;
    }

    size_t capacity = 256;
    DWORD* slots = (DWORD*)malloc(capacity * sizeof(DWORD));
    if (!slots) {
        return NULL;
    }
    const IMAGE_IMPORT_DESCRIPTOR* descriptor =
        (const IMAGE_IMPORT_DESCRIPTOR*)(base + directory->VirtualAddress);
    for (; descriptor->Name != 0; ++descriptor) {
        const DWORD names_rva = descriptor->OriginalFirstThunk ? descriptor->OriginalFirstThunk
                                                               : descriptor->FirstThunk;
        if (names_rva == 0 || descriptor->FirstThunk == 0) {
            continue;
        }
        const ULONGLONG* names = (const ULONGLONG*)(base + names_rva);
        for (size_t slot = 0; names[slot] != 0; ++slot) {
            if (*count == capacity) {
                capacity *= 2;
                DWORD* grown = (DWORD*)realloc(slots, capacity * sizeof(DWORD));
                if (!grown) {
                    free(slots);
                    *count = 0;
                    return NULL;
                }
                slots = grown;
            }
            slots[(*count)++] = descriptor->FirstThunk + (DWORD)(slot * sizeof(ULONGLONG));
        }
    }
    qsort(slots, *count, sizeof(DWORD), compare_rva);
    return slots;
}

static int compare_rva(const void* left, const void* right)
{
    const DWORD a = *(const DWORD*)left;
    const DWORD b = *(const DWORD*)right;
    return a < b ? -1 : (a > b ? 1 : 0);
}

static int slot_index(const DWORD* slots, size_t count, DWORD rva)
{
    const DWORD* found = (const DWORD*)bsearch(&rva, slots, count, sizeof(DWORD), compare_rva);
    return found ? (int)(found - slots) : -1;
}

/* Count rip-relative calls and jumps through the import table inside one code range. This is a
   byte scan, not a disassembly, but requiring the target to be an exact import slot keeps false
   positives negligible. */
static uint32_t count_iat_references(const unsigned char* code, size_t size, DWORD code_rva,
                                     const DWORD* slots, size_t slot_count, uint32_t* hits)
{
    if (!slots || slot_count == 0 || size < 6) {
        return 0;
    }
    uint32_t total = 0;
    for (size_t offset = 0; offset + 6 <= size; ++offset) {
        if (code[offset] != 0xFF) {
            continue;
        }
        if (code[offset + 1] != 0x15 && code[offset + 1] != 0x25) {
            continue;
        }
        int32_t displacement;
        memcpy(&displacement, code + offset + 2, sizeof(displacement));
        const DWORD target = (DWORD)(code_rva + (DWORD)offset + 6u + (DWORD)displacement);
        const int index = slot_index(slots, slot_count, target);
        if (index >= 0) {
            ++total;
            if (hits) {
                ++hits[index];
            }
        }
    }
    return total;
}

static const rsf_module_entry* module_for(const rsf_module_entry* entries, size_t count,
                                          const unsigned char* address)
{
    for (size_t index = 0; index < count; ++index) {
        if (address >= entries[index].base && address < entries[index].base + entries[index].size) {
            return &entries[index];
        }
    }
    return NULL;
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

static void describe_imports(FILE* stream, const unsigned char* base,
                             const IMAGE_NT_HEADERS64* headers, const rsf_module_entry* modules,
                             size_t module_count, uint32_t* described)
{
    *described = 0;
    const IMAGE_DATA_DIRECTORY* directory =
        &headers->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT];
    if (directory->VirtualAddress == 0 || directory->Size == 0) {
        return;
    }

    const IMAGE_IMPORT_DESCRIPTOR* descriptor =
        (const IMAGE_IMPORT_DESCRIPTOR*)(base + directory->VirtualAddress);
    int first = 1;
    for (; descriptor->Name != 0; ++descriptor) {
        const char* library = (const char*)(base + descriptor->Name);
        const DWORD names_rva = descriptor->OriginalFirstThunk ? descriptor->OriginalFirstThunk
                                                               : descriptor->FirstThunk;
        if (names_rva == 0 || descriptor->FirstThunk == 0) {
            continue;
        }
        const ULONGLONG* names = (const ULONGLONG*)(base + names_rva);
        const ULONGLONG* values = (const ULONGLONG*)(base + descriptor->FirstThunk);
        for (size_t slot = 0; names[slot] != 0; ++slot) {
            fprintf(stream, "%s\n    {\"dll\": \"", first ? "" : ",");
            first = 0;
            rsf_json_ascii(stream, library, 260);
            fputs("\", ", stream);

            if (names[slot] & IMAGE_ORDINAL_FLAG64) {
                fprintf(stream, "\"ordinal\": %llu", (unsigned long long)(names[slot] & 0xFFFFu));
            } else {
                const IMAGE_IMPORT_BY_NAME* by_name =
                    (const IMAGE_IMPORT_BY_NAME*)(base + (DWORD)names[slot]);
                fputs("\"name\": \"", stream);
                rsf_json_ascii(stream, by_name->Name, 260);
                fputc('"', stream);
            }

            const DWORD slot_rva =
                descriptor->FirstThunk + (DWORD)(slot * sizeof(ULONGLONG));
            fprintf(stream, ", \"slot_rva\": \"0x%lx\", \"value\": \"0x%llx\"",
                    (unsigned long)slot_rva, (unsigned long long)values[slot]);

            const rsf_module_entry* owner =
                module_for(modules, module_count, (const unsigned char*)(uintptr_t)values[slot]);
            if (owner) {
                fputs(", \"resolves_into\": \"", stream);
                rsf_json_wide(stream, owner->path);
                fputc('"', stream);
            }
            fputc('}', stream);
            ++(*described);
        }
    }
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
    describe_imports(stream, base, headers, modules, module_count, &report->imports_described);
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
    if (GetModuleFileNameW((HMODULE)(void*)base, module_path, MAX_PATH) == 0) {
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
    _snwprintf(dump_path, MAX_PATH * 2, L"%s\\%s.dump", directory, name);
    _snwprintf(sidecar_path, MAX_PATH * 2, L"%s\\%s.dump.json", directory, name);

    const DWORD alignment = headers->OptionalHeader.SectionAlignment;
    const DWORD header_size = align_up(headers->OptionalHeader.SizeOfHeaders, alignment);
    unsigned char* header_copy = (unsigned char*)malloc(header_size);
    if (!header_copy) {
        return RSF_DUMP_ERROR_WRITE_FAILED;
    }
    rsf_dump_read_guarded(base, header_copy, header_size);

    IMAGE_NT_HEADERS64* copied_headers =
        (IMAGE_NT_HEADERS64*)(header_copy + ((const IMAGE_DOS_HEADER*)header_copy)->e_lfanew);
    IMAGE_SECTION_HEADER* copied_sections = IMAGE_FIRST_SECTION(copied_headers);
    const IMAGE_SECTION_HEADER* sections = IMAGE_FIRST_SECTION(headers);

    /* Point every section at its own virtual address so file offset equals RVA. */
    for (WORD index = 0; index < headers->FileHeader.NumberOfSections; ++index) {
        DWORD size = sections[index].Misc.VirtualSize;
        if (size == 0) {
            size = sections[index].SizeOfRawData;
        }
        copied_sections[index].PointerToRawData = sections[index].VirtualAddress;
        copied_sections[index].SizeOfRawData = align_up(size, alignment);
        /* These are file offsets into a layout that no longer exists. */
        copied_sections[index].PointerToRelocations = 0;
        copied_sections[index].PointerToLinenumbers = 0;
        copied_sections[index].NumberOfRelocations = 0;
        copied_sections[index].NumberOfLinenumbers = 0;
        if (copied_sections[index].Characteristics & IMAGE_SCN_CNT_UNINITIALIZED_DATA) {
            /* The section has real bytes in the dump even if it had none in the original file. */
            copied_sections[index].Characteristics &= ~(DWORD)IMAGE_SCN_CNT_UNINITIALIZED_DATA;
            copied_sections[index].Characteristics |= IMAGE_SCN_CNT_INITIALIZED_DATA;
        }
    }

    copied_headers->OptionalHeader.FileAlignment = alignment;
    copied_headers->OptionalHeader.SizeOfHeaders = header_size;
    /* Record where the image actually lives, so an analysis tool's addresses match the process. */
    copied_headers->OptionalHeader.ImageBase = (ULONGLONG)(uintptr_t)base;
    copied_headers->OptionalHeader.DllCharacteristics &=
        (WORD)~IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE;
    copied_headers->OptionalHeader.CheckSum = 0;
    /* The certificate directory is a file offset rather than an RVA, so after this relayout it
       would point at unrelated bytes. */
    copied_headers->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_SECURITY].VirtualAddress = 0;
    copied_headers->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_SECURITY].Size = 0;

    size_t slot_count = 0;
    DWORD* slots = collect_iat_slots(base, headers, &slot_count);

    FILE* stream = _wfopen(dump_path, L"wb");
    if (!stream) {
        free(header_copy);
        return RSF_DUMP_ERROR_WRITE_FAILED;
    }
    size_t written = fwrite(header_copy, 1, header_size, stream);
    free(header_copy);

    unsigned char* buffer = NULL;
    size_t buffer_size = 0;
    rsf_section_digest* digests = (rsf_section_digest*)calloc(
        headers->FileHeader.NumberOfSections ? headers->FileHeader.NumberOfSections : 1u,
        sizeof(rsf_section_digest));
    double hash_ms = 0.0;
    for (WORD index = 0; index < headers->FileHeader.NumberOfSections; ++index) {
        DWORD size = sections[index].Misc.VirtualSize;
        if (size == 0) {
            size = sections[index].SizeOfRawData;
        }
        if (size == 0) {
            continue;
        }
        if (size > buffer_size) {
            unsigned char* grown = (unsigned char*)realloc(buffer, size);
            if (!grown) {
                free(buffer);
                free(digests);
                free(slots);
                fclose(stream);
                return RSF_DUMP_ERROR_WRITE_FAILED;
            }
            buffer = grown;
            buffer_size = size;
        }
        const size_t readable_bytes =
            rsf_dump_read_guarded(base + sections[index].VirtualAddress, buffer, size);
        if (digests) {
            const double hash_started = rsf_dump_now_ms();
            digests[index].valid = 1;
            digests[index].dumped_size = size;
            digests[index].readable_bytes = readable_bytes;
            digests[index].entropy =
                rsf_sampled_entropy(buffer, size, RSF_ENTROPY_WINDOW, RSF_ENTROPY_SAMPLES);
            rsf_sha256_hex(buffer, size, digests[index].sha256);
            hash_ms += rsf_dump_now_ms() - hash_started;
        }
        if (sections[index].Characteristics & IMAGE_SCN_MEM_EXECUTE) {
            local.iat_references += count_iat_references(
                buffer, size, sections[index].VirtualAddress, slots, slot_count, NULL);
        }
        if (fseek(stream, (long)sections[index].VirtualAddress, SEEK_SET) != 0) {
            free(buffer);
            free(digests);
            free(slots);
            fclose(stream);
            return RSF_DUMP_ERROR_WRITE_FAILED;
        }
        written += fwrite(buffer, 1, size, stream);
        ++local.sections_written;
    }
    free(buffer);
    free(slots);
    const int failed = ferror(stream);
    fclose(stream);
    if (failed) {
        free(digests);
        return RSF_DUMP_ERROR_WRITE_FAILED;
    }
    local.bytes_written = (uint64_t)written;

    rsf_module_entry* modules =
        (rsf_module_entry*)calloc(RSF_MAX_MODULES, sizeof(rsf_module_entry));
    size_t module_count = 0;
    if (modules) {
        rsf_dump_collect_modules(modules, RSF_MAX_MODULES, &module_count);
    }
    local.modules_listed = (uint32_t)module_count;
    const rsf_dump_result sidecar =
        write_sidecar(sidecar_path, base, headers, module_path, modules, module_count, options,
                      digests, hash_ms, started_ms, &local);
    free(modules);
    free(digests);

    copy_report(report, &local);
    return sidecar;
}
