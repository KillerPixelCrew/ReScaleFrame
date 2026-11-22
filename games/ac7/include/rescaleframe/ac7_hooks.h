/* SPDX-License-Identifier: MIT */
#pragma once
/* MinHook batch effects for the AC7 capture. Internal. */
#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef bool (*rsf_ac7_hook_create_fn)(void*, void*, void**);
typedef bool (*rsf_ac7_hook_fn)(void*);
bool rsf_ac7_install_hook_batch(void**, void* const*, void** const*, uint32_t,
    rsf_ac7_hook_create_fn, rsf_ac7_hook_fn, rsf_ac7_hook_fn, rsf_ac7_hook_fn);
void rsf_ac7_remove_hook_batch(void**, uint32_t, rsf_ac7_hook_fn, rsf_ac7_hook_fn);

bool rsf_ac7_capture_initialize_minhook(void);
bool rsf_ac7_capture_install_hooks(void** targets, void* const* detours, void** const* originals, uint32_t count);
bool rsf_ac7_capture_install_root_batch(void** targets, uint32_t count);
bool rsf_ac7_capture_install_engine_batch(void** targets, uint32_t count);
void rsf_ac7_capture_remove_hook_batch(void** targets, uint32_t count);

#ifdef __cplusplus
}
#endif
