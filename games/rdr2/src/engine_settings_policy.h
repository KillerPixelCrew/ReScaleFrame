// SPDX-License-Identifier: MIT
#pragma once
#include <cstddef>
#include <cstdint>
#include <cstring>

// The policy is owned by the Rust crate rsf-rdr2 (games/rdr2/src/engine_settings.rs). These
// declare its C ABI and keep the former signatures for the native engine-settings fixture.
extern "C" {
uint32_t rsf_rdr2_dlss_choice_abi(const uint32_t* modes, uint32_t count, uint32_t quality) noexcept;
bool rsf_rdr2_pin_engine_settings_abi(void* collection, size_t bytes, uint32_t vendor, uint32_t quality) noexcept;
bool rsf_rdr2_storable_refresh_abi(uint32_t* numerator, uint32_t* denominator, uint32_t desktop_hz) noexcept;
}

// Index in the active DLSS capability list whose mode serves an RSF quality, or UINT32_MAX.
inline uint32_t rsf_rdr2_dlss_choice(const uint32_t* modes, uint32_t count, uint32_t quality) noexcept
{
    return rsf_rdr2_dlss_choice_abi(modes, count, quality);
}
// Exact 1491.50 generated collection/parser layout. This changes only the native input carrier
// and marker pump, not RSF preferences or other graphics settings.
inline bool rsf_rdr2_pin_engine_settings(void* collection, size_t bytes, uint32_t vendor, uint32_t quality = 2) noexcept
{
    return rsf_rdr2_pin_engine_settings_abi(collection, bytes, vendor, quality);
}
// RAGE saves the refresh rational clamped to this value (engine_settings.rs REFRESH_LIMIT).
inline constexpr uint32_t rsf_rdr2_refresh_limit = 262144;
inline bool rsf_rdr2_storable_refresh(uint32_t& numerator, uint32_t& denominator, uint32_t desktop_hz) noexcept
{
    return rsf_rdr2_storable_refresh_abi(&numerator, &denominator, desktop_hz);
}
