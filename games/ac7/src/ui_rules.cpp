/* SPDX-License-Identifier: GPL-3.0-only */
#include <rescaleframe/ac7_ui_rules.h>
#include <rescaleframe/ui_identify.h>
#include <cstring>

// Allocation-free rules over caller-owned shadow state. Native object pointers are identity keys;
// the header documents registries and the researched UE4.18 input-layout signatures.
extern "C" int rsf_ac7_ui_is_hud_producer(const void* bytecode, uint32_t bytes)
{
    static constexpr unsigned char checksum[] = {
        0x6c,0xac,0x06,0x44,0xb6,0xa4,0x9b,0xe3,0xef,0x92,0x58,0x58,0xa7,0xd3,0x8e,0x63
    };
    if (!bytecode || bytes != 1104) return 0;
    const auto* data = static_cast<const unsigned char*>(bytecode);
    return std::memcmp(data, "DXBC", 4) == 0 &&
           std::memcmp(data + 4, checksum, sizeof(checksum)) == 0 &&
           rsf_ui_shader_hash(bytecode, bytes) == 0xb2f7719du;
}

namespace {

bool contains(void* const* set, uint32_t count, const void* value)
{
    if (!set || !value) {
        return false;
    }
    for (uint32_t index = 0; index < count; ++index) {
        if (set[index] == value) {
            return true;
        }
    }
    return false;
}

bool reads_widget_target(const rsf_ac7_ui_registry& registry, const rsf_ac7_draw_facts& draw,
                         uint32_t* slot_out)
{
    if (!draw.inputs) {
        return false;
    }
    const uint32_t examined =
        draw.input_count < RSF_AC7_UI_MAX_INPUTS ? draw.input_count : RSF_AC7_UI_MAX_INPUTS;
    for (uint32_t index = 0; index < examined; ++index) {
        if (contains(registry.widget_targets, registry.widget_target_count,
                     draw.inputs[index].texture)) {
            if (slot_out) {
                *slot_out = draw.inputs[index].slot;
            }
            return true;
        }
    }
    return false;
}

/* Supported source-over colour factors. Diverting a translucent draw still needs compatible
   alpha factors, which the graphics policy selects separately. */
bool blends_over(const rsf_ac7_draw_facts& draw)
{
    return draw.blend_enabled != 0 && draw.dest_blend == RSF_AC7_BLEND_INV_SRC_ALPHA &&
           (draw.src_blend == RSF_AC7_BLEND_SRC_ALPHA || draw.src_blend == RSF_AC7_BLEND_ONE);
}

/* Destination-dependent colour multiplication cannot accumulate in a transparent UI layer. */
bool blends_modulate(const rsf_ac7_draw_facts& draw)
{
    return draw.blend_enabled != 0 && draw.dest_blend == RSF_AC7_BLEND_SRC_COLOR;
}

/* Captured widget-component quad shape: two triangles, six indices. */
bool is_quad(const rsf_ac7_draw_facts& draw)
{
    return draw.indexed != 0 && draw.element_count == 6u;
}

/* Retarget only one colour target with no UAV side effects. */
bool retargetable(const rsf_ac7_draw_facts& draw)
{
    return draw.render_target != nullptr && draw.target_count == 1u && draw.uav_bound == 0u;
}

/* One element of a declaration we are looking for. Matched by content rather than by position, so
   the order the engine happens to add them in is not part of the fingerprint. */
struct expected_element {
    uint32_t semantic_index;
    uint32_t format;
    uint32_t input_slot;
    uint32_t byte_offset;
    uint32_t per_instance;
};

bool has_element(const rsf_ac7_layout_element* elements, uint32_t count,
                 const expected_element& wanted)
{
    for (uint32_t index = 0; index < count; ++index) {
        const rsf_ac7_layout_element& element = elements[index];
        /* B8G8R8A8 arrives typeless or sRGB in some declarations, and a colour channel that is the
           same eight bits either way is the same element for this purpose. */
        const bool format_matches =
            element.format == wanted.format ||
            (wanted.format == RSF_AC7_FORMAT_B8G8R8A8_UNORM &&
             (element.format == RSF_AC7_FORMAT_B8G8R8A8_TYPELESS ||
              element.format == RSF_AC7_FORMAT_B8G8R8A8_UNORM_SRGB));
        if (format_matches && element.semantic_index == wanted.semantic_index &&
            element.input_slot == wanted.input_slot && element.byte_offset == wanted.byte_offset &&
            (element.per_instance != 0) == (wanted.per_instance != 0)) {
            return true;
        }
    }
    return false;
}

bool has_all(const rsf_ac7_layout_element* elements, uint32_t count,
             const expected_element* wanted, uint32_t wanted_count)
{
    for (uint32_t index = 0; index < wanted_count; ++index) {
        if (!has_element(elements, count, wanted[index])) {
            return false;
        }
    }
    return true;
}

/* FSlateVertex, stride 40. The five elements every Slate declaration carries. */
constexpr expected_element slate_elements[] = {
    {0, RSF_AC7_FORMAT_R32G32B32A32_FLOAT, 0, 0, 0},
    {1, RSF_AC7_FORMAT_R32G32_FLOAT, 0, 16, 0},
    {2, RSF_AC7_FORMAT_R32G32_FLOAT, 0, 24, 0},
    {3, RSF_AC7_FORMAT_B8G8R8A8_UNORM, 0, 32, 0},
    {4, RSF_AC7_FORMAT_R16G16_UINT, 0, 36, 0},
};

/* What the instanced declaration appends: a transform on its own stream, per instance. */
constexpr expected_element slate_instance_element = {5, RSF_AC7_FORMAT_R32G32B32A32_FLOAT, 1, 0, 1};

/* FSimpleElementVertex, stride 44. */
constexpr expected_element canvas_elements[] = {
    {0, RSF_AC7_FORMAT_R32G32B32A32_FLOAT, 0, 0, 0},
    {1, RSF_AC7_FORMAT_R32G32_FLOAT, 0, 16, 0},
    {2, RSF_AC7_FORMAT_R32G32B32A32_FLOAT, 0, 24, 0},
    {3, RSF_AC7_FORMAT_B8G8R8A8_UNORM, 0, 40, 0},
};

} // namespace

