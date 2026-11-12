// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <array>
#include <cstddef>
#include <cstdint>
#include <span>

namespace rsf::anomaly {
struct HookSite {
    const char* name{};
    uint32_t rva{};
    std::array<std::byte, 16> expected{};
    void* replacement{};
    void** original{};
};
// Keep trampolines alive for the process. Disabling detours is not a proof that
// another thread has retired from a callback or its original trampoline.
class HookSet {
public:
    bool install(std::byte* module, uint32_t image_size, std::span<const HookSite>) noexcept;
    bool enable() noexcept;
    bool disable() noexcept;
    bool created() const noexcept { return count_ != 0; }
private:
    std::array<void*, 16> targets_{};
    size_t count_{};
    bool enabled_{};
};
}
