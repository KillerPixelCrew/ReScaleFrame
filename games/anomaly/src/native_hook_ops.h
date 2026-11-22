// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>

#ifdef __cplusplus
#define RSF_HOOK_OP_NOEXCEPT noexcept
extern "C" {
#else
#define RSF_HOOK_OP_NOEXCEPT
#endif

// All entries are required. Rust serializes queued MinHook operations.
typedef bool (*rsf_anomaly_hook_apply_fn)(void) RSF_HOOK_OP_NOEXCEPT;
typedef bool (*rsf_anomaly_hook_create_fn)(void*, void*, void**) RSF_HOOK_OP_NOEXCEPT;
typedef void (*rsf_anomaly_hook_remove_fn)(void*) RSF_HOOK_OP_NOEXCEPT;
typedef bool (*rsf_anomaly_hook_queue_fn)(void*, bool) RSF_HOOK_OP_NOEXCEPT;
typedef struct rsf_anomaly_hook_ops {
    rsf_anomaly_hook_apply_fn initialize;
    rsf_anomaly_hook_create_fn create;
    rsf_anomaly_hook_remove_fn remove_unqueued;
    rsf_anomaly_hook_queue_fn queue;
    rsf_anomaly_hook_apply_fn apply;
} rsf_anomaly_hook_ops;

// The MinHook table in native_hooks_platform.c.
const rsf_anomaly_hook_ops* rsf_anomaly_minhook_ops(void) RSF_HOOK_OP_NOEXCEPT;

#ifdef __cplusplus
}
#endif
#undef RSF_HOOK_OP_NOEXCEPT
