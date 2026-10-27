/* SPDX-License-Identifier: GPL-3.0-only */
/* Structured sidecar sections: guarded memory access, threads, function starts from the exception
   directory and the caller-supplied name list. RTTI lives in module_dump_rtti.c. */

#include "module_dump_internal.h"

#include <stdlib.h>
#include <string.h>
#include <tlhelp32.h>
#include <wchar.h>

#define RSF_MAX_THREADS 4096u
#define RSF_MAX_PDATA_BYTES (96u * 1024u * 1024u)
#define RSF_MAX_NAME_LINES 2000000u
#define RSF_MAX_NAME_BYTES 1024u

/* ---- guarded memory ---- */

static int region_usable(const MEMORY_BASIC_INFORMATION* region)
{
    const DWORD readable = PAGE_READONLY | PAGE_READWRITE | PAGE_WRITECOPY | PAGE_EXECUTE_READ |
                           PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY;
    return region->State == MEM_COMMIT && (region->Protect & readable) != 0 &&
           (region->Protect & PAGE_GUARD) == 0 && (region->Protect & PAGE_NOACCESS) == 0;
}

size_t rsf_dump_readable_extent(rsf_mem_cache* cache, const void* address, size_t wanted)
{
    const unsigned char* cursor = (const unsigned char*)address;
    size_t total = 0;
    while (total < wanted) {
        if (!(cache->low <= cursor && cursor < cache->high)) {
            MEMORY_BASIC_INFORMATION region;
            if (VirtualQuery(cursor, &region, sizeof(region)) != sizeof(region) ||
                !region_usable(&region)) {
                cache->low = cache->high = NULL;
                break;
            }
            cache->low = (const unsigned char*)region.BaseAddress;
            cache->high = cache->low + region.RegionSize;
            if (!(cache->low <= cursor && cursor < cache->high)) {
                cache->low = cache->high = NULL;
                break;
            }
        }
        size_t chunk = (size_t)(cache->high - cursor);
        if (chunk > wanted - total) {
            chunk = wanted - total;
        }
        total += chunk;
        cursor += chunk;
    }
    return total;
}

int rsf_dump_read_exact(rsf_mem_cache* cache, const void* address, void* destination, size_t size)
{
    if (rsf_dump_readable_extent(cache, address, size) < size) {
        return 0;
    }
    memcpy(destination, address, size);
    return 1;
}

/* Copy from another module's memory, tolerating pages that are not committed or not readable.
   A section's virtual size routinely exceeds what was loaded from disk, and reading blindly
   would fault. Unreadable bytes become zeroes, which is what those pages hold anyway. */
size_t rsf_dump_read_guarded(const unsigned char* source, unsigned char* destination, size_t size)
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
        if (region_usable(&region)) {
            memcpy(destination + offset, source + offset, available);
            copied += available;
        }
        offset += available;
    }
    return copied;
}

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

/* UTF-8 text becomes \u escapes of its UTF-16 units. Bytes that are not valid UTF-8 fall back to
   one escape per byte, which keeps the JSON valid. */
void rsf_json_utf8(FILE* stream, const char* text)
{
    wchar_t wide[RSF_MAX_NAME_BYTES + 1];
    const int length = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, text, -1, wide,
                                           (int)(sizeof(wide) / sizeof(wide[0])));
    if (length > 0) {
        rsf_json_wide(stream, wide);
    } else {
        rsf_json_ascii(stream, text, RSF_MAX_NAME_BYTES);
    }
}

double rsf_dump_now_ms(void)
{
    LARGE_INTEGER frequency;
    LARGE_INTEGER counter;
    if (!QueryPerformanceFrequency(&frequency) || frequency.QuadPart == 0) {
        return (double)GetTickCount64();
    }
    QueryPerformanceCounter(&counter);
    return (double)counter.QuadPart * 1000.0 / (double)frequency.QuadPart;
}

const wchar_t* rsf_dump_basename(const wchar_t* path)
{
    const wchar_t* leaf = wcsrchr(path, L'\\');
    return leaf ? leaf + 1 : path;
}

static void write_elapsed(FILE* stream, double started)
{
    fprintf(stream, "\"elapsed_ms\": %lu", (unsigned long)(rsf_dump_now_ms() - started + 0.5));
}

