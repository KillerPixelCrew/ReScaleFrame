// SPDX-License-Identifier: MIT
// Ported from tests/dlss_generation_state.cpp, which exercised the former C++ copy.
use super::*;
use crate::render_abi::RSF_FG_ABI_VERSION;
use std::ptr;
use std::sync::atomic::{AtomicBool as Flag, Ordering};
use std::sync::{Barrier, mpsc};
use std::thread;

fn fixed_options() -> Options {
    let mut value: Options = unsafe { zeroed() };
    value.struct_size = size_of::<Options>() as u32;
    value.abi_version = RSF_FG_ABI_VERSION;
    value.mode = RSF_FG_FIXED;
    value.generated_frames = 2;
    value.reflex_mode = REFLEX_ON;
    value
}

fn initial() -> Status {
    let mut value: Status = unsafe { zeroed() };
    value.struct_size = size_of::<Status>() as u32;
    value.supported = 1;
    value.total_presented = 100;
    value.latency_paced_frames = 7;
    value
}

fn retirement(fence: usize, value: u64) -> Retirement {
    Retirement {
        struct_size: size_of::<Retirement>() as u32,
        fence: fence as Handle,
        value,
    }
}

#[test]
fn abort_wins_over_late_publication() {
    let fences = [0_u64; 2];
    let fence = |index: usize| ptr::from_ref(&fences[index]) as usize;
    let state = Copied::new();
    let initial = initial();
    state.initialize(initial);
    let mut options = fixed_options();
    state.configured(&options, REFLEX_ON, 2, true);
    let source = state.read();
    assert!(!source.history_valid && !source.prepared_enabled);
    assert!(state.stage_prepare(source.revision, 10, RSF_FG_FIXED, 2));
    assert!(state.complete_prepare(source.revision, 10, true));
    let mut observed = initial;
    observed.valid_statistics = RSF_FG_STAT_TOTAL_PRESENTED | RSF_FG_STAT_ACTIVITY;
    observed.max_generated_frames = 4;
    observed.low_latency_available = 1;
    let published = state.polled(
        source.revision,
        &observed,
        3,
        true,
        retirement(fence(0), 20),
    );
    assert!(published.active == 1 && published.total_presented == 103);

    // The presenting owner sits in its SDK gap while another thread aborts.
    let (entered, entered_wait) = mpsc::channel();
    let (resume, resume_wait) = mpsc::channel::<()>();
    let shared = &state;
    let poll_before_abort = thread::scope(|scope| {
        let presenting = scope.spawn(move || {
            let before = shared.read();
            let staged = shared.stage_prepare(before.revision, 11, RSF_FG_FIXED, 2);
            entered.send(()).unwrap();
            resume_wait.recv().unwrap();
            (staged, shared.complete_prepare(before.revision, 11, true))
        });
        entered_wait.recv().unwrap();
        let poll_before_abort = state.read();
        state.abort();
        let aborted = state.read();
        assert!(!aborted.history_valid && !aborted.prepared_enabled && aborted.status.active == 0);
        resume.send(()).unwrap();
        let (staged, late_success) = presenting.join().unwrap();
        assert!(staged && !late_success);
        poll_before_abort
    });
    let after = state.read();
    assert!(after.last_prepare == 11 && !after.history_valid);
    let published = state.polled(
        poll_before_abort.revision,
        &observed,
        4,
        true,
        retirement(fence(1), 21),
    );
    let stale_poll = state.read();
    assert!(published.active == 0 && !stale_poll.prepared_enabled);
    assert_eq!(published.total_presented, 107);
    assert!(stale_poll.retirement.fence as usize == fence(1) && stale_poll.retirement.value == 21);
    assert!(!state.stage_prepare(source.revision, 12, RSF_FG_FIXED, 2));
    let source = state.read();
    assert!(!source.history_valid);
    assert!(state.stage_prepare(source.revision, 12, RSF_FG_FIXED, 2));
    assert!(state.complete_prepare(source.revision, 12, true));

    // Latency-only configuration keeps valid history.
    let valid = state.read();
    options.reflex_mode = RSF_REFLEX_BOOST;
    options.frame_limit_us = 1000;
    state.configured(&options, RSF_REFLEX_BOOST, 0, false);
    let latency_only = state.read();
    assert!(latency_only.revision == valid.revision && latency_only.history_valid);
    assert!(latency_only.prepared_enabled && latency_only.status.effective_generated_frames == 2);
    assert!(state.stage_prepare(latency_only.revision, 13, RSF_FG_FIXED, 2));
    options.mode = RSF_FG_OFF;
    options.reflex_mode = RSF_REFLEX_OFF;
    state.configured(&options, RSF_REFLEX_OFF, 0, true);
    assert!(!state.complete_prepare(latency_only.revision, 13, true));
    let published = state.polled(
        latency_only.revision,
        &observed,
        2,
        true,
        retirement(fence(1), 21),
    );
    assert!(published.active == 0 && published.effective_mode == RSF_FG_OFF);
    assert!(published.configured_mode == RSF_FG_OFF && published.effective_generated_frames == 0);
    assert_eq!(published.total_presented, 109);
    state.paced();
    assert_eq!(state.read().status.latency_paced_frames, 7);
    state.abort();
    options.reflex_mode = REFLEX_ON;
    state.configured(&options, REFLEX_ON, 0, false);
    assert!(!state.read().history_valid && !state.read().prepared_enabled);
}

#[test]
fn concurrent_copies_stay_coherent() {
    const ITERATIONS: u32 = 4096;
    let fences = [0_u64; 2];
    let fences = [
        ptr::from_ref(&fences[0]) as usize,
        ptr::from_ref(&fences[1]) as usize,
    ];
    let initial = initial();
    let state = Copied::new();
    state.initialize(initial);
    state.configured(&fixed_options(), REFLEX_ON, 2, true);
    let start = Barrier::new(3);
    let finished = Flag::new(false);
    let coherent = Flag::new(true);
    thread::scope(|scope| {
        scope.spawn(|| {
            start.wait();
            for i in 1..=ITERATIONS {
                let before = state.read();
                let mut current = initial;
                current.min_dimension = i;
                current.max_generated_frames = i * 2;
                current.vendor_status = i as i32;
                current.valid_statistics = RSF_FG_STAT_TOTAL_PRESENTED;
                let completion = retirement(fences[(i % 2) as usize], u64::from(i));
                state.polled(before.revision, &current, 1, true, completion);
            }
            finished.store(true, Ordering::Release);
        });
        scope.spawn(|| {
            start.wait();
            for _ in 0..ITERATIONS {
                state.abort();
                state.paced();
            }
        });
        scope.spawn(|| {
            start.wait();
            while !finished.load(Ordering::Acquire) {
                let copy = state.read();
                let sequence = copy.status.min_dimension;
                let torn = sequence != 0
                    && (copy.status.max_generated_frames != sequence * 2
                        || copy.status.vendor_status != sequence as i32
                        || copy.retirement.value != u64::from(sequence)
                        || copy.retirement.fence as usize != fences[(sequence % 2) as usize]);
                if copy.status.struct_size != size_of::<Status>() as u32
                    || copy.status.active != 0
                    || torn
                {
                    coherent.store(false, Ordering::Relaxed);
                }
            }
        });
    });
    let last = state.read();
    assert!(coherent.load(Ordering::Relaxed));
    assert_eq!(last.status.total_presented, 100 + u64::from(ITERATIONS));
    assert_eq!(last.status.latency_paced_frames, 7 + u64::from(ITERATIONS));
    assert!(!last.history_valid && !last.prepared_enabled && last.status.active == 0);
}
