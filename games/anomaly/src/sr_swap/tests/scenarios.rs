// SPDX-License-Identifier: MIT
use super::*;
use crate::sys::{
    RSF_ANOMALY_SWAP_RESTORE_COMBINE_END as COMBINE,
    RSF_ANOMALY_SWAP_RESTORE_FRAME_END as FRAME_END, RSF_ANOMALY_SWAP_RESTORE_ROLLBACK as ROLLBACK,
    RSF_ANOMALY_SWAP_RESTORE_SEAM as SEAM, RSF_ANOMALY_SWAP_RESTORE_SHUTDOWN as SHUTDOWN,
    RSF_ANOMALY_SWAP_RESTORE_STALE_PREPARE as STALE, RSF_ANOMALY_SWAP_SKIP_DEPTH as DEPTH,
    RSF_ANOMALY_SWAP_SKIP_LAYOUT as LAYOUT, RSF_ANOMALY_SWAP_SKIP_MSAA as MSAA,
    RSF_ANOMALY_SWAP_SKIP_OFF as OFF, RSF_ANOMALY_SWAP_SKIP_SCOPE as SCOPE,
    RSF_ANOMALY_SWAP_SKIP_TWINS as TWINS,
};

#[test]
fn blur_only() {
    let c = Case::new(true);
    assert!(c.prepare(&c.facts()) && c.seam());
    crate::sr_swap::combine_end(c.swap);
    assert!(c.stock() && c.view().counters.seams_without_window == 0);
}

#[test]
fn combine_without_seam() {
    let c = Case::new(true);
    assert!(c.prepare(&c.facts()));
    crate::sr_swap::combine_end(c.swap);
    assert!(c.stock() && !c.view().refusal.is_null());
    assert_eq!((c.restored(COMBINE), c.host().round_trips), (1, 0));
    assert!(!c.prepare(&c.facts()) && c.skipped(OFF) == 1 && c.stock());
}

#[test]
fn stale_prepare() {
    let c = Case::new(true);
    assert!(c.prepare(&c.facts()) && c.prepare(&c.facts()));
    assert!(c.restored(STALE) == 1 && c.twin_installed(0) && c.twin_installed(1));
    assert!(c.seam() && c.stock() && c.prepare(&c.facts()));
    crate::sr_swap::close_stale(c.swap);
    assert!(c.stock() && c.restored(STALE) == 2 && !c.view().open);
}

#[test]
fn frame_end_open() {
    let c = Case::new(true);
    assert!(c.prepare(&c.facts()));
    crate::sr_swap::frame_end(c.swap);
    assert!(c.stock() && !c.view().refusal.is_null() && c.restored(FRAME_END) == 1);
}

#[test]
fn shutdown_open() {
    let c = Case::new(true);
    assert!(c.prepare(&c.facts()) && crate::sr_swap::shutdown(c.swap));
    assert!(c.stock() && !c.view().open && c.host().drops == 1 && c.restored(SHUTDOWN) == 1);
    assert!(crate::sr_swap::shutdown(c.swap) && c.stock() && c.host().drops == 1);
}

fn check_ineligible(world: bool) {
    let c = Case::new(true);
    let mut facts = if world { c.facts2() } else { c.facts() };
    facts.scope = true;
    assert!(!c.prepare(&facts) && c.stock() && c.skipped(SCOPE) == 1);
    facts.scope = false;
    facts.msaa = true;
    assert!(!c.prepare(&facts) && c.stock() && c.skipped(MSAA) == 1);
    assert!(!c.seam());
    crate::sr_swap::combine_end(c.swap);
    crate::sr_swap::frame_end(c.swap);
    assert!(
        c.stock()
            && c.view().refusal.is_null()
            && c.host().makes == 0
            && c.host().viewports.is_empty()
    );
    facts.msaa = false;
    crate::sr_swap::set_enabled(c.swap, false);
    assert!(!c.prepare(&facts) && c.stock() && c.skipped(OFF) == 1);
    crate::sr_swap::set_enabled(c.swap, true);
    assert!(c.prepare(&facts) && c.seam() && c.stock());
}

#[test]
fn ineligible() {
    check_ineligible(false);
}

fn refused(mut change: impl FnMut(&mut Engine), world: bool, reason: u32) {
    let mut c = Case::new(true);
    change(c.engine());
    c.rebase();
    let facts = if world { c.facts2() } else { c.facts() };
    assert!(!c.prepare(&facts));
    assert!(
        c.stock() && !c.view().refusal.is_null() && c.skipped(reason) == 1 && c.host().makes == 0
    );
}