/* ---- threads ---- */

typedef LONG(NTAPI* nt_query_information_thread_fn)(HANDLE, ULONG, PVOID, ULONG, PULONG);
typedef HRESULT(WINAPI* get_thread_description_fn)(HANDLE, PWSTR*);

#define RSF_THREAD_QUERY_WIN32_START_ADDRESS 9u

uint32_t rsf_dump_write_threads(FILE* stream, const rsf_module_entry* modules,
                                size_t module_count)
{
    const double started = rsf_dump_now_ms();
    HMODULE ntdll = GetModuleHandleW(L"ntdll.dll");
    HMODULE kernel = GetModuleHandleW(L"kernel32.dll");
    nt_query_information_thread_fn query =
        ntdll ? (nt_query_information_thread_fn)(void*)GetProcAddress(ntdll,
                                                                       "NtQueryInformationThread")
              : NULL;
    get_thread_description_fn describe =
        kernel ? (get_thread_description_fn)(void*)GetProcAddress(kernel, "GetThreadDescription")
               : NULL;

    fputs("  \"threads\": {\"list\": [", stream);
    uint32_t listed = 0;
    int truncated = 0;
    const DWORD process = GetCurrentProcessId();
    const DWORD current = GetCurrentThreadId();
    HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
    if (snapshot != INVALID_HANDLE_VALUE) {
        THREADENTRY32 entry;
        entry.dwSize = sizeof(entry);
        if (Thread32First(snapshot, &entry)) {
            do {
                if (entry.th32OwnerProcessID != process) {
                    continue;
                }
                if (listed >= RSF_MAX_THREADS) {
                    truncated = 1;
                    break;
                }
                HANDLE thread = OpenThread(THREAD_QUERY_INFORMATION | THREAD_QUERY_LIMITED_INFORMATION,
                                           FALSE, entry.th32ThreadID);
                if (!thread) {
                    thread = OpenThread(THREAD_QUERY_LIMITED_INFORMATION, FALSE, entry.th32ThreadID);
                }

                uint64_t start = 0;
                if (thread && query) {
                    PVOID address = NULL;
                    if (query(thread, RSF_THREAD_QUERY_WIN32_START_ADDRESS, &address,
                              (ULONG)sizeof(address), NULL) >= 0) {
                        start = (uint64_t)(uintptr_t)address;
                    }
                }

                fprintf(stream, "%s\n    {\"tid\": %lu, \"dumping\": %s, \"start_address\": \"0x%llx\"",
                        listed ? "," : "", (unsigned long)entry.th32ThreadID,
                        entry.th32ThreadID == current ? "true" : "false",
                        (unsigned long long)start);

                const rsf_module_entry* owner = NULL;
                for (size_t index = 0; index < module_count && start; ++index) {
                    if ((const unsigned char*)(uintptr_t)start >= modules[index].base &&
                        (const unsigned char*)(uintptr_t)start <
                            modules[index].base + modules[index].size) {
                        owner = &modules[index];
                        break;
                    }
                }
                if (owner) {
                    fputs(", \"start_module\": \"", stream);
                    rsf_json_wide(stream, rsf_dump_basename(owner->path));
                    fprintf(stream, "\", \"start_rva\": \"0x%llx\"",
                            (unsigned long long)(start - (uint64_t)(uintptr_t)owner->base));
                }

                if (thread && describe) {
                    PWSTR text = NULL;
                    if (SUCCEEDED(describe(thread, &text)) && text) {
                        if (text[0]) {
                            fputs(", \"description\": \"", stream);
                            rsf_json_wide(stream, text);
                            fputc('"', stream);
                        }
                        LocalFree(text);
                    }
                }
                if (thread) {
                    const int priority = GetThreadPriority(thread);
                    if (priority != (int)THREAD_PRIORITY_ERROR_RETURN) {
                        fprintf(stream, ", \"priority\": %d", priority);
                    }
                    CloseHandle(thread);
                }
                fprintf(stream, ", \"base_priority\": %ld}", (long)entry.tpBasePri);
                ++listed;
            } while (Thread32Next(snapshot, &entry));
        }
        CloseHandle(snapshot);
    }
    fprintf(stream, "\n  ], \"count\": %lu, \"truncated\": %s, \"start_address_source\": \"%s\", ",
            (unsigned long)listed, truncated ? "true" : "false",
            query ? "NtQueryInformationThread(ThreadQuerySetWin32StartAddress)" : "unavailable");
    write_elapsed(stream, started);
    fputs("},\n", stream);
    return listed;
}

