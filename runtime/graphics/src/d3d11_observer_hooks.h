/* SPDX-License-Identifier: MIT */
#pragma once
#include <stddef.h>
#include <stdint.h>

struct rsf_observer11_slot { void** address; void* hook; };
struct rsf_observer11_probe_attempt {
    void* context;
    void* chain;
    void* device;
    int32_t result;
    uint32_t returned;
};
struct rsf_observer11_probe_route { void** table; size_t index; void* hook; };
struct rsf_observer_options;
struct rsf_observer11_callbacks;
using RsfObserver11Probe = int32_t (*)(void* user, rsf_observer11_slot*, uint32_t* has_present1);
struct rsf_observer11_hook_ops {
    void* (*read)(void** address);
    int32_t (*replace)(void** address, void* expected, void* replacement);
    int32_t (*detour)(void* user, uint32_t method);
    int32_t (*remove)(void* target);
};

extern "C" {
uint32_t rsf_core_observer11_probe(rsf_observer11_slot*, rsf_observer11_probe_route*);
int32_t rsf_core_observer11_install(const rsf_observer_options*, const rsf_observer11_callbacks*,
    void* user, RsfObserver11Probe, const rsf_observer11_hook_ops*);
int32_t rsf_core_observer11_uninstall_hooks(const rsf_observer11_hook_ops*);
int32_t rsf_graphics_replace_vtable_slot(void** address, void* expected, void* replacement);
void rsf_core_observer11_publish_route(uint32_t method, void* target, void* chain, void* genuine);
}
