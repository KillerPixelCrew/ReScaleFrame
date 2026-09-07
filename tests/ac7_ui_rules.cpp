/* SPDX-License-Identifier: GPL-3.0-only */
/* The AC7 interface classifier, without a device.
 *
 * The rule is a pure function over facts the frame tap shadows, so it can be driven from a table.
 * The cases below are the frame the run log recorded: four world space widget quads reading
 * 1920x1080 converter targets into a render resolution layer, the converter's own Slate draws that
 * fill those textures, and the scene draws that must stay untouched. Everything that was ever
 * classified wrongly in this project has a row here.
 */
#include <rescaleframe/ac7_ui_rules.h>

#include <cstdio>

namespace {

bool passed = true;

void check(bool condition, const char* what)
{
    if (!condition) {
        std::fprintf(stderr, "FAIL: %s\n", what);
        passed = false;
    }
}

void stage(const char* what) { std::fprintf(stderr, "[stage] %s\n", what); }

/* Stand-in pointers. The classifier compares addresses and never dereferences them, so the values
   only have to be distinct. */
char slate_layout_object;
char slate_instanced_layout_object;
char canvas_layout_object;
char scene_layout_object;
char widget_texture_a;
char widget_texture_b;
char back_buffer_object;
char ui_layer_object;
char scene_colour_object;
char forced_shader_object;
char skipped_shader_object;
char scene_shader_object;

void* slate_layouts[] = {&slate_layout_object, &slate_instanced_layout_object};
void* canvas_layouts[] = {&canvas_layout_object};
void* widget_targets[] = {&widget_texture_a, &widget_texture_b};
void* force_shaders[] = {&forced_shader_object};
void* skip_shaders[] = {&skipped_shader_object};

rsf_ac7_ui_registry make_registry(void* back_buffer)
{
    rsf_ac7_ui_registry registry{};
    registry.struct_size = sizeof(registry);
    registry.slate_layouts = slate_layouts;
    registry.slate_layout_count = 2;
    registry.canvas_layouts = canvas_layouts;
    registry.canvas_layout_count = 1;
    registry.widget_targets = widget_targets;
    registry.widget_target_count = 2;
    registry.back_buffer = back_buffer;
    registry.force_shaders = force_shaders;
    registry.force_shader_count = 1;
    registry.skip_shaders = skip_shaders;
    registry.skip_shader_count = 1;
    return registry;
}

rsf_ac7_draw_facts make_draw()
{
    rsf_ac7_draw_facts draw{};
    draw.struct_size = sizeof(draw);
    draw.pixel_shader = &scene_shader_object;
    draw.vertex_shader = &scene_shader_object;
    draw.target_count = 1;
    draw.blend_enabled = 1;
    draw.src_blend = RSF_AC7_BLEND_SRC_ALPHA;
    draw.dest_blend = RSF_AC7_BLEND_INV_SRC_ALPHA;
    return draw;
}

/* The briefing's widget quad: six indices, one target at render resolution, reading a converter
   texture in slot 0, with the scene's depth bound. */
rsf_ac7_draw_facts make_widget_quad(rsf_ac7_draw_input* inputs)
{
    rsf_ac7_draw_facts draw = make_draw();
    draw.input_layout = &scene_layout_object;
    draw.render_target = &ui_layer_object;
    draw.target_width = 1024;
    draw.target_height = 576;
    draw.depth_bound = 1;
    draw.indexed = 1;
    draw.element_count = 6;
    inputs[0].slot = 0;
    inputs[0].texture = &widget_texture_a;
    inputs[0].width = 1920;
    inputs[0].height = 1080;
    draw.input_count = 1;
    draw.inputs = inputs;
    return draw;
}

} // namespace

