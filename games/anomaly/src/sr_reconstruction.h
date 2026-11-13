// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// The binder first writes the stock vector through R_constants::GetCBuffer and dx10ConstantBuffer::set. Correct its
// CPU staging copy before Flush uploads it, preserving the engine's stage/slot ownership and dirty protocol.
#include "geometry_profile.h"
#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>

namespace rsf::anomaly::sr {
using PositionVector = std::array<float, 4>;
inline constexpr uint32_t constant_destination_offset = 0x10, constant_load_offset = 0x14;
inline constexpr uint32_t constant_buffer_changed_offset = 0x68, backend_constants_offset = 0x5c0;
inline constexpr std::array<uint32_t, 6> constant_stage_bits{1, 2, 8, 16, 32, 64};
inline constexpr std::array<uint32_t, 6> constant_stage_shifts{16, 12, 8, 20, 24, 28};

inline bool correct_position_vector(PositionVector& value, float x, float y) noexcept {
    for (float component : value) if (!std::isfinite(component)) return false;
    if (!std::isfinite(x) || !std::isfinite(y)) return false;
    value[0] += x * value[2];
    value[1] += y * value[3];
    return std::isfinite(value[0]) && std::isfinite(value[1]);
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
    if (!constant) return false;
    auto* c = static_cast<std::byte*>(constant);
    uint32_t destination{};
    if (!host.read(c + constant_destination_offset, &destination, sizeof(destination)) ||
        !(destination & 0x7b) || (destination & 0x84)) return false;
    struct Slot { std::byte* data{}; std::byte* dirty{}; PositionVector stock{}, corrected{}; uint8_t was_dirty{}; };
    std::array<Slot, 6> slots{};
    std::size_t count{};
    // Preflight every stage before touching any data. Indices are byte offsets, not float/vector indices.
    for (uint32_t stage = 0; stage < constant_stage_bits.size(); ++stage) {
        if (!(destination & constant_stage_bits[stage])) continue;
        if (((destination >> constant_stage_shifts[stage]) & 15u) >= geometry::constant_buffer_slots) return false;
        uint16_t load[2]{};
        if (!host.read(c + constant_load_offset + stage * 4, load, sizeof(load)) || load[1] != 1 || (load[0] & 3u)) return false;
        auto* cb = host.buffer(constant, stage);
        uint32_t size{};
        std::byte* data{};
        if (!cb || !host.read(cb + geometry::constant_buffer_size_offset, &size, sizeof(size)) ||
            !host.read(cb + geometry::constant_buffer_data_offset, &data, sizeof(data)) || !data ||
            size < load[0] || size - load[0] < sizeof(PositionVector)) return false;
        Slot slot{data + load[0], cb + constant_buffer_changed_offset};
        if (!host.read(slot.data, slot.stock.data(), sizeof(slot.stock)) ||
            !host.read(slot.dirty, &slot.was_dirty, sizeof(slot.was_dirty))) return false;
        slot.corrected = slot.stock;
        if (!correct_position_vector(slot.corrected, x, y)) return false;
        // Merged stage constants can alias the same CPU storage. Correct that storage once, never twice.
        bool duplicate{};
        for (std::size_t i = 0; i < count; ++i) {
            if (slots[i].data != slot.data) continue;
            if (slots[i].dirty != slot.dirty) return false;
            duplicate = true;
        }
        if (!duplicate) slots[count++] = slot;
    }
    const uint8_t changed = 1;
    for (std::size_t i = 0; i < count; ++i) {
        if (host.write(slots[i].data, slots[i].corrected.data(), sizeof(PositionVector)) &&
            host.write(slots[i].dirty, &changed, sizeof(changed))) continue;
        // Keep a partial correction from mixing stock and corrected stages if a protected memory access failed.
        for (std::size_t j = 0; j <= i; ++j) {
            (void)host.write(slots[j].data, slots[j].stock.data(), sizeof(PositionVector));
            (void)host.write(slots[j].dirty, &slots[j].was_dirty, sizeof(slots[j].was_dirty));
        }
        return false;
    }
    return count != 0;
}
}
