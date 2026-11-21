// SPDX-License-Identifier: MIT
#pragma once
#include "native_memory.h"
#include "native_profile.h"

struct rsf_anomaly_guard_image {
    const std::byte* base;
    uint32_t size;
    decltype(&rsf::anomaly::guarded_copy) read;
};
static_assert(sizeof(rsf_anomaly_guard_image) == 24 && offsetof(rsf_anomaly_guard_image, read) == 16);

extern "C" {
bool rsf_anomaly_guard_site(const rsf_anomaly_guard_image*, uint32_t rva, const std::byte* expected) noexcept;
bool rsf_anomaly_guard_hex(const rsf_anomaly_guard_image*, uint32_t rva, const char* hex) noexcept;
bool rsf_anomaly_guard_first_person(const rsf_anomaly_guard_image*, uint32_t set_p_call, uint32_t rm_near_call,
    uint32_t set_p, uint32_t rm_near) noexcept;
}

namespace rsf::anomaly {
inline rsf_anomaly_guard_image guard_image(const std::byte* base, uint32_t size) noexcept {
    return {base, size, guarded_copy};
}

inline bool matches(const rsf_anomaly_guard_image& image, const NativeSite& site) noexcept {
    return rsf_anomaly_guard_site(&image, site.rva, site.expected.data());
}

inline bool matches(const rsf_anomaly_guard_image& image, const UniqueGuard& guard) noexcept {
    return rsf_anomaly_guard_hex(&image, guard.rva, guard.expected_hex);
}

inline bool unique_guards_match(const rsf_anomaly_guard_image& image, std::span<const UniqueGuard> guards,
    uint32_t rva) noexcept {
    for (const auto& guard : guards)
        if (guard.rva == rva && !matches(image, guard)) return false;
    return true;
}
}