#[test]
fn layout_refusals() {
    let cases: [fn(&mut Engine); 7] = [
        |e| put(&mut e.crt[0], 0x40, 28u32),
        |e| put(&mut e.crt[1], 0x38, 960u32),
        |e| put(&mut e.texture[0], 0xa8, 0usize),
        |e| put(&mut e.texture[1], 0x50, (&raw mut e.objects[15]) as usize),
        |e| put(&mut e.crt[0], 0x20, (&raw mut e.objects[15]) as usize),
        |e| put(&mut e.target, 0x3b8, 0usize),
        |e| put(&mut e.target, 0x3b8, e.crt[0].as_mut_ptr() as usize),
    ];
    for change in cases {
        refused(change, false, LAYOUT);
    }
    let c = Case::new(true);
    assert!(
        !c.prepare(&Facts {
            render_target: null_mut(),
            ..c.facts()
        }) && c.stock()
            && !c.view().refusal.is_null()
    );
}

#[test]
fn twin_failures() {
    let c = Case::new(true);
    c.host().make_result = RSF_ANOMALY_SWAP_MAKE_FAILED;
    assert!(!c.prepare(&c.facts()) && c.stock() && c.view().refusal.is_null());
    assert!(!c.prepare(&c.facts()) && c.host().makes == 1 && c.skipped(TWINS) == 2);
    c.host().make_result = RSF_ANOMALY_SWAP_MAKE_CREATED;
    assert!(
        c.prepare(&Facts {
            generation: 1,
            ..c.facts()
        }) && c.host().makes == 2
            && c.seam()
            && c.stock()
    );
    assert_eq!(c.view().counters.allocation_failures, 1);
    let wrong = Case::new(true);
    wrong.host().wrong_format = true;
    assert!(
        !wrong.prepare(&wrong.facts())
            && wrong.stock()
            && !wrong.view().refusal.is_null()
            && wrong.host().drops == 1
    );
    let denied = Case::new(true);
    denied.host().make_result = RSF_ANOMALY_SWAP_MAKE_REFUSED;
    assert!(
        !denied.prepare(&denied.facts())
            && denied.stock()
            && !denied.view().refusal.is_null()
            && denied.skipped(TWINS) == 1
    );
}

#[test]
fn write_fault() {
    for at in 0..22 {
        let c = Case::new(true);
        c.host().fail_write_at = Some(at);
        assert!(
            !c.prepare(&c.facts()) && c.stock() && !c.view().refusal.is_null() && !c.view().open
        );
        assert_eq!(c.restored(ROLLBACK), 1);
    }
}

#[test]
fn foreign_change() {
    let c = Case::new(true);
    assert!(c.prepare(&c.facts()));
    let foreign = &raw mut c.engine().objects[50];
    put(&mut c.engine().texture[0], 0xa8, foreign);
    assert!(
        c.seam()
            && c.stock()
            && !c.view().refusal.is_null()
            && c.view().counters.foreign_changes == 1
    );
}

fn failed_draw(world: bool) {
    let c = Case::new(true);
    c.host().fail_pass = true;
    let facts = if world { c.facts2() } else { c.facts() };
    assert!(c.prepare(&facts) && c.seam() && c.stock());
    assert!(!c.view().refusal.is_null() && c.view().counters.round_trip_failures == 1);
}

#[test]
fn round_trip_failure() {
    failed_draw(false);
}

#[test]
fn reset_resize() {
    let mut c = Case::new(true);
    assert!(c.prepare(&c.facts()) && c.seam());
    let facts = Facts {
        generation: 1,
        ..c.facts()
    };
    assert!(c.prepare(&facts) && c.seam() && c.stock());
    assert_eq!(
        (
            c.host().makes,
            c.host().drops,
            c.view().counters.twin_generations
        ),
        (2, 1, 2)
    );
    c.resize([1280, 720], false);
    let facts = Facts {
        generation: 2,
        ..c.facts()
    };
    assert!(c.prepare(&facts) && c.twin_installed(0) && c.twin_installed(1));
    assert_eq!(
        (
            c.view().twins.targets[0].width,
            c.view().twins.targets[1].height
        ),
        (1280, 720)
    );
    assert!(c.seam() && c.stock() && c.host().makes == 3 && c.host().drops == 2);
    let e = c.engine();
    let surface = &raw mut e.objects[59];
    put(&mut e.crt[0], 0x10, surface);
    put(&mut e.texture[0], 0x50, surface);
    put(&mut e.texture[0], 0x70, 0usize);
    c.rebase();
    assert!(c.prepare(&facts) && c.seam() && c.stock() && c.host().makes == 4);
    assert!(
        c.prepare(&Facts {
            device: ptr::without_provenance_mut(0x5678),
            ..facts
        }) && c.seam()
            && c.stock()
            && c.host().makes == 5
    );
}

