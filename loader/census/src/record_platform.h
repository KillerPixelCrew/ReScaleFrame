/* SPDX-License-Identifier: MIT */
#ifndef RSF_CENSUS_RECORD_PLATFORM_H
#define RSF_CENSUS_RECORD_PLATFORM_H

#include "hook_platform.h"

#define RSF_CENSUS_RECORD_BODY_MAX 6144
#define RSF_CENSUS_RECORD_STACK_FRAMES 32
#define RSF_CENSUS_RECORD_THREAD_NAME 40

typedef struct rsf_census_queued_record {
    uint32_t len;
    uint16_t fn;
    uint64_t t_us, present, ord, site_ord, base, ret;
    uint32_t rva, tid, stack_n;
    void* stack[RSF_CENSUS_RECORD_STACK_FRAMES];
    char thread[RSF_CENSUS_RECORD_THREAD_NAME];
    char body[RSF_CENSUS_RECORD_BODY_MAX];
} rsf_census_queued_record;

typedef struct rsf_census_record_writer {
    const rsf_census_hook_desc* entries;
    size_t entry_count;
    const char* (*module_name)(uint64_t base);
    uint64_t (*frame_base)(void* address);
    void (*append)(const char* data, size_t bytes);
    void (*drop_size)(void);
} rsf_census_record_writer;

#ifdef __cplusplus
extern "C" {
#endif
bool rsf_census_record_init(void);
rsf_census_queued_record* rsf_census_record_acquire(size_t* position);
void rsf_census_record_publish(size_t position);
/* Drain must have a single consumer. Callbacks and their strings are borrowed. */
void rsf_census_record_drain(const rsf_census_record_writer*);
#ifdef __cplusplus
}
#endif
#endif
