// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/ac7_view.h>
#include <vector>
#include <initializer_list>
#include <cstdint>
namespace ac7_fixture {
constexpr uint32_t kViewToTranslatedWorld = 0x0C0;
constexpr uint32_t kViewToClip = 0x180;
constexpr uint32_t kClipToView = 0x1C0;
constexpr uint32_t kViewForward = 0x300;
constexpr uint32_t kViewUp = 0x310;
constexpr uint32_t kViewRight = 0x320;
constexpr uint32_t kWorldCameraOrigin = 0x370;
constexpr uint32_t kPreViewTranslation = 0x3A0;
constexpr uint32_t kClipToPrevClip = 0x6E0;
constexpr uint32_t kTemporalAAJitter = 0x720;
constexpr uint32_t kViewRectMin = 0x7E0;
constexpr uint32_t kViewSize = 0x7F0;
constexpr uint32_t kBufferSize = 0x800;

struct Buffer {
    // Parentheses, not braces: braces here would build a two element vector holding 1024 and 0.
    std::vector<float> values = std::vector<float>(RSF_AC7_VIEW_BUFFER_BYTES / 4, 0.0f);

    void put(uint32_t offset, std::initializer_list<float> data)
    {
        uint32_t index = offset / 4;
        for (float value : data) {
            values[index++] = value;
        }
    }

    const void* bytes() const { return values.data(); }
};

// A view buffer that satisfies every relationship the reader tests. The camera basis is a real
// rotation rather than the identity, so a reader that transposed a matrix would be caught.
Buffer make_view(float horizontal_scale, float vertical_scale, float near_plane,
                 uint32_t view_width, uint32_t view_height, uint32_t buffer_width,
                 uint32_t buffer_height)
{
    Buffer buffer;

    const float right[3] = {0.0f, 1.0f, 0.0f};
    const float up[3] = {0.0f, 0.0f, 1.0f};
    const float forward[3] = {1.0f, 0.0f, 0.0f};

    // Rows of ViewToTranslatedWorld are the basis, in Unreal's order where Z is forward.
    buffer.put(kViewToTranslatedWorld,
               {right[0], right[1], right[2], 0.0f, up[0], up[1], up[2], 0.0f, forward[0],
                forward[1], forward[2], 0.0f, 0.0f, 0.0f, 0.0f, 1.0f});
    buffer.put(kViewRight, {right[0], right[1], right[2], 0.0f});
    buffer.put(kViewUp, {up[0], up[1], up[2], 0.0f});
    buffer.put(kViewForward, {forward[0], forward[1], forward[2], 0.0f});

    // Reversed Z with an infinite far plane, the form the game uses.
    buffer.put(kViewToClip, {horizontal_scale, 0.0f, 0.0f, 0.0f,
                             0.0f, vertical_scale, 0.0f, 0.0f,
                             0.0f, 0.0f, 0.0f, 1.0f,
                             0.0f, 0.0f, near_plane, 0.0f});
    buffer.put(kClipToView, {1.0f / horizontal_scale, 0.0f, 0.0f, 0.0f,
                             0.0f, 1.0f / vertical_scale, 0.0f, 0.0f,
                             0.0f, 0.0f, 0.0f, 1.0f / near_plane,
                             0.0f, 0.0f, 1.0f, 0.0f});

    const float camera[3] = {260968.45f, 107399.90f, 21228.20f};
    buffer.put(kWorldCameraOrigin, {camera[0], camera[1], camera[2], 0.0f});
    buffer.put(kPreViewTranslation, {-camera[0], -camera[1], -camera[2], 0.0f});

    // A small camera movement, the shape ClipToPrevClip actually takes.
    buffer.put(kClipToPrevClip, {1.0f, -0.00009f, 0.0f, 0.0f,
                                 0.00003f, 1.0f, 0.0f, 0.0f,
                                 -0.02808f, -0.20304f, 1.0f, 17.49332f,
                                 -0.00001f, 0.00003f, 0.0f, 1.0f});

    buffer.put(kViewRectMin, {0.0f, 0.0f, 0.0f, 0.0f});
    buffer.put(kViewSize, {float(view_width), float(view_height), 1.0f / float(view_width),
                           1.0f / float(view_height)});
    buffer.put(kBufferSize, {float(buffer_width), float(buffer_height),
                             1.0f / float(buffer_width), 1.0f / float(buffer_height)});
    return buffer;
}

}
