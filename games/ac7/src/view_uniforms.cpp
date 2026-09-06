// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/ac7_view.h>

#include <cmath>
#include <cstring>

namespace {

// Byte offsets into the view uniform buffer. Established by relationships that have to hold between
// fields rather than by reading a single buffer; see docs/research/ac7-frame-capture.md.
constexpr uint32_t offset_translated_world_to_view = 0x0C0;  // ViewToTranslatedWorld
constexpr uint32_t offset_view_to_clip = 0x180;
constexpr uint32_t offset_clip_to_view = 0x1C0;
constexpr uint32_t offset_view_forward = 0x300;
constexpr uint32_t offset_view_up = 0x310;
constexpr uint32_t offset_view_right = 0x320;
constexpr uint32_t offset_world_camera_origin = 0x370;
constexpr uint32_t offset_pre_view_translation = 0x3A0;
constexpr uint32_t offset_clip_to_prev_clip = 0x6E0;
// TemporalAAJitter: current x, current y, previous x, previous y, all in clip space. Located by
// differencing a run with the anti-aliasing gate patched against one without it, then confirmed
// against the two elements of ViewToClip the engine writes the same values into.
constexpr uint32_t offset_temporal_aa_jitter = 0x720;
constexpr uint32_t offset_view_rect_min = 0x7E0;
constexpr uint32_t offset_view_size = 0x7F0;
constexpr uint32_t offset_buffer_size = 0x800;

float at(const float* values, uint32_t byte_offset, uint32_t index = 0)
{
    return values[byte_offset / 4 + index];
}

void read_matrix(const float* values, uint32_t byte_offset, float* out)
{
    std::memcpy(out, values + byte_offset / 4, 16 * sizeof(float));
}

void read_vector(const float* values, uint32_t byte_offset, float* out)
{
    std::memcpy(out, values + byte_offset / 4, 3 * sizeof(float));
}

float element(const float* matrix, int row, int column)
{
    return matrix[row * 4 + column];
}

bool finite(float value)
{
    return value == value && std::fabs(value) < 1e30f;
}

// Gauss-Jordan with partial pivoting. Used only on ClipToPrevClip, which sits close to the
// identity, so there is no conditioning problem to worry about here.
bool invert(const float* source, float* out)
{
    double work[4][8];
    for (int row = 0; row < 4; ++row) {
        for (int column = 0; column < 4; ++column) {
            work[row][column] = source[row * 4 + column];
            work[row][column + 4] = row == column ? 1.0 : 0.0;
        }
    }

    for (int column = 0; column < 4; ++column) {
        int pivot = column;
        for (int row = column + 1; row < 4; ++row) {
            if (std::fabs(work[row][column]) > std::fabs(work[pivot][column])) {
                pivot = row;
            }
        }
        if (std::fabs(work[pivot][column]) < 1e-12) {
            return false;
        }
        if (pivot != column) {
            for (int k = 0; k < 8; ++k) {
                const double swap = work[column][k];
                work[column][k] = work[pivot][k];
                work[pivot][k] = swap;
            }
        }
        const double divisor = work[column][column];
        for (int k = 0; k < 8; ++k) {
            work[column][k] /= divisor;
        }
        for (int row = 0; row < 4; ++row) {
            if (row == column) {
                continue;
            }
            const double factor = work[row][column];
            if (factor == 0.0) {
                continue;
            }
            for (int k = 0; k < 8; ++k) {
                work[row][k] -= factor * work[column][k];
            }
        }
    }

    for (int row = 0; row < 4; ++row) {
        for (int column = 0; column < 4; ++column) {
            out[row * 4 + column] = static_cast<float>(work[row][column + 4]);
        }
    }
    return true;
}

// A size paired with its own reciprocal, which is what Unreal stores and what arbitrary bytes do
// not happen to contain.
bool is_size_and_inverse(const float* values, uint32_t byte_offset)
{
    for (uint32_t axis = 0; axis < 2; ++axis) {
        const float size = at(values, byte_offset, axis);
        const float inverse = at(values, byte_offset, axis + 2);
        if (!finite(size) || !finite(inverse) || size < 1.0f || size > 65536.0f) {
            return false;
        }
        if (std::fabs(size * inverse - 1.0f) > 1e-3f) {
            return false;
        }
    }
    return true;
}

bool is_view_buffer(const float* values)
{
    if (!is_size_and_inverse(values, offset_view_size) ||
        !is_size_and_inverse(values, offset_buffer_size)) {
        return false;
    }

    // The camera basis has to be orthonormal and has to be the rows of ViewToTranslatedWorld.
    // Reading the basis out of the projection instead gives a different vector that looks equally
    // plausible, which is why this agreement is what settles it.
    const uint32_t basis[3] = {offset_view_right, offset_view_up, offset_view_forward};
    for (uint32_t index = 0; index < 3; ++index) {
        float vector[3];
        read_vector(values, basis[index], vector);
        const float length =
            std::sqrt(vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]);
        if (!finite(length) || std::fabs(length - 1.0f) > 1e-3f) {
            return false;
        }
        for (uint32_t axis = 0; axis < 3; ++axis) {
            const float expected =
                at(values, offset_translated_world_to_view, index * 4 + axis);
            if (std::fabs(vector[axis] - expected) > 1e-3f) {
                return false;
            }
        }
    }

