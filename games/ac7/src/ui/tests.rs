// SPDX-License-Identifier: MIT
use super::*;
use std::ptr::{null, without_provenance_mut};

fn id(key: usize) -> *mut c_void {
    without_provenance_mut(key)
}

#[test]
fn briefing_draws_match_their_producers_and_blend() {
    let slate = [id(1), id(2)];
    let canvas = [id(3)];
    let widget = [id(4), id(5)];
    let forced = [id(6)];
    let skipped = [id(7)];
    let registry = rsf_ac7_ui_registry {
        struct_size: size_of::<rsf_ac7_ui_registry>() as u32,
        slate_layouts: slate.as_ptr(),
        slate_layout_count: 2,
        canvas_layouts: canvas.as_ptr(),
        canvas_layout_count: 1,
        widget_targets: widget.as_ptr(),
        widget_target_count: 2,
        back_buffer: id(8),
        force_shaders: forced.as_ptr(),
        force_shader_count: 1,
        skip_shaders: skipped.as_ptr(),
        skip_shader_count: 1,
    };
    let input = rsf_ac7_draw_input {
        slot: 0,
        texture: widget[0],
        width: 1920,
        height: 1080,
    };
    let scene = rsf_ac7_draw_facts {
        struct_size: size_of::<rsf_ac7_draw_facts>() as u32,
        input_layout: id(9),
        vertex_shader: id(10),
        pixel_shader: id(10),
        render_target: id(11),
        target_width: 1024,
        target_height: 576,
        target_count: 1,
        depth_bound: 1,
        uav_bound: 0,
        indexed: 1,
        element_count: 3000,
        vertex_stride: 0,
        blend_enabled: 1,
        src_blend: RSF_AC7_BLEND_SRC_ALPHA,
        dest_blend: RSF_AC7_BLEND_INV_SRC_ALPHA,
        input_count: 0,
        inputs: null(),
    };
    let quad = rsf_ac7_draw_facts {
        element_count: 6,
        input_count: 1,
        inputs: &input,
        ..scene
    };
    let cases = [
        (scene, 0, RSF_AC7_DRAW_SCENE),
        (quad, 1, RSF_AC7_DRAW_UI_WIDGET_QUAD),
        (
            rsf_ac7_draw_facts {
                depth_bound: 0,
                ..quad
            },
            1,
            RSF_AC7_DRAW_UI_WIDGET_QUAD,
        ),
        (
            rsf_ac7_draw_facts {
                indexed: 0,
                element_count: 3,
                ..quad
            },
            1,
            RSF_AC7_DRAW_UNKNOWN,
        ),
        (
            rsf_ac7_draw_facts {
                input_layout: slate[0],
                render_target: widget[0],
                element_count: 60,
                ..scene
            },
            1,
            RSF_AC7_DRAW_WIDGET_RASTER,
        ),
        (
            rsf_ac7_draw_facts {
                input_layout: slate[0],
                ..scene
            },
            1,
            RSF_AC7_DRAW_UNKNOWN,
        ),
        (
            rsf_ac7_draw_facts {
                dest_blend: RSF_AC7_BLEND_SRC_COLOR,
                ..quad
            },
            1,
            RSF_AC7_DRAW_UI_MODULATE,
        ),
        (
            rsf_ac7_draw_facts {
                target_count: 3,
                ..quad
            },
            1,
            RSF_AC7_DRAW_UNKNOWN,
        ),
        (
            rsf_ac7_draw_facts {
                uav_bound: 1,
                ..quad
            },
            1,
            RSF_AC7_DRAW_UNKNOWN,
        ),
        (
            rsf_ac7_draw_facts {
                pixel_shader: skipped[0],
                ..quad
            },
            1,
            RSF_AC7_DRAW_SKIP,
        ),
        (
            rsf_ac7_draw_facts {
                pixel_shader: forced[0],
                ..scene
            },
            1,
            RSF_AC7_DRAW_UI_SLATE,
        ),
        (
            rsf_ac7_draw_facts {
                struct_size: 8,
                ..quad
            },
            0,
            RSF_AC7_DRAW_SCENE,
        ),
    ];
    for (index, (draw, candidate, class)) in cases.iter().enumerate() {
        assert_eq!(
            unsafe { rsf_ac7_ui_is_candidate(&registry, draw) },
            *candidate,
            "case {index}"
        );
        assert_eq!(
            unsafe { rsf_ac7_ui_classify(&registry, draw) },
            *class,
            "case {index}"
        );
    }
    for layout in [slate[0], slate[1], canvas[0]] {
        let draw = rsf_ac7_draw_facts {
            input_layout: layout,
            render_target: registry.back_buffer,
            ..scene
        };
        assert_eq!(
            unsafe { rsf_ac7_ui_classify(&registry, &draw) },
            RSF_AC7_DRAW_UI_SLATE
        );
    }
    assert_eq!(
        unsafe { rsf_ac7_ui_classify(null(), &quad) },
        RSF_AC7_DRAW_SCENE
    );
    assert_eq!(
        unsafe { rsf_ac7_ui_classify(&registry, null()) },
        RSF_AC7_DRAW_SCENE
    );
}

