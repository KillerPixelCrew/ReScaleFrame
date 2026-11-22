/* SPDX-License-Identifier: MIT */
#ifndef RSF_CENSUS_JSON_PLATFORM_H
#define RSF_CENSUS_JSON_PLATFORM_H

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct rsf_census_json {
    char* buffer;
    size_t capacity;
    size_t length;
    uint32_t comma;
    uint32_t depth;
    bool overflow;
} rsf_census_json;

void rsf_census_json_open(rsf_census_json*, const char* key, char delimiter);
void rsf_census_json_close(rsf_census_json*, char delimiter);
void rsf_census_json_string(rsf_census_json*, const char* key, const char* value);
void rsf_census_json_wide(rsf_census_json*, const char* key, const uint16_t* value);
void rsf_census_json_u64(rsf_census_json*, const char* key, uint64_t value);
void rsf_census_json_i64(rsf_census_json*, const char* key, int64_t value);
void rsf_census_json_hex(rsf_census_json*, const char* key, uint64_t value);
void rsf_census_json_bytes(rsf_census_json*, const char* key, const void* data, size_t size);
void rsf_census_json_raw(rsf_census_json*, const char* key, const char* value);

#ifdef __cplusplus
}
#endif
#endif
