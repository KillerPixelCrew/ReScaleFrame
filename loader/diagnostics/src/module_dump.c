/* SPDX-License-Identifier: GPL-3.0-only */
/* Read a loaded module out of its own address space and write it as a file whose raw offsets
   equal its virtual addresses, so the result maps one to one onto runtime addresses.

   The dump is for analysis and is never meant to run again. Nothing here repairs the entry point
   or rebuilds an import address table. */

#include <rescaleframe/module_dump.h>

#include <windows.h>
#include <tlhelp32.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

#define RSF_ENTROPY_WINDOW 65536u
#define RSF_ENTROPY_SAMPLES 16u

typedef struct rsf_module_entry {
    wchar_t path[MAX_PATH];
    const unsigned char* base;
    size_t size;
} rsf_module_entry;

#define RSF_MAX_MODULES 384

static const IMAGE_NT_HEADERS64* nt_headers(const void* base)
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

/* Copy from another module's memory, tolerating pages that are not committed or not readable.
   A section's virtual size routinely exceeds what was loaded from disk, and reading blindly
   would fault. Unreadable bytes become zeroes, which is what those pages hold anyway. */
static size_t read_guarded(const unsigned char* source, unsigned char* destination, size_t size)
{
    SYSTEM_INFO info;
    GetSystemInfo(&info);
    const size_t page = info.dwPageSize ? (size_t)info.dwPageSize : 4096u;

    memset(destination, 0, size);
    size_t copied = 0;
    size_t offset = 0;
    while (offset < size) {
        MEMORY_BASIC_INFORMATION region;
        if (VirtualQuery(source + offset, &region, sizeof(region)) != sizeof(region)) {
            break;
        }
        /* Measure from the region's end. The region can start before the address we asked about,
           so subtracting its base from `source` can underflow. */
        const unsigned char* region_end =
            (const unsigned char*)region.BaseAddress + region.RegionSize;
        size_t available = (size_t)(region_end - (source + offset));
        if (available > size - offset) {
            available = size - offset;
        }
        if (available == 0) {
            available = page;
            if (available > size - offset) {
                available = size - offset;
            }
        }

        const DWORD readable = PAGE_READONLY | PAGE_READWRITE | PAGE_WRITECOPY | PAGE_EXECUTE_READ |
                               PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY;
        const int usable = region.State == MEM_COMMIT && (region.Protect & readable) != 0 &&
                           (region.Protect & PAGE_GUARD) == 0;
        if (usable) {
            memcpy(destination + offset, source + offset, available);
            copied += available;
        }
        offset += available;
    }
    return copied;
}

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

static const void* resolve_base(const void* module_base)
{
    return module_base ? module_base : (const void*)GetModuleHandleW(NULL);
}

rsf_dump_result rsf_measure_module_code(const void* module_base, double* entropy)
{
    if (!entropy) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }
    const unsigned char* base = (const unsigned char*)resolve_base(module_base);
    const IMAGE_NT_HEADERS64* headers = nt_headers(base);
    if (!headers) {
        return RSF_DUMP_ERROR_NOT_A_PE;
    }

    /* The section holding the entry point is the one that has to be readable for analysis to be
       worth anything. On a protected build that is the packer stub, so also consider the largest
       executable section, which is where the real code lives. */
    const IMAGE_SECTION_HEADER* sections = IMAGE_FIRST_SECTION(headers);
    const IMAGE_SECTION_HEADER* chosen = NULL;
    DWORD largest = 0;
    for (WORD index = 0; index < headers->FileHeader.NumberOfSections; ++index) {
        if ((sections[index].Characteristics & IMAGE_SCN_MEM_EXECUTE) == 0) {
            continue;
        }
        const DWORD size = sections[index].Misc.VirtualSize;
        if (size > largest) {
            largest = size;
            chosen = &sections[index];
        }
    }
    if (!chosen) {
        return RSF_DUMP_ERROR_NOT_A_PE;
    }

    const size_t size = (size_t)chosen->Misc.VirtualSize;
    unsigned char* buffer = (unsigned char*)malloc(size < RSF_ENTROPY_WINDOW ? RSF_ENTROPY_WINDOW : size);
    if (!buffer) {
        return RSF_DUMP_ERROR_WRITE_FAILED;
    }
    read_guarded(base + chosen->VirtualAddress, buffer, size);
    *entropy = rsf_sampled_entropy(buffer, size, RSF_ENTROPY_WINDOW, RSF_ENTROPY_SAMPLES);
    free(buffer);
    return RSF_DUMP_OK;
}

