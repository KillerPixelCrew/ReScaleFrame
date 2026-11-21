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

// Mutated by Rust under the native lifecycle mutex.
struct rsf_skyrim_observer_control {
    uint32_t created, active, pinned;
};
struct rsf_skyrim_observer_leaves {
    bool (*initialize)();
    bool (*create)(uint32_t);
    bool (*remove)(uint32_t);
    int32_t (*enable)(uint32_t);
    bool (*disable)(uint32_t);
    bool (*pin)();
    void (*log)(const rsf_game_host_services*, const char*);
};
static_assert(sizeof(rsf_skyrim_observer_control) == 12);
static_assert(sizeof(rsf_skyrim_observer_leaves) == 56);

namespace rsf::skyrim {
const rsf_skyrim_creation_platform* creation_platform() noexcept;
}

extern "C" const char* rsf_skyrim_executable_sha256() noexcept;
extern "C" int32_t rsf_skyrim_observer_admit(const wchar_t* path, size_t length,
    const rsf_skyrim_creation_platform*, int32_t (*sampling)(), int32_t (*presentation_inputs)(),
    rsf_skyrim_observer_targets* out) noexcept;
extern "C" rsf_result rsf_skyrim_publish_api(uint32_t requested, rsf_game_plugin_api* out,
    const rsf_game_hooks_api* hooks) noexcept;
extern "C" bool rsf_skyrim_observers_prepare(rsf_skyrim_observer_control*,
    const rsf_skyrim_observer_leaves*) noexcept;
extern "C" bool rsf_skyrim_observers_start(rsf_skyrim_observer_control*,
    const rsf_skyrim_observer_leaves*, const rsf_game_host_services*) noexcept;
extern "C" bool rsf_skyrim_observers_disable(rsf_skyrim_observer_control*,
    const rsf_skyrim_observer_leaves*, bool rollback) noexcept;