fn globals(c: &Case, width: u32, height: u32) {
    let tw = c.view().twins;
    assert_eq!(c.engine().device_size, [width, height]);
    assert!(!tw.base_rt.is_null() && !tw.depth_dsv.is_null());
    assert_eq!(
        (c.engine().base_rt, c.engine().base_zb),
        (tw.base_rt as usize, tw.depth_dsv as usize)
    );
}

#[test]
fn stage2_normal() {
    let mut c = Case::new(true);
    c.ssao([960., 540.]);
    assert!(c.prepare(&c.facts2()));
    let view = c.view();
    let request = c.host().request.unwrap();
    assert_eq!(
        (view.stage, view.render_width, view.output_width),
        (2, 1286, 1920)
    );
    for (i, target) in view.twins.targets.iter().enumerate() {
        assert!(c.twin_installed(i));
        assert_eq!((target.width, target.height), (1286, 724));
        assert!(request.spec[i].present && request.spec[i].promote == (i < 2));
    }
    assert_eq!(
        (view.twins.targets[2].format, view.twins.targets[7].format),
        (113, 111)
    );
    assert!(!view.twins.targets[8].uav.is_null());
    globals(&c, 1286, 724);
    assert_eq!(view.twins.depth_width, 1286);
    assert_eq!(&c.engine().viewport[2..4], &[643., 362.]);
    assert!(c.texture_cache_cleared() && get::<usize>(&c.engine().backend, 0x5c8) == 0);
    assert_eq!(
        (request.engine_dsv as usize, request.output_width),
        (c.original.base_zb, 1920)
    );
    crate::sr_swap::prepared(c.swap);
    assert_eq!(c.host().viewports, [(1286, 724)]);
    put(&mut c.engine().target, 0x8, 1286u32);
    put(&mut c.engine().target, 0xc, 724u32);
    c.engine().backend.fill(0x5a);
    crate::sr_swap::rm_near(c.swap);
    assert!(c.seam() && c.stock() && c.before_draw_stock() && c.texture_cache_cleared());
    assert_eq!(get::<usize>(&c.engine().backend, 0x5c8), 0);
    let views = c.host().seam_views.unwrap();
    for (i, slot) in [0, 2, 5].into_iter().enumerate() {
        assert_eq!(views[i].surface, view.twins.targets[slot].surface);
        assert_eq!(
            views[i + 3].rtv,
            get::<*mut c_void>(&c.original.crt[slot], 0x18)
        );
    }
    assert_eq!(c.host().viewports, [(1286, 724), (1920, 1080)]);
    assert!(!c.seam());
    crate::sr_swap::combine_end(c.swap);
    crate::sr_swap::frame_end(c.swap);
    for _ in 0..3 {
        assert!(c.prepare(&c.facts2()));
        crate::sr_swap::prepared(c.swap);
        assert!(c.seam() && c.stock());
        crate::sr_swap::combine_end(c.swap);
    }
    let n = c.view().counters;
    assert_eq!(
        (
            c.host().makes,
            n.swapped,
            n.round_trips,
            n.ssao_viewport_swaps,
            n.viewports,
            n.rm_near_in_window,
            c.restored(SEAM)
        ),
        (1, 4, 4, 4, 8, 1, 4)
    );
    assert!(crate::sr_swap::shutdown(c.swap) && c.stock() && c.host().drops == 1);
}

// Row and column index two matrices and the axes together.
#[allow(clippy::needless_range_loop)]
#[test]
fn stage2_optional_odd() {
    let c = Case::new(false);
    assert!(c.prepare(&c.facts2()));
    assert!((0..6).all(|slot| c.twin_installed(slot)));
    for i in 6..SLOTS {
        assert!(
            !c.host().request.unwrap().spec[i].present
                && c.view().twins.targets[i].surface.is_null()
        );
        assert_eq!(get::<usize>(&c.engine().target, WORLD[i].0), 0);
    }
    assert!(c.seam() && c.stock());
    let mut c = Case::new(true);
    c.resize([1919, 1079], true);
    assert!(c.prepare(&c.facts2()));
    let tw = c.view().twins.targets;
    assert_eq!(
        (tw[7].width, tw[7].height, tw[0].width, tw[0].height),
        (643, 361, 1286, 723)
    );
    assert!(c.seam() && c.stock());
    let c = Case::new(true);
    for (scale, width, height) in [(0.5, 960, 540), (1., 1920, 1080)] {
        assert!(c.prepare(&c.facts_at_scale(scale)));
        globals(&c, width, height);
        assert_eq!(c.view().twins.targets[2].width, width);
        assert!(c.seam() && c.stock());
    }
    assert_eq!(c.host().makes, 2);
}

