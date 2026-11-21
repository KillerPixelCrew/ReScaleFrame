/* SPDX-License-Identifier: MIT */
#pragma once
#include <stddef.h>
#include <stdint.h>

enum {
    RSF_OBSERVER11_TEXTURE = 0, RSF_OBSERVER11_BUFFER = 1, RSF_OBSERVER11_PRESENT = 2,
    RSF_OBSERVER11_LAYOUT = 3, RSF_OBSERVER11_STAGE_BASE = 4, RSF_OBSERVER11_SLOT_COUNT = 11
};

struct rsf_observer11_slot { void** address; void* hook; };
struct rsf_observer11_probe_desc {
    uint32_t width, height, format, usage, buffer_count, sample_count, feature_level;
};
struct rsf_observer11_probe_attempt {
    void* context;
    void* chain;
    void* device;
    int32_t result;
    uint32_t returned;
};
struct rsf_observer11_probe_tables { void** device; void** chain; void** chain1; };
struct rsf_observer11_probe_ops {
    void* (*window)(uint32_t width, uint32_t height);
    void (*destroy_window)(void*);
    void (*create)(void* window, uint32_t driver, const rsf_observer11_probe_desc*, rsf_observer11_probe_attempt*);
    uint32_t (*query_chain1)(void*, void** owned, int32_t* result);
    void** (*vtable)(void*);
};
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
uint32_t rsf_core_observer11_probe(const rsf_observer11_probe_ops*, rsf_observer11_probe_tables*);
int32_t rsf_core_observer11_install(const rsf_observer_options*, const rsf_observer11_callbacks*,
    void* user, RsfObserver11Probe, const rsf_observer11_hook_ops*);
int32_t rsf_core_observer11_uninstall_hooks(const rsf_observer11_hook_ops*);
int32_t rsf_core_observer11_installed(void);
void* rsf_core_observer11_original(size_t slot);
void* rsf_core_observer11_forward(uint32_t method, uint32_t nested);
void rsf_core_observer11_publish_route(uint32_t method, void* target, void* chain, void* genuine);
}