static void collect_modules(rsf_module_entry* entries, size_t capacity, size_t* count)
{
    *count = 0;
    HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, 0);
    if (snapshot == INVALID_HANDLE_VALUE) {
        return;
    }
    MODULEENTRY32W entry;
    entry.dwSize = sizeof(entry);
    if (Module32FirstW(snapshot, &entry)) {
        do {
            if (*count >= capacity) {
                break;
            }
            rsf_module_entry* target = &entries[*count];
            wcsncpy(target->path, entry.szExePath, MAX_PATH - 1);
            target->path[MAX_PATH - 1] = L'\0';
            target->base = (const unsigned char*)entry.modBaseAddr;
            target->size = (size_t)entry.modBaseSize;
            ++(*count);
        } while (Module32NextW(snapshot, &entry));
    }
    CloseHandle(snapshot);
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

static void write_json_wide(FILE* stream, const wchar_t* text)
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

static void write_json_ascii(FILE* stream, const char* text, size_t limit)
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
            write_json_ascii(stream, library, 260);
            fputs("\", ", stream);

            if (names[slot] & IMAGE_ORDINAL_FLAG64) {
                fprintf(stream, "\"ordinal\": %llu", (unsigned long long)(names[slot] & 0xFFFFu));
            } else {
                const IMAGE_IMPORT_BY_NAME* by_name =
                    (const IMAGE_IMPORT_BY_NAME*)(base + (DWORD)names[slot]);
                fputs("\"name\": \"", stream);
                write_json_ascii(stream, by_name->Name, 260);
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
                write_json_wide(stream, owner->path);
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
                                     rsf_dump_report* report)
{
    FILE* stream = _wfopen(path, L"wb");
    if (!stream) {
        return RSF_DUMP_ERROR_WRITE_FAILED;
    }

    fputs("{\n  \"tool\": \"rescaleframe-module-dump\",\n", stream);
    fprintf(stream, "  \"abi_version\": %u,\n", RSF_MODULE_DUMP_ABI_VERSION);
    fputs("  \"module\": {\"path\": \"", stream);
    write_json_wide(stream, module_path);
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
        write_json_ascii(stream, (const char*)sections[index].Name, IMAGE_SIZEOF_SHORT_NAME);
        fprintf(stream,
                "\", \"rva\": \"0x%lx\", \"virtual_size\": %lu, \"characteristics\": \"0x%lx\"}",
                (unsigned long)sections[index].VirtualAddress,
                (unsigned long)sections[index].Misc.VirtualSize,
                (unsigned long)sections[index].Characteristics);
    }
    fputs("\n  ],\n", stream);

    fputs("  \"modules\": [", stream);
    for (size_t index = 0; index < module_count; ++index) {
        fprintf(stream, "%s\n    {\"path\": \"", index ? "," : "");
        write_json_wide(stream, modules[index].path);
        fprintf(stream, "\", \"base\": \"0x%llx\", \"size\": %llu}",
                (unsigned long long)(uintptr_t)modules[index].base,
                (unsigned long long)modules[index].size);
    }
    fputs("\n  ],\n", stream);

    fputs("  \"imports\": [", stream);
    describe_imports(stream, base, headers, modules, module_count, &report->imports_described);
    fputs("\n  ],\n", stream);

    fputs("  \"limits\": \"Section raw offsets equal virtual addresses, so the dump maps one to "
          "one onto runtime addresses. It is not runnable: the entry point is unchanged and no "
          "import table was rebuilt. Import values are the addresses resolved in this process at "
          "dump time.\"\n}\n", stream);

    const int failed = ferror(stream);
    fclose(stream);
    return failed ? RSF_DUMP_ERROR_WRITE_FAILED : RSF_DUMP_OK;
}

