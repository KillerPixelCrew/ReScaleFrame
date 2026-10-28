// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_d3d12_observer rsf_d3d12_observer;
typedef struct rsf_d3d12_observer_setup {
    uint32_t struct_size, abi_version;
    void* user;
    int (*accept_window)(void*, void*);
    void (*submit)(void*, void* queue, uint64_t id, uint32_t count, void* const* lists, uint32_t begin);
    void (*present)(void*, void* chain, uint32_t interval, uint32_t flags, uint32_t begin);
    void (*swapchain)(void*, void* queue, void* chain, void* window);
} rsf_d3d12_observer_setup;
/* Method-entry observation leaves complete native COM tables and destruction
   untouched. The owner remains alive for its installed process-lifetime hooks. */
int32_t rsf_d3d12_observer_create(const rsf_d3d12_observer_setup*, rsf_d3d12_observer**);
int32_t rsf_d3d12_observer_device(rsf_d3d12_observer*, void* device);
int32_t rsf_d3d12_observer_factory(rsf_d3d12_observer*, void* factory);
/* Explicit diagnostics: enable before device creation; attach after creation.
   No global registry settings are changed. */
int32_t rsf_d3d12_diagnostics_enable(void (*log)(void*, const char*), void* user);
#ifdef __cplusplus
}
#endif
