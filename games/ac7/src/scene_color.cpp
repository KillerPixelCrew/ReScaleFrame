/* SPDX-License-Identifier: GPL-3.0-only */
#include <rescaleframe/ac7_scene_color.h>
#include <rescaleframe/resource_ref.h>

#include <dxgiformat.h>

extern "C" void rsf_ac7_scene_color_clear(rsf_ac7_scene_color* state)
{
    rsf_resource_release(state->source);
    rsf_resource_release(state->composed);
    rsf_resource_release(state->composed_layer);
    *state = rsf_ac7_scene_color{};
}

extern "C" int rsf_ac7_scene_color_source(rsf_ac7_scene_color* state, void* source, void* context,
                                          uint32_t width, uint32_t height)
{
    if (state->source == source && state->context == context && state->width == width &&
        state->height == height) {
        return 0;
    }
    // Retain first: source may still be the same allocation when only the context changed.
    rsf_resource_retain(source);
    rsf_ac7_scene_color_clear(state);
    state->source = source;
    state->context = context;
    state->width = width;
    state->height = height;
    return 1;
}

extern "C" void rsf_ac7_scene_color_draw(rsf_ac7_scene_color* state,
                                         const rsf_frame_tap_target_draw* draw)
{
    // A single hop from the identified input, never a walk through successive post-process outputs.
    // The captured AC7 tonemap writes byte-format colour; only its earlier linear recombine shape
    // qualifies here. See docs/research/ac7-composed-scene-color.md for limits of this inference.
    if (!state->source || state->composed_this_frame || state->search_closed ||
        draw->context != state->context ||
        !draw->render_target || draw->render_target == state->source || draw->depth_bound ||
        draw->target_count != 1 || draw->target_samples != 1 || draw->inputs_truncated ||
        draw->element_count != 3 ||
        draw->target_width != state->width || draw->target_height != state->height ||
        draw->viewport_width != state->width || draw->viewport_height != state->height ||
        draw->viewport_x != 0.0f || draw->viewport_y != 0.0f) {
        return;
    }
    bool reads_source = false;
    void* layer = nullptr;
    bool ambiguous_layer = false;
    bool reads_layer = false;
    for (uint32_t i = 0; i < draw->input_count; ++i) {
        const rsf_frame_tap_input& input = draw->inputs[i];
        if (input.texture == state->source && input.format == DXGI_FORMAT_R11G11B10_FLOAT &&
            input.width == state->width && input.height == state->height) {
            reads_source = true;
        }
        if (input.texture && input.texture != state->source &&
            input.format == DXGI_FORMAT_R16G16B16A16_FLOAT &&
            ((input.width == state->width && input.height == state->height) ||
             (input.width == (state->width + 1) / 2 &&
              input.height == (state->height + 1) / 2))) {
            if (layer && layer != input.texture) {
                ambiguous_layer = true;
            }
            layer = input.texture;
            reads_layer = true;
        }
    }
    // The captured tonemap/composite boundary is byte-format colour. Close discovery when a
    // full-size consumer reaches it, so a later float target cannot reopen the scene chain.
    if (reads_source && (draw->target_format == DXGI_FORMAT_B8G8R8A8_UNORM ||
                         draw->target_format == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB ||
                         draw->target_format == DXGI_FORMAT_R8G8B8A8_UNORM ||
                         draw->target_format == DXGI_FORMAT_R8G8B8A8_UNORM_SRGB)) {
        state->search_closed = 1;
        return;
    }
    if (!reads_source || !reads_layer || draw->target_format != DXGI_FORMAT_R11G11B10_FLOAT) {
        return;
    }
    if (state->composed != draw->render_target) {
        rsf_resource_retain(draw->render_target);
        rsf_resource_release(state->composed);
        state->composed = draw->render_target;
    }
    if (ambiguous_layer) {
        layer = nullptr;
    }
    rsf_resource_retain(layer);
    rsf_resource_release(state->composed_layer);
    state->composed_layer = layer;
    state->composed_this_frame = 1;
}

extern "C" void* rsf_ac7_scene_color_selected(const rsf_ac7_scene_color* state, void* source)
{
    return state->source == source && state->composed_this_frame ? state->composed : source;
}

extern "C" void rsf_ac7_scene_color_end_frame(rsf_ac7_scene_color* state)
{
    rsf_resource_release(state->composed_layer);
    state->composed_layer = nullptr;
    state->composed_this_frame = 0;
    state->search_closed = 0;
}

extern "C" int rsf_ac7_scene_depth_candidate(const rsf_frame_tap_geometry* draw)
{
    // Briefing capture 1486-2015: one full-size RGBA16F target, geometry with read-only scene
    // depth. Identity is confirmed later by the recombine reading this exact target.
    return draw && draw->target && draw->depth_view && draw->samples == 1 &&
           draw->format == DXGI_FORMAT_R16G16B16A16_FLOAT;
}
