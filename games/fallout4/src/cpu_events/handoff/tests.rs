// SPDX-License-Identifier: MIT
//! The worker successor handoff contract, ported from tests/fallout4_worker_handoff.cpp.
use super::*;
use rsf_game_sys::{
    RSF_GAME_CPU_ABORT_INTERVENING_POLL, RSF_GAME_CPU_ABORT_OVERLAP, RSF_GAME_CPU_ABORT_QUIESCE,
    RSF_GAME_CPU_ABORT_STATE_CHANGED,
};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread;

fn predecessor() -> WorkerCycle {
    WorkerCycle {
        session: 7,
        source: 40,
        idle: 20,
        main: 1,
        worker: 2,
        window: 3,
        manager: 4,
        queue: 5,
        receiver: 6,
        start_event: 7,
        complete_event: 8,
        frame: 100,
    }
}

fn successor() -> CpuIdentity {
    CpuIdentity {
        session: 7,
        source: 41,
        submission: 0,
        began_qpc: 0,
        frequency: 10_000_000,
    }
}

fn consumed_ticket() -> WorkerTicket {
    let mut ticket = WorkerTicket::ZERO;
    assert!(
        ticket.reserve(&predecessor(), &successor(), 9, true),
        "reserve after predecessor allocation/Present entry"
    );
    assert!(
        ticket.begin_callback(0, 0, 0),
        "registered no-native-poll-yet callback"
    );
    ticket.end_callback();
    assert!(
        ticket.arm(&predecessor(), 9, 12, true, true),
        "raw revision snapped after RESERVE return"
    );
    ticket.native_poll_entry();
    assert!(
        ticket.sample(9, 12, 77, 1000, true),
        "actual poll completed"
    );
    assert!(
        ticket.begin(1010, 9, 12, 77, true),
        "actual receiver begins future flags0 frame"
    );
    assert!(
        ticket.bind(9, 12, 77, true),
        "strict matching sample binds at receiver"
    );
    assert!(
        ticket.consumed(9, 77, true),
        "native receiver returns with matching consumed cursor"
    );
    ticket
}

fn joined_ticket() -> WorkerTicket {
    let mut ticket = consumed_ticket();
    assert!(ticket.complete(true), "selected worker callback returns");
    assert!(
        ticket.join(&predecessor(), true),
        "verified actual wait completed"
    );
    ticket
}

fn next_swap() -> WorkerCycle {
    let mut owner = predecessor();
    owner.source = 41;
    owner.idle += 1;
    owner
}

// Forces the observed ordering: the worker snapshots false, Main publishes the selected join,
// then the worker sees native mode0 and revalidates the lease.
#[test]
fn stale_join_snapshot_revalidates_only_the_published_lease() {
    let joining = Arc::new(Mutex::new(false));
    let (copied_tx, copied_rx) = channel();
    let (published_tx, published_rx) = channel::<()>();
    let reader = {
        let joining = Arc::clone(&joining);
        thread::spawn(move || {
            let snapshot = *joining.lock().unwrap();
            copied_tx.send(()).unwrap();
            published_rx.recv().unwrap();
            let joining = *joining.lock().unwrap();
            assert!(!snapshot, "worker retains stale pre-join snapshot");
            let current = predecessor();
            let admits = |expected: &WorkerCycle, source: u64, active: bool, invalid: bool| {
                active_join_matches(
                    expected,
                    &current,
                    &predecessor(),
                    source,
                    41,
                    active,
                    true,
                    true,
                    invalid,
                )
            };
            assert!(
                admits(&predecessor(), 41, joining, false),
                "exact published join survives stale snapshot"
            );
            let mut other = predecessor();
            other.frame += 1;
            assert!(
                !admits(&other, 41, joining, false),
                "different native frame cannot borrow join"
            );
            other = predecessor();
            other.source += 1;
            assert!(
                !admits(&other, 41, joining, false),
                "different source cannot borrow join"
            );
            assert!(
                !admits(&predecessor(), 42, joining, false),
                "different worker cannot borrow join"
            );
            assert!(
                !admits(&predecessor(), 41, false, false),
                "mode0 without active join stays refused"
            );
            assert!(
                !admits(&predecessor(), 41, joining, true),
                "invalid cycle stays refused"
            );
        })
    };
    copied_rx.recv().unwrap();
    *joining.lock().unwrap() = true;
    published_tx.send(()).unwrap();
    reader.join().unwrap();
}

