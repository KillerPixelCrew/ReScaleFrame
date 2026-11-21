/* SPDX-License-Identifier: MIT */
#pragma once
#include <stddef.h>
#include <stdint.h>

enum {
    RSF_OBSERVER11_TEXTURE = 0, RSF_OBSERVER11_BUFFER = 1, RSF_OBSERVER11_PRESENT = 2,
    RSF_OBSERVER11_LAYOUT = 3, RSF_OBSERVER11_STAGE_BASE = 4, RSF_OBSERVER11_SLOT_COUNT = 11
};

struct rsf_observer11_slot { void** address; void* hook; };
struct rsf_observer11_hook_ops {
    void* (*read)(void** address);
    int32_t (*replace)(void** address, void* expected, void* replacement);
    int32_t (*detour)(void* user, uint32_t method);
    int32_t (*remove)(void* target);
};

extern "C" {
int32_t rsf_core_observer11_can_install(void);
int32_t rsf_core_observer11_install_hooks(const rsf_observer11_slot*, uint32_t has_present1,
    void* user, const rsf_observer11_hook_ops*);
int32_t rsf_core_observer11_uninstall_hooks(const rsf_observer11_hook_ops*);
int32_t rsf_core_observer11_installed(void);
void* rsf_core_observer11_original(size_t slot);
void* rsf_core_observer11_forward(uint32_t method, uint32_t nested);
void rsf_core_observer11_publish_route(uint32_t method, void* target, void* chain, void* genuine);
}
