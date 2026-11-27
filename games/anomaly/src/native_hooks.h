// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
#define RSF_HOOK_NOEXCEPT noexcept
extern "C" {
#else
#define RSF_HOOK_NOEXCEPT
#endif

// `expected` holds the first 16 bytes at the RVA, the same convention as the profile sites.
typedef struct rsf_anomaly_hook_site {
    const char* name;
    uint32_t rva;
    uint8_t expected[16];
    void* replacement;
    void** original;
} rsf_anomaly_hook_site;

// Only for immediate-context D3D11 methods resolved through their COM vtable.
typedef struct rsf_anomaly_hook_target {
    void* address;
    void* replacement;
    void** original;
} rsf_anomaly_hook_target;

// One installed batch. Keep trampolines alive for the process: disabling is not callback retirement.
// Zero-initialize before first use.
typedef struct rsf_anomaly_hook_set {
    void* batch;
} rsf_anomaly_hook_set;

// native_hooks.rs: the guarded MinHook batch. Installation checks the image headers and each site's mapped code.
bool rsf_anomaly_hooks_install(rsf_anomaly_hook_set*, void* module, uint32_t image_size,
    const rsf_anomaly_hook_site*, size_t count) RSF_HOOK_NOEXCEPT;
bool rsf_anomaly_hooks_install_resolved(rsf_anomaly_hook_set*, const rsf_anomaly_hook_target*,
    size_t count) RSF_HOOK_NOEXCEPT;
bool rsf_anomaly_hooks_enable(rsf_anomaly_hook_set*) RSF_HOOK_NOEXCEPT;
bool rsf_anomaly_hooks_disable(rsf_anomaly_hook_set*) RSF_HOOK_NOEXCEPT;
bool rsf_anomaly_hooks_created(const rsf_anomaly_hook_set*) RSF_HOOK_NOEXCEPT;

// native_hooks_platform.c: the guarded engine-memory copy (native_memory.h) for the Rust owners.
bool rsf_anomaly_guarded_copy(void* output, const void* input, size_t size) RSF_HOOK_NOEXCEPT;

#ifdef __cplusplus
}
#endif
#undef RSF_HOOK_NOEXCEPT
