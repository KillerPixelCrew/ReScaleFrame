/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_FSR4_COMPAT_PLATFORM_ABI_VERSION 1u

typedef bool (*rsf_fsr4_compat_capability_fn)(void* api, void* device);
typedef void (*rsf_fsr4_compat_device_ref_fn)(void* device);
typedef void (*rsf_fsr4_compat_log_fn)(void* user, const char* message);
typedef struct rsf_fsr4_compat_lease rsf_fsr4_compat_lease;

typedef struct rsf_fsr4_compat_platform {
    uint32_t struct_size;
    uint32_t abi_version;
    uint32_t (*adapter_vendor)(void* adapter);
    bool (*device_supports_int8)(void* device);
    rsf_fsr4_compat_device_ref_fn retain_device;
    rsf_fsr4_compat_device_ref_fn release_device;
    /* SHA-256 is read only for a file size in sizes. */
    bool (*module_digest)(void* module, const uint32_t* sizes, size_t size_count,
        uint32_t* size, uint8_t digest[32]);
    void* (*match_entry)(void* module, size_t rva, const uint8_t* expected, size_t length);
    rsf_fsr4_compat_capability_fn (*create_hook)(void* target, rsf_fsr4_compat_capability_fn detour);
    /* Pins both code owners. On failure, removes the disabled hook. */
    void* (*enable_hook)(void* target, rsf_fsr4_compat_capability_fn detour);
} rsf_fsr4_compat_platform;

typedef struct rsf_fsr4_compat_diagnostic {
    const char* refusal;
    const char* sdk_version;
    size_t capability_rva;
} rsf_fsr4_compat_diagnostic;

/* Each successful acquire owns one reference and pairs with one release.
   The adapter must already match the device. Diagnostic strings are static. */
const rsf_fsr4_compat_lease* rsf_fsr4_compat_acquire(const rsf_fsr4_compat_platform* platform,
    void* module, void* adapter, void* device, rsf_fsr4_compat_diagnostic* diagnostic);
const rsf_fsr4_compat_lease* rsf_fsr4_compat_acquire_logged(const rsf_fsr4_compat_platform* platform,
    void* module, void* adapter, void* device, rsf_fsr4_compat_log_fn log, void* user);
void rsf_fsr4_compat_release(const rsf_fsr4_compat_lease* lease);
const rsf_fsr4_compat_lease* rsf_fsr4_compat_acquire_native_adapter(
    void* module, void* adapter, void* device, rsf_fsr4_compat_log_fn log, void* user);
const rsf_fsr4_compat_lease* rsf_fsr4_compat_acquire_native_device(
    void* module, void* device, rsf_fsr4_compat_log_fn log, void* user);

#ifdef __cplusplus
}
#endif
