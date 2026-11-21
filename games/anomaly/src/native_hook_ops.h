// SPDX-License-Identifier: MIT
#pragma once
#include <MinHook.h>

namespace rsf::anomaly {
struct HookOps {
    bool (*initialize)() noexcept;
    bool (*create)(void*, void*, void**) noexcept;
    void (*remove_unqueued)(void*) noexcept;
    bool (*queue)(void*, bool) noexcept;
    bool (*apply)() noexcept;
};
static_assert(sizeof(HookOps) == 5 * sizeof(void*));

inline constexpr HookOps hook_ops{
    []() noexcept {
        const auto status = MH_Initialize();
        return status == MH_OK || status == MH_ERROR_ALREADY_INITIALIZED;
    },
    [](void* target, void* replacement, void** original) noexcept {
        return MH_CreateHook(target, replacement, original) == MH_OK;
    },
    // A never enabled or queued entry has no disable step.
    [](void* target) noexcept { (void)MH_RemoveHook(target); },
    [](void* target, bool enable) noexcept {
        return (enable ? MH_QueueEnableHook(target) : MH_QueueDisableHook(target)) == MH_OK;
    },
    []() noexcept { return MH_ApplyQueued() == MH_OK; },
};
}