/* ---- function starts ---- */

typedef struct pdata_entry {
    uint32_t begin;
    uint32_t end;
    uint32_t unwind;
    uint32_t chained;
} pdata_entry;

static int compare_pdata(const void* left, const void* right)
{
    const pdata_entry* a = (const pdata_entry*)left;
    const pdata_entry* b = (const pdata_entry*)right;
    if (a->begin != b->begin) {
        return a->begin < b->begin ? -1 : 1;
    }
    /* Unchained first, so the surviving duplicate is the real function start. */
    if (a->chained != b->chained) {
        return a->chained < b->chained ? -1 : 1;
    }
    return a->end < b->end ? -1 : (a->end > b->end ? 1 : 0);
}

void rsf_dump_write_functions(FILE* stream, const unsigned char* base,
                              const IMAGE_NT_HEADERS64* headers, uint32_t* function_count)
{
    const double started = rsf_dump_now_ms();
    const IMAGE_DATA_DIRECTORY* directory =
        &headers->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_EXCEPTION];
    const uint32_t image_size = headers->OptionalHeader.SizeOfImage;
    const char* reason = NULL;
    uint32_t raw = 0;
    uint32_t invalid = 0;
    uint32_t chained_count = 0;
    uint32_t unique = 0;
    pdata_entry* entries = NULL;
    rsf_mem_cache cache = {NULL, NULL};

    if (directory->VirtualAddress == 0 || directory->Size < sizeof(RUNTIME_FUNCTION)) {
        reason = "the module has no exception directory";
    } else if (directory->Size > RSF_MAX_PDATA_BYTES ||
               (uint64_t)directory->VirtualAddress + directory->Size > image_size) {
        reason = "the exception directory lies outside the image or exceeds the size bound";
    } else {
        const size_t wanted = directory->Size / sizeof(RUNTIME_FUNCTION);
        const size_t readable =
            rsf_dump_readable_extent(&cache, base + directory->VirtualAddress,
                                     wanted * sizeof(RUNTIME_FUNCTION)) /
            sizeof(RUNTIME_FUNCTION);
        entries = (pdata_entry*)malloc((readable ? readable : 1) * sizeof(pdata_entry));
        if (!entries) {
            reason = "allocation failed";
        } else {
            if (readable < wanted) {
                reason = "part of the exception directory is not readable";
            }
            const RUNTIME_FUNCTION* table = (const RUNTIME_FUNCTION*)(base + directory->VirtualAddress);
            for (size_t index = 0; index < readable; ++index) {
                RUNTIME_FUNCTION item;
                memcpy(&item, &table[index], sizeof(item));
                ++raw;
                if (item.BeginAddress == 0 || item.EndAddress <= item.BeginAddress ||
                    item.EndAddress > image_size) {
                    ++invalid;
                    continue;
                }
                uint32_t chained = 0;
                const uint32_t unwind = item.UnwindData & ~1u;
                unsigned char flags = 0;
                if (!(item.UnwindData & 1u) && unwind < image_size &&
                    rsf_dump_read_exact(&cache, base + unwind, &flags, 1)) {
                    /* UNW_FLAG_CHAININFO marks a split-off part of another function, not a
                       function start of its own. */
                    chained = (flags >> 3) & 4u ? 1u : 0u;
                }
                entries[unique].begin = item.BeginAddress;
                entries[unique].end = item.EndAddress;
                entries[unique].unwind = item.UnwindData;
                entries[unique].chained = chained;
                ++unique;
            }
        }
    }

    fputs("  \"functions\": {\"entries\": [", stream);
    uint32_t written = 0;
    if (entries) {
        qsort(entries, unique, sizeof(pdata_entry), compare_pdata);
        for (uint32_t index = 0; index < unique; ++index) {
            if (index && entries[index].begin == entries[index - 1].begin) {
                continue;
            }
            fprintf(stream, "%s\n    [%lu,%lu,%lu,%lu]", written ? "," : "",
                    (unsigned long)entries[index].begin, (unsigned long)entries[index].end,
                    (unsigned long)entries[index].unwind, (unsigned long)entries[index].chained);
            chained_count += entries[index].chained;
            ++written;
        }
    }
    free(entries);
    if (function_count) {
        *function_count = written;
    }
    fprintf(stream,
            "\n  ], \"format\": \"[begin_rva, end_rva, unwind_rva, chained] as decimal integers\", "
            "\"source\": \"exception directory (.pdata RUNTIME_FUNCTION)\", "
            "\"raw_entries\": %lu, \"invalid\": %lu, \"count\": %lu, \"chained\": %lu, ",
            (unsigned long)raw, (unsigned long)invalid, (unsigned long)written,
            (unsigned long)chained_count);
    if (reason) {
        fprintf(stream, "\"reason\": \"%s\", ", reason);
    }
    write_elapsed(stream, started);
    fputs("},\n", stream);
}

