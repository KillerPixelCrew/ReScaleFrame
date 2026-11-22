// SPDX-License-Identifier: MIT
#pragma once
#include "engine_guards.h"
#include "native_hook_ops.h"
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
#define RSF_HOOK_NOEXCEPT noexcept
extern "C" {
#else
#define RSF_HOOK_NOEXCEPT
#endif

typedef struct rsf_anomaly_dos_facts {
    int32_t nt_offset;
    uint16_t signature;
} rsf_anomaly_dos_facts;

typedef struct rsf_anomaly_pe_facts {
    uint32_t signature;
    uint32_t image_size;
    uint16_t machine;
    uint16_t optional_magic;
} rsf_anomaly_pe_facts;

typedef bool (*rsf_anomaly_hook_dos_fn)(const void*, rsf_anomaly_dos_facts*) RSF_HOOK_NOEXCEPT;
typedef bool (*rsf_anomaly_hook_pe_fn)(const void*, const void*, rsf_anomaly_pe_facts*) RSF_HOOK_NOEXCEPT;
typedef bool (*rsf_anomaly_hook_mapped_read_fn)(const void*, const void*, void*, size_t, bool) RSF_HOOK_NOEXCEPT;

typedef struct rsf_anomaly_image_headers {
    rsf_anomaly_hook_dos_fn dos;
    rsf_anomaly_hook_pe_fn pe;
} rsf_anomaly_image_headers;

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

typedef struct rsf_anomaly_hook_platform {
    rsf_anomaly_image_headers headers;
    rsf_anomaly_hook_mapped_read_fn read;
    rsf_anomaly_hook_ops hooks;
} rsf_anomaly_hook_platform;

// One installed batch. Keep trampolines alive for the process: disabling is not callback retirement.
// Zero-initialize before first use.
typedef struct rsf_anomaly_hook_set {
    void* batch;
} rsf_anomaly_hook_set;

// Observer preparation uses SEH reads. Hook installation also checks the mapped region.
extern const rsf_anomaly_image_headers rsf_anomaly_guarded_image_headers;
extern const rsf_anomaly_hook_platform rsf_anomaly_native_hook_platform;

// native_hooks_platform.c: the hook set bound to the production MinHook platform.
bool rsf_anomaly_hooks_install(rsf_anomaly_hook_set*, void* module, uint32_t image_size,
    const rsf_anomaly_hook_site*, size_t count) RSF_HOOK_NOEXCEPT;
bool rsf_anomaly_hooks_install_resolved(rsf_anomaly_hook_set*, const rsf_anomaly_hook_target*,
    size_t count) RSF_HOOK_NOEXCEPT;
bool rsf_anomaly_hooks_enable(rsf_anomaly_hook_set*) RSF_HOOK_NOEXCEPT;
bool rsf_anomaly_hooks_disable(rsf_anomaly_hook_set*) RSF_HOOK_NOEXCEPT;
bool rsf_anomaly_hooks_created(const rsf_anomaly_hook_set*) RSF_HOOK_NOEXCEPT;

// native_hooks.rs.
bool rsf_anomaly_hook_set_install(void**, const rsf_anomaly_hook_platform*, void*, uint32_t,
    const rsf_anomaly_hook_site*, size_t) RSF_HOOK_NOEXCEPT;
bool rsf_anomaly_hook_set_install_resolved(void**, const rsf_anomaly_hook_platform*,
    const rsf_anomaly_hook_target*, size_t) RSF_HOOK_NOEXCEPT;
bool rsf_anomaly_hook_set_set_enabled(void**, const rsf_anomaly_hook_platform*, bool) RSF_HOOK_NOEXCEPT;
bool rsf_anomaly_hook_set_created(void* const*) RSF_HOOK_NOEXCEPT;

bool rsf_anomaly_hook_read_mapped(const void*, const void*, void*, size_t, bool,
    rsf_anomaly_guard_read_fn) RSF_HOOK_NOEXCEPT;
bool rsf_anomaly_hook_read_dos(const void*, rsf_anomaly_dos_facts*, bool,
    rsf_anomaly_guard_read_fn) RSF_HOOK_NOEXCEPT;
bool rsf_anomaly_hook_read_pe(const void*, const void*, rsf_anomaly_pe_facts*, bool,
    rsf_anomaly_guard_read_fn) RSF_HOOK_NOEXCEPT;

#ifdef __cplusplus
}
#endif
#undef RSF_HOOK_NOEXCEPT
