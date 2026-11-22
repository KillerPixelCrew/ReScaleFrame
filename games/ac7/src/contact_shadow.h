// SPDX-License-Identifier: MIT
#pragma once
#include <cstddef>
#include <cstdint>
enum rsf_ac7_contact_shadow_result : int32_t {
    RSF_AC7_CONTACT_UNMATCHED = 0,
    RSF_AC7_CONTACT_PATCHED = 1,
    RSF_AC7_CONTACT_REFUSED = 2,
};

// Internal plugin helper for AC7's directional light shader (SHA-256 60a2ca61...): adds the
// View's noise phase and a depth quantisation bias to the contact-shadow ray. Input starts at
// DXBC; any engine trailer is retained. The caller provides writable storage on demand
// and uses it only when the result is RSF_AC7_CONTACT_PATCHED.
extern "C" int32_t rsf_ac7_contact_shadow_correct(const uint8_t* bytes, size_t size,
    uint8_t* (*resize)(void*, size_t), void* user) noexcept;
// CRC-32C of the DXBC container, matching the shader hash captures record.
extern "C" uint32_t rsf_ac7_contact_shadow_crc(const uint8_t* bytes, size_t size) noexcept;
