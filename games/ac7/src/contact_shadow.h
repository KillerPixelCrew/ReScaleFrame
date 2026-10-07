// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <cstddef>
#include <cstdint>
#include <vector>

// unmatched: another shader; patched: output owns corrected bytes; refused: fingerprint matched
// but token/container validation or allocation failed, so the caller must keep original bytes.
enum class rsf_ac7_contact_shadow_result { unmatched, patched, refused };

// Internal plugin helper for AC7's directional light shader (SHA-256 60a2ca61...): adds the
// View's noise phase and a depth quantisation bias to the contact-shadow ray. Input starts at
// DXBC; any engine trailer is retained. Output owns its bytes and is empty on refusal or an
// unmatched shader.
rsf_ac7_contact_shadow_result rsf_ac7_contact_shadow_correct(
    const void* bytes, size_t size, std::vector<uint8_t>& output) noexcept;
// CRC-32C of the DXBC container, matching the shader hash captures record.
uint32_t rsf_ac7_contact_shadow_crc(const void* bytes, size_t size) noexcept;
