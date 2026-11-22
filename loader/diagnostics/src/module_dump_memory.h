/* SPDX-License-Identifier: MIT */
#ifndef RSF_MODULE_DUMP_MEMORY_H
#define RSF_MODULE_DUMP_MEMORY_H

#include <stddef.h>

typedef struct rsf_mem_cache {
    const unsigned char* low;
    const unsigned char* high;
} rsf_mem_cache;

/* Contiguous readable bytes starting at address, up to wanted. */
size_t rsf_dump_readable_extent(rsf_mem_cache* cache, const void* address, size_t wanted);
/* Copy exactly size bytes, or return zero and leave destination untouched. */
int rsf_dump_read_exact(rsf_mem_cache* cache, const void* address, void* destination, size_t size);
/* Copy readable ranges and leave zeroes for unreadable ranges. Return the bytes copied. */
size_t rsf_dump_read_guarded(const unsigned char* source, unsigned char* destination, size_t size);

#endif
