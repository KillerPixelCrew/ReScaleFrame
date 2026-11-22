// SPDX-License-Identifier: MIT
use super::*;
use std::ptr::{NonNull, null_mut};

fn scope() -> rsf_game_render_pass {
    // SAFETY: Every field of the plain C scope is valid as zero.
    let mut scope: rsf_game_render_pass = unsafe { std::mem::zeroed() };
    scope.struct_size = size_of::<rsf_game_render_pass>() as u32;
    scope.role = RSF_GAME_RENDER_TRANSLUCENT_MESH;
    scope.camera_valid = 1;
    scope.flags = RSF_GAME_RENDER_PRIMARY;
    scope
}

fn draw(kind: u32, count: u32, instances: u32, topology: u32) -> rsf_frame_tap_geometry {
    let mut draw = rsf_frame_tap_geometry {
        samples: 1,
        kind,
        count,
        instances,
        topology,
        ..Default::default()
    };
    draw.context = NonNull::<u8>::dangling().as_ptr().cast();
    draw.depth_view = NonNull::<u8>::dangling().as_ptr().cast();
    draw
}

#[test]
fn draw_admission_needs_a_primary_translucent_camera_scope() {
    let geometry = draw(0, 3, 0, TRIANGLE_LIST);
    assert!(draw_admitted(&scope(), &geometry));
    let mut other = scope();
    other.role = RSF_GAME_RENDER_TRANSLUCENT_MESH + 1;
    assert!(!draw_admitted(&other, &geometry));
    let mut no_camera = scope();
    no_camera.camera_valid = 0;
    assert!(!draw_admitted(&no_camera, &geometry));
    let mut secondary = scope();
    secondary.flags = 0;
    assert!(!draw_admitted(&secondary, &geometry));
}

#[test]
fn draw_admission_checks_context_depth_samples_kind_and_count() {
    let scope = scope();
    let mut geometry = draw(1, 3, 0, TRIANGLE_LIST);
    geometry.context = null_mut();
    assert!(!draw_admitted(&scope, &geometry));
    let mut geometry = draw(1, 3, 0, TRIANGLE_LIST);
    geometry.depth_view = null_mut();
    assert!(!draw_admitted(&scope, &geometry));
    let mut geometry = draw(1, 3, 0, TRIANGLE_LIST);
    geometry.samples = 4;
    assert!(!draw_admitted(&scope, &geometry));
    assert!(!draw_admitted(&scope, &draw(7, 3, 0, TRIANGLE_LIST)));
    // Direct draws need vertices; DrawAuto and indirect draws read theirs on the GPU.
    for kind in 0..DRAW_AUTO {
        assert!(!draw_admitted(&scope, &draw(kind, 0, 1, TRIANGLE_LIST)));
    }
    for kind in DRAW_AUTO..=LAST_KIND {
        assert!(draw_admitted(&scope, &draw(kind, 0, 0, TRIANGLE_LIST)));
    }
}

#[test]
fn routes_follow_the_bound_stages_and_topology() {
    assert_eq!(route(TRIANGLE_LIST, 0), ROUTE_ADMITTED);
    assert_eq!(route(TRIANGLE_STRIP, STAGE_GEOMETRY), ROUTE_ADMITTED);
    assert_eq!(route(1, 0), ROUTE_SKIPPED);
    assert_eq!(route(PATCH_LIST_BASE + 3, STAGE_GEOMETRY), ROUTE_SKIPPED);
    // Patch topology is checked by the plan once the variant knows its control points.
    assert_eq!(route(1, STAGE_HULL | STAGE_DOMAIN), ROUTE_ADMITTED);
    for stages in [
        STAGE_HULL,
        STAGE_DOMAIN,
        STAGE_GEOMETRY | STAGE_HULL | STAGE_DOMAIN,
        STAGE_GEOMETRY | STAGE_HULL,
    ] {
        assert_eq!(route(TRIANGLE_LIST, stages), ROUTE_MISMATCHED);
    }
}