extern "C" rsf_ac7_layout_kind rsf_ac7_ui_classify_layout(const rsf_ac7_layout_element* elements,
                                                          uint32_t count)
{
    if (!elements || count == 0 || count > RSF_AC7_UI_MAX_LAYOUT_ELEMENTS) {
        return RSF_AC7_LAYOUT_OTHER;
    }

    /* Exact counts prevent a declaration with unrelated extra streams from matching a prefix. */
    constexpr uint32_t slate_count = sizeof(slate_elements) / sizeof(slate_elements[0]);
    constexpr uint32_t canvas_count = sizeof(canvas_elements) / sizeof(canvas_elements[0]);

    if (count == slate_count && has_all(elements, count, slate_elements, slate_count)) {
        return RSF_AC7_LAYOUT_SLATE;
    }
    if (count == slate_count + 1 && has_all(elements, count, slate_elements, slate_count) &&
        has_element(elements, count, slate_instance_element)) {
        return RSF_AC7_LAYOUT_SLATE_INSTANCED;
    }
    if (count == canvas_count && has_all(elements, count, canvas_elements, canvas_count)) {
        return RSF_AC7_LAYOUT_CANVAS;
    }
    return RSF_AC7_LAYOUT_OTHER;
}

extern "C" int rsf_ac7_ui_is_widget_target(const rsf_ac7_texture_facts* texture,
                                           const uint32_t* draw_sizes, uint32_t pair_count)
{
    if (!texture || texture->struct_size < sizeof(rsf_ac7_texture_facts)) {
        return 0;
    }
    /* Widget producers write this target, then scene-space quads sample it. */
    if (!texture->is_render_target || !texture->is_shader_resource) {
        return 0;
    }
    if (texture->mip_levels != 1u || texture->array_size != 1u || texture->sample_count != 1u) {
        return 0;
    }
    if (texture->format != RSF_AC7_FORMAT_B8G8R8A8_TYPELESS &&
        texture->format != RSF_AC7_FORMAT_B8G8R8A8_UNORM &&
        texture->format != RSF_AC7_FORMAT_B8G8R8A8_UNORM_SRGB) {
        return 0;
    }
    if (!draw_sizes || pair_count == 0) {
        return 0;
    }
    for (uint32_t index = 0; index < pair_count; ++index) {
        if (texture->width == draw_sizes[index * 2] &&
            texture->height == draw_sizes[index * 2 + 1]) {
            return 1;
        }
    }
    return 0;
}

