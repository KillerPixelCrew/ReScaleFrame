// SPDX-License-Identifier: MIT
#pragma once
#include <cstddef>
#include <cstdint>

namespace rsf::rdr2 {
struct DiagnosticHook {
    const void* detour;
    void** original;
};
static_assert(sizeof(DiagnosticHook) == 2 * sizeof(void*));

// Typed native leaves of ReScaleFrame.RDR2.asi lent to the Rust crate rsf-rdr2 (games/rdr2/src/asi.rs,
// same order and size). The crate owns process admission, the [RDR2] configuration gates, the
// ForceD3D12 write, every RDR2 address and expected byte, and engine-settings ownership. These are
// Win32 path and profile reads, this module's MinHook instance, DXGI and display queries, calls into
// engine functions the crate verified, and the ASI's native diagnostic owners.
struct AsiPlatform {
    uint32_t struct_size;
    uint32_t version;
    uintptr_t (*main_module)() noexcept;
    bool (*read)(uintptr_t address, void* out, size_t size) noexcept;
    uint32_t (*executable_path)(wchar_t* out, uint32_t capacity) noexcept;
    uint32_t (*module_path)(wchar_t* out, uint32_t capacity) noexcept;
    uint32_t (*current_thread)() noexcept;
    uint32_t (*profile_int)(const wchar_t* path, const wchar_t* section, const wchar_t* key, int32_t fallback) noexcept;
    uint32_t (*profile_string)(const wchar_t* path, const wchar_t* section, const wchar_t* key, wchar_t* out,
        uint32_t capacity) noexcept;
    // Raw MH_STATUS values.
    int32_t (*hook_initialize)() noexcept;
    int32_t (*hook_create)(uintptr_t target, const void* detour, void** original) noexcept;
    int32_t (*hook_queue_enable)(uintptr_t target) noexcept;
    int32_t (*hook_apply_queued)() noexcept;
    int32_t (*hook_disable)(uintptr_t target) noexcept;
    int32_t (*hook_remove)(uintptr_t target) noexcept;
    const char* (*hook_status)(int32_t status) noexcept;
    uint32_t (*adapter_vendor)(uint32_t index) noexcept;
    uint32_t (*desktop_refresh)() noexcept;
    void* (*engine_manager)(uintptr_t function) noexcept;
    void* (*engine_owner)(uintptr_t function, void* manager) noexcept;
    void* (*engine_find)(uintptr_t function, void* manager, const uint32_t* ids, uint64_t count,
        void* scratch) noexcept;
    int32_t (*d3d12_trace)(uintptr_t target, uint32_t image_size, const wchar_t* path) noexcept;
    bool (*native_capture)(const wchar_t* directory) noexcept;
    // Null when the ASI is built without RenderDoc capture support.
    int32_t (*frame_probe)(const wchar_t* library, const wchar_t* prefix) noexcept;
    int32_t (*census)(const wchar_t* directory, uint32_t flags) noexcept;
    bool (*camera_probe)(const wchar_t* directory) noexcept;
    bool (*camera_reference)(const wchar_t* library) noexcept;
    bool (*social_club)(bool trace) noexcept;
    void (*start_worker)() noexcept;
};
static_assert(sizeof(wchar_t) == 2);
static_assert(sizeof(AsiPlatform) == 8 + 27 * sizeof(void*));
static_assert(offsetof(AsiPlatform, hook_initialize) == 8 + 7 * sizeof(void*));
static_assert(offsetof(AsiPlatform, adapter_vendor) == 8 + 14 * sizeof(void*));
static_assert(offsetof(AsiPlatform, d3d12_trace) == 8 + 19 * sizeof(void*));
static_assert(offsetof(AsiPlatform, start_worker) == 8 + 26 * sizeof(void*));

inline constexpr uint32_t asi_platform_version = 1;
}

extern "C" {
// DllMain process attach: admission, configuration gates and startup owners, then the worker.
void rsf_rdr2_asi_attach(const rsf::rdr2::AsiPlatform* platform) noexcept;
// Logs the startup results from the worker, outside the loader lock.
void rsf_rdr2_asi_report_startup(void (*log)(void*, const char*), void* user) noexcept;
}
