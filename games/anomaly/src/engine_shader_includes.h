// SPDX-License-Identifier: MIT
#pragma once
#include <stddef.h>
#include <stdint.h>
#ifndef __cplusplus
#include <stdbool.h>
#endif

typedef struct rsf_anomaly_engine_include_ops {
    bool (*open)(void* table, const char*, const void**, uint32_t*);
    bool (*copy)(void*, const void*, size_t);
    void (*close)(void* table, const void*);
} rsf_anomaly_engine_include_ops;

typedef struct rsf_anomaly_engine_shader_profile {
    const uint16_t* compiler_name;
    uint32_t setup_rva;
} rsf_anomaly_engine_shader_profile;