#[test]
fn stage2_layout() {
    let cases: [fn(&mut Engine); 11] = [
        |e| put(&mut e.crt[2], 0x40, 21u32),
        |e| put(&mut e.crt[7], 0x40, 21u32),
        |e| put(&mut e.crt[7], 0x38, 480u32),
        |e| put(&mut e.crt[5], 0x38, 960u32),
        |e| put(&mut e.crt[7], 0x38, 960u32),
        |e| put(&mut e.target, WORLD[2].0, 0usize),
        |e| put(&mut e.target, WORLD[6].0, e.crt[5].as_mut_ptr() as usize),
        |e| put(&mut e.target, 0x390, e.crt[6].as_mut_ptr() as usize),
        |e| put(&mut e.target, 0x358, e.crt[6].as_mut_ptr() as usize),
        |e| e.device_size[0] = 1600,
        |e| e.base_rt = 0,
    ];
    for change in cases {
        refused(change, true, LAYOUT);
    }
    refused(|e| e.base_zb = 0, true, DEPTH);
    let c = Case::new(true);
    c.host().make_result = RSF_ANOMALY_SWAP_MAKE_DEPTH;
    assert!(
        !c.prepare(&c.facts2())
            && c.stock()
            && !c.view().refusal.is_null()
            && c.skipped(DEPTH) == 1
    );
    for invalid in [0, 1] {
        let c = Case::new(true);
        let mut facts = c.facts2();
        if invalid == 0 {
            facts.base_zb = null_mut();
        } else {
            facts.render_width = 2000;
        }
        assert!(!c.prepare(&facts) && c.stock() && !c.view().refusal.is_null());
    }
    let c = Case::new(true);
    c.host().wrong_format = true;
    assert!(
        !c.prepare(&c.facts2()) && c.stock() && !c.view().refusal.is_null() && c.host().drops == 1
    );
}

fn close(c: &Case, reason: u32) {
    match reason {
        COMBINE => crate::sr_swap::combine_end(c.swap),
        FRAME_END => crate::sr_swap::frame_end(c.swap),
        STALE => crate::sr_swap::close_stale(c.swap),
        SHUTDOWN => assert!(crate::sr_swap::shutdown(c.swap)),
        _ => unreachable!(),
    }
}

#[test]
fn stage2_exits() {
    for reason in [COMBINE, FRAME_END, STALE, SHUTDOWN] {
        let c = Case::new(true);
        assert!(c.prepare(&c.facts2()));
        if reason == COMBINE {
            crate::sr_swap::prepared(c.swap);
            put(&mut c.engine().target, 0x8, 1286u32);
        }
        close(&c, reason);
        assert!(c.stock() && c.restored(reason) == 1 && c.host().upscales == 0);
        match reason {
            SHUTDOWN => assert!(c.host().viewports.is_empty() && c.host().drops == 1),
            STALE => {
                assert!(
                    c.view().refusal.is_null() && c.host().viewports.last() == Some(&(1920, 1080))
                );
                assert!(c.prepare(&c.facts2()) && c.seam() && c.stock());
            }
            _ => assert!(
                !c.view().refusal.is_null() && c.host().viewports.last() == Some(&(1920, 1080))
            ),
        }
    }
    failed_draw(true);
    check_ineligible(true);
}

#[test]
fn stage2_foreign() {
    for mutator in [0, 1, 2] {
        let c = Case::new(true);
        assert!(c.prepare(&c.facts2()));
        let e = c.engine();
        match mutator {
            0 => e.device_size[0] = 800,
            1 => e.base_zb = (&raw mut e.objects[50]) as usize,
            _ => e.base_rt = (&raw mut e.objects[51]) as usize,
        }
        assert!(
            c.seam()
                && c.stock()
                && !c.view().refusal.is_null()
                && c.view().counters.foreign_changes == 1
        );
    }
}