// The same unmatched generation refuses a future ticket in both cases; only the current
// renderer's claim to sampled input changes the outcome.
#[test]
fn unmatched_poll_preserves_only_input_unbound_renderer() {
    for bound_source in [0u64, 40] {
        let mut future = joined_ticket();
        future.refuse(RSF_GAME_CPU_ABORT_INTERVENING_POLL);
        let invalid = invalidates_render_source(bound_source, true);
        assert_eq!(
            invalid,
            bound_source != 0,
            "pure unmatched future poll preserves only input-unbound renderer"
        );
        assert!(
            !future.claim(&next_swap(), 77, 9) && future.frame_begun && future.refusal != 0,
            "preserved renderer never revives the refused future or drops abort/end obligations"
        );
        let mut early = WorkerTicket::ZERO;
        assert!(
            !early.reserve(&predecessor(), &successor(), 10, false),
            "unbound preservation never admits an early future"
        );
        // These are explicitly not poll-only facts in the production callsite: quiesce, active
        // raw writer/overlap, and native owner/state mismatch.
        for reason in [
            RSF_GAME_CPU_ABORT_QUIESCE,
            RSF_GAME_CPU_ABORT_OVERLAP,
            RSF_GAME_CPU_ABORT_STATE_CHANGED,
        ] {
            let mut refused = consumed_ticket();
            refused.refuse(reason);
            assert!(
                invalidates_render_source(bound_source, false),
                "lifecycle/raw/native refusal invalidates bound and unbound renderer"
            );
            assert!(
                refused.refusal == reason && !refused.complete(true),
                "native refusal remains sticky on future ticket"
            );
        }
    }
}

#[test]
fn reservation_requires_constructed_allocated_predecessor() {
    let mut empty = WorkerTicket::ZERO;
    assert!(
        !empty.reserve(&predecessor(), &successor(), 9, false),
        "poll cannot reserve before ordinary world/source construction"
    );
    let mut bad_source = successor();
    bad_source.source = 40;
    assert!(
        !empty.reserve(&predecessor(), &bad_source, 9, true),
        "future id must follow allocated predecessor"
    );
    let mut unknown = predecessor();
    unknown.source = 0;
    assert!(
        !empty.reserve(&unknown, &successor(), 9, true),
        "unknown predecessor refuses"
    );
}

#[test]
fn sample_and_begin_require_the_actual_unchanged_poll() {
    let mut pending = WorkerTicket::ZERO;
    assert!(
        pending.reserve(&predecessor(), &successor(), 9, true),
        "pending fixture reserve"
    );
    assert!(
        pending.begin_callback(0, 0, 0),
        "unbound predecessor callback begins"
    );
    pending.end_callback();
    assert!(
        pending.arm(&predecessor(), 9, 12, true, true),
        "post-wait raw revision may differ from pre-reserve raw revision"
    );
    assert!(
        !pending.sample(9, 12, 77, 1000, true),
        "arming does not fabricate an actual native Poll"
    );
    pending.native_poll_entry();
    assert!(
        !pending.sample(10, 12, 77, 1000, true),
        "intervening poll before sample refuses"
    );
    assert!(
        !pending.sample(9, 13, 77, 1000, true),
        "overlapped raw sample refuses"
    );
    assert!(
        pending.sample(9, 12, 77, 1000, true),
        "unchanged actual sample succeeds"
    );
    assert!(
        !pending.begin(1010, 9, 13, 77, true),
        "raw change before BIND refuses"
    );
    assert!(
        !pending.begin(1010, 9, 12, 78, true),
        "queue advance without matching sampled cursor refuses"
    );
    assert!(
        !pending.begin(1010, 9, 12, 77, false),
        "native state/receiver mismatch refuses"
    );
}

