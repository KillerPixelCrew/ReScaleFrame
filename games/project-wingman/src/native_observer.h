// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>
#include <stddef.h>

namespace rsf::wingman {
struct ObserverPlatform {
    bool (*verify_executable)() noexcept;
    uintptr_t (*image_base)() noexcept;
    bool (*copy)(void*, const void*, size_t) noexcept;
    uint64_t (*tick_count)() noexcept;
    bool (*create_hook)(uintptr_t) noexcept;
    bool (*enable_hook)(uintptr_t) noexcept;
    bool (*disable_hook)(uintptr_t) noexcept;
    bool (*prepare_execution)(const rsf_game_host_services*) noexcept;
    bool (*start_execution)() noexcept;
    void (*quiesce_execution)() noexcept;
    bool (*stop_execution)() noexcept;
    void (*log)(rsf_game_log_fn, void*, const char*) noexcept;
};
const ObserverPlatform& observer_platform() noexcept;
}
