// SPDX-License-Identifier: MIT
use super::*;
use crate::sys::{
    RSF_FG_FIXED, RSF_FG_STAT_ACTIVITY, RSF_FG_STAT_PRESENT_CALLBACKS, RSF_FG_STAT_TOTAL_PRESENTED,
};
use std::sync::Barrier;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

fn enabled() -> (State, Options) {
    let state = State::new(initial_status(2));
    let options = Options {
        struct_size: size_of::<Options>() as u32,
        abi_version: RSF_FG_ABI_VERSION,
        mode: RSF_FG_FIXED,
        generated_frames: 4,
        dynamic_target_fps: 0.0,
        reflex_mode: 1,
        frame_limit_us: 0,
    };
    state.configured(options, RSF_FG_FIXED, 2, 1, true);
    (state, options)
}

#[test]
fn abort_wins_over_an_in_flight_preparation_and_observation() {
    let (state, _) = enabled();
    let revision = state.read().order.revision;
    assert!(state.stage(revision, 28, RSF_FG_FIXED, 2));
    thread::scope(|scope| scope.spawn(|| state.abort()).join().unwrap());
    assert!(!state.complete(revision, 28, true, RSF_FG_FIXED, 2));
    assert_eq!(state.read().order.last_prepare, 28);
    assert_eq!(state.read().order.interpolated, None);
    assert!(!state.reset_history(revision));
    assert!(!state.stage(revision, 29, RSF_FG_FIXED, 2));
    let status = state.observe(
        revision,
        Observation {
            vendor_status: 0,
            statistics: RSF_FG_STAT_TOTAL_PRESENTED,
            presented: 3,
            active: true,
            callbacks: None,
        },
    );
    assert_eq!((status.active, status.total_presented), (0, 3));
}

#[test]
fn option_changes_keep_requested_count_and_consume_disabled_sources() {
    let (state, mut options) = enabled();
    let before = state.read();
    assert_eq!(before.request.unwrap().generated_frames, 4);
    assert_eq!(before.status.effective_generated_frames, 2);
    assert!(state.stage(before.order.revision, 7, RSF_FG_FIXED, 2));
    assert!(state.complete(before.order.revision, 7, true, RSF_FG_FIXED, 2));

    options.frame_limit_us = 8_000;
    state.configured(options, RSF_FG_FIXED, 2, 1, false);
    assert_eq!(state.read().order.interpolated, Some(7));
    assert_eq!(state.read().order.revision, before.order.revision);

    options.mode = RSF_FG_OFF;
    state.configured(options, RSF_FG_OFF, 0, 0, true);
    assert!(!state.complete(before.order.revision, 8, true, RSF_FG_FIXED, 2));
    assert_eq!(state.read().status.effective_mode, RSF_FG_OFF);
    let current = state.read();
    assert!(state.stage(current.order.revision, 9, RSF_FG_OFF, 0));
    assert!(state.complete(current.order.revision, 9, false, RSF_FG_OFF, 0));
    assert_eq!(state.read().order.last_prepare, 9);
    assert_eq!(state.read().order.interpolated, None);
}

#[test]
fn callback_counts_and_dxgi_counts_have_separate_validity() {
    let (state, _) = enabled();
    let rev = state.read().order.revision;
    assert!(state.stage(rev, 2, RSF_FG_FIXED, 1));
    assert!(state.complete(rev, 2, true, RSF_FG_FIXED, 1));
    let total = RSF_FG_STAT_TOTAL_PRESENTED;
    let callbacks = RSF_FG_STAT_PRESENT_CALLBACKS;
    let status = state.observe(
        rev,
        Observation {
            statistics: total | callbacks,
            callbacks: Some((3, 7)),
            presented: 8,
            active: true,
            ..Observation::default()
        },
    );
    assert_eq!(
        (
            status.active,
            status.total_presented,
            status.generated_presented
        ),
        (1, 8, 0)
    );
    let status = state.observe(
        rev,
        Observation {
            statistics: callbacks,
            callbacks: Some((4, 9)),
            ..Observation::default()
        },
    );
    assert_eq!(
        (
            status.valid_statistics,
            status.total_presented,
            status.generated_callbacks
        ),
        (callbacks, 8, 4)
    );
    let status = state.observe(
        rev,
        Observation {
            vendor_status: -7,
            statistics: total,
            presented: 2,
            ..Observation::default()
        },
    );
    assert_eq!(
        (
            status.active,
            status.vendor_status,
            status.total_presented,
            status.valid_statistics
        ),
        (0, -7, 10, callbacks | total)
    );
    let warning = state.observe(
        rev,
        Observation {
            vendor_status: 5,
            ..Observation::default()
        },
    );
    assert_eq!((warning.vendor_status, warning.total_presented), (5, 10));
}

#[test]
fn status_snapshots_are_coherent_while_other_threads_abort_and_pace() {
    const SAMPLES: u64 = 512;
    let (state, _) = enabled();
    let gate = Barrier::new(3);
    let done = AtomicBool::new(false);
    thread::scope(|scope| {
        scope.spawn(|| {
            gate.wait();
            for count in 1..=SAMPLES {
                state.observe(
                    state.read().order.revision,
                    Observation {
                        vendor_status: count as i32,
                        statistics: RSF_FG_STAT_ACTIVITY,
                        callbacks: Some((count, count * 3)),
                        presented: 2,
                        active: true,
                    },
                );
            }
            done.store(true, Ordering::Release);
        });
        scope.spawn(|| {
            gate.wait();
            for _ in 0..SAMPLES {
                state.abort();
                state.paced();
            }
        });
        gate.wait();
        loop {
            let sample = state.read().status;
            let serial = sample.vendor_status as u64;
            assert_eq!(
                (
                    sample.total_presented,
                    sample.generated_callbacks,
                    sample.present_callbacks
                ),
                (serial * 2, serial, serial * 3)
            );
            assert_eq!((sample.active, sample.generated_presented), (0, 0));
            if done.load(Ordering::Acquire) {
                break;
            }
        }
    });
    let status = state.read().status;
    assert_eq!(status.total_presented, SAMPLES * 2);
    assert_eq!(status.latency_paced_frames, SAMPLES);
}
