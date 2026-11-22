// SPDX-License-Identifier: MIT
#pragma once
#include <array>
#include <compare>
#include <cstddef>
#include <cstdint>
#include <stdexcept>

namespace rsf::anomaly::temporal {
enum class ViewId : std::uint8_t { Main, Scope };
enum class ProjectionDomain : std::uint8_t { World, FirstPerson };

struct InstanceKey {
    std::uint64_t owner{}, visual{}, generation{};
    auto operator<=>(const InstanceKey&) const = default;
};

// Native fixture and geometry packet representation, row-major with column vectors.
struct Mat4 {
    std::array<double, 16> values{};
    double& at(std::size_t row, std::size_t column) {
        if (row >= 4 || column >= 4) throw std::out_of_range("Mat4 index");
        return values[row * 4 + column];
    }
    double at(std::size_t row, std::size_t column) const {
        if (row >= 4 || column >= 4) throw std::out_of_range("Mat4 index");
        return values[row * 4 + column];
    }
    static Mat4 identity() {
        Mat4 result;
        for (std::size_t i = 0; i < 4; ++i) result.at(i, i) = 1;
        return result;
    }
    bool operator==(const Mat4&) const = default;
};
}
