// SPDX-License-Identifier: MIT
#pragma once
#include "native_creation_platform.h"

struct rsf_skyrim_observer_targets {
    void* jitter;
    void* process;
    void* resolution;
    void* end;
    void* upload;
};
static_assert(sizeof(rsf_skyrim_observer_targets) == 40);

// MinHook leaves for the five observers. The Rust session (src/session.rs) calls them under its
// lock and owns the created/active/pinned record.
struct rsf_skyrim_observer_leaves {
    bool (*initialize)();
    bool (*create)(uint32_t);
    bool (*remove)(uint32_t);
    int32_t (*enable)(uint32_t);
    bool (*disable)(uint32_t);
    bool (*pin)();
    void (*log)(const rsf_game_host_services*, const char*);
};
static_assert(sizeof(rsf_skyrim_observer_leaves) == 56);

namespace rsf::skyrim {
const rsf_skyrim_creation_platform* creation_platform() noexcept;
}

extern "C" const char* rsf_skyrim_executable_sha256() noexcept;
extern "C" int32_t rsf_skyrim_observer_admit(
    int32_t (*sampling)(), int32_t (*presentation_inputs)(),
    rsf_skyrim_observer_targets* out) noexcept;
extern "C" bool rsf_skyrim_observer_initialize() noexcept;
extern "C" bool rsf_skyrim_observer_create(void* target, void* detour, void** original) noexcept;
extern "C" bool rsf_skyrim_observer_remove(void* target, void** original) noexcept;
extern "C" int32_t rsf_skyrim_observer_enable(void* target) noexcept;
extern "C" bool rsf_skyrim_observer_disable(void* target) noexcept;
extern "C" bool rsf_skyrim_observer_pin(uintptr_t address) noexcept;
