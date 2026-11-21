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

namespace rsf::skyrim {
const rsf_skyrim_creation_platform* creation_platform() noexcept;
}

extern "C" const char* rsf_skyrim_executable_sha256() noexcept;
extern "C" int32_t rsf_skyrim_observer_admit(const wchar_t* path, size_t length,
    const rsf_skyrim_creation_platform*, int32_t (*sampling)(), int32_t (*presentation_inputs)(),
    rsf_skyrim_observer_targets* out) noexcept;
extern "C" rsf_result rsf_skyrim_publish_api(uint32_t requested, rsf_game_plugin_api* out,
    const rsf_game_hooks_api* hooks) noexcept;
