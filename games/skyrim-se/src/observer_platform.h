// SPDX-License-Identifier: MIT
#pragma once
#include "native_creation_platform.h"

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

extern "C" int32_t rsf_skyrim_observer_admit() noexcept;
extern "C" bool rsf_skyrim_observer_initialize() noexcept;
extern "C" bool rsf_skyrim_observer_create(uint32_t) noexcept;
extern "C" bool rsf_skyrim_observer_remove(uint32_t) noexcept;
extern "C" int32_t rsf_skyrim_observer_enable(uint32_t) noexcept;
extern "C" bool rsf_skyrim_observer_disable(uint32_t) noexcept;
extern "C" bool rsf_skyrim_observer_pin() noexcept;
extern "C" void* rsf_skyrim_observer_original(uint32_t) noexcept;
