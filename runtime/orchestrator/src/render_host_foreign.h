/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>

/* Streamline PFun_slGetNativeInterface returns the SDK's int-sized Result. */
typedef int32_t (*rsf_render_host_native_interface_fn)(void* proxy, void** native);

#ifdef __cplusplus
extern "C" {
#endif
int32_t rsf_render_host_call_native_interface(
    rsf_render_host_native_interface_fn entry, void* proxy, void** native)
#ifdef __cplusplus
    noexcept
#endif
    ;
#ifdef __cplusplus
}
#endif
