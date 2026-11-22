/* SPDX-License-Identifier: MIT */
#pragma once

#ifdef __cplusplus
extern "C" {
#endif

/* FSR4 INT8 compatibility lease exported by the Rust rsf-fsr4-compat archive. */
typedef void (*rsf_fsr4_compat_log_fn)(void* user, const char* message);
typedef struct rsf_fsr4_compat_lease rsf_fsr4_compat_lease;

/* Each successful acquire owns one reference and pairs with one release.
   Log messages are borrowed for the call. */
const rsf_fsr4_compat_lease* rsf_fsr4_compat_acquire_native_device(
    void* module, void* device, rsf_fsr4_compat_log_fn log, void* user);
void rsf_fsr4_compat_release(const rsf_fsr4_compat_lease* lease);

#ifdef __cplusplus
}
#endif