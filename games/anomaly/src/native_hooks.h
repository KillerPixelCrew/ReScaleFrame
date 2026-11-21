// SPDX-License-Identifier: MIT
#pragma once
#include <array>
#include <cstddef>
#include <cstdint>
#include <span>

namespace rsf::anomaly {
struct DosFacts {
    int32_t nt_offset;
    uint16_t signature;
};

struct PeFacts {
    uint32_t signature;
    uint32_t image_size;
    uint16_t machine;
    uint16_t optional_magic;
};

struct ImageHeaders {
    bool (*dos)(const void*, DosFacts*) noexcept;
    bool (*pe)(const void*, const void*, PeFacts*) noexcept;
};

// Observer prepare uses SEH reads. Hook installation also checks mapped region ownership and protection.
extern const ImageHeaders guarded_image_headers;

struct HookSite {
    const char* name{};
    uint32_t rva{};
    std::array<std::byte, 16> expected{};
    void* replacement{};
    void** original{};
};

struct ResolvedHookSite {
    void* target{};
    void* replacement{};
    void** original{};
};

// Keep trampolines alive for the process. Disabling detours is not a proof that
// another thread has retired from a callback or its original trampoline.
class HookSet {
public:
    HookSet() noexcept = default;
    HookSet(const HookSet&) = delete;
    HookSet& operator=(const HookSet&) = delete;
    bool install(std::byte* module, uint32_t image_size, std::span<const HookSite>) noexcept;
    // Only for the immediate-context D3D11 methods resolved through their COM vtable.
    bool install_resolved(std::span<const ResolvedHookSite>) noexcept;
    bool enable() noexcept;
    bool disable() noexcept;
    bool created() const noexcept;
private:
    void* batch_{};
};
}
