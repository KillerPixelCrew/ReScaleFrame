// SPDX-License-Identifier: MIT
#include "native_hooks.h"
#include "native_memory.h"
#include <MinHook.h>

namespace rsf::anomaly {
namespace {
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
static_assert(sizeof(IMAGE_NT_HEADERS64) == 264 && IMAGE_DOS_SIGNATURE == 0x5a4d &&
    IMAGE_NT_SIGNATURE == 0x4550 && IMAGE_FILE_MACHINE_AMD64 == 0x8664 &&
    IMAGE_NT_OPTIONAL_HDR64_MAGIC == 0x20b);
static_assert(sizeof(DosFacts) == 8 && sizeof(PeFacts) == 12);
static_assert(sizeof(HookSite) == 48 && sizeof(ResolvedHookSite) == 24);

bool read_mapped(const void* owner, const void* address, void* out, size_t size, bool executable) noexcept {
    if (!address || !size) return false;
    MEMORY_BASIC_INFORMATION info{};
    if (!VirtualQuery(address, &info, sizeof(info)) || info.State != MEM_COMMIT ||
        info.AllocationBase != owner || (info.Protect & (PAGE_GUARD | PAGE_NOACCESS))) return false;
    const auto start = reinterpret_cast<uintptr_t>(address);
    const auto region = reinterpret_cast<uintptr_t>(info.BaseAddress);
    if (start < region || start - region > info.RegionSize || size > info.RegionSize - (start - region))
        return false;
    const auto protection = info.Protect & 0xff;
    const bool exec_read = protection == PAGE_EXECUTE_READ || protection == PAGE_EXECUTE_READWRITE ||
        protection == PAGE_EXECUTE_WRITECOPY;
    const bool data_read = protection == PAGE_READONLY || protection == PAGE_READWRITE ||
        protection == PAGE_WRITECOPY;
    return (exec_read || (!executable && data_read)) && guarded_copy(out, address, size);
}

struct Platform {
    bool (*dos)(const void*, DosFacts*) noexcept;
    bool (*pe)(const void*, const void*, PeFacts*) noexcept;
    bool (*read)(const void*, const void*, void*, size_t, bool) noexcept;
    bool (*initialize)() noexcept;
    bool (*create)(void*, void*, void**) noexcept;
    void (*remove_unqueued)(void*) noexcept;
    bool (*queue)(void*, bool) noexcept;
    bool (*apply)() noexcept;
};
constexpr Platform platform{
    [](const void* module, DosFacts* out) noexcept {
        IMAGE_DOS_HEADER dos{};
        if (!read_mapped(module, module, &dos, sizeof(dos), false)) return false;
        *out = {dos.e_lfanew, dos.e_magic};
        return true;
    },
    [](const void* module, const void* address, PeFacts* out) noexcept {
        IMAGE_NT_HEADERS64 pe{};
        if (!read_mapped(module, address, &pe, sizeof(pe), false)) return false;
        *out = {pe.Signature, pe.OptionalHeader.SizeOfImage, pe.FileHeader.Machine, pe.OptionalHeader.Magic};
        return true;
    },
    read_mapped,
    []() noexcept {
        const auto status = MH_Initialize();
        return status == MH_OK || status == MH_ERROR_ALREADY_INITIALIZED;
    },
    [](void* target, void* replacement, void** original) noexcept {
        return MH_CreateHook(target, replacement, original) == MH_OK;
    },
    // An unqueued, never enabled entry has no disable step that can fail.
    [](void* target) noexcept { (void)MH_RemoveHook(target); },
    [](void* target, bool enable) noexcept {
        return (enable ? MH_QueueEnableHook(target) : MH_QueueDisableHook(target)) == MH_OK;
    },
    []() noexcept { return MH_ApplyQueued() == MH_OK; },
};

extern "C" bool rsf_anomaly_hook_set_install(void**, const Platform*, std::byte*, uint32_t,
    const HookSite*, size_t) noexcept;
extern "C" bool rsf_anomaly_hook_set_install_resolved(void**, const Platform*, const ResolvedHookSite*, size_t) noexcept;
extern "C" bool rsf_anomaly_hook_set_set_enabled(void**, const Platform*, bool) noexcept;
extern "C" bool rsf_anomaly_hook_set_created(void* const*) noexcept;
}

bool HookSet::install(std::byte* module, uint32_t image_size, std::span<const HookSite> sites) noexcept {
    return rsf_anomaly_hook_set_install(&batch_, &platform, module, image_size, sites.data(), sites.size());
}
bool HookSet::install_resolved(std::span<const ResolvedHookSite> sites) noexcept {
    return rsf_anomaly_hook_set_install_resolved(&batch_, &platform, sites.data(), sites.size());
}
bool HookSet::enable() noexcept {
    return rsf_anomaly_hook_set_set_enabled(&batch_, &platform, true);
}
bool HookSet::disable() noexcept {
    return rsf_anomaly_hook_set_set_enabled(&batch_, &platform, false);
}
bool HookSet::created() const noexcept {
    return rsf_anomaly_hook_set_created(&batch_);
}
}