extern "C" int rsf_ac7_ui_is_candidate(const rsf_ac7_ui_registry* registry,
                                       const rsf_ac7_draw_facts* draw)
{
    if (!registry || !draw || registry->struct_size < sizeof(rsf_ac7_ui_registry) ||
        draw->struct_size < sizeof(rsf_ac7_draw_facts)) {
        return 0;
    }
    if (contains(registry->force_shaders, registry->force_shader_count, draw->pixel_shader) ||
        contains(registry->force_shaders, registry->force_shader_count, draw->vertex_shader)) {
        return 1;
    }
    if (contains(registry->slate_layouts, registry->slate_layout_count, draw->input_layout) ||
        contains(registry->canvas_layouts, registry->canvas_layout_count, draw->input_layout)) {
        return 1;
    }
    return reads_widget_target(*registry, *draw, nullptr) ? 1 : 0;
}

extern "C" rsf_ac7_draw_class rsf_ac7_ui_classify(const rsf_ac7_ui_registry* registry,
                                                  const rsf_ac7_draw_facts* draw)
{
    if (!registry || !draw || registry->struct_size < sizeof(rsf_ac7_ui_registry) ||
        draw->struct_size < sizeof(rsf_ac7_draw_facts)) {
        return RSF_AC7_DRAW_SCENE;
    }

    /* Explicit skip wins before recognition. Force supplies a route only where later rules permit it. */
    if (contains(registry->skip_shaders, registry->skip_shader_count, draw->pixel_shader) ||
        contains(registry->skip_shaders, registry->skip_shader_count, draw->vertex_shader)) {
        return RSF_AC7_DRAW_SKIP;
    }
    const bool forced =
        contains(registry->force_shaders, registry->force_shader_count, draw->pixel_shader) ||
        contains(registry->force_shaders, registry->force_shader_count, draw->vertex_shader);

    const bool slate = contains(registry->slate_layouts, registry->slate_layout_count,
                                draw->input_layout);
    const bool canvas = contains(registry->canvas_layouts, registry->canvas_layout_count,
                                 draw->input_layout);
    const bool target_is_widget =
        contains(registry->widget_targets, registry->widget_target_count, draw->render_target);

    /* Preserve widget rasterization before classifying its later consumers. */
    if ((slate || canvas) && target_is_widget) {
        return RSF_AC7_DRAW_WIDGET_RASTER;
    }

    if (slate || canvas) {
        /* Only the identified frame target establishes final UI; other Slate destinations stay unknown. */
        if (registry->back_buffer && draw->render_target == registry->back_buffer) {
            return blends_modulate(*draw) ? RSF_AC7_DRAW_UI_MODULATE : RSF_AC7_DRAW_UI_SLATE;
        }
        return RSF_AC7_DRAW_UNKNOWN;
    }

    uint32_t widget_slot = 0;
    if (reads_widget_target(*registry, *draw, &widget_slot)) {
        /* Require destination, quad shape and source-over blend. A stale bound widget texture
           alone does not distinguish a postprocess consumer from scene-space widget geometry. */
        if (retargetable(*draw) && !target_is_widget &&
            (!registry->back_buffer || draw->render_target != registry->back_buffer)) {
            if (blends_modulate(*draw)) {
                return RSF_AC7_DRAW_UI_MODULATE;
            }
            if (is_quad(*draw) && blends_over(*draw)) {
                return RSF_AC7_DRAW_UI_WIDGET_QUAD;
            }
        }
        /* Interface shaped and unexplained. Reported per screen rather than acted on. */
        return forced ? RSF_AC7_DRAW_UI_WIDGET_QUAD : RSF_AC7_DRAW_UNKNOWN;
    }

    /* Forced unmatched draws still require safe retargeting and preserve modulate classification. */
    if (forced && retargetable(*draw)) {
        return blends_modulate(*draw) ? RSF_AC7_DRAW_UI_MODULATE : RSF_AC7_DRAW_UI_SLATE;
    }

    return RSF_AC7_DRAW_SCENE;
}