#[test]
fn stage2_ssao_viewport() {
    let mut c = Case::new(true);
    c.ssao([640., 360.]);
    assert!(c.prepare(&c.facts2()) && c.engine().viewport[2] == 643.);
    assert!(c.seam() && c.stock());
    let mut c = Case::new(true);
    assert!(
        c.prepare(&c.facts2())
            && c.engine().viewport[2] == 0.
            && c.view().counters.ssao_viewport_swaps == 0
    );
    c.ssao([643., 362.]);
    assert!(c.seam() && c.view().refusal.is_null());
    assert_eq!(&c.engine().viewport[2..4], &[960., 540.]);
    c.rebase();
    assert!(c.prepare(&c.facts2()) && c.engine().viewport[2] == 643. && c.seam() && c.stock());
    let mut c = Case::new(true);
    c.engine().guard = -1;
    c.rebase();
    assert!(c.prepare(&c.facts2()) && c.engine().viewport[2] == 0. && c.seam() && c.stock());
    c.ssao([960., 540.]);
    let facts = Facts {
        ssao_viewport: null_mut(),
        ssao_viewport_guard: null_mut(),
        ..c.facts2()
    };
    assert!(c.prepare(&facts) && c.engine().viewport[2] == 960. && c.seam() && c.stock());
}

#[test]
fn stage2_lifecycle() {
    let mut c = Case::new(true);
    c.ssao([960., 540.]);
    assert!(c.prepare(&c.facts2()) && c.seam() && c.stock());
    let mut facts = c.facts_at_scale(0.5);
    assert!(c.prepare(&facts) && c.view().twins.targets[0].width == 960);
    globals(&c, 960, 540);
    assert!(c.seam() && c.stock() && c.host().makes == 2 && c.host().drops == 1);
    assert_eq!(c.view().counters.twin_generations, 2);
    assert!(c.prepare(&facts) && c.seam() && c.host().makes == 2);
    facts.generation = 1;
    for (i, slot) in [61, 62, 58].into_iter().enumerate() {
        let address = (&raw mut c.engine().objects[slot]) as usize;
        if slot == 58 {
            c.engine().base_rt = address;
        } else {
            c.engine().base_zb = address;
        }
        c.rebase();
        assert!(c.prepare(&facts));
        let request = c.host().request.unwrap();
        assert_eq!(
            (request.engine_dsv as usize, request.engine_rtv as usize),
            (c.original.base_zb, c.original.base_rt)
        );
        assert!(c.seam() && c.stock() && c.host().makes == i + 3);
    }
    c.resize([1280, 720], false);
    c.ssao([640., 360.]);
    facts = Facts {
        generation: 2,
        ..c.facts2()
    };
    assert!(c.prepare(&facts));
    assert_eq!(
        (c.view().twins.targets[2].width, c.view().twins.depth_height),
        (858, 482)
    );
    assert!(c.seam() && c.stock() && c.host().makes == 6 && c.host().drops == 5);
    assert!(
        c.prepare(&c.facts()) && c.host().makes == 7 && c.view().twins.targets[0].width == 1280
    );
    assert!(c.seam() && c.stock());
    c.host().make_result = RSF_ANOMALY_SWAP_MAKE_FAILED;
    assert!(
        !c.prepare(&c.facts_at_scale(0.6))
            && c.stock()
            && c.view().refusal.is_null()
            && c.skipped(TWINS) == 1
    );
    assert!(!c.prepare(&c.facts_at_scale(0.6)) && c.host().makes == 8);
    c.host().make_result = RSF_ANOMALY_SWAP_MAKE_CREATED;
    assert!(c.prepare(&c.facts_at_scale(0.7)) && c.host().makes == 9 && c.seam() && c.stock());
    assert!(
        crate::sr_swap::shutdown(c.swap)
            && c.stock()
            && c.view().counters.twin_drops == c.host().drops as u64
    );
}

fn first_person(c: &Case) -> Matrix {
    let p = perspective(0.9, c.size[0] as f32 / c.size[1] as f32, 0.05);
    c.engine().project = p;
    c.engine().full = engine_mul(&p, &c.engine().view);
    p
}

