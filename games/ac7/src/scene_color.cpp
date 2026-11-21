/* SPDX-License-Identifier: MIT */
#include <rescaleframe/ac7_scene_color.h>
#include <rescaleframe/resource_ref.h>

#include <dxgiformat.h>

static_assert(DXGI_FORMAT_R16G16B16A16_FLOAT == 10 && DXGI_FORMAT_R11G11B10_FLOAT == 26);
static_assert(DXGI_FORMAT_R8G8B8A8_UNORM == 28 && DXGI_FORMAT_R8G8B8A8_UNORM_SRGB == 29);

struct rsf_ac7_scene_resource_calls {
    void (*retain)(void*);
    void (*release)(void*);
};
static_assert(sizeof(rsf_ac7_scene_resource_calls) == 16);
static constexpr rsf_ac7_scene_resource_calls calls{rsf_resource_retain, rsf_resource_release};

extern "C" {
void rsf_core_ac7_scene_color_clear(rsf_ac7_scene_color*, const rsf_ac7_scene_resource_calls*);
int rsf_core_ac7_scene_color_source(rsf_ac7_scene_color*, void*, void*, uint32_t, uint32_t,
    const rsf_ac7_scene_resource_calls*);
void rsf_core_ac7_scene_color_draw(rsf_ac7_scene_color*, const rsf_frame_tap_target_draw*,
    const rsf_ac7_scene_resource_calls*);
void rsf_core_ac7_scene_color_end_frame(rsf_ac7_scene_color*, const rsf_ac7_scene_resource_calls*);

void rsf_ac7_scene_color_clear(rsf_ac7_scene_color* state)
{
    rsf_core_ac7_scene_color_clear(state, &calls);
}
int rsf_ac7_scene_color_source(rsf_ac7_scene_color* state, void* source, void* context,
    uint32_t width, uint32_t height)
{
    return rsf_core_ac7_scene_color_source(state, source, context, width, height, &calls);
}
void rsf_ac7_scene_color_draw(rsf_ac7_scene_color* state, const rsf_frame_tap_target_draw* draw)
{
    rsf_core_ac7_scene_color_draw(state, draw, &calls);
}
void rsf_ac7_scene_color_end_frame(rsf_ac7_scene_color* state)
{
    rsf_core_ac7_scene_color_end_frame(state, &calls);
}
}
