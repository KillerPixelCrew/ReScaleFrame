// SPDX-License-Identifier: MIT
#pragma once
#include <cstddef>
#include <cstdint>
enum rsf_ac7_contact_shadow_result : int32_t {
    RSF_AC7_CONTACT_UNMATCHED = 0,
    RSF_AC7_CONTACT_PATCHED = 1,
    RSF_AC7_CONTACT_REFUSED = 2,
};

// Input contains the RHI prefix, followed by DXBC at offset and any engine trailer.
// On success the factory borrows the complete replacement and its DXBC CRC for the call.
extern "C" int32_t rsf_ac7_contact_shadow_correct(const uint8_t* bytes, size_t size, size_t offset,
    bool (*use_shader)(void*, const uint8_t*, size_t, uint32_t), void* user) noexcept;