    // Translated world is world shifted so the camera sits at the origin.
    for (uint32_t axis = 0; axis < 3; ++axis) {
        const float camera = at(values, offset_world_camera_origin, axis);
        const float translation = at(values, offset_pre_view_translation, axis);
        if (!finite(camera) || !finite(translation) || std::fabs(camera + translation) > 1.0f) {
            return false;
        }
    }

    // The projection and its inverse must multiply to the identity.
    float view_to_clip[16];
    float clip_to_view[16];
    read_matrix(values, offset_view_to_clip, view_to_clip);
    read_matrix(values, offset_clip_to_view, clip_to_view);
    for (int row = 0; row < 4; ++row) {
        for (int column = 0; column < 4; ++column) {
            double sum = 0.0;
            for (int k = 0; k < 4; ++k) {
                sum += double(element(view_to_clip, row, k)) * element(clip_to_view, k, column);
            }
            const double expected = row == column ? 1.0 : 0.0;
            if (std::fabs(sum - expected) > 0.01) {
                return false;
            }
        }
    }
    return true;
}

} // namespace

extern "C" rsf_ac7_view_result rsf_ac7_view_read(const void* buffer, uint32_t bytes,
                                                 uint32_t abi_version, rsf_ac7_view* out)
{
    if (!buffer || !out || out->struct_size < sizeof(rsf_ac7_view)) {
        return RSF_AC7_VIEW_ERROR_INVALID_ARGUMENT;
    }
    if (abi_version != RSF_AC7_VIEW_ABI_VERSION) {
        return RSF_AC7_VIEW_ERROR_ABI_MISMATCH;
    }
    if (bytes != RSF_AC7_VIEW_BUFFER_BYTES) {
        return RSF_AC7_VIEW_ERROR_NOT_A_VIEW_BUFFER;
    }

    const auto* values = static_cast<const float*>(buffer);
    if (!is_view_buffer(values)) {
        return RSF_AC7_VIEW_ERROR_NOT_A_VIEW_BUFFER;
    }

    const uint32_t saved_size = out->struct_size;
    std::memset(out, 0, sizeof(rsf_ac7_view));
    out->struct_size = saved_size;

    read_matrix(values, offset_view_to_clip, out->view_to_clip);
    read_matrix(values, offset_clip_to_view, out->clip_to_view);
    read_matrix(values, offset_clip_to_prev_clip, out->clip_to_prev_clip);
    if (!invert(out->clip_to_prev_clip, out->prev_clip_to_clip)) {
        return RSF_AC7_VIEW_ERROR_NOT_A_VIEW_BUFFER;
    }

    read_vector(values, offset_world_camera_origin, out->camera_position);
    read_vector(values, offset_view_forward, out->camera_forward);
    read_vector(values, offset_view_up, out->camera_up);
    read_vector(values, offset_view_right, out->camera_right);

    // An orthographic view has no perspective divide, and the interface renders through one, so it
    // is a real thing to encounter rather than a malformed buffer.
    const float perspective = element(out->view_to_clip, 2, 3);
    if (std::fabs(perspective - 1.0f) > 1e-3f) {
        return RSF_AC7_VIEW_ERROR_NOT_PERSPECTIVE;
    }

    // Reversed Z against an infinite far plane: the near distance is what is left in the matrix,
    // and there is no far value to read.
    out->near_plane = element(out->view_to_clip, 3, 2);

    const float horizontal_scale = element(out->view_to_clip, 0, 0);
    const float vertical_scale = element(out->view_to_clip, 1, 1);
    if (horizontal_scale <= 0.0f || vertical_scale <= 0.0f) {
        return RSF_AC7_VIEW_ERROR_NOT_PERSPECTIVE;
    }
    out->vertical_fov = 2.0f * std::atan(1.0f / vertical_scale);
    out->aspect_ratio = vertical_scale / horizontal_scale;

    out->view_width = static_cast<uint32_t>(at(values, offset_view_size, 0) + 0.5f);
    out->view_height = static_cast<uint32_t>(at(values, offset_view_size, 1) + 0.5f);
    out->buffer_width = static_cast<uint32_t>(at(values, offset_buffer_size, 0) + 0.5f);
    out->buffer_height = static_cast<uint32_t>(at(values, offset_buffer_size, 1) + 0.5f);
    out->view_rect_x = static_cast<uint32_t>(at(values, offset_view_rect_min, 0) + 0.5f);
    out->view_rect_y = static_cast<uint32_t>(at(values, offset_view_rect_min, 1) + 0.5f);

    // Clip space to pixels, dividing by the view rect rather than the buffer. Those differ once
    // the render scale moves, and using the buffer would scale every offset by the render scale
    // without ever looking wrong.
    const float clip_x = at(values, offset_temporal_aa_jitter, 0);
    const float clip_y = at(values, offset_temporal_aa_jitter, 1);
    const float previous_clip_x = at(values, offset_temporal_aa_jitter, 2);
    const float previous_clip_y = at(values, offset_temporal_aa_jitter, 3);
    const float half_width = float(out->view_width) * 0.5f;
    const float half_height = float(out->view_height) * 0.5f;
    out->jitter_pixels[0] = clip_x * half_width;
    out->jitter_pixels[1] = clip_y * -half_height;
    out->previous_jitter_pixels[0] = previous_clip_x * half_width;
    out->previous_jitter_pixels[1] = previous_clip_y * -half_height;
    out->has_jitter = (clip_x != 0.0f || clip_y != 0.0f) ? 1u : 0u;

    // The engine adds the jitter to these two elements of the projection and keeps no copy without
    // it, so taking it back out is how a backend gets the matrix it requires.
    std::memcpy(out->view_to_clip_no_jitter, out->view_to_clip, sizeof(out->view_to_clip));
    out->view_to_clip_no_jitter[2 * 4 + 0] -= clip_x;
    out->view_to_clip_no_jitter[2 * 4 + 1] -= clip_y;

    // The main view fills its target. The smaller ones the engine renders into the same target do
    // not, and their camera describes something the player is not looking through.
    out->is_main_view = (out->view_width == out->buffer_width &&
                         out->view_height == out->buffer_height && out->view_rect_x == 0 &&
                         out->view_rect_y == 0)
                            ? 1u
                            : 0u;
    return RSF_AC7_VIEW_OK;
}
