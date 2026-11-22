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

extern "C" bool rsf_anomaly_hook_read_mapped(const void*, const void*, void*, size_t, bool,
    decltype(&guarded_copy)) noexcept;

bool read_mapped(const void* owner, const void* address, void* out, size_t size, bool executable) noexcept {
    return rsf_anomaly_hook_read_mapped(owner, address, out, size, executable, guarded_copy);
}

using Read = bool (*)(const void*, const void*, void*, size_t, bool) noexcept;

extern "C" bool rsf_anomaly_hook_read_dos(const void*, DosFacts*, bool, decltype(&guarded_copy)) noexcept;
extern "C" bool rsf_anomaly_hook_read_pe(const void*, const void*, PeFacts*, bool, decltype(&guarded_copy)) noexcept;

template<bool mapped>
constexpr ImageHeaders image_headers{
    [](const void* module, DosFacts* out) noexcept {
        return rsf_anomaly_hook_read_dos(module, out, mapped, guarded_copy);
    },
    [](const void* module, const void* address, PeFacts* out) noexcept {
        return rsf_anomaly_hook_read_pe(module, address, out, mapped, guarded_copy);
    },
};

struct Platform {
    ImageHeaders headers;
    Read read;
    HookOps hooks;
};
static_assert(sizeof(Platform) == 64 && offsetof(Platform, hooks) == 24);
constexpr Platform platform{
    image_headers<true>,
    read_mapped,
    hook_ops,
};

extern "C" bool rsf_anomaly_hook_set_install(void**, const Platform*, std::byte*, uint32_t,
    const HookSite*, size_t) noexcept;
extern "C" bool rsf_anomaly_hook_set_install_resolved(void**, const Platform*, const ResolvedHookSite*, size_t) noexcept;
extern "C" bool rsf_anomaly_hook_set_set_enabled(void**, const Platform*, bool) noexcept;
extern "C" bool rsf_anomaly_hook_set_created(void* const*) noexcept;
}

const ImageHeaders guarded_image_headers = image_headers<false>;

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

#if defined(RSF_ANOMALY_HOOK_FIXTURE)
// The native hook contract exercises the production MinHook operations.
extern "C" const void* rsf_anomaly_hook_native_platform() noexcept {
    return &rsf::anomaly::platform;
}
#endif
