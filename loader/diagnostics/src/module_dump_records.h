/* SPDX-License-Identifier: MIT */
#ifndef RSF_DUMP_RECORDS_H
#define RSF_DUMP_RECORDS_H

#include <stddef.h>
#include <stdint.h>

/* Windows module path storage from MODULEENTRY32W. */
#define RSF_DUMP_MODULE_PATH_UNITS 260
#define RSF_MAX_MODULES 384

typedef struct rsf_module_entry {
    wchar_t path[RSF_DUMP_MODULE_PATH_UNITS];
    const unsigned char* base;
    size_t size;
} rsf_module_entry;

typedef void (*rsf_dump_stream_write)(void* stream, const unsigned char* data, size_t size);

typedef struct rsf_section_digest {
    int valid;
    uint64_t dumped_size;
    uint64_t readable_bytes;
    double entropy;
    char sha256[65];
} rsf_section_digest;

typedef struct rsf_dump_image_info {
    const unsigned char* base;
    const wchar_t* path;
    uint32_t image_size;
    uint32_t header_size;
    uint32_t alignment;
    uint32_t nt_offset;
    uint32_t sections_offset;
    uint32_t sections;
    uint32_t import_directory;
} rsf_dump_image_info;

typedef struct rsf_dump_image_stats {
    uint64_t bytes_written;
    double hash_ms;
    uint32_t sections_written;
    uint32_t iat_references;
} rsf_dump_image_stats;

typedef struct rsf_dump_sidecar_info {
    const rsf_dump_image_info* image;
    const void* headers;
    const wchar_t* module_path;
    const rsf_module_entry* modules;
    size_t module_count;
    const rsf_section_digest* digests;
    const char* name_list;
    uint64_t preferred_base;
    double code_entropy;
    double hash_ms;
    double started_ms;
    uint32_t abi_version;
    uint32_t schema_version;
    uint32_t entry_rva;
    uint32_t iat_references;
    uint32_t import_bytes;
    uint32_t exception_rva;
    uint32_t exception_bytes;
    uint32_t rtti_budget_ms;
} rsf_dump_sidecar_info;

typedef struct rsf_dump_sidecar_stats {
    uint32_t imports_described;
    uint32_t threads_listed;
    uint32_t rtti_classes;
    uint32_t function_starts;
} rsf_dump_sidecar_stats;

typedef struct rsf_dump_module_platform {
    int32_t (*write_sidecar)(const wchar_t* path, const rsf_dump_sidecar_info*, rsf_dump_sidecar_stats*);
} rsf_dump_module_platform;

#endif