int main()
{
    rsf_ac7_draw_input inputs[4]{};
    const rsf_ac7_ui_registry registry = make_registry(&back_buffer_object);

    stage("a scene draw is a scene draw");
    {
        rsf_ac7_draw_facts draw = make_draw();
        draw.input_layout = &scene_layout_object;
        draw.render_target = &scene_colour_object;
        draw.indexed = 1;
        draw.element_count = 3000;
        check(rsf_ac7_ui_is_candidate(&registry, &draw) == 0,
              "A draw with no interface layout, no converter input and no named shader must not "
              "even be a candidate: the prefilter is what the whole frame pays.");
        check(rsf_ac7_ui_classify(&registry, &draw) == RSF_AC7_DRAW_SCENE, "Scene draws classify as scene.");
    }

    stage("the world space widget quad");
    {
        rsf_ac7_draw_facts draw = make_widget_quad(inputs);
        check(rsf_ac7_ui_is_candidate(&registry, &draw) == 1, "Reading a converter target makes a draw a candidate.");
        check(rsf_ac7_ui_classify(&registry, &draw) == RSF_AC7_DRAW_UI_WIDGET_QUAD,
              "Six indices reading a 1920x1080 converter target into a render resolution target "
              "with an over blend is the interface drawn as scene geometry.");
    }

    stage("depth being bound does not disqualify the quad");
    {
        rsf_ac7_draw_facts draw = make_widget_quad(inputs);
        draw.depth_bound = 0;
        check(rsf_ac7_ui_classify(&registry, &draw) == RSF_AC7_DRAW_UI_WIDGET_QUAD,
              "The briefing's quads bind the scene's depth, so a rule that only took depth-free "
              "draws would extract nothing there. Depth decides the divert's semantics, not the "
              "classification.");
    }

    stage("a post pass with a widget texture left bound is not the interface");
    {
        rsf_ac7_draw_facts draw = make_widget_quad(inputs);
        draw.indexed = 0;
        draw.element_count = 3;
        check(rsf_ac7_ui_classify(&registry, &draw) == RSF_AC7_DRAW_UNKNOWN,
              "A fullscreen triangle that happens to have a converter texture bound reads it "
              "without drawing it. D3D11 leaves slots bound, which is the mistake this frame has "
              "made four times; it is reported, not diverted.");
    }

    stage("the converter rasterizing its own widget is left alone");
    {
        rsf_ac7_draw_facts draw = make_draw();
        draw.input_layout = &slate_layout_object;
        draw.render_target = &widget_texture_a;
        draw.target_width = 1920;
        draw.target_height = 1080;
        draw.indexed = 1;
        draw.element_count = 60;
        check(rsf_ac7_ui_classify(&registry, &draw) == RSF_AC7_DRAW_WIDGET_RASTER,
              "Slate drawing into a converter target is the interface being made. Diverting it "
              "would leave the quads reading an empty texture.");
    }

    stage("Slate into the frame's own target is the interface at native");
    {
        rsf_ac7_draw_facts draw = make_draw();
        draw.input_layout = &slate_layout_object;
        draw.render_target = &back_buffer_object;
        draw.target_width = 2048;
        draw.target_height = 1152;
        draw.indexed = 1;
        draw.element_count = 6;
        check(rsf_ac7_ui_classify(&registry, &draw) == RSF_AC7_DRAW_UI_SLATE, "Slate into the back buffer is interface.");
        draw.input_layout = &slate_instanced_layout_object;
        check(rsf_ac7_ui_classify(&registry, &draw) == RSF_AC7_DRAW_UI_SLATE,
              "The instanced Slate declaration carries a sixth element and is the same producer.");
        draw.input_layout = &canvas_layout_object;
        check(rsf_ac7_ui_classify(&registry, &draw) == RSF_AC7_DRAW_UI_SLATE,
              "A canvas HUD draw into the same target is the same case.");
    }

    stage("Slate before the tail is identified");
    {
        const rsf_ac7_ui_registry unknown_tail = make_registry(nullptr);
        rsf_ac7_draw_facts draw = make_draw();
        draw.input_layout = &slate_layout_object;
        draw.render_target = &scene_colour_object;
        check(rsf_ac7_ui_classify(&unknown_tail, &draw) == RSF_AC7_DRAW_UNKNOWN,
              "Without the frame's own target there is half a fact, and half a fact is what the "
              "retired format rule was.");
    }

    stage("a modulate blend is counted, not diverted");
    {
        rsf_ac7_draw_facts draw = make_widget_quad(inputs);
        draw.dest_blend = RSF_AC7_BLEND_SRC_COLOR;
        check(rsf_ac7_ui_classify(&registry, &draw) == RSF_AC7_DRAW_UI_MODULATE,
              "Modulate writes colour only, so a transparent layer keeps nothing of it.");
    }

    stage("multiple targets and unordered access views refuse");
    {
        rsf_ac7_draw_facts draw = make_widget_quad(inputs);
        draw.target_count = 3;
        check(rsf_ac7_ui_classify(&registry, &draw) == RSF_AC7_DRAW_UNKNOWN,
              "Moving slot zero of a multiple target draw changes what the other targets mean.");
        draw = make_widget_quad(inputs);
        draw.uav_bound = 1;
        check(rsf_ac7_ui_classify(&registry, &draw) == RSF_AC7_DRAW_UNKNOWN,
              "A draw with unordered access views bound is not safely retargetable.");
    }

    stage("the settings override both ways");
    {
        rsf_ac7_draw_facts draw = make_widget_quad(inputs);
        draw.pixel_shader = &skipped_shader_object;
        check(rsf_ac7_ui_classify(&registry, &draw) == RSF_AC7_DRAW_SKIP,
              "A skipped shader loses to no rule.");

        rsf_ac7_draw_facts forced = make_draw();
        forced.input_layout = &scene_layout_object;
        forced.pixel_shader = &forced_shader_object;
        forced.render_target = &ui_layer_object;
        forced.indexed = 1;
        forced.element_count = 12;
        check(rsf_ac7_ui_is_candidate(&registry, &forced) == 1, "A named shader is always a candidate.");
        check(rsf_ac7_ui_classify(&registry, &forced) == RSF_AC7_DRAW_UI_SLATE,
              "A forced shader is interface even when no rule recognised the draw, because the "
              "setting exists for the case where these rules are wrong.");
    }

    stage("arguments are checked");
    {
        rsf_ac7_draw_facts draw = make_widget_quad(inputs);
        check(rsf_ac7_ui_classify(nullptr, &draw) == RSF_AC7_DRAW_SCENE, "A null registry classifies as scene.");
        check(rsf_ac7_ui_classify(&registry, nullptr) == RSF_AC7_DRAW_SCENE, "A null draw classifies as scene.");
        rsf_ac7_draw_facts short_draw = draw;
        short_draw.struct_size = 8;
        check(rsf_ac7_ui_classify(&registry, &short_draw) == RSF_AC7_DRAW_SCENE,
              "A short structure classifies as scene rather than reading fields that may not be there.");
        check(rsf_ac7_ui_is_candidate(&registry, &short_draw) == 0, "A short structure is not a candidate.");
    }

    std::fprintf(stderr, "%s\n", passed ? "ac7_ui_rules: all checks passed" : "ac7_ui_rules: FAILED");
    return passed ? 0 : 1;
}