#[test]
fn consumed_ticket_joins_claims_and_adopts_once() {
    let mut consumed = consumed_ticket();
    assert!(
        !consumed.complete(false),
        "changed worker/native state cannot complete"
    );
    // Consumed sources do not compare later raw revisions. Future adoption still requires their
    // exact consumed cursor and absence of an unmatched poll.
    assert!(
        consumed.complete(true),
        "later raw arrival does not rewrite genuinely consumed source"
    );
    assert!(
        !consumed.join(&predecessor(), false),
        "early/no-op join EAX0 is not actual completed wait"
    );
    let mut changed = predecessor();
    changed.complete_event += 1;
    assert!(
        !consumed.join(&changed, true),
        "changed native completion handle refuses join"
    );
    assert!(
        consumed.join(&predecessor(), true),
        "actual matching wait closes worker handoff"
    );
    assert!(
        !consumed.claim(&next_swap(), 78, 9),
        "new queue cursor cannot be relabeled as consumed ticket"
    );
    assert!(
        !consumed.claim(&next_swap(), 77, 10),
        "unmatched later poll invalidates successor"
    );
    changed = next_swap();
    changed.idle += 1;
    assert!(
        !consumed.claim(&changed, 77, 9),
        "no immediate successor refuses delayed ticket"
    );
    changed = next_swap();
    changed.frame += 1;
    assert!(
        !consumed.claim(&changed, 77, 9),
        "Swap must precede next native frame increment"
    );
    assert!(
        consumed.claim(&next_swap(), 77, 9),
        "next Swap claims already allocated id without second FRAME_BEGIN"
    );
    assert!(
        !consumed.adopt(&next_swap(), 77, 9),
        "World requires actual native frame increment"
    );
    changed = next_swap();
    changed.frame += 1;
    assert!(
        consumed.adopt(&changed, 77, 9),
        "immediate ordinary World native+1 adopts consumed source"
    );
    assert!(
        !consumed.adopt(&changed, 77, 9),
        "duplicate world cannot adopt same ticket twice"
    );

    let mut stopped = joined_ticket();
    stopped.refuse(1);
    stopped.refuse(2);
    assert!(
        stopped.refusal == 1 && stopped.active() && stopped.frame_begun,
        "quiesce retains sticky abort/frame-end obligations"
    );
    assert!(
        !stopped.claim(&next_swap(), 77, 9),
        "quiesced ticket cannot be replaced with fallback ahead of abort drain"
    );
}

// Intention protection belongs only to the one unchanged consumed sample.
#[test]
fn intention_protects_only_the_exact_bound_predecessor() {
    let p = predecessor();
    let mut intention = WorkerTicket::ZERO;
    assert!(
        intention.reserve(&p, &successor(), 9, true) && intention.begin_callback(8, 12, 76),
        "construct/source allocation precede a genuine callback intention"
    );
    assert!(
        intention.protects_bound(&p, 40, 8, 9, 12, 76, 76, true),
        "exact bound predecessor is protected"
    );
    assert!(
        !intention.protects_bound(&p, 39, 8, 9, 12, 76, 76, true),
        "wrong bound source cannot borrow intention"
    );
    assert!(
        !intention.protects_bound(&p, 40, 7, 9, 12, 76, 76, true),
        "wrong previous generation refuses"
    );
    assert!(
        !intention.protects_bound(&p, 40, 8, 10, 12, 76, 76, true),
        "intervening or nested Poll removes protection"
    );
    assert!(
        intention.protects_bound(&p, 40, 8, 9, 13, 76, 76, true),
        "new raw arrival preserves already consumed predecessor"
    );
    assert!(
        !intention.protects_bound(&p, 40, 8, 9, 12, 77, 77, true),
        "consumed cursor advance refuses"
    );
    assert!(
        intention.protects_bound(&p, 40, 8, 9, 12, 76, 77, true),
        "later queue tail preserves already consumed predecessor"
    );
    let mut changed = p;
    changed.main += 1;
    assert!(
        !intention.protects_bound(&changed, 40, 8, 9, 12, 76, 76, true),
        "changed native Main/owner refuses"
    );
    assert!(
        !intention.protects_bound(&p, 40, 8, 9, 12, 76, 76, false),
        "lifecycle/native/raw-writer proof failure refuses"
    );
    assert!(
        !intention.begin(1010, 9, 12, 76, true),
        "consumer cannot adopt callback intention before actual Poll"
    );
    intention.end_callback();
    assert!(
        !intention.arm(&p, 9, 12, true, WorkerTicket::poll_safe(40, false, false)),
        "core timeout before actual seal does not retain bound generation"
    );
    assert!(
        !intention.callback_inflight && !intention.protects_bound(&p, 40, 8, 9, 12, 76, 76, true),
        "timeout expires protection before native continuation"
    );
    intention.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED);
    intention.native_poll_entry();
    assert!(
        intention.native_poll_started && !intention.sample(9, 12, 77, 1000, true),
        "native call still proceeds while refused ticket cannot publish sample"
    );
    let mut gap = WorkerTicket::ZERO;
    assert!(
        gap.reserve(&p, &successor(), 10, true) && !gap.begin_callback(8, 12, 76),
        "a generation gap cannot create bound protection"
    );
    let mut withdrawn = WorkerTicket::ZERO;
    assert!(
        withdrawn.reserve(&p, &successor(), 9, true) && withdrawn.begin_callback(8, 12, 76),
        "withdrawal fixture intention"
    );
    withdrawn.refuse(RSF_GAME_CPU_ABORT_QUIESCE);
    assert!(
        !withdrawn.protects_bound(&p, 40, 8, 9, 12, 76, 76, true),
        "quiesce/reentrant withdrawal immediately removes protection"
    );
    withdrawn.end_callback();
    assert!(
        !withdrawn.arm(&p, 9, 12, true, true) && withdrawn.active(),
        "withdrawn intention retains abort ownership until drained"
    );
    assert!(
        WorkerTicket::poll_safe(0, false, false)
            && WorkerTicket::poll_safe(40, true, false)
            && WorkerTicket::poll_safe(40, false, true),
        "unbound, completed, or genuinely sealed predecessors permit normal native continuation"
    );
}

