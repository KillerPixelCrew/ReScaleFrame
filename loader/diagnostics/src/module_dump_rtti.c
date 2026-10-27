/* SPDX-License-Identifier: GPL-3.0-only */
/* MSVC x64 RTTI recovery from a loaded image.

   Every polymorphic class has a vtable whose slot at index -1 points at an
   RTTICompleteObjectLocator. On x64 the locator stores image-relative offsets (signature 1) and
   its pSelf field holds its own RVA, which is what makes a byte scan reliable: a qword that points
   into a data section, at a structure whose first dword is 1 and whose pSelf names itself, is
   essentially never a coincidence.

   Locator (24 bytes): signature, offset, cdOffset, pTypeDescriptor, pClassDescriptor, pSelf.
   Class hierarchy descriptor (16): signature, attributes, numBaseClasses, pBaseClassArray.
   Base class descriptor (28): pTypeDescriptor, numContainedBases, PMD{mdisp, pdisp, vdisp},
   attributes, pClassDescriptor. Type descriptor: pVFTable, spare, then the decorated name. */

#include "module_dump_internal.h"

#include <stdlib.h>
#include <string.h>

#define RSF_RTTI_DEFAULT_BUDGET_MS 20000u
#define RSF_RTTI_MAX_CLASSES 400000u
#define RSF_RTTI_MAX_BASES 512u
#define RSF_RTTI_NAME_BYTES 2048u
#define RSF_RTTI_CACHE_SLOTS 4096u
#define RSF_RTTI_PRETTY_BYTES 512u

typedef struct locator {
    uint32_t signature;
    int32_t offset;
    uint32_t cd_offset;
    uint32_t type_descriptor;
    uint32_t class_descriptor;
    uint32_t self;
} locator;

typedef struct name_slot {
    uint32_t rva; /* 0 means empty; a type descriptor never sits at RVA 0 */
    char pretty[RSF_RTTI_PRETTY_BYTES];
} name_slot;

typedef struct scan {
    const unsigned char* base;
    uint32_t image_size;
    rsf_mem_cache cache;
    const IMAGE_SECTION_HEADER* sections;
    WORD section_count;
    name_slot* names;
    uint32_t classes;
    FILE* stream;
    double deadline_ms;
    int out_of_time;
    int out_of_classes;
} scan;

typedef DWORD(WINAPI* undecorate_fn)(const char*, char*, DWORD, DWORD);
static undecorate_fn undecorate_symbol;
static int undecorate_tried;