/* ---- caller-supplied names ---- */

static int parse_rva(const char* text, uint64_t* value)
{
    char* end = NULL;
    const int hex = text[0] == '0' && (text[1] == 'x' || text[1] == 'X');
    const unsigned long long parsed = strtoull(hex ? text + 2 : text, &end, hex ? 16 : 10);
    if (end == text || (hex && end == text + 2)) {
        return 0;
    }
    while (*end == ' ') {
        ++end;
    }
    if (*end != '\0' || parsed > 0xFFFFFFFFull) {
        return 0;
    }
    *value = parsed;
    return 1;
}

void rsf_dump_write_names(FILE* stream, const char* path_utf8)
{
    fputs("  \"names\": {\"entries\": [", stream);
    uint32_t accepted = 0;
    uint32_t skipped = 0;
    const char* reason = NULL;

    FILE* input = NULL;
    if (path_utf8) {
        wchar_t path[MAX_PATH * 2];
        if (MultiByteToWideChar(CP_UTF8, 0, path_utf8, -1, path, MAX_PATH * 2) == 0) {
            reason = "the name list path is not valid UTF-8";
        } else {
            input = _wfopen(path, L"rb");
            if (!input) {
                reason = "the name list could not be opened";
            }
        }
    }

    if (input) {
        char line[RSF_MAX_NAME_BYTES + 64];
        uint32_t lines = 0;
        while (fgets(line, (int)sizeof(line), input)) {
            if (++lines > RSF_MAX_NAME_LINES) {
                reason = "the name list exceeds the line bound";
                break;
            }
            size_t length = strlen(line);
            while (length && (line[length - 1] == '\n' || line[length - 1] == '\r')) {
                line[--length] = '\0';
            }
            if (lines == 1 && (unsigned char)line[0] == 0xEF && (unsigned char)line[1] == 0xBB &&
                (unsigned char)line[2] == 0xBF) {
                memmove(line, line + 3, length - 2);
            }
            if (line[0] == '\0' || line[0] == '#') {
                continue;
            }
            char* tab = strchr(line, '\t');
            uint64_t rva = 0;
            if (!tab) {
                ++skipped;
                continue;
            }
            *tab = '\0';
            const char* name = tab + 1;
            if (*name == '\0' || !parse_rva(line, &rva)) {
                ++skipped;
                continue;
            }
            fprintf(stream, "%s\n    {\"rva\": \"0x%llx\", \"name\": \"", accepted ? "," : "",
                    (unsigned long long)rva);
            rsf_json_utf8(stream, name);
            fputs("\"}", stream);
            ++accepted;
        }
        fclose(input);
    }

    fputs("\n  ], \"source\": ", stream);
    if (path_utf8) {
        fputc('"', stream);
        rsf_json_utf8(stream, path_utf8);
        fputc('"', stream);
    } else {
        fputs("null", stream);
    }
    fprintf(stream, ", \"count\": %lu, \"skipped_lines\": %lu", (unsigned long)accepted,
            (unsigned long)skipped);
    if (reason) {
        fprintf(stream, ", \"reason\": \"%s\"", reason);
    }
    fputs("},\n", stream);
}