// Exercises the scalar helper across the producer/graphics handoff. The simulated blocking host
// callback must release the adapter's scalar lock; Present and the original native join can
// progress while it is blocked.
#[test]
fn blocking_reserve_callback_holds_no_scalar_lock() {
    let guard = Arc::new(Mutex::new(WorkerTicket::ZERO));
    let (entered_tx, entered_rx) = channel();
    let (returned_tx, returned_rx) = channel::<()>();
    let worker = {
        let guard = Arc::clone(&guard);
        thread::spawn(move || {
            {
                let mut cross = guard.lock().unwrap();
                assert!(
                    cross.reserve(&predecessor(), &successor(), 9, true),
                    "constructed predecessor admits intention before Present"
                );
                assert!(
                    cross.begin_callback(8, 12, 76),
                    "register exact bound-source callback intention"
                );
            }
            entered_tx.send(()).unwrap();
            returned_rx.recv().unwrap();
            let mut cross = guard.lock().unwrap();
            cross.end_callback();
            assert!(
                !cross.protects_bound(&predecessor(), 40, 8, 9, 12, 76, 76, true),
                "callback return always expires bound protection"
            );
            assert!(
                cross.arm(
                    &predecessor(),
                    9,
                    20,
                    true,
                    WorkerTicket::poll_safe(40, true, true)
                ),
                "core returns after Main Present progress"
            );
            cross.native_poll_entry();
            assert!(
                cross.sample(9, 20, 77, 1000, true),
                "worker actual poll after core wait"
            );
            assert!(
                cross.begin(1010, 9, 20, 77, true) && cross.bind(9, 20, 77, true),
                "actual receiver FRAME_BEGIN/BIND"
            );
            assert!(
                cross.consumed(9, 77, true) && cross.complete(true),
                "native worker consumed source completion"
            );
        })
    };
    entered_rx.recv().unwrap();
    assert!(
        guard.try_lock().is_ok(),
        "no scalar lock held across blocking host callback"
    );
    {
        let cross = guard.lock().unwrap();
        assert!(
            cross.protects_bound(&predecessor(), 40, 8, 9, 12, 76, 76, true),
            "Main bound source remains valid during exact pre-poll intention and can reach Present"
        );
        assert!(
            !WorkerTicket::poll_safe(40, false, false),
            "a live unsealed bound predecessor forbids actual Poll"
        );
    }
    returned_tx.send(()).unwrap();
    worker.join().unwrap();
    let mut cross = guard.lock().unwrap();
    assert!(
        cross.join(&predecessor(), true) && cross.claim(&next_swap(), 77, 9),
        "post-Swap join transfers ticket to next graphics Swap"
    );
    let mut changed = next_swap();
    changed.frame += 1;
    assert!(
        cross.adopt(&changed, 77, 9),
        "cross-thread handoff preserves actual native+1 source association"
    );
}