static void load_undecorator(void)
{
    if (undecorate_tried) {
        return;
    }
    undecorate_tried = 1;
    /* Templates and anonymous scopes need the full MSVC grammar; dbghelp already has it. System32
       only, so a planted copy next to the game is never loaded. */
    HMODULE library = LoadLibraryExW(L"dbghelp.dll", NULL, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (library) {
        undecorate_symbol = (undecorate_fn)(void*)GetProcAddress(library, "UnDecorateSymbolName");
    }
}

/* Turn ".?AVBar@Foo@@" into "Foo::Bar". Anything the quick path cannot read goes to dbghelp, and
   the decorated text is the last resort so a name is never lost. */
static void pretty_name(const char* decorated, char* out, size_t capacity)
{
    out[0] = '\0';
    const int is_class = strncmp(decorated, ".?AV", 4) == 0 || strncmp(decorated, ".?AU", 4) == 0;
    if (is_class && !strchr(decorated + 4, '?')) {
        const char* body = decorated + 4;
        const char* end = strstr(body, "@@");
        size_t length = end ? (size_t)(end - body) : strlen(body);
        size_t used = 0;
        while (length > 0) {
            size_t start = length;
            while (start > 0 && body[start - 1] != '@') {
                --start;
            }
            const size_t part = length - start;
            if (used + part + 3 >= capacity) {
                break;
            }
            if (used) {
                out[used++] = ':';
                out[used++] = ':';
            }
            memcpy(out + used, body + start, part);
            used += part;
            length = start ? start - 1 : 0;
        }
        out[used] = '\0';
        if (used) {
            return;
        }
    }
    if (is_class) {
        load_undecorator();
        if (undecorate_symbol) {
            char symbol[RSF_RTTI_NAME_BYTES + 16];
            char text[RSF_RTTI_PRETTY_BYTES + 64];
            if (strlen(decorated) + 8 < sizeof(symbol)) {
                strcpy(symbol, "??_R0");
                strcat(symbol, decorated + 1);
                strcat(symbol, "@8");
                if (undecorate_symbol(symbol, text, (DWORD)sizeof(text), 0) != 0) {
                    const char* name = text;
                    if (strncmp(name, "class ", 6) == 0) {
                        name += 6;
                    } else if (strncmp(name, "struct ", 7) == 0) {
                        name += 7;
                    }
                    const char* suffix = strstr(name, " `RTTI Type Descriptor'");
                    size_t length = suffix ? (size_t)(suffix - name) : strlen(name);
                    if (length >= capacity) {
                        length = capacity - 1;
                    }
                    memcpy(out, name, length);
                    out[length] = '\0';
                    return;
                }
            }
        }
    }
    size_t length = strlen(decorated);
    if (length >= capacity) {
        length = capacity - 1;
    }
    memcpy(out, decorated, length);
    out[length] = '\0';
}

static int in_image(const scan* state, uint32_t rva, uint32_t size)
{
    return rva != 0 && rva < state->image_size && size <= state->image_size - rva;
}

/* Read the decorated name of a type descriptor. Returns its length, or 0 when the descriptor is
   unreadable or does not hold a type name. */
static size_t read_decorated(scan* state, uint32_t td_rva, char* out, size_t capacity)
{
    if (!in_image(state, td_rva, 24)) {
        return 0;
    }
    const unsigned char* name = state->base + td_rva + 16;
    const size_t wanted = capacity - 1 < (size_t)(state->image_size - td_rva - 16)
                              ? capacity - 1
                              : (size_t)(state->image_size - td_rva - 16);
    const size_t available = rsf_dump_readable_extent(&state->cache, name, wanted);
    if (available < 4) {
        return 0;
    }
    memcpy(out, name, available);
    out[available] = '\0';
    const size_t length = strlen(out);
    if (length == available && available == wanted) {
        return 0; /* no terminator within the bound */
    }
    if (length < 4 || out[0] != '.' || out[1] != '?' || out[2] != 'A') {
        return 0;
    }
    return length;
}

static const char* cached_name(scan* state, uint32_t td_rva)
{
    name_slot* slot = &state->names[(td_rva >> 3) % RSF_RTTI_CACHE_SLOTS];
    if (slot->rva == td_rva) {
        return slot->pretty;
    }
    char decorated[RSF_RTTI_NAME_BYTES];
    if (!read_decorated(state, td_rva, decorated, sizeof(decorated))) {
        return NULL;
    }
    slot->rva = td_rva;
    pretty_name(decorated, slot->pretty, sizeof(slot->pretty));
    return slot->pretty;
}

static int target_in_data(const scan* state, uint32_t rva)
{
    for (WORD index = 0; index < state->section_count; ++index) {
        const IMAGE_SECTION_HEADER* section = &state->sections[index];
        uint32_t size = section->Misc.VirtualSize ? section->Misc.VirtualSize : section->SizeOfRawData;
        if (rva >= section->VirtualAddress && rva - section->VirtualAddress < size) {
            return (section->Characteristics & (IMAGE_SCN_MEM_EXECUTE | IMAGE_SCN_MEM_DISCARDABLE)) == 0;
        }
    }
    return 0;
}

static void emit_class(scan* state, uint32_t vtable_rva, uint32_t locator_rva, const locator* col)
{
    char decorated[RSF_RTTI_NAME_BYTES];
    if (!read_decorated(state, col->type_descriptor, decorated, sizeof(decorated))) {
        return;
    }
    char pretty[RSF_RTTI_PRETTY_BYTES];
    pretty_name(decorated, pretty, sizeof(pretty));

    uint32_t hierarchy[4] = {0, 0, 0, 0};
    const int have_hierarchy =
        in_image(state, col->class_descriptor, sizeof(hierarchy)) &&
        rsf_dump_read_exact(&state->cache, state->base + col->class_descriptor, hierarchy,
                            sizeof(hierarchy)) &&
        hierarchy[0] == 0;

    fprintf(state->stream, "%s\n    {\"vtable_rva\": \"0x%lx\", \"locator_rva\": \"0x%lx\", \"name\": \"",
            state->classes ? "," : "", (unsigned long)vtable_rva, (unsigned long)locator_rva);
    rsf_json_ascii(state->stream, pretty, sizeof(pretty));
    fputs("\", \"decorated\": \"", state->stream);
    rsf_json_ascii(state->stream, decorated, sizeof(decorated));
    fprintf(state->stream, "\", \"offset\": %ld, \"cd_offset\": %lu, \"bases\": [",
            (long)col->offset, (unsigned long)col->cd_offset);

    uint32_t emitted = 0;
    uint32_t declared = 0;
    if (have_hierarchy) {
        declared = hierarchy[2];
        const uint32_t count = declared < RSF_RTTI_MAX_BASES ? declared : RSF_RTTI_MAX_BASES;
        const uint32_t array_rva = hierarchy[3];
        for (uint32_t index = 0; index < count; ++index) {
            uint32_t base_rva = 0;
            if (!in_image(state, array_rva + index * 4u, 4) ||
                !rsf_dump_read_exact(&state->cache, state->base + array_rva + index * 4u, &base_rva,
                                     sizeof(base_rva))) {
                break;
            }
            uint32_t item[7];
            if (!in_image(state, base_rva, sizeof(item)) ||
                !rsf_dump_read_exact(&state->cache, state->base + base_rva, item, sizeof(item))) {
                continue;
            }
            /* The first entry describes the class itself. */
            if (index == 0 && item[0] == col->type_descriptor) {
                continue;
            }
            const char* name = cached_name(state, item[0]);
            if (!name) {
                continue;
            }
            fprintf(state->stream, "%s{\"name\": \"", emitted ? ", " : "");
            rsf_json_ascii(state->stream, name, RSF_RTTI_PRETTY_BYTES);
            fprintf(state->stream,
                    "\", \"contained\": %lu, \"mdisp\": %ld, \"pdisp\": %ld, \"vdisp\": %ld, "
                    "\"attributes\": \"0x%lx\"}",
                    (unsigned long)item[1], (long)(int32_t)item[2], (long)(int32_t)item[3],
                    (long)(int32_t)item[4], (unsigned long)item[5]);
            ++emitted;
        }
    }
    fprintf(state->stream, "], \"declared_bases\": %lu", (unsigned long)declared);
    fputc('}', state->stream);
    ++state->classes;
}

static void try_slot(scan* state, uint32_t slot_rva, uint32_t locator_rva)
{
    locator col;
    if (!in_image(state, locator_rva, sizeof(col)) ||
        !rsf_dump_read_exact(&state->cache, state->base + locator_rva, &col, sizeof(col))) {
        return;
    }
    if (col.signature != 1 || col.self != locator_rva || !in_image(state, col.type_descriptor, 24) ||
        !in_image(state, col.class_descriptor, 16)) {
        return;
    }
    emit_class(state, slot_rva + 8u, locator_rva, &col);
}

static void scan_run(scan* state, const unsigned char* begin, const unsigned char* end)
{
    const uint64_t base = (uint64_t)(uintptr_t)state->base;
    uint32_t until_clock = 0;
    for (const unsigned char* cursor = begin; cursor + 8 <= end; cursor += 8) {
        uint64_t value;
        memcpy(&value, cursor, sizeof(value));
        const uint64_t rva = value - base;
        if (rva >= state->image_size || (rva & 3u) != 0 || !target_in_data(state, (uint32_t)rva)) {
            continue;
        }
        try_slot(state, (uint32_t)(cursor - state->base), (uint32_t)rva);
        if (state->classes >= RSF_RTTI_MAX_CLASSES) {
            state->out_of_classes = 1;
            return;
        }
        if (++until_clock >= 2048u) {
            until_clock = 0;
            if (rsf_dump_now_ms() > state->deadline_ms) {
                state->out_of_time = 1;
                return;
            }
        }
    }
}

static int range_overlaps_directory(const IMAGE_NT_HEADERS64* headers, DWORD index, uint32_t start,
                                    uint32_t size)
{
    const IMAGE_DATA_DIRECTORY* directory = &headers->OptionalHeader.DataDirectory[index];
    return directory->VirtualAddress != 0 && directory->Size != 0 &&
           start < directory->VirtualAddress + directory->Size &&
           directory->VirtualAddress < start + size;
}

void rsf_dump_write_rtti(FILE* stream, const unsigned char* base, const IMAGE_NT_HEADERS64* headers,
                         uint32_t budget_ms, uint32_t* class_count)
{
    const double started = rsf_dump_now_ms();
    scan state;
    memset(&state, 0, sizeof(state));
    state.base = base;
    state.image_size = headers->OptionalHeader.SizeOfImage;
    state.sections = IMAGE_FIRST_SECTION(headers);
    state.section_count = headers->FileHeader.NumberOfSections;
    state.stream = stream;
    state.deadline_ms = started + (budget_ms ? budget_ms : RSF_RTTI_DEFAULT_BUDGET_MS);
    state.names = (name_slot*)calloc(RSF_RTTI_CACHE_SLOTS, sizeof(name_slot));

    fputs("  \"rtti\": {\"classes\": [", stream);
    if (state.names) {
        for (WORD index = 0; index < state.section_count; ++index) {
            const IMAGE_SECTION_HEADER* section = &state.sections[index];
            const uint32_t size =
                section->Misc.VirtualSize ? section->Misc.VirtualSize : section->SizeOfRawData;
            if (size < 8 || (section->Characteristics & (IMAGE_SCN_MEM_EXECUTE | IMAGE_SCN_MEM_DISCARDABLE)) ||
                !(section->Characteristics & IMAGE_SCN_MEM_READ) ||
                section->VirtualAddress >= state.image_size ||
                size > state.image_size - section->VirtualAddress) {
                continue;
            }
            /* Unwind tables, resources and relocations hold many image offsets but no vtables. */
            if (range_overlaps_directory(headers, IMAGE_DIRECTORY_ENTRY_EXCEPTION,
                                         section->VirtualAddress, size) ||
                range_overlaps_directory(headers, IMAGE_DIRECTORY_ENTRY_RESOURCE,
                                         section->VirtualAddress, size) ||
                range_overlaps_directory(headers, IMAGE_DIRECTORY_ENTRY_BASERELOC,
                                         section->VirtualAddress, size)) {
                continue;
            }

            const unsigned char* cursor = base + section->VirtualAddress;
            const unsigned char* end = cursor + size;
            while (cursor < end && !state.out_of_time && !state.out_of_classes) {
                MEMORY_BASIC_INFORMATION region;
                if (VirtualQuery(cursor, &region, sizeof(region)) != sizeof(region)) {
                    break;
                }
                const unsigned char* region_end =
                    (const unsigned char*)region.BaseAddress + region.RegionSize;
                if (region_end > end) {
                    region_end = end;
                }
                if (region_end <= cursor) {
                    break;
                }
                const DWORD readable = PAGE_READONLY | PAGE_READWRITE | PAGE_WRITECOPY |
                                       PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE |
                                       PAGE_EXECUTE_WRITECOPY;
                if (region.State == MEM_COMMIT && (region.Protect & readable) &&
                    !(region.Protect & PAGE_GUARD)) {
                    scan_run(&state, cursor, region_end);
                }
                cursor = region_end;
            }
        }
    }
    fputs("\n  ], ", stream);

    const char* reason = NULL;
    if (!state.names) {
        reason = "allocation failed";
    } else if (state.out_of_time) {
        reason = "the time budget expired before the scan finished; the list is partial";
    } else if (state.out_of_classes) {
        reason = "the class bound was reached; the list is partial";
    } else if (state.classes == 0) {
        reason = "no RTTICompleteObjectLocator referenced from a vtable was found; the module has "
                 "no MSVC x64 RTTI, it was stripped, or the image is not decrypted";
    }
    fprintf(stream, "\"count\": %lu, \"partial\": %s, ", (unsigned long)state.classes,
            (state.out_of_time || state.out_of_classes) ? "true" : "false");
    if (reason) {
        fprintf(stream, "\"reason\": \"%s\", ", reason);
    }
    fprintf(stream, "\"budget_ms\": %lu, \"elapsed_ms\": %lu},\n",
            (unsigned long)(budget_ms ? budget_ms : RSF_RTTI_DEFAULT_BUDGET_MS),
            (unsigned long)(rsf_dump_now_ms() - started + 0.5));
    if (class_count) {
        *class_count = state.classes;
    }
    free(state.names);
}