#[test]
fn direct_lists_count_instanced_vertices() {
    let made = plan(&draw(2, 300, 4, TRIANGLE_LIST), 0, 0, 1, 0).unwrap();
    assert_eq!(
        made.bounds,
        IndirectBounds {
            vertices: 1200,
            records: 1200,
            packing: PACK_VERTICES,
            points: 0
        }
    );
    assert_eq!(
        (made.capacity, made.built, made.zero_source),
        (2048, 1200, 0)
    );
    // Non-instanced kinds ignore the instance count.
    let made = plan(&draw(1, 300, 9, TRIANGLE_LIST), 0, 0, 1, 0).unwrap();
    assert_eq!((made.bounds.vertices, made.capacity), (300, MIN_CAPACITY));
}

#[test]
fn strips_expand_records_and_build_their_triangles() {
    let made = plan(&draw(3, 10, 2, TRIANGLE_STRIP), 0, 0, 1, 0).unwrap();
    assert_eq!(
        (
            made.bounds.vertices,
            made.bounds.records,
            made.bounds.packing
        ),
        (20, 60, PACK_STRIP)
    );
    assert_eq!(made.built, 48);
    let made = plan(&draw(0, 1, 0, TRIANGLE_STRIP), 0, 0, 1, 0).unwrap();
    assert_eq!(made.built, 0);
}

#[test]
fn patches_need_their_topology_and_count_patches() {
    let stages = STAGE_HULL | STAGE_DOMAIN;
    assert!(plan(&draw(0, 9, 0, PATCH_LIST_BASE + 4), stages, 0, 1, 3).is_none());
    assert!(plan(&draw(0, 9, 0, PATCH_LIST_BASE), stages, 0, 1, 0).is_none());
    let made = plan(&draw(2, 10, 2, PATCH_LIST_BASE + 3), stages, 0, 1, 3).unwrap();
    assert_eq!(
        made.bounds,
        IndirectBounds {
            vertices: 20,
            records: 20,
            packing: PACK_PATCHES,
            points: 3
        }
    );
    assert_eq!((made.built, made.zero_source), (18, 0));
    // Indirect patches read the capacity as triangles of control points.
    let made = plan(&draw(6, 0, 0, PATCH_LIST_BASE + 1), stages, 9, 3, 1).unwrap();
    assert_eq!((made.bounds.vertices, made.bounds.records), (3, 9));
    assert_eq!(made.built, 9);
}

#[test]
fn native_emission_and_draw_auto_use_the_output_bound_and_zero_source() {
    let made = plan(&draw(1, 30, 0, TRIANGLE_LIST), STAGE_GEOMETRY, 0, 6, 0).unwrap();
    assert_eq!(
        (made.bounds.records, made.bounds.packing, made.built),
        (180, PACK_EMITTED, 180)
    );
    assert_eq!(made.zero_source, 1);
    let made = plan(&draw(DRAW_AUTO, 0, 0, TRIANGLE_LIST), 0, 99, 1, 0).unwrap();
    assert_eq!(
        (made.bounds.vertices, made.built, made.zero_source),
        (99, 99, 1)
    );
    // Indirect draws build up to the bound and read their exact count on the GPU.
    let made = plan(&draw(5, 0, 0, TRIANGLE_STRIP), 0, 12, 1, 0).unwrap();
    assert_eq!(
        (made.bounds.records, made.built, made.zero_source),
        (36, 36, 0)
    );
}

#[test]
fn record_bounds_refuse_empty_unbounded_and_overflowing_draws() {
    assert!(plan(&draw(DRAW_AUTO, 0, 0, TRIANGLE_LIST), 0, 0, 1, 0).is_none());
    assert!(plan(&draw(2, 3, 0, TRIANGLE_LIST), 0, 0, 1, 0).is_none());
    assert!(plan(&draw(0, 3, 0, TRIANGLE_LIST), 0, 0, 0, 0).is_none());
    assert!(plan(&draw(0, 3, 0, TRIANGLE_STRIP), 0, 0, u32::MAX, 0).is_none());
    let limit = RECORD_LIMIT / 3;
    let made = plan(&draw(0, limit, 0, TRIANGLE_STRIP), 0, 0, 1, 0).unwrap();
    assert_eq!(made.bounds.records, limit * 3);
    assert!(plan(&draw(0, limit + 1, 0, TRIANGLE_STRIP), 0, 0, 1, 0).is_none());
    assert!(plan(&draw(2, u32::MAX, u32::MAX, TRIANGLE_LIST), 0, 0, 1, 0).is_none());
    let made = plan(&draw(0, RECORD_LIMIT, 0, TRIANGLE_LIST), 0, 0, 1, 0).unwrap();
    assert_eq!(made.capacity, RECORD_LIMIT + 1);
}

