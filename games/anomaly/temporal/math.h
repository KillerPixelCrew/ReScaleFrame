#pragma once

#include "planning.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstddef>
#include <optional>
#include <stdexcept>

namespace rsf::anomaly::temporal {

struct Vec2 {
    double x{}, y{};
    bool operator==(const Vec2&) const = default;
};
struct Vec3 {
    double x{}, y{}, z{};
    bool operator==(const Vec3&) const = default;
};
struct Vec4 {
    double x{}, y{}, z{}, w{};
    bool operator==(const Vec4&) const = default;
};

// Row-major storage, COLUMN vectors: clip = projection * view * world * position.
// LH camera (+Z forward), D3D depth [0,1]. Engine/SDK transposes belong at the hook boundary.
struct Mat4 {
    std::array<double, 16> values{};
    double& at(std::size_t row, std::size_t column)
    {
        if (row >= 4 || column >= 4) {
            throw std::out_of_range("Mat4 index");
        }
        return values[row * 4 + column];
    }
    double at(std::size_t row, std::size_t column) const
    {
        if (row >= 4 || column >= 4) {
            throw std::out_of_range("Mat4 index");
        }
        return values[row * 4 + column];
    }
    static Mat4 identity()
    {
        Mat4 result;
        for (std::size_t i = 0; i < 4; ++i) {
            result.at(i, i) = 1;
        }
        return result;
    }
    bool operator==(const Mat4&) const = default;
};

inline bool finite(Vec2 v) { return std::isfinite(v.x) && std::isfinite(v.y); }
inline bool finite(Vec3 v) { return std::isfinite(v.x) && std::isfinite(v.y) && std::isfinite(v.z); }
inline bool finite(Vec4 v)
{
    return std::isfinite(v.x) && std::isfinite(v.y) && std::isfinite(v.z) && std::isfinite(v.w);
}
inline bool finite(const Mat4& m)
{
    for (const auto value : m.values) {
        if (!std::isfinite(value)) {
            return false;
        }
    }
    return true;
}

inline std::optional<Mat4> multiply(const Mat4& a, const Mat4& b)
{
    if (!finite(a) || !finite(b)) {
        return std::nullopt;
    }
    Mat4 result;
    for (std::size_t row = 0; row < 4; ++row) {
        for (std::size_t column = 0; column < 4; ++column) {
            for (std::size_t k = 0; k < 4; ++k) {
                result.at(row, column) += a.at(row, k) * b.at(k, column);
            }
        }
    }
    return finite(result) ? std::optional{result} : std::nullopt;
}

// Scaled partial-pivot Gauss-Jordan; reject singular/numerically degenerate matrices.
inline std::optional<Mat4> inverse(const Mat4& matrix)
{
    if (!finite(matrix)) {
        return std::nullopt;
    }
    std::array<std::array<double, 8>, 4> augmented{};
    std::array<double, 4> scale{};
    for (std::size_t row = 0; row < 4; ++row) {
        for (std::size_t column = 0; column < 4; ++column) {
            augmented[row][column] = matrix.at(row, column);
            scale[row] = std::max(scale[row], std::abs(matrix.at(row, column)));
        }
        if (scale[row] == 0) {
            return std::nullopt;
        }
        augmented[row][row + 4] = 1;
    }
    for (std::size_t column = 0; column < 4; ++column) {
        std::size_t pivot = column;
        for (std::size_t row = column + 1; row < 4; ++row) {
            if (std::abs(augmented[row][column]) / scale[row] >
                std::abs(augmented[pivot][column]) / scale[pivot]) {
                pivot = row;
            }
        }
        if (std::abs(augmented[pivot][column]) / scale[pivot] <= 1e-12) {
            return std::nullopt;
        }
        std::swap(augmented[pivot], augmented[column]);
        std::swap(scale[pivot], scale[column]);
        const auto divisor = augmented[column][column];
        for (auto& value : augmented[column]) {
            value /= divisor;
        }
        for (std::size_t row = 0; row < 4; ++row) {
            if (row == column) {
                continue;
            }
            const auto factor = augmented[row][column];
            for (std::size_t element = 0; element < 8; ++element) {
                augmented[row][element] -= factor * augmented[column][element];
            }
        }
    }
    Mat4 result;
    for (std::size_t row = 0; row < 4; ++row) {
        for (std::size_t column = 0; column < 4; ++column) {
            result.at(row, column) = augmented[row][column + 4];
        }
    }
    return finite(result) ? std::optional{result} : std::nullopt;
}

inline std::optional<Vec4> transform(const Mat4& matrix, Vec4 point)
{
    if (!finite(matrix) || !finite(point)) {
        return std::nullopt;
    }
    const std::array<double, 4> p{point.x, point.y, point.z, point.w};
    std::array<double, 4> result{};
    for (std::size_t row = 0; row < 4; ++row) {
        for (std::size_t column = 0; column < 4; ++column) {
            result[row] += matrix.at(row, column) * p[column];
        }
    }
    const Vec4 output{result[0], result[1], result[2], result[3]};
    return finite(output) ? std::optional{output} : std::nullopt;
}

// Pixel jitter: +X right, +Y down in RENDER pixels, never output pixels.
inline std::optional<Mat4> jitter_projection(const Mat4& unjittered, Vec2 pixels, Extent render)
{
    if (!finite(unjittered) || !finite(pixels) || !valid_extent(render)) {
        return std::nullopt;
    }
    auto result = unjittered;
    for (std::size_t column = 0; column < 4; ++column) {
        result.at(0, column) += (2 * pixels.x / render.width) * unjittered.at(3, column);
        result.at(1, column) -= (2 * pixels.y / render.height) * unjittered.at(3, column);
    }
    return finite(result) ? std::optional{result} : std::nullopt;
}

inline std::optional<Vec2> clip_uv(Vec4 clip)
{
    // Reject behind/at eye and depth-clipped points. Offscreen XY remains useful motion.
    if (!finite(clip) || clip.w <= 1e-9 || clip.z < 0 || clip.z > clip.w) {
        return std::nullopt;
    }
    const Vec2 uv{0.5 * (clip.x / clip.w + 1), 0.5 * (1 - clip.y / clip.w)};
    return finite(uv) ? std::optional{uv} : std::nullopt;
}

// Current -> previous in motion-grid pixels, jitter excluded. No velocity fabrication on failure.
inline std::optional<Vec2> pixel_motion(Vec4 current_unjittered, Vec4 previous_unjittered, Extent grid)
{
    const auto current = clip_uv(current_unjittered);
    const auto previous = clip_uv(previous_unjittered);
    if (!current || !previous || !valid_extent(grid)) {
        return std::nullopt;
    }
    const Vec2 result{(previous->x - current->x) * grid.width,
        (previous->y - current->y) * grid.height};
    return finite(result) ? std::optional{result} : std::nullopt;
}

// Both clips must have jitter applied in the same render grid; resize invalidates history.
inline std::optional<Vec2> pixel_motion_from_jittered(Vec4 current, Vec4 previous,
    Extent render, Vec2 current_jitter, Vec2 previous_jitter)
{
    const auto motion = pixel_motion(current, previous, render);
    if (!motion || !finite(current_jitter) || !finite(previous_jitter)) {
        return std::nullopt;
    }
    const Vec2 result{motion->x + current_jitter.x - previous_jitter.x,
        motion->y + current_jitter.y - previous_jitter.y};
    return finite(result) ? std::optional{result} : std::nullopt;
}

} // namespace rsf::anomaly::temporal
