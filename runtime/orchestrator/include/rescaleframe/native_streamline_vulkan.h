// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/native_sr_vulkan.h>

#define RSF_SLVK_ABI_VERSION 1u
#define RSF_SLVK_DLSS_SR 1u

typedef struct rsf_slvk_setup {
    uint32_t struct_size, abi_version;
    const char* runtime_directory_utf8;
    const char* project_id;
    uint32_t features;
} rsf_slvk_setup;

typedef rsf_backend_result (*rsf_slvk_callback)(void* sdk_module, uintptr_t frame_token, void* user);
typedef struct rsf_slvk_call {
    uint32_t struct_size, abi_version;
    uint64_t session_id, frame_id;
    rsf_slvk_callback callback;
    void* user;
} rsf_slvk_call;

#ifdef __cplusplus
extern "C" {
#endif
/* PCL/Reflex and selected providers share one early Vulkan SDK initialization.
   The legacy latency setup remains ABI 1 and requests no SR providers. */
RSF_RUNTIME_API rsf_backend_result rsf_slvk_initialize(const rsf_slvk_setup*);
/* Serialized synchronous SDK lease. Both identities zero permits non-frame
   operations. Otherwise the original CPU token must still match both IDs.
   The callback must not reenter this service or PCL. Module/token are borrowed
   only during the call; the current CPU token is never replaced by this API. */
RSF_RUNTIME_API rsf_backend_result rsf_slvk_invoke(const rsf_slvk_call*);
#ifdef __cplusplus
}
#endif