#[test]
fn stage3_normal() {
    stage3_window_jitters_main_and_first_person_once();
    let mut c = Case::new(true);
    c.ssao([960., 540.]);
    let (x, y) = (0.25, -1. / 6.);
    assert!(c.prepare(&c.facts3(x, y)));
    let main = (c.engine().project, c.engine().full);
    let expected_bias = 0.25 + (1286.0f64 / 1920.).log2() as f32;
    assert!((c.engine().sampler_bias - expected_bias).abs() < 1e-6);
    assert_eq!(c.view().window_mip_bias, c.engine().sampler_bias);
    let hud = first_person(&c);
    crate::sr_swap::rm_near(c.swap);
    let jittered_hud = (c.engine().project, c.engine().full);
    assert_ne!(jittered_hud.0, hud);
    crate::sr_swap::rm_near(c.swap);
    assert_eq!((c.engine().project, c.engine().full), jittered_hud);
    (c.engine().project, c.engine().full) = main;
    crate::sr_swap::rm_near(c.swap);
    let mut foreign = hud;
    foreign[8] = 0.01;
    c.engine().project = foreign;
    crate::sr_swap::rm_near(c.swap);
    assert_eq!(c.engine().project, foreign);
    for _ in 0..5 {
        first_person(&c);
        crate::sr_swap::rm_near(c.swap);
        assert_eq!((c.engine().project, c.engine().full), jittered_hud);
        (c.engine().project, c.engine().full) = main;
    }
    let n = c.view().counters;
    assert_eq!(
        (
            n.jittered,
            n.fp_jittered,
            n.fp_already,
            n.fp_foreign,
            n.rm_near_in_window
        ),
        (1, 6, 2, 1, 9)
    );
    assert!(c.seam() && c.stock() && c.view().refusal.is_null());
    crate::sr_swap::rm_near(c.swap);
    assert_eq!(c.view().counters.rm_near_in_window, 9);
    let n = c.view().counters;
    assert_eq!(
        (n.projection_restores, n.mip_applied, n.mip_restored),
        (1, 1, 1)
    );
    for index in [1, 2, 17] {
        let sample = crate::sr_stage::math::jitter_sample(index, 18);
        assert!(c.prepare(&c.facts3(sample.x, sample.y)));
        assert_eq!(
            c.engine().project,
            jitter_projection(&c.original.project, sample.x, sample.y, 1286, 724).unwrap()
        );
        assert!(c.seam() && c.stock());
    }
    assert!(crate::sr_swap::shutdown(c.swap) && c.stock());
}

#[test]
fn stage3_exits() {
    for reason in [COMBINE, FRAME_END, STALE, SHUTDOWN] {
        let c = Case::new(true);
        let facts = c.facts3(0.25, -1. / 6.);
        assert!(c.prepare(&facts));
        crate::sr_swap::prepared(c.swap);
        let main = (c.engine().project, c.engine().full);
        first_person(&c);
        crate::sr_swap::rm_near(c.swap);
        (c.engine().project, c.engine().full) = main;
        let uploads = c.host().upload_calls;
        close(&c, reason);
        assert!(
            c.stock()
                && c.restored(reason) == 1
                && c.host().biases.last() == Some(&c.original.sampler_bias)
        );
        match reason {
            SHUTDOWN => assert_eq!(c.host().upload_calls, uploads),
            _ => {
                assert_eq!(c.host().upload_calls, uploads + 1);
                assert_eq!(c.host().uploads.last(), Some(&c.original.project));
                assert_eq!(c.view().refusal.is_null(), reason == STALE);
            }
        }
        let count = c.view().counters;
        assert_eq!((count.projection_restores, count.mip_restored), (1, 1));
    }
    let probe = Case::new(true);
    assert!(probe.prepare(&probe.facts2()));
    for extra in 0..2 {
        let c = Case::new(true);
        c.host().fail_write_at = Some(probe.host().writes + extra);
        assert!(!c.prepare(&c.facts3(0.25, -1. / 6.)));
        assert!(
            c.stock() && !c.view().open && !c.view().refusal.is_null() && !c.view().jitter_applied
        );
        assert_eq!(
            (
                c.restored(ROLLBACK),
                c.host().upload_calls,
                c.host().bias_calls
            ),
            (1, 0, 0)
        );
    }
    let c = Case::new(true);
    c.host().fail_upload_at = Some(0);
    assert!(!c.prepare(&c.facts3(0.25, -1. / 6.)) && c.stock() && !c.view().refusal.is_null());
    assert_eq!(c.host().upload_calls, 2);
    assert_eq!(c.host().uploads, [c.original.project]);
    let c = Case::new(true);
    c.host().fail_bias_at = Some(0);
    assert!(!c.prepare(&c.facts3(0.25, -1. / 6.)) && c.stock() && !c.view().refusal.is_null());
    assert_eq!(c.host().bias_calls, 2);
    assert_eq!(c.host().biases, [c.original.sampler_bias]);
    assert_eq!(c.host().uploads.last(), Some(&c.original.project));
    pending_camera_and_sampler_restoration_retains_twins_until_quiesce_retries();
}

