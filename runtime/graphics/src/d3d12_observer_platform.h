/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/d3d12_observer.h>
#include <stddef.h>

enum {
    RSF_OBSERVER12_FACTORY = 0,
    RSF_OBSERVER12_DEVICE = 1,
    RSF_OBSERVER12_QUEUE = 2,
    RSF_OBSERVER12_CHAIN = 3,
    RSF_OBSERVER12_CAPACITY = 8
};

struct rsf_observer12_hooks {
    int32_t (*initialize)(void);
    int32_t (*create)(void* target, void* detour, void** original);
    int32_t (*enable)(void* target);
    void (*remove)(void* target);
};

extern "C" {
int32_t rsf_core_observer12_create(const rsf_d3d12_observer_setup*, rsf_d3d12_observer**,
    const rsf_observer12_hooks*);
int32_t rsf_core_observer12_owns(const rsf_d3d12_observer*);
int32_t rsf_core_observer12_bind(uint32_t kind, void* target, void* const* wrappers,
    const rsf_observer12_hooks*);
void* rsf_core_observer12_original(uint32_t kind, size_t index);
int32_t rsf_core_observer12_enter(uint32_t kind);
void rsf_core_observer12_leave(uint32_t kind);
int32_t rsf_core_observer12_accept(void* window);
void rsf_core_observer12_swapchain(void* queue, void* chain, void* window);
uint64_t rsf_core_observer12_submit_begin(void* queue, uint32_t count, void* const* lists);
void rsf_core_observer12_submit_end(void* queue, uint64_t id, uint32_t count, void* const* lists);
void rsf_core_observer12_present(void* chain, uint32_t interval, uint32_t flags, uint32_t begin);
}
