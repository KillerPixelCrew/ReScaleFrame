// SPDX-License-Identifier: MIT
#pragma once

#include <cstddef>
#include <cstdint>

// Success transfers one COM reference for each returned interface.
extern "C" bool rsf_anomaly_sr_fixture_device(bool hardware_only, std::uint32_t vendor, bool debug,
    void** device, void** context, void** info) noexcept;

extern "C" bool rsf_anomaly_sr_fixture_readback(void* device, void* context, void* source,
    void* staging, std::uint8_t* output, std::size_t output_size) noexcept;
