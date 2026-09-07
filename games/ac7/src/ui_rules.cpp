/* SPDX-License-Identifier: GPL-3.0-only */
#include <rescaleframe/ac7_ui_rules.h>

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

/* A blend that puts source over destination, which is what an interface draw does and what a
   premultiplied layer can represent. Both of UE's over blends qualify: Slate's
   `SrcAlpha / InvSrcAlpha` and its premultiplied `One / InvSrcAlpha`, and so does the base pass's
   translucent blend, whose colour factors are the same and whose alpha factors are the thing that
   has to be patched at divert time. */
bool blends_over(const rsf_ac7_draw_facts& draw)
{
    return draw.blend_enabled != 0 && draw.dest_blend == RSF_AC7_BLEND_INV_SRC_ALPHA &&
           (draw.src_blend == RSF_AC7_BLEND_SRC_ALPHA || draw.src_blend == RSF_AC7_BLEND_ONE);
}

/* Multiply against what is already there. It writes colour only, so a transparent layer keeps
   nothing, and the draw is counted rather than diverted. */
bool blends_modulate(const rsf_ac7_draw_facts& draw)
{
    return draw.blend_enabled != 0 && draw.dest_blend == RSF_AC7_BLEND_SRC_COLOR;
}

/* A single quad: two triangles, six indices. The widget component builds exactly this, and the
   count is what separates it from the fullscreen triangles and the scene's meshes. */
bool is_quad(const rsf_ac7_draw_facts& draw)
{
    return draw.indexed != 0 && draw.element_count == 6u;
}

/* One colour target and nothing that makes retargeting unsafe. A draw with several targets or with
   unordered access views bound cannot have its slot zero moved without changing what the other
   bindings mean, so it is never interface as far as this rule is concerned. */
bool retargetable(const rsf_ac7_draw_facts& draw)
{
    return draw.render_target != nullptr && draw.target_count == 1u && draw.uav_bound == 0u;
}

} // namespace

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

    /* The escape hatch first, in both directions. A run that finds the rules below wrong about one
       shader can name it in the settings without waiting for a build, which is what SpecialK's HUD
       registry is for and why it is worth carrying. */
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

    /* Slate drawing into a converter's own target is the converter filling the texture the quads
       read. It is the interface being made, not the interface being shown, and diverting it would
       leave the quads reading an empty texture. Checked before anything else that could call a
       Slate draw interface. */
    if ((slate || canvas) && target_is_widget) {
        return RSF_AC7_DRAW_WIDGET_RASTER;
    }

    if (slate || canvas) {
        /* Into the frame's own target this is the interface at native resolution. Anywhere else it
           is Slate drawing into something this rule cannot name, and saying so is better than
           guessing: the tail may simply not be identified yet. */
        if (registry->back_buffer && draw->render_target == registry->back_buffer) {
            return blends_modulate(*draw) ? RSF_AC7_DRAW_UI_MODULATE : RSF_AC7_DRAW_UI_SLATE;
        }
        return RSF_AC7_DRAW_UNKNOWN;
    }

    uint32_t widget_slot = 0;
    if (reads_widget_target(*registry, *draw, &widget_slot)) {
        /* Reading the interface and writing somewhere that is neither another converter target nor
           the frame's own target is the world space quad: the interface rasterized by the scene.
           The shape and the blend are both required, because a post pass that happens to have a
           widget texture left bound in a low slot reads it without drawing it. */
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

    /* A setting named this shader and no rule recognised the draw. Trust the setting: it exists for
       the case where these rules are wrong and a run should not have to wait for a build. */
    if (forced && retargetable(*draw)) {
        return blends_modulate(*draw) ? RSF_AC7_DRAW_UI_MODULATE : RSF_AC7_DRAW_UI_SLATE;
    }

    return RSF_AC7_DRAW_SCENE;
}
