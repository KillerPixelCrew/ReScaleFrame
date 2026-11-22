// SPDX-License-Identifier: MIT
#pragma once
#include <stdint.h>

#ifdef __cplusplus
namespace rsf::latency {
#endif
typedef struct AntiLagState {
    uint32_t available, enabled;
    uint64_t updates, end_markers, refused;
} AntiLagState;
typedef struct AntiLagCallbacks {
    uint32_t (*initialize)(void*, uint32_t api, void* device);
    uint32_t (*update)(void*, uint32_t api, uint32_t enabled, uint32_t max_fps);
    uint32_t (*end_render)(void*, void* chain, uint32_t attach);
    void (*shutdown)(void*);
} AntiLagCallbacks;
typedef struct AntiLagContext AntiLagContext;

/* Native AMD SDK leaves. HRESULT values cross unchanged. The allocator
   returns typed storage whose address stays valid after the owner retires. */
typedef struct AntiLagNativeOps {
    void* (*allocate)(void);
    int32_t (*initialize_dx11)(void*);
    int32_t (*initialize_dx12)(void*, void* device);
    int32_t (*update_dx11)(void*, uint32_t enabled, uint32_t max_fps);
    int32_t (*update_dx12)(void*, uint32_t enabled, uint32_t max_fps);
    void* (*context_dx12)(void*);
    int32_t (*mark_dx12)(void*);
    void (*shutdown)(void*);
} AntiLagNativeOps;
#ifdef __cplusplus
static_assert(sizeof(AntiLagCallbacks) == 32);
static_assert(sizeof(AntiLagNativeOps) == 64);
extern "C" {
#endif
/* A null table selects the unavailable SDK implementation. */
AntiLagContext* rsf_core_antilag_create(const AntiLagCallbacks*, void* driver);
AntiLagContext* rsf_core_antilag_create_sdk(const AntiLagNativeOps*);
void rsf_core_antilag_destroy(AntiLagContext*);
uint32_t rsf_core_antilag_bind(AntiLagContext*, uint32_t api, void* device, uint32_t vendor);
uint32_t rsf_core_antilag_pace(AntiLagContext*, uint64_t id, uint32_t mode, uint32_t limit_us, uint32_t other_owner);
uint32_t rsf_core_antilag_end_render(AntiLagContext*, uint64_t id, void* chain, uint32_t generated);
void rsf_core_antilag_state(const AntiLagContext*, AntiLagState*);
void rsf_core_antilag_reset_sources(AntiLagContext*);

#ifdef __cplusplus
}
}
#endif