#[test]
fn capacity_doubles_from_the_smallest_buffer() {
    for (records, capacity) in [(1, 1024), (1024, 1024), (1025, 2048), (5000, 8192)] {
        let made = plan(&draw(0, records, 0, TRIANGLE_LIST), 0, 0, 1, 0).unwrap();
        assert_eq!(made.capacity, capacity);
    }
}

#[test]
fn indirect_arguments_fit_aligned_within_the_buffer() {
    assert_eq!(indirect_end(5, 0, 20), Some(20));
    assert_eq!(indirect_end(6, 0, 16), Some(16));
    assert_eq!(indirect_end(5, 4, 20), None);
    assert_eq!(indirect_end(6, 48, 64), Some(64));
    assert_eq!(indirect_end(6, 50, 80), None);
    assert_eq!(indirect_end(6, 68, 64), None);
    assert_eq!(indirect_end(6, u32::MAX - 3, u32::MAX), None);
}

#[test]
fn parameters_pack_viewport_jitter_cutoff_and_history() {
    let mut scope = scope();
    scope.camera.jitter_pixels = [0.5, -0.25];
    scope.camera.previous_jitter_pixels = [-1.0, 1.0];
    scope.camera.clip_to_previous_clip = std::array::from_fn(|i| i as f32);
    scope.material_opacity_clip = 0.25;
    let made = parameters(&scope, [10.0, 20.0, 400.0, 200.0], true, 2048);
    assert_eq!(made.rect, [10.0, 20.0, 400.0, 200.0]);
    assert_eq!(made.jitter, [0.0025, 0.0025, -0.005, -0.01]);
    assert_eq!(made.cutoff, 0.25 + 1.0 / 255.0);
    assert_eq!((made.history, made.mask, made.padding), (1, 2047, 0));
    assert_eq!(made.matrix, scope.camera.clip_to_previous_clip);

    scope.material_blend = ALPHA_COMPOSITE;
    let made = parameters(&scope, [0.0, 0.0, 1.0, 1.0], true, 0);
    assert_eq!((made.cutoff, made.history, made.mask), (-1.0, 0, 0));
    let made = parameters(&scope, [0.0, 0.0, 1.0, 1.0], false, 1024);
    assert_eq!((made.history, made.mask), (0, 1023));
}

#[test]
fn parameters_need_one_viewport_of_positive_extent() {
    let scope = scope();
    let call = |viewports, rect: [f32; 4]| draw_parameters(&scope, viewports, rect, true, 0).is_some();
    assert!(call(1, [0.0, 0.0, 8.0, 8.0]));
    assert!(!call(0, [0.0, 0.0, 8.0, 8.0]));
    assert!(!call(2, [0.0, 0.0, 8.0, 8.0]));
    assert!(!call(1, [0.0, 0.0, 0.0, 8.0]));
    assert!(!call(1, [0.0, 0.0, 8.0, -1.0]));
    assert!(call(1, [0.0, 0.0, f32::NAN, 8.0]));
}

#[test]
fn hash_dispatch_splits_groups_and_reads_indirect_counts() {
    assert!(hash_dispatch(0, 1024, 0, false).is_none());
    let made = hash_dispatch(65, 1024, 1, false).unwrap();
    assert_eq!(
        made.parameters,
        HashParameters {
            count: 65,
            mask: 1023,
            mode: 1,
            work_width: 2
        }
    );
    assert_eq!(made.groups, [2, 1]);
    let made = hash_dispatch(65536 * 64, 1 << 27, 0, true).unwrap();
    assert_eq!(made.parameters.work_width, 0);
    assert_eq!(made.groups, [65535, 2]);
    let made = hash_dispatch(RECORD_LIMIT, 1 << 26, 0, false).unwrap();
    assert_eq!(made.groups, [65535, 9]);
}
