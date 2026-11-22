/* SPDX-License-Identifier: MIT */
#ifndef RSF_DUMP_RECORDS_H
#define RSF_DUMP_RECORDS_H

#include <stddef.h>
#include <stdint.h>

/* Windows module path storage from MODULEENTRY32W. */
#define RSF_DUMP_MODULE_PATH_UNITS 260

typedef struct rsf_module_entry {
    wchar_t path[RSF_DUMP_MODULE_PATH_UNITS];
    const unsigned char* base;
    size_t size;
} rsf_module_entry;

typedef void (*rsf_dump_stream_write)(void* stream, const unsigned char* data, size_t size);

#endif
