// SPDX-License-Identifier: MIT
#pragma once
#include <cstddef>
#include <cstdint>

namespace rsf::rdr2 {
struct ResourceFacts {
    uint64_t width;
    uint32_t height;
    uint32_t format;
    uint32_t samples;
    uint32_t reserved;
};
static_assert(sizeof(ResourceFacts) == 24);

// Typed native leaves lent to the Rust plugin policy (games/rdr2/src/platform.rs, same order and
// size). MinHook, D3D12/DXGI COM calls, Win32 input and memory reads, and return-address capture
// stay native; no RDR2 address, layout or decision lives here.
struct Platform {
    uint32_t struct_size;
    uint32_t version;
    uintptr_t (*main_module)() noexcept;
    uintptr_t (*module_handle)(const char* name) noexcept;
    uintptr_t (*export_address)(uintptr_t module, const char* name) noexcept;
    bool (*read)(uintptr_t address, void* out, size_t size) noexcept;
    uintptr_t (*thread_slots)() noexcept;
    void (*qpc)(uint64_t* now, uint64_t* frequency) noexcept;
    bool (*module_path)(uintptr_t address, uintptr_t* base, char* path, size_t capacity) noexcept;
    bool (*hook_initialize)() noexcept;
    bool (*hook_create)(uintptr_t target, const void* detour, void** original) noexcept;
    bool (*hook_enable)(uintptr_t target) noexcept;
    bool (*hook_disable)(uintptr_t target) noexcept;
    bool (*hook_remove)(uintptr_t target) noexcept;
    void* (*pop_detour)(void* ring, void* record) noexcept;
    uintptr_t (*output_detour)(void* depth, void* color, uint32_t technique) noexcept;
    uintptr_t (*blit_detour)(void* shader, uint32_t technique, uint32_t pass, const void* source,
        const void* destination, const void* clip) noexcept;
    int32_t (*fsr_dispatch_detour)(void* context, const void* description) noexcept;
    bool (*attach_queue)(void* queue, const void* detour, void** original) noexcept;
    bool (*attach_present)(void* swapchain, const void* detour, void** original) noexcept;
    bool (*resource_facts)(void* resource, ResourceFacts* out) noexcept;
    void (*barrier)(void* list, void* resource, uint32_t before, uint32_t after) noexcept;
    bool (*swapchain_facts)(void* swapchain, uint32_t* buffers, uint64_t* window) noexcept;
    void* (*swapchain_buffer)(void* swapchain, uint32_t index) noexcept;
    uint32_t (*back_buffer_index)(void* swapchain) noexcept;
    void (*release)(void* object) noexcept;
    void (*input_install)(void (*message)(uint32_t kind, uint32_t message), void (*controller)()) noexcept;
    void (*input_disable)() noexcept;
    bool (*input_idle)() noexcept;
    bool (*input_unhook)() noexcept;
    bool (*input_remove)() noexcept;
};
static_assert(sizeof(Platform) == 8 + 29 * sizeof(void*));
static_assert(offsetof(Platform, pop_detour) == 8 + 12 * sizeof(void*));
static_assert(offsetof(Platform, attach_queue) == 8 + 16 * sizeof(void*));
static_assert(offsetof(Platform, input_install) == 8 + 24 * sizeof(void*));

inline constexpr uint32_t platform_version = 2;
const Platform& platform() noexcept;
}
