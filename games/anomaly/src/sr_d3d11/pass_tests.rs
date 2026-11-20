// SPDX-License-Identifier: MIT
use super::*;
use crate::sr_d3d11::tests::{CONTEXT, Call, DEVICE, PLATFORM, calls, extent, fake, ptr};

fn pass(kind: u32) -> Option<Pass> {
    Pass::new(&PLATFORM, ptr(DEVICE), kind, None, null_mut())
}

fn views() -> DrawViews {
    DrawViews {
        output: extent(24, 16),
        source: extent(12, 8),
        targets: [ptr(20), ptr(21), ptr(22)],
        depth: ptr(23),
        inputs: [ptr(24), ptr(25), ptr(26), ptr(27), ptr(28)],
        first_person_depth: 0.02f32.to_bits(),
        first_person_zero: 1,
    }
}

#[test]
fn all_passes_bind_in_order_and_restore_the_saved_pipeline() {
    let context = ptr(CONTEXT);
    let round_trip = pass(ROUND_TRIP).unwrap();
    let upscale = pass(UPSCALE).unwrap();
    let views = views();
    calls();

    assert!(round_trip.run(context, ROUND_TRIP, &views));
    assert_eq!(
        calls(),
        [
            Call::Save,
            Call::Resources(0, vec![0; 4]),
            Call::Update([24, 16, 12, 8]),
            Call::Targets(vec![20], 0),
            Call::Resources(0, vec![24]),
            Call::Draw(views.output),
            Call::Restore,
        ]
    );

    assert!(upscale.run(context, PREPARE, &views));
    assert_eq!(
        calls(),
        [
            Call::Save,
            Call::Resources(0, vec![0; 4]),
            Call::Update([24, 16, 0.02f32.to_bits(), 1]),
            Call::Constant(1),
            Call::Targets(vec![20, 21], 0),
            Call::Resources(0, vec![0; 6]),
            Call::Resources(0, vec![24]),
            Call::Resources(3, vec![25]),
            Call::Resources(5, vec![26]),
            Call::Draw(views.output),
            Call::Restore,
        ]
    );

    for (mode, inputs) in [
        (UPSCALE, vec![24, 25, 26, 27]),
        (REINSERT, vec![24, 25, 26, 27, 28]),
    ] {
        assert!(upscale.run(context, mode, &views));
        assert_eq!(
            calls(),
            [
                Call::Save,
                Call::Resources(0, vec![0; 4]),
                Call::Update([24, 16, 12, 8]),
                Call::Stencil(23),
                Call::Targets(vec![20, 21, 22], 23),
                Call::Resources(0, inputs),
                Call::Draw(views.output),
                Call::Restore,
            ]
        );
    }
    drop(round_trip);
    drop(upscale);
    assert!(fake(|state| state.owned.is_empty()));
}

#[test]
fn unavailable_snapshot_does_not_bind_and_bad_inputs_never_save_state() {
    let pass = pass(UPSCALE).unwrap();
    let mut views = views();
    let context = ptr(CONTEXT);
    calls();
    views.inputs[4] = null_mut();
    assert!(!pass.run(context, REINSERT, &views));
    views.source = extent(25, 8);
    assert!(!pass.run(context, UPSCALE, &views));
    assert!(calls().is_empty());

    fake(|state| state.save = false);
    assert!(!pass.run(context, PREPARE, &views));
    assert_eq!(calls(), [Call::Save]);
}

#[test]
fn ordinary_temporal_compile_failure_keeps_the_spatial_pass_but_exceptions_retire_it() {
    fake(|state| state.fail_shader = Some(("rsf_reinsert_ps", 0)));
    let spatial = pass(UPSCALE).unwrap();
    assert!(spatial.temporal.is_none());
    assert_eq!(fake(|state| state.owned.len()), 7);
    let views = views();
    assert!(!spatial.run(ptr(CONTEXT), PREPARE, &views));
    assert!(spatial.run(ptr(CONTEXT), UPSCALE, &views));
    drop(spatial);
    assert!(fake(|state| state.owned.is_empty()));

    fake(|state| state.fail_shader = Some(("rsf_reinsert_ps", -1)));
    assert!(pass(UPSCALE).is_none());
    assert!(fake(|state| state.owned.is_empty()));
}