#[test]
fn vertex_declarations_preserve_the_ue418_signatures() {
    let base = [
        element(0, 2, 0, 0, 0),
        element(1, 16, 0, 16, 0),
        element(2, 16, 0, 24, 0),
        element(3, 87, 0, 32, 0),
        element(4, 36, 0, 36, 0),
    ];
    let mut slate = [
        base[4],
        base[3],
        base[2],
        base[1],
        base[0],
        element(5, 2, 1, 0, 1),
    ];
    let classify = |elements: &[rsf_ac7_layout_element]| unsafe {
        rsf_ac7_ui_classify_layout(elements.as_ptr(), elements.len() as u32)
    };
    assert_eq!(classify(&base), RSF_AC7_LAYOUT_SLATE);
    assert_eq!(classify(&slate[..5]), RSF_AC7_LAYOUT_SLATE);
    assert_eq!(classify(&slate), RSF_AC7_LAYOUT_SLATE_INSTANCED);
    slate[5].per_instance = 0;
    assert_eq!(classify(&slate), RSF_AC7_LAYOUT_OTHER);
    slate[5] = element(7, 16, 0, 40, 0);
    assert_eq!(classify(&slate), RSF_AC7_LAYOUT_OTHER);
    let canvas = [
        element(0, 2, 0, 0, 0),
        base[1],
        element(2, 2, 0, 24, 0),
        element(3, 87, 0, 40, 0),
    ];
    assert_eq!(classify(&canvas), RSF_AC7_LAYOUT_CANVAS);
    let mut ue5 = canvas;
    ue5[1].byte_offset = 32;
    ue5[2].byte_offset = 40;
    ue5[3].byte_offset = 56;
    assert_eq!(classify(&ue5), RSF_AC7_LAYOUT_OTHER);
    assert_eq!(classify(&[]), RSF_AC7_LAYOUT_OTHER);
    assert_eq!(
        unsafe { rsf_ac7_ui_classify_layout(null(), 5) },
        RSF_AC7_LAYOUT_OTHER
    );
    assert_eq!(
        unsafe { rsf_ac7_ui_classify_layout(slate.as_ptr(), 99) },
        RSF_AC7_LAYOUT_OTHER
    );
}

#[test]
fn converter_targets_require_the_declared_shape() {
    let sizes = [1920, 1080, 1280, 720];
    let texture = rsf_ac7_texture_facts {
        struct_size: size_of::<rsf_ac7_texture_facts>() as u32,
        width: 1920,
        height: 1080,
        mip_levels: 1,
        array_size: 1,
        sample_count: 1,
        format: RSF_AC7_FORMAT_B8G8R8A8_TYPELESS,
        is_render_target: 1,
        is_shader_resource: 1,
    };
    let admit = |facts| unsafe { rsf_ac7_ui_is_widget_target(facts, sizes.as_ptr(), 2) };
    assert_eq!(admit(&texture), 1);
    assert_eq!(
        admit(&rsf_ac7_texture_facts {
            width: 1280,
            height: 720,
            ..texture
        }),
        1
    );
    let refused = [
        rsf_ac7_texture_facts {
            is_shader_resource: 0,
            ..texture
        },
        rsf_ac7_texture_facts {
            mip_levels: 4,
            ..texture
        },
        rsf_ac7_texture_facts {
            sample_count: 4,
            ..texture
        },
        rsf_ac7_texture_facts {
            format: RSF_AC7_FORMAT_R32G32B32A32_FLOAT,
            ..texture
        },
        rsf_ac7_texture_facts {
            width: 2048,
            height: 1152,
            ..texture
        },
        rsf_ac7_texture_facts {
            struct_size: 8,
            ..texture
        },
    ];
    for facts in &refused {
        assert_eq!(admit(facts), 0);
    }
    assert_eq!(admit(null()), 0);
    assert_eq!(
        unsafe { rsf_ac7_ui_is_widget_target(&texture, null(), 0) },
        0
    );
}
