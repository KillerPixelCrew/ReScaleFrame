// SPDX-License-Identifier: MIT
#pragma once

#include <cstddef>
#include <cstdint>

extern "C" bool rsf_anomaly_sr_fixture_readback(void* device, void* context, void* source,
    void* staging, std::uint8_t* output, std::size_t output_size) noexcept;
