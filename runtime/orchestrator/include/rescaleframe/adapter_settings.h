/* SPDX-License-Identifier: MIT */
#ifndef RSF_ADAPTER_SETTINGS_H
#define RSF_ADAPTER_SETTINGS_H
#include <stddef.h>
#include <stdint.h>
#include <rescaleframe/runtime.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Access an adapter-owned boolean in the shared settings store. The adapter lends
   NUL-terminated paths and key names for the call. Read returns 0 or 1, or a negative
   rsf_result. Write returns rsf_result. The store serializes writes with section updates. */
typedef int32_t (*rsf_adapter_setting_read_bool_fn)(const wchar_t*, const wchar_t*, const wchar_t*, uint32_t);
typedef int32_t (*rsf_adapter_setting_write_bool_fn)(const wchar_t*, const wchar_t*, const wchar_t*, uint32_t);
RSF_RUNTIME_API int32_t rsf_adapter_setting_read_bool(
    const wchar_t* path, const wchar_t* section, const wchar_t* key, uint32_t fallback);
RSF_RUNTIME_API int32_t rsf_adapter_setting_write_bool(
    const wchar_t* path, const wchar_t* section, const wchar_t* key, uint32_t value);

#ifdef __cplusplus
}
#endif
#endif