#[test]
fn stage3_foreign_ineligible() {
    for mode in [0, 1] {
        let c = Case::new(true);
        assert!(c.prepare(&c.facts3(0.25, 0.1)));
        if mode == 0 {
            c.engine().project[0] *= 1.01;
        } else {
            c.engine().sampler_bias = 2.;
        }
        assert!(
            c.seam()
                && c.stock()
                && !c.view().refusal.is_null()
                && c.view().counters.foreign_changes == 1
        );
    }
    let mut c = Case::new(true);
    let mut facts = c.facts3(0.25, 0.1);
    facts.scope = true;
    assert!(!c.prepare(&facts) && !c.view().jitter_applied);
    first_person(&c);
    c.rebase();
    crate::sr_swap::rm_near(c.swap);
    facts.scope = false;
    facts.msaa = true;
    assert!(!c.prepare(&facts));
    crate::sr_swap::rm_near(c.swap);
    crate::sr_swap::set_enabled(c.swap, false);
    assert!(!c.prepare(&c.facts3(0.25, 0.1)));
    assert!(
        c.stock()
            && c.host().upload_calls == 0
            && c.host().bias_calls == 0
            && c.view().counters.fp_jittered == 0
    );
    let c = Case::new(true);
    let facts = Facts {
        jitter: false,
        ..c.facts3(0.25, 0.1)
    };
    assert!(c.prepare(&facts) && !c.view().jitter_applied && c.view().mip_bias_applied);
    let hud = first_person(&c);
    crate::sr_swap::rm_near(c.swap);
    assert_eq!(c.engine().project, hud);
    assert_eq!(c.view().counters.fp_jittered, 0);
    (c.engine().project, c.engine().full) = (c.original.project, c.original.full);
    assert!(c.seam() && c.stock() && c.host().upload_calls == 0);
    let c = Case::new(true);
    let facts = c.facts3(0.25, 0.1);
    assert!(
        c.prepare(&Facts {
            mip_bias: false,
            ..facts
        }) && !c.view().mip_bias_applied
            && c.seam()
            && c.stock()
    );
    assert!(
        c.prepare(&Facts {
            mip_bias_delta: 0.,
            ..facts
        }) && c.seam()
            && c.stock()
    );
    assert_eq!(
        (c.host().bias_calls, c.view().counters.mip_unchanged),
        (0, 1)
    );
    let c = Case::new(true);
    c.engine().project[0] = f32::NAN;
    assert!(c.prepare(&c.facts3(0.25, 0.1)) && !c.view().jitter_applied);
    assert_eq!(c.view().counters.jitter_invalid, 1);
    assert!(
        c.seam()
            && c.view().refusal.is_null()
            && c.engine().project[0].is_nan()
            && c.host().upload_calls == 0
    );
}

#[test]
fn stage3_sun_cascades() {
    sun_cascades_borrow_the_unjittered_camera();
    for index in [0, 5, 17] {
        let c = Case::new(true);
        let sample = crate::sr_stage::math::jitter_sample(index, 18);
        assert!(c.prepare(&c.facts3(sample.x, sample.y)));
        let current = *c.engine();
        let token = crate::sr_swap::sun_begin(c.swap);
        assert!(token);
        for _ in 0..3 {
            assert_eq!(
                (c.engine().project, c.engine().full, c.engine().view),
                (c.original.project, c.original.full, c.original.view)
            );
            assert!(unsafe {
                upload_projection(c.host.cast(), (&raw const c.engine().project).cast())
            });
            crate::sr_swap::rm_near(c.swap);
            assert_eq!(
                (c.engine().project, c.engine().full),
                (c.original.project, c.original.full)
            );
        }
        let nested = crate::sr_swap::sun_begin(c.swap);
        assert!(!nested);
        crate::sr_swap::sun_end(c.swap, nested);
        assert_eq!(c.engine().project, c.original.project);
        crate::sr_swap::sun_end(c.swap, token);
        assert!(*c.engine() == current);
        assert_eq!(c.host().uploads.last(), Some(&current.project));
        let n = c.view().counters;
        assert_eq!(
            (n.sun_calls, n.sun_unjittered, n.sun_restored, n.sun_refused),
            (2, 1, 1, 0)
        );
        assert!(c.seam() && c.stock() && c.view().refusal.is_null());
    }
    for mode in [0, 1, 2] {
        let c = Case::new(true);
        let mut facts = c.facts3(0.25, 0.1);
        if mode == 0 {
            crate::sr_swap::set_enabled(c.swap, false);
        }
        if mode == 1 {
            facts.scope = true;
        }
        if mode == 2 {
            facts.jitter = false;
        }
        c.prepare(&facts);
        let before = *c.engine();
        let counts = (c.host().writes, c.host().upload_calls);
        let token = crate::sr_swap::sun_begin(c.swap);
        assert!(!token);
        crate::sr_swap::sun_end(c.swap, token);
        assert!(*c.engine() == before);
        assert_eq!((c.host().writes, c.host().upload_calls), counts);
        let n = c.view().counters;
        assert_eq!(
            (n.sun_calls, n.sun_unjittered, n.sun_restored, n.sun_refused),
            (1, 0, 0, 0)
        );
        assert!(crate::sr_swap::shutdown(c.swap) && c.stock());
    }
    stage3_sun_rollback();
    let c = Case::new(true);
    assert!(c.prepare(&c.facts3(0.25, 0.1)));
    let token = crate::sr_swap::sun_begin(c.swap);
    assert!(token && crate::sr_swap::shutdown(c.swap));
    let uploads = c.host().upload_calls;
    crate::sr_swap::sun_end(c.swap, token);
    assert!(
        c.stock() && c.host().upload_calls == uploads && c.view().counters.foreign_changes == 0
    );
}

