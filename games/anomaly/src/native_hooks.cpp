// SPDX-License-Identifier: MIT
#include "native_hooks.h"
#include "native_hook_ops.h"
#include "native_memory.h"

namespace rsf::anomaly {
namespace {
static_assert(sizeof(IMAGE_NT_HEADERS64) == 264 && IMAGE_DOS_SIGNATURE == 0x5a4d &&
    IMAGE_NT_SIGNATURE == 0x4550 && IMAGE_FILE_MACHINE_AMD64 == 0x8664 &&
    IMAGE_NT_OPTIONAL_HDR64_MAGIC == 0x20b);
static_assert(sizeof(DosFacts) == 8 && sizeof(PeFacts) == 12);
static_assert(sizeof(ImageHeaders) == 16 && offsetof(ImageHeaders, pe) == 8);
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

using Read = bool (*)(const void*, const void*, void*, size_t, bool) noexcept;

template<Read read>
constexpr ImageHeaders image_headers{
    [](const void* module, DosFacts* out) noexcept {
        IMAGE_DOS_HEADER dos{};
        if (!read(module, module, &dos, sizeof(dos), false)) return false;
        *out = {dos.e_lfanew, dos.e_magic};
        return true;
    },
    [](const void* module, const void* address, PeFacts* out) noexcept {
        IMAGE_NT_HEADERS64 pe{};
        if (!read(module, address, &pe, sizeof(pe), false)) return false;
        *out = {pe.Signature, pe.OptionalHeader.SizeOfImage, pe.FileHeader.Machine, pe.OptionalHeader.Magic};
        return true;
    },
};

bool read_guarded(const void*, const void* address, void* out, size_t size, bool) noexcept {
    return guarded_copy(out, address, size);
}

struct Platform {
    ImageHeaders headers;
    Read read;
    HookOps hooks;
};
static_assert(sizeof(Platform) == 64 && offsetof(Platform, hooks) == 24);
constexpr Platform platform{
    image_headers<read_mapped>,
    read_mapped,
    hook_ops,
};

extern "C" bool rsf_anomaly_hook_set_install(void**, const Platform*, std::byte*, uint32_t,
    const HookSite*, size_t) noexcept;
extern "C" bool rsf_anomaly_hook_set_install_resolved(void**, const Platform*, const ResolvedHookSite*, size_t) noexcept;
extern "C" bool rsf_anomaly_hook_set_set_enabled(void**, const Platform*, bool) noexcept;
extern "C" bool rsf_anomaly_hook_set_created(void* const*) noexcept;
}

const ImageHeaders guarded_image_headers = image_headers<read_guarded>;

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
