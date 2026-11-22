// SPDX-License-Identifier: MIT
use super::*;
use std::panic::catch_unwind;

fn profile(native: &Layout) {
    let fixture = layout();
    macro_rules! equal {
        ($($field:ident),+ $(,)?) => {
            $(assert_eq!(native.$field, fixture.$field, stringify!($field));)+
        };
    }
    equal!(
        struct_size,
        crt_surface,
        crt_rtv,
        crt_dsv,
        crt_uav,
        crt_texture,
        crt_width,
        crt_height,
        crt_format,
        texture_surface,
        texture_desc_cache,
        texture_desc,
        texture_srv,
        target_width,
        target_height,
        backend_texture_list,
        backend_ctable,
        ssao_viewport_width,
        format_a16b16g16r16f,
        format_r16f,
        format_r32f,
        absent,
    );
    for (a, b) in native.backend_textures.iter().zip(fixture.backend_textures) {
        assert_eq!((a.offset, a.count), (b.offset, b.count));
    }
    for (a, b) in native.world.iter().zip(fixture.world) {
        assert_eq!(
            (a.offset, a.d3d_format, a.optional, a.half_allowed),
            (b.offset, b.d3d_format, b.optional, b.half_allowed)
        );
    }
}

fn independent_jitter() {
    for i in [0, 1, 2, 5, 17] {
        let c = Case::new(true);
        let sample = crate::sr_stage::math::jitter_sample(i, 18);
        let (x, y) = (sample.x, sample.y);
        let expected = |projection: &Matrix| {
            crate::temporal_fixture::swap_camera(projection, &c.original.view, x, y, [1286, 724])
                .unwrap()
        };
        assert!(c.prepare(&c.facts3(x, y)));
        assert_eq!(
            (c.engine().project, c.engine().full),
            expected(&c.original.project)
        );
        let main = (c.engine().project, c.engine().full);
        let hud = first_person(&c);
        rsf_anomaly_swap_rm_near(c.swap);
        assert_eq!((c.engine().project, c.engine().full), expected(&hud));
        (c.engine().project, c.engine().full) = main;
        assert!(c.seam() && c.stock());
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_swap_fixture(
    argc: i32,
    argv: *const *const c_char,
    native: &Layout,
) -> i32 {
    // The C launcher lends its process arguments.
    let args = unsafe { crate::ffi::borrow_span(argv, argc.max(0) as usize) };
    let selection = args
        .get(1)
        .map(|&arg| unsafe { CStr::from_ptr(arg) }.to_bytes());
    if catch_unwind(|| profile(native)).is_err() {
        return 1;
    }
    let cases: &[(&str, fn())] = &[
        ("normal", stage1_window_swaps_and_restores_engine_bytes),
        ("blur_only", blur_only),
        ("combine_without_seam", combine_without_seam),
        ("stale_prepare", stale_prepare),
        ("frame_end_open", frame_end_open),
        ("shutdown_open", shutdown_open),
        ("ineligible", ineligible),
        ("layout", layout_refusals),
        ("twin_failures", twin_failures),
        ("write_fault", write_fault),
        ("foreign_change", foreign_change),
        ("round_trip_failure", round_trip_failure),
        ("reset_resize", reset_resize),
        ("names", names),
        ("stage2_normal", stage2_normal),
        ("stage2_optional_odd", stage2_optional_odd),
        ("stage2_layout", stage2_layout),
        ("stage2_exits", stage2_exits),
        ("stage2_foreign", stage2_foreign),
        ("stage2_ssao_viewport", stage2_ssao_viewport),
        (
            "stage2_write_faults",
            a_failed_exchange_rolls_every_write_back,
        ),
        (
            "stage2_retains_twins",
            a_failed_restore_keeps_twins_until_shutdown_can_verify_the_fields,
        ),
        ("stage2_lifecycle", stage2_lifecycle),
        ("stage3_normal", stage3_normal),
        ("stage3_exits", stage3_exits),
        ("stage3_foreign_ineligible", stage3_foreign_ineligible),
        ("stage3_sun_cascades", stage3_sun_cascades),
        ("stage3_sun_rollback", stage3_sun_rollback),
        (
            "stage3_position_reconstruction",
            stage3_position_reconstruction,
        ),
        (
            "published_twin_generation",
            upscale_can_read_the_published_twin_generation_during_the_seam,
        ),
        (
            "first_person_rollback",
            failed_first_person_jitter_restores_the_camera_and_uploaded_projection,
        ),
        ("scope_stock", scope_frames_stay_stock),
        (
            "partial_cache_rollback",
            a_partially_written_cache_range_rolls_back_before_drawing,
        ),
        (
            "cache_readback_retention",
            cache_readback_refusal_keeps_twins_until_the_range_is_clear,
        ),
        (
            "layout_admission",
            layout_and_platform_are_checked_at_creation,
        ),
        ("row_vector_jitter", jitter_follows_the_row_vector_rule),
        ("row_vector_transform", full_transform_composes_row_vectors),
        (
            "row_vector_first_person",
            first_person_projection_is_jittered_once,
        ),
        ("twin_extents", twin_extents_follow_the_engine_division),
    ];
    let (mut ran, mut failures) = (0, 0);
    for &(name, exercise) in cases {
        if selection.is_none_or(|value| value == name.as_bytes()) {
            ran += 1;
            let result = catch_unwind(|| {
                exercise();
                if name == "stage3_normal" {
                    independent_jitter();
                }
            });
            if result.is_err() {
                eprintln!("FAIL: {name}");
                failures += 1;
            }
        }
    }
    if ran == 0 {
        eprintln!("unknown case");
        return 2;
    }
    println!("{ran} case(s), {failures} failure(s)");
    i32::from(failures != 0)
}