#[test]
fn stage3_sun_rollback() {
    sun_refusal_returns_the_frame_to_output_size();
    for failure in 0..5 {
        let c = Case::new(true);
        assert!(c.prepare(&c.facts3(0.25, -1. / 6.)));
        crate::sr_swap::prepared(c.swap);
        put(&mut c.engine().target, 0x8, 1286u32);
        put(&mut c.engine().target, 0xc, 724u32);
        if failure < 2 {
            let at = c.host().writes + failure;
            c.host().fail_write_at = Some(at);
        }
        let token = crate::sr_swap::sun_begin(c.swap);
        assert_eq!(token, failure >= 2);
        if (2..4).contains(&failure) {
            let at = c.host().writes + failure - 2;
            c.host().fail_write_at = Some(at);
        } else if failure == 4 {
            let at = c.host().upload_calls;
            c.host().fail_upload_at = Some(at);
        }
        crate::sr_swap::sun_end(c.swap, token);
        assert!(c.view().open && !c.view().refusal.is_null() && c.host().viewports.len() == 1);
        assert!(c.seam() && c.host().upscales == 1 && c.host().recovery_inputs.is_some());
        assert!(c.stock() && !c.view().open && !c.view().refusal.is_null());
        assert_eq!(c.host().uploads.last(), Some(&c.original.project));
        assert_eq!(c.host().viewports, [(1286, 724), (1920, 1080)]);
        let n = c.view().counters;
        assert_eq!((n.sun_refused, n.sun_restored, c.restored(SEAM)), (1, 0, 1));
    }
}

#[test]
fn stage3_position_reconstruction() {
    let c = Case::new(true);
    let mut byte = 0u8;
    let constant = (&raw mut byte).cast();
    let position = |c: &Case| crate::sr_swap::position_constant(c.swap, constant);
    position(&c);
    assert!(c.view().counters.position_stock == 1 && c.host().positions.is_empty());
    assert!(c.prepare(&c.facts3(0.25, 0.1)));
    position(&c);
    assert_eq!(c.host().constant, constant);
    assert_eq!(c.host().positions, [(0.25, 0.1)]);
    let token = crate::sr_swap::sun_begin(c.swap);
    assert!(token);
    position(&c);
    assert_eq!(c.view().counters.position_corrected, 2);
    crate::sr_swap::sun_end(c.swap, token);
    assert!(c.seam());
    position(&c);
    assert!(c.view().counters.position_stock == 2 && c.host().positions.len() == 2 && c.stock());
    for mode in [0, 1, 2] {
        let c = Case::new(true);
        let mut facts = c.facts3(0.25, 0.1);
        if mode == 0 {
            facts.jitter = false;
        }
        if mode == 1 {
            facts.scope = true;
        }
        if mode == 2 {
            crate::sr_swap::set_enabled(c.swap, false);
        }
        c.prepare(&facts);
        position(&c);
        assert!(c.view().counters.position_stock == 1 && c.host().positions.is_empty());
        assert!(crate::sr_swap::shutdown(c.swap) && c.stock());
    }
    let c = Case::new(true);
    assert!(c.prepare(&c.facts3(0.25, 0.1)));
    c.host().refuse_position = true;
    position(&c);
    assert!(c.view().counters.position_refused == 1 && !c.view().refusal.is_null());
    assert!(crate::sr_swap::shutdown(c.swap) && c.stock());
}

/// The jittered main and first-person cameras against the independent native camera oracle (temporal_fixture.rs).
#[test]
fn stage3_matches_the_independent_camera_oracle() {
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
        crate::sr_swap::rm_near(c.swap);
        assert_eq!((c.engine().project, c.engine().full), expected(&hud));
        (c.engine().project, c.engine().full) = main;
        assert!(c.seam() && c.stock());
    }
}
