/* SPDX-License-Identifier: GPL-3.0-only */
/* Shared between the module dump writer and its structured sidecar sections. Not installed. */

#ifndef RSF_MODULE_DUMP_INTERNAL_H
#define RSF_MODULE_DUMP_INTERNAL_H

#include <rescaleframe/module_dump.h>

#include <windows.h>

#include <stdio.h>
#include <stdint.h>

#define RSF_MAX_MODULES 384

typedef struct rsf_module_entry {
    wchar_t path[MAX_PATH];
    const unsigned char* base;
    size_t size;
} rsf_module_entry;

/* Per-section result gathered while the dump is written. */
typedef struct rsf_section_digest {
    int valid;
    uint64_t dumped_size;
    uint64_t readable_bytes;
    double entropy;
    char sha256[65];
} rsf_section_digest;

/* Guarded memory access. Every read of the target image outside the dump copy goes through these,
   so a guard page, a decommitted range or an unmapped hole shortens the result instead of
   faulting. */
typedef struct rsf_mem_cache {
    const unsigned char* low;
    const unsigned char* high;
} rsf_mem_cache;

/* Contiguous readable bytes starting at `address`, up to `wanted`. */
size_t rsf_dump_readable_extent(rsf_mem_cache* cache, const void* address, size_t wanted);
/* Copy exactly `size` bytes, or return 0 and leave `destination` untouched. */
int rsf_dump_read_exact(rsf_mem_cache* cache, const void* address, void* destination, size_t size);

size_t rsf_dump_read_guarded(const unsigned char* source, unsigned char* destination, size_t size);

void rsf_json_wide(FILE* stream, const wchar_t* text);
void rsf_json_ascii(FILE* stream, const char* text, size_t limit);
void rsf_json_utf8(FILE* stream, const char* text);

double rsf_dump_now_ms(void);
const wchar_t* rsf_dump_basename(const wchar_t* path);

void rsf_sha256_hex(const void* data, size_t size, char out[65]);

/* Sidecar sections. Each writes one `"key": value,` entry including the trailing comma. */
uint32_t rsf_dump_write_threads(FILE* stream, const rsf_module_entry* modules,
                                size_t module_count);
void rsf_dump_write_rtti(FILE* stream, const unsigned char* base, const IMAGE_NT_HEADERS64* headers,
                         uint32_t budget_ms, uint32_t* class_count);
void rsf_dump_write_functions(FILE* stream, const unsigned char* base,
                              const IMAGE_NT_HEADERS64* headers, uint32_t* function_count);
void rsf_dump_write_names(FILE* stream, const char* path_utf8);

#endif /* RSF_MODULE_DUMP_INTERNAL_H */
