// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
#define RSF_GUARD_NOEXCEPT noexcept
extern "C" {
#else
#define RSF_GUARD_NOEXCEPT
#endif

// The guarded image borrowed by the Rust guard owner (engine_guards.rs). `read` is the fault-contained copy.
typedef bool (*rsf_anomaly_guard_read_fn)(void*, const void*, size_t) RSF_GUARD_NOEXCEPT;
typedef struct rsf_anomaly_guard_image {
    const uint8_t* base;
    uint32_t size;
    rsf_anomaly_guard_read_fn read;
} rsf_anomaly_guard_image;

bool rsf_anomaly_guard_first_person(const rsf_anomaly_guard_image*, uint32_t set_p_call, uint32_t rm_near_call,
    uint32_t set_p, uint32_t rm_near) RSF_GUARD_NOEXCEPT;

#ifdef __cplusplus
}
#endif
#undef RSF_GUARD_NOEXCEPT