rsf_dump_result rsf_dump_module(const void* module_base, const rsf_dump_options* options,
                                rsf_dump_report* report)
{
    if (!options || options->struct_size < sizeof(rsf_dump_options) ||
        !options->output_directory_utf8) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }
    if (options->abi_version != RSF_MODULE_DUMP_ABI_VERSION) {
        return RSF_DUMP_ERROR_ABI_MISMATCH;
    }
    if (report && report->struct_size < sizeof(rsf_dump_report)) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }

    const unsigned char* base = (const unsigned char*)resolve_base(module_base);
    const IMAGE_NT_HEADERS64* headers = nt_headers(base);
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
        if (report) {
            const uint32_t size = report->struct_size;
            memcpy(report, &local, sizeof(local));
            report->struct_size = size;
        }
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
    read_guarded(base, header_copy, header_size);

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
                fclose(stream);
                return RSF_DUMP_ERROR_WRITE_FAILED;
            }
            buffer = grown;
            buffer_size = size;
        }
        read_guarded(base + sections[index].VirtualAddress, buffer, size);
        if (sections[index].Characteristics & IMAGE_SCN_MEM_EXECUTE) {
            local.iat_references += count_iat_references(
                buffer, size, sections[index].VirtualAddress, slots, slot_count, NULL);
        }
        if (fseek(stream, (long)sections[index].VirtualAddress, SEEK_SET) != 0) {
            free(buffer);
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
        return RSF_DUMP_ERROR_WRITE_FAILED;
    }
    local.bytes_written = (uint64_t)written;

    rsf_module_entry* modules =
        (rsf_module_entry*)calloc(RSF_MAX_MODULES, sizeof(rsf_module_entry));
    size_t module_count = 0;
    if (modules) {
        collect_modules(modules, RSF_MAX_MODULES, &module_count);
    }
    local.modules_listed = (uint32_t)module_count;
    const rsf_dump_result sidecar =
        write_sidecar(sidecar_path, base, headers, module_path, modules, module_count, &local);
    free(modules);

    if (report) {
        const uint32_t size = report->struct_size;
        memcpy(report, &local, sizeof(local));
        report->struct_size = size;
    }
    return sidecar;
}

rsf_dump_result rsf_patch_code(uint32_t rva, const uint8_t* bytes, uint32_t count,
                               const uint8_t* expected, uint32_t expected_count,
                               uint8_t* previous)
{
    if (!bytes || count == 0) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }
    unsigned char* base = (unsigned char*)resolve_base(NULL);
    const IMAGE_NT_HEADERS64* headers = nt_headers(base);
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

rsf_dump_result rsf_console_set_float(const char* name_utf8, float expected_current,
                                      float new_value, uint32_t singleton_rva, uint32_t find_slot,
                                      uint32_t* found_offset)
{
    if (!name_utf8 || singleton_rva == 0) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }
    unsigned char* base = (unsigned char*)resolve_base(NULL);
    const IMAGE_NT_HEADERS64* headers = nt_headers(base);
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
    unsigned char* base = (unsigned char*)resolve_base(NULL);
    const IMAGE_NT_HEADERS64* headers = nt_headers(base);
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
    collect_modules(modules, RSF_MAX_MODULES, &count);
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

rsf_dump_result rsf_dump_when_decrypted(const rsf_dump_options* options, uint32_t poll_interval_ms,
                                        uint32_t timeout_ms, uint32_t stable_samples,
                                        rsf_dump_report* report)
{
    if (!options || poll_interval_ms == 0 || stable_samples == 0) {
        return RSF_DUMP_ERROR_INVALID_ARGUMENT;
    }

    const DWORD deadline = GetTickCount() + timeout_ms;
    uint32_t stable = 0;
    for (;;) {
        double entropy = 0.0;
        const rsf_dump_result measured = rsf_measure_module_code(NULL, &entropy);
        if (measured != RSF_DUMP_OK) {
            return measured;
        }
        stable = entropy < options->entropy_threshold ? stable + 1 : 0;
        if (stable >= stable_samples) {
            return rsf_dump_module(NULL, options, report);
        }
        if (timeout_ms != 0 && (LONG)(GetTickCount() - deadline) >= 0) {
            if (report && report->struct_size >= sizeof(rsf_dump_report)) {
                report->code_entropy = entropy;
            }
            return RSF_DUMP_ERROR_STILL_ENCRYPTED;
        }
        Sleep(poll_interval_ms);
    }
}
