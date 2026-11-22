/* SPDX-License-Identifier: MIT */
/* Shared between the module dump writer and its structured sidecar sections. Not installed. */

#ifndef RSF_MODULE_DUMP_INTERNAL_H
#define RSF_MODULE_DUMP_INTERNAL_H

#include <rescaleframe/module_dump.h>

#include <windows.h>

#include <stdio.h>
#include <stdint.h>

#include "module_dump_memory.h"
#include "module_dump_records.h"

#define RSF_MAX_MODULES 384

const void* rsf_dump_resolve_base(const void* module_base);
const IMAGE_NT_HEADERS64* rsf_dump_nt_headers(const void* base);
void rsf_dump_collect_modules(rsf_module_entry* entries, size_t capacity, size_t* count);

void rsf_json_wide(FILE* stream, const wchar_t* text);
void rsf_json_ascii(FILE* stream, const char* text, size_t limit);

double rsf_dump_now_ms(void);
const wchar_t* rsf_dump_basename(const wchar_t* path);

rsf_dump_result rsf_dump_poll_code(double threshold, uint32_t poll_ms, uint32_t timeout_ms,
                                   uint32_t required_stable, double* last_entropy);

/* Sidecar sections. Each writes one `"key": value,` entry including the trailing comma. */
uint32_t rsf_dump_write_threads(FILE* stream, const rsf_module_entry* modules,
                                size_t module_count);
void rsf_dump_write_rtti(FILE* stream, const unsigned char* base, const IMAGE_NT_HEADERS64* headers,
                         uint32_t budget_ms, uint32_t* class_count);
void rsf_dump_write_functions(FILE* stream, const unsigned char* base,
                              const IMAGE_NT_HEADERS64* headers, uint32_t* function_count);
void rsf_dump_write_names(FILE* stream, const char* path_utf8);

#endif /* RSF_MODULE_DUMP_INTERNAL_H */
