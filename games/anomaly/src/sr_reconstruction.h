// SPDX-License-Identifier: MIT
#pragma once
// The binder first writes the stock vector through R_constants::GetCBuffer and dx10ConstantBuffer::set. Correct its
// CPU staging copy before Flush uploads it, preserving the engine's stage/slot ownership and dirty protocol.
#include "geometry_profile.h"
#include <array>
#include <cstddef>
#include <cstdint>

namespace rsf::anomaly::sr {
using PositionVector = std::array<float, 4>;
inline constexpr uint32_t constant_destination_offset = 0x10, constant_load_offset = 0x14;
inline constexpr uint32_t constant_buffer_changed_offset = 0x68, backend_constants_offset = 0x5c0;
inline constexpr std::array<uint32_t, 6> constant_stage_bits{1, 2, 8, 16, 32, 64};

struct PositionLayout {
    uint32_t destination, load, buffer_size, buffer_data, buffer_changed, buffer_slots;
    std::array<uint32_t, 6> stage_bits, stage_shifts;
};
struct PositionPlatform {
    void* user;
    bool (*read)(void*, uintptr_t, void*, std::size_t);
    bool (*write)(void*, uintptr_t, const void*, std::size_t);
    uintptr_t (*buffer)(void*, uintptr_t, uint32_t);
};
static_assert(sizeof(PositionLayout) == 72 && sizeof(PositionPlatform) == 32);
extern "C" {
bool rsf_anomaly_sr_correct_position_vector(float*, float, float) noexcept;
bool rsf_anomaly_sr_correct_position_constant(const PositionLayout*, const PositionPlatform*, uintptr_t, float, float) noexcept;
}

inline bool correct_position_vector(PositionVector& value, float x, float y) noexcept {
    return rsf_anomaly_sr_correct_position_vector(value.data(), x, y);
}

class PositionConstantHost {
public:
    virtual bool read(const void*, void*, std::size_t) noexcept = 0;
    virtual bool write(void*, const void*, std::size_t) noexcept = 0;
    virtual std::byte* buffer(void* constant, uint32_t stage) noexcept = 0;
protected:
    ~PositionConstantHost() = default;
};
inline bool correct_position_constant(PositionConstantHost& host, void* constant, float x, float y) noexcept {
    static constexpr PositionLayout layout{constant_destination_offset, constant_load_offset, geometry::constant_buffer_size_offset,
        geometry::constant_buffer_data_offset, constant_buffer_changed_offset, geometry::constant_buffer_slots,
        constant_stage_bits, {16, 12, 8, 20, 24, 28}};
    const PositionPlatform platform{&host,
        [](void* user, uintptr_t address, void* out, std::size_t size) {
            return static_cast<PositionConstantHost*>(user)->read(reinterpret_cast<const void*>(address), out, size);
        },
        [](void* user, uintptr_t address, const void* in, std::size_t size) {
            return static_cast<PositionConstantHost*>(user)->write(reinterpret_cast<void*>(address), in, size);
        },
        [](void* user, uintptr_t constant_address, uint32_t stage) {
            return reinterpret_cast<uintptr_t>(
                static_cast<PositionConstantHost*>(user)->buffer(reinterpret_cast<void*>(constant_address), stage));
        }};
    return rsf_anomaly_sr_correct_position_constant(&layout, &platform, reinterpret_cast<uintptr_t>(constant), x, y);
}
}
