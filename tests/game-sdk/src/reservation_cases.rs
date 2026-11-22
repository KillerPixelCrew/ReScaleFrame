// SPDX-License-Identifier: MIT
use super::*;

pub(super) fn run() {
    // No session yet: a reservation cannot establish one.
    assert!(!reserve(2, 5, 1));
    // Source S=1 renders while the late native poll reserves future source P=2.
    assert!(send(&fact(1, RSF_GAME_CPU_FRAME_BEGIN, 10, 0)));
    assert!(send(&fact(1, RSF_GAME_CPU_SIMULATION_END, 20, 0)));
    assert!(reserve(2, 30, 1));
    assert!(read(2).state == RSF_NATIVE_CPU_RESERVATION_OPEN && read(2).reserve_qpc == 30);
    assert!(read(2).predecessor_present == RSF_NATIVE_CPU_DEPENDENCY_PENDING as i32);
    assert!(!bind(2, 31)); // poll still in flight and no consuming frame
    let mut early = source_fact(2, RSF_GAME_CPU_SOURCE_SAMPLED, 32, 0);
    early.header.flags = RSF_GAME_CPU_SOURCE_PRODUCED;
    early.poll_begin_qpc = 29;
    assert!(!send(&early)); // the poll cannot begin before the reservation opened
    early.poll_begin_qpc = 33;
    assert!(!send(&early)); // nor after it returned
    assert!(sampled(2, 32, true));
    // Reservation opened at 30, before anything the runtime might do inside RESERVE; the
    // actual sample is the poll entry at 31, after RESERVE returned; 32 is the poll's return.
    assert!(read(2).poll_begin_qpc == 31 && read(2).sampled_qpc == 32);
    // S's actual Present returns before its CPU FRAME_END; the outcome does not need it.
    assert!(presented(1));
    assert!(read(2).predecessor_present == RSF_NATIVE_CPU_DEPENDENCY_PRESENTED as i32);
    assert!(!presented(1)); // the first actual outcome stands
    assert!(send(&fact(1, RSF_GAME_CPU_FRAME_END, 40, 0)));
    // The next tick adopts the reserved identity. Its actual input predates FRAME_BEGIN.
    delivery_clear();
    assert!(send(&fact(2, RSF_GAME_CPU_FRAME_BEGIN, 50, 0)));
    assert!(read(2).state == RSF_NATIVE_CPU_RESERVATION_ADOPTED && read(2).frame_begin_qpc == 50);
    assert!(read(2).reserve_qpc < read(2).frame_begin_qpc);
    assert!(!bind(2, 49)); // binding cannot precede the consuming update
    assert!(bind(2, 55));
    assert!(
        read(2).state == RSF_NATIVE_CPU_RESERVATION_BOUND
            && read(2).bind_qpc == 55
            && read(2).input_kind == RSF_GAME_INPUT_MOUSE
            && read(2).native_generation == 20
    );
    assert!(!bind(2, 56)); // one binding per reservation
    let mut frame: rsf_native_cpu_frame = unsafe { std::mem::zeroed() };
    // A binding is no INPUT_SAMPLE, SIMULATION_BEGIN, marker or pacing time of the frame.
    assert!(
        read_frame(2, &mut frame)
            && frame.stage_mask == 1
            && frame.marker_mask == 0
            && frame.pacing_qpc == 0
    );
    assert!(!source_stage_delivered()); // facts stay in the core; no backend sink sees them
    // The next late poll overlaps P's own render: the steady state, not an overlap.
    assert!(reserve(3, 60, 2) && sampled(3, 61, true));
    assert!(send(&fact(2, RSF_GAME_CPU_SIMULATION_END, 62, 0)));
    assert!(send(&fact(2, RSF_GAME_CPU_FRAME_END, 70, 0)));
    assert!(read(2).state == RSF_NATIVE_CPU_RESERVATION_CLOSED);
    // P's CPU closure says nothing about its Present.
    assert!(read(3).predecessor_present == RSF_NATIVE_CPU_DEPENDENCY_PENDING as i32);
    assert!(send(&fact(3, RSF_GAME_CPU_FRAME_BEGIN, 80, 0)));
    assert!(reserve(4, 85, 3) && sampled(4, 86, true));
    // A completion in another session never resolves this session's reused identity.
    assert!(!present_result(
        SESSION + 1,
        3,
        RSF_NATIVE_CPU_PRESENT_RETURNED,
        0
    ));
    assert!(!present_result(SESSION, 3, 99, 0));
    assert!(read(4).predecessor_present == RSF_NATIVE_CPU_DEPENDENCY_PENDING as i32);
    // Source 3 never bound: the paused tick closes its reservation unbound, and the engine's
    // skipped render is its Present outcome.
    assert!(send(&fact(
        3,
        RSF_GAME_CPU_FRAME_END,
        90,
        RSF_GAME_CPU_RENDER_SKIPPED
    )));
    assert!(read(3).state == RSF_NATIVE_CPU_RESERVATION_UNBOUND);
    assert!(read(4).predecessor_present == RSF_NATIVE_CPU_DEPENDENCY_CANCELLED as i32);

    // Intervening poll: a newer reservation withdraws the sampled, unadopted one.
    assert!(reserve(5, 95, 3) && sampled(5, 96, true));
    assert!(
        read(4).state == RSF_NATIVE_CPU_RESERVATION_ABORTED
            && read(4).abort_reason == RSF_GAME_CPU_ABORT_INTERVENING_POLL
    );
    assert!(reserve(6, 98, 3));
    assert!(read(5).state == RSF_NATIVE_CPU_RESERVATION_ABORTED);
    // Concurrent polls: a reservation while another poll is in flight refuses both.
    assert!(!reserve(7, 99, 3));
    assert!(
        read(6).state == RSF_NATIVE_CPU_RESERVATION_ABORTED
            && read(6).abort_reason == RSF_GAME_CPU_ABORT_OVERLAP
    );
    assert!(read(7).state == 0 && !sampled(6, 100, true) && !bind(6, 101));
    // Reuse: an identity at or below one already reserved or begun is refused.
    assert!(!reserve(6, 102, 3) && !reserve(3, 102, 2) && !reserve(7, 102, 3));
    // A frame cannot adopt a reservation whose poll is still in flight. The engine frame is kept,
    // without input ownership.
    assert!(reserve(8, 103, 3));
    assert!(send(&fact(8, RSF_GAME_CPU_FRAME_BEGIN, 104, 0)));
    assert!(
        read(8).state == RSF_NATIVE_CPU_RESERVATION_ABORTED
            && read(8).abort_reason == RSF_GAME_CPU_ABORT_OVERLAP
    );
    assert!(!bind(8, 105) && !sampled(8, 105, true));
    assert!(send(&fact(8, RSF_GAME_CPU_FRAME_END, 106, 0)));
    // A frame with another identity supersedes an outstanding sampled reservation.
    assert!(reserve(9, 107, 3) && sampled(9, 108, true));
    assert!(send(&fact(10, RSF_GAME_CPU_FRAME_BEGIN, 109, 0)));
    assert!(
        read(9).state == RSF_NATIVE_CPU_RESERVATION_ABORTED
            && read(9).abort_reason == RSF_GAME_CPU_ABORT_SUPERSEDED
    );
    // A poll without exclusively completed control state can be adopted but never bound.
    assert!(reserve(11, 110, 10) && sampled(11, 111, false));
    assert!(present_result(
        SESSION,
        10,
        RSF_NATIVE_CPU_PRESENT_RESULT_UNKNOWN,
        0
    ));
    assert!(read(11).predecessor_present == RSF_NATIVE_CPU_DEPENDENCY_UNVERIFIED as i32);
    assert!(send(&fact(10, RSF_GAME_CPU_FRAME_END, 112, 0)));
    assert!(send(&fact(11, RSF_GAME_CPU_FRAME_BEGIN, 113, 0)) && !bind(11, 114));
    assert!(send(&fact(11, RSF_GAME_CPU_FRAME_END, 115, 0)));
    assert!(read(11).state == RSF_NATIVE_CPU_RESERVATION_UNBOUND);
    // A failed actual Present is not a successful predecessor; an older unreported one is superseded.
    assert!(reserve(12, 120, 11) && sampled(12, 121, true));
    assert!(present_result(
        SESSION,
        11,
        RSF_NATIVE_CPU_PRESENT_RETURNED,
        0x8000_4005_u32 as i32
    ));
    assert!(read(12).predecessor_present == RSF_NATIVE_CPU_DEPENDENCY_FAILED as i32);
    // Explicit abort.
    assert!(abort_source(12, 122, RSF_GAME_CPU_ABORT_STATE_CHANGED));
    assert!(
        read(12).state == RSF_NATIVE_CPU_RESERVATION_ABORTED
            && read(12).abort_reason == RSF_GAME_CPU_ABORT_STATE_CHANGED
    );
    assert!(!bind(12, 123) && !abort_source(12, 124, 0));
    // A withdrawn identity still begins as an ordinary frame, but never gains input ownership.
    assert!(send(&fact(12, RSF_GAME_CPU_FRAME_BEGIN, 124, 0)));
    assert!(read(12).state == RSF_NATIVE_CPU_RESERVATION_ABORTED && !bind(12, 125));
    assert!(send(&fact(12, RSF_GAME_CPU_FRAME_END, 125, 0)));
    // Source 2 never reported a Present; a later successful one supersedes it.
    assert!(
        presented(12) && read(3).predecessor_present == RSF_NATIVE_CPU_DEPENDENCY_SUPERSEDED as i32
    );
    // Stale session, frequency and malformed packets are refused.
    let mut stale = source_fact(13, RSF_GAME_CPU_SOURCE_RESERVE, 125, 11);
    stale.header.session_id = SESSION + 1;
    assert!(!send(&stale));
    let mut frequency = source_fact(13, RSF_GAME_CPU_SOURCE_RESERVE, 125, 11);
    frequency.header.qpc_frequency = 999;
    assert!(!send(&frequency));
    let future_predecessor = source_fact(14, RSF_GAME_CPU_SOURCE_RESERVE, 125, 13);
    assert!(!send(&future_predecessor)); // a predecessor must be a begun source
    let mut version_two_base = fact(13, RSF_GAME_CPU_FRAME_BEGIN, 126, 0);
    version_two_base.event_version = RSF_GAME_CPU_EVENT_VERSION_SOURCE;
    assert!(!send(&version_two_base));
    let mut version_one_source = source_fact(13, RSF_GAME_CPU_SOURCE_RESERVE, 126, 11);
    version_one_source.header.event_version = RSF_GAME_CPU_EVENT_VERSION;
    assert!(!send(&version_one_source));
    let mut unknown_flags = source_fact(13, RSF_GAME_CPU_SOURCE_RESERVE, 126, 11);
    unknown_flags.header.flags = 0x80000000;
    assert!(!send(&unknown_flags));
    let mut kinds_on_reserve = source_fact(13, RSF_GAME_CPU_SOURCE_RESERVE, 126, 11);
    kinds_on_reserve.header.input_kind = 1;
    assert!(!send(&kinds_on_reserve));
    let mut sample_on_reserve = source_fact(13, RSF_GAME_CPU_SOURCE_RESERVE, 126, 11);
    sample_on_reserve.poll_begin_qpc = 126;
    assert!(!send(&sample_on_reserve)); // a sample time belongs to SAMPLED only
    sample_on_reserve.header.struct_size = RSF_GAME_CPU_SOURCE_EVENT_SIZE_1 + 1;
    assert!(!send(&sample_on_reserve)); // a partial timestamp must not become a fabricated QPC
    // A first-version packet, ending before poll_begin_qpc, is read only to its size: the
    // sample time stays unknown.
    assert!(reserve(13, 127, 12));
    let mut first_version = source_fact(13, RSF_GAME_CPU_SOURCE_SAMPLED, 128, 0);
    first_version.header.flags = RSF_GAME_CPU_SOURCE_PRODUCED;
    first_version.poll_begin_qpc = 127;
    first_version.header.struct_size = RSF_GAME_CPU_SOURCE_EVENT_SIZE_1;
    assert!(
        send(&first_version)
            && read(13).state == RSF_NATIVE_CPU_RESERVATION_SAMPLED
            && read(13).poll_begin_qpc == 0
    );
    assert!(abort_source(13, 129, RSF_GAME_CPU_ABORT_UNSPECIFIED));
    assert!(!source_stage_delivered());
    let totals = counters();
    assert!(
        totals.reserved == 10
            && totals.sampled == 8
            && totals.adopted == 3
            && totals.bound == 1
            && totals.closed == 1
    );
    assert!(totals.unbound == 2 && totals.aborted == 7 && totals.rejected >= 18);

    // Quiesce: session release drops every reservation and Present outcome.
    assert!(send(&fact(14, RSF_GAME_CPU_FRAME_BEGIN, 130, 0)));
    assert!(reserve(15, 131, 14) && sampled(15, 132, true));
    assert!(unsafe { rsf_native_cpu_release_session(SESSION) != 0 });
    assert!(read(15).state == 0 && counters().reserved == 0 && !presented(14));
    // A new session starts clean: no outcome of the old one resolves its identities.
    assert!(send(&fact(20, RSF_GAME_CPU_FRAME_BEGIN, 200, 0)));
    assert!(reserve(21, 201, 20) && sampled(21, 202, true));
    assert!(read(21).predecessor_present == RSF_NATIVE_CPU_DEPENDENCY_PENDING as i32);
    assert!(
        abort_source(21, 203, RSF_GAME_CPU_ABORT_QUIESCE)
            && read(21).state == RSF_NATIVE_CPU_RESERVATION_ABORTED
    );
    // Existing version 1 clients keep their behavior beside reservations.
    assert!(
        send(&fact(20, RSF_GAME_CPU_SIMULATION_END, 210, 0))
            && send(&fact(20, RSF_GAME_CPU_FRAME_END, 220, 0))
    );
    unsafe { rsf_native_cpu_rendered_source_set(0) };
    unsafe { rsf_native_cpu_release() };
    // Input-timeline hosts deliver RESERVE synchronously, before the poll. A later
    // valid binding promotes the real poll entry without moving it to FRAME_BEGIN.
    delivery_clear();
    assert!(send(&fact(1, RSF_GAME_CPU_FRAME_BEGIN, 10, 0)));
    assert!(send(&fact(1, RSF_GAME_CPU_INPUT_SAMPLE, 11, 0)));
    assert!(send(&fact(1, RSF_GAME_CPU_SIMULATION_BEGIN, 12, 0)));
    assert!(send(&fact(1, RSF_GAME_CPU_SIMULATION_END, 20, 0)));
    assert!(reserve(2, 30, 1));
    assert!(delivered_last().stage == RSF_GAME_CPU_SOURCE_RESERVE);
    let after_reserve = delivered_len();
    assert!(!reserve(2, 31, 1) && delivered_len() == after_reserve);
    assert!(sampled(2, 35, true));
    assert!(presented(1)); // no FRAME_END dependency for pre-input pacing
    assert!(send(&fact(1, RSF_GAME_CPU_FRAME_END, 40, 0)));
    let mut pending = fact(0, RSF_GAME_CPU_INPUT_EVENT, 45, RSF_GAME_CPU_INPUT_UNFRAMED);
    pending.input_kind = RSF_GAME_INPUT_MOUSE;
    assert!(send(&pending));
    delivery_clear();
    assert!(send(&fact(2, RSF_GAME_CPU_FRAME_BEGIN, 50, 0)));
    assert!(read_frame(2, &mut frame) && frame.stage_mask == 1 && frame.timestamps_qpc[1] == 0);
    assert!(bind(2, 55));
    assert!(
        read_frame(2, &mut frame)
            && frame.stage_mask == 7
            && frame.timestamps_qpc[1] == 34
            && frame.timestamps_qpc[2] == 55
            && (frame.flags & RSF_GAME_CPU_DERIVED_BOUNDARY) != 0
    );
    assert!(
        frame.pacing_qpc == 0
            && frame.input_ready_qpc == 0
            && frame.marker_mask == 0
            && frame.input_mask == 0
            && frame.pings == 0
    );
    assert!(frame.timestamps_qpc[1] < frame.timestamps_qpc[0]);
    assert!(
        delivered_len() == 3
            && delivered_at(0).stage == RSF_GAME_CPU_FRAME_BEGIN
            && delivered_at(1).stage == RSF_GAME_CPU_SOURCE_BIND
            && delivered_at(2).stage == RSF_GAME_CPU_SIMULATION_BEGIN
            && (delivered_at(2).flags & RSF_GAME_CPU_DERIVED_BOUNDARY) != 0
            && (delivered_at(2).flags & RSF_GAME_CPU_INPUT_SUBSTITUTE) == 0
    ); // no second INPUT_SAMPLE or pace
    let after_bind = delivered_len();
    assert!(!bind(2, 56) && delivered_len() == after_bind);
    assert!(send(&fact(2, RSF_GAME_CPU_SIMULATION_BEGIN, 56, 0)));
    assert!(send(&fact(2, RSF_GAME_CPU_SIMULATION_END, 60, 0)));
    assert!(send(&fact(2, RSF_GAME_CPU_FRAME_END, 70, 0)));
    assert!(
        read_frame(2, &mut frame)
            && frame.stage_mask == 31
            && frame.failed == 0
            && (frame.flags & RSF_GAME_CPU_INPUT_SUBSTITUTE) == 0
            && frame.timestamps_qpc[1] == 34
            && frame.timestamps_qpc[2] == 56
    );
    assert!(read(2).state == RSF_NATIVE_CPU_RESERVATION_CLOSED);
    let mut measured: rsf_native_cpu_counters = unsafe { std::mem::zeroed() };
    assert!(
        read_counters(&mut measured)
            && measured.substitutes == 0
            && measured.order_violations == 0
            && measured.input_events[1] == 0
            && measured.pings == 0
    );

    // Old packets without actual poll entry remain unknown, even after native bind.
    assert!(reserve(3, 71, 2));
    let mut unknown_sample = source_fact(3, RSF_GAME_CPU_SOURCE_SAMPLED, 74, 0);
    unknown_sample.header.struct_size = RSF_GAME_CPU_SOURCE_EVENT_SIZE_1;
    unknown_sample.header.flags = RSF_GAME_CPU_SOURCE_PRODUCED;
    assert!(send(&unknown_sample));
    assert!(send(&fact(3, RSF_GAME_CPU_FRAME_BEGIN, 80, 0)) && bind(3, 81));
    assert!(read_frame(3, &mut frame) && frame.stage_mask == 1 && frame.timestamps_qpc[1] == 0);
    assert!(send(&fact(3, RSF_GAME_CPU_SIMULATION_BEGIN, 82, 0)));
    assert!(send(&fact(3, RSF_GAME_CPU_SIMULATION_END, 90, 0)));
    assert!(read_frame(3, &mut frame) && (frame.flags & RSF_GAME_CPU_INPUT_SUBSTITUTE) != 0);
    assert!(send(&fact(3, RSF_GAME_CPU_FRAME_END, 100, 0)));

    // Aborted consumption cannot promote its otherwise real poll timestamp.
    assert!(reserve(4, 101, 3) && sampled(4, 103, true));
    assert!(send(&fact(4, RSF_GAME_CPU_FRAME_BEGIN, 110, 0)));
    assert!(abort_source(4, 111, RSF_GAME_CPU_ABORT_NOT_CONSUMED) && !bind(4, 112));
    assert!(read_frame(4, &mut frame) && frame.stage_mask == 1 && frame.timestamps_qpc[1] == 0);
    let before_refused_simulation = delivered_len();
    assert!(send(&fact(4, RSF_GAME_CPU_SIMULATION_BEGIN, 113, 0)));
    assert!(send(&fact(4, RSF_GAME_CPU_SIMULATION_END, 114, 0)));
    assert!(delivered_len() == before_refused_simulation); // facts retained, no latency callback
    assert!(
        read_frame(4, &mut frame)
            && frame.stage_mask == 13
            && frame.timestamps_qpc[1] == 0
            && frame.timestamps_qpc[2] == 113
            && frame.timestamps_qpc[3] == 114
            && frame.pacing_qpc == 0
            && frame.marker_mask == 0
    );
    assert!(send(&fact(4, RSF_GAME_CPU_FRAME_END, 115, 0)));
    assert!(
        read_frame(4, &mut frame)
            && frame.ended != 0
            && frame.failed == 0
            && frame.stage_mask == 29
    );
    assert!(marker(4, 0) == -1);
    assert!(read_counters(&mut measured) && measured.order_violations == 0);
    // A refused bind can also leave an adopted reservation, without an ABORT.
    assert!(reserve(5, 120, 4) && sampled(5, 122, false));
    assert!(send(&fact(5, RSF_GAME_CPU_FRAME_BEGIN, 130, 0)) && !bind(5, 131));
    let before_unbound_simulation = delivered_len();
    assert!(send(&fact(5, RSF_GAME_CPU_SIMULATION_BEGIN, 132, 0)));
    assert!(send(&fact(5, RSF_GAME_CPU_SIMULATION_END, 140, 0)));
    assert!(delivered_len() == before_unbound_simulation);
    assert!(send(&fact(5, RSF_GAME_CPU_FRAME_END, 150, 0)));
    assert!(read(5).state == RSF_NATIVE_CPU_RESERVATION_UNBOUND);
    assert!(marker(5, 0) == -1);
    assert!(
        read_frame(5, &mut frame)
            && frame.stage_mask == 29
            && frame.failed == 0
            && frame.timestamps_qpc[1] == 0
            && frame.pacing_qpc == 0
            && frame.marker_mask == 0
    );
    // A flags0 frame is canonical only after real input ownership is established.
    assert!(reserve(6, 160, 5) && sampled(6, 162, true));
    let mut unkeyed = fact(6, RSF_GAME_CPU_FRAME_BEGIN, 170, 0);
    unkeyed.flags = 0;
    assert!(send(&unkeyed) && bind(6, 171));
    assert!(
        read_frame(6, &mut frame)
            && frame.stage_mask == 7
            && (frame.flags & RSF_GAME_CPU_INPUT_KEYED) != 0
            && frame.timestamps_qpc[1] == 161
    );
    let derived_end = fact(
        6,
        RSF_GAME_CPU_SIMULATION_END,
        180,
        RSF_GAME_CPU_DERIVED_BOUNDARY,
    );
    assert!(send(&derived_end));
    let derived_delivered = delivered_len();
    assert!(send(&derived_end) && delivered_len() == derived_delivered);
    let mut actual_begin = fact(6, RSF_GAME_CPU_SIMULATION_BEGIN, 172, 0);
    actual_begin.flags = 0;
    let mut bad_clock = actual_begin;
    bad_clock.qpc_frequency = 999;
    assert!(!send(&bad_clock));
    let mut backwards = actual_begin;
    backwards.timestamp_qpc = 169;
    assert!(!send(&backwards));
    let mut crosses_end = actual_begin;
    crosses_end.timestamp_qpc = 181;
    assert!(!send(&crosses_end));
    assert!(marker(6, 0) == -1); // derived bookkeeping cannot authorize a marker
    assert!(send(&actual_begin) && delivered_len() == derived_delivered + 1);
    assert!((delivered_last().flags & RSF_GAME_CPU_DERIVED_BOUNDARY) == 0);
    assert!(marker(6, 0) == 1);
    assert!(marker(6, 1) == -1);
    assert!(
        read_frame(6, &mut frame)
            && frame.timestamps_qpc[2] == 172
            && (frame.flags & RSF_GAME_CPU_DERIVED_BOUNDARY) != 0
    ); // end remains derived
    let mut actual_end = fact(6, RSF_GAME_CPU_SIMULATION_END, 181, 0);
    actual_end.flags = 0;
    assert!(send(&actual_end) && delivered_len() == derived_delivered + 2);
    assert!((delivered_last().flags & RSF_GAME_CPU_DERIVED_BOUNDARY) == 0);
    assert!(marker(6, 1) == 1);
    assert!(
        read_frame(6, &mut frame)
            && frame.timestamps_qpc[3] == 181
            && (frame.flags & RSF_GAME_CPU_DERIVED_BOUNDARY) == 0
    );
    assert!(send(&fact(6, RSF_GAME_CPU_FRAME_END, 182, 0)));
    // Withdrawal from the synchronous bind sink must prevent derived simulation.
    assert!(reserve(7, 190, 6) && sampled(7, 192, true));
    assert!(send(&fact(7, RSF_GAME_CPU_FRAME_BEGIN, 200, 0)));
    inject(Injection::Withdraw);
    assert!(bind(7, 201));
    assert!(
        read_frame(7, &mut frame)
            && frame.stage_mask == 3
            && (frame.flags & RSF_GAME_CPU_DERIVED_BOUNDARY) == 0
    );
    assert!(send(&fact(
        7,
        RSF_GAME_CPU_FRAME_END,
        203,
        RSF_GAME_CPU_RENDER_SKIPPED
    )));
    // Missing end remains missing for an explicitly skipped source; no completion promotion.
    assert!(reserve(8, 210, 7) && sampled(8, 212, true));
    assert!(send(&fact(8, RSF_GAME_CPU_FRAME_BEGIN, 220, 0)) && bind(8, 221));
    assert!(abort_source(8, 222, RSF_GAME_CPU_ABORT_STATE_CHANGED));
    assert!(send(&fact(
        8,
        RSF_GAME_CPU_FRAME_END,
        223,
        RSF_GAME_CPU_RENDER_SKIPPED
    )));
    assert!(
        read_frame(8, &mut frame)
            && frame.stage_mask == 23
            && frame.ended != 0
            && frame.failed == 0
            && frame.timestamps_qpc[3] == 0
    );
    assert!(reserve(9, 230, 8) && sampled(9, 232, true));
    assert!(send(&fact(9, RSF_GAME_CPU_FRAME_BEGIN, 240, 0)) && bind(9, 241));
    assert!(!send(&fact(9, RSF_GAME_CPU_FRAME_END, 243, 0))); // unflagged partial still refuses
    assert!(reserve(10, 250, 9) && sampled(10, 252, true));
    assert!(send(&fact(10, RSF_GAME_CPU_FRAME_BEGIN, 260, 0)));
    inject(Injection::ActualBegin);
    assert!(bind(10, 261));
    assert!(
        read_frame(10, &mut frame)
            && frame.stage_mask == 7
            && frame.timestamps_qpc[2] == 262
            && (frame.flags & RSF_GAME_CPU_DERIVED_BOUNDARY) == 0
    );
    assert!(
        send(&fact(10, RSF_GAME_CPU_SIMULATION_END, 263, 0))
            && send(&fact(10, RSF_GAME_CPU_FRAME_END, 264, 0))
    );
    // Replacing end first must not erase the still-derived begin's provenance.
    assert!(reserve(11, 270, 10) && sampled(11, 272, true));
    assert!(send(&fact(11, RSF_GAME_CPU_FRAME_BEGIN, 280, 0)) && bind(11, 281));
    assert!(send(&fact(
        11,
        RSF_GAME_CPU_SIMULATION_END,
        290,
        RSF_GAME_CPU_DERIVED_BOUNDARY
    )));
    assert!(send(&fact(11, RSF_GAME_CPU_SIMULATION_END, 291, 0)));
    assert!(
        read_frame(11, &mut frame)
            && frame.timestamps_qpc[2] == 281
            && frame.timestamps_qpc[3] == 291
            && (frame.flags & RSF_GAME_CPU_DERIVED_BOUNDARY) != 0
    );
    assert!(send(&fact(11, RSF_GAME_CPU_SIMULATION_BEGIN, 282, 0)));
    assert!(
        read_frame(11, &mut frame)
            && frame.timestamps_qpc[2] == 282
            && (frame.flags & RSF_GAME_CPU_DERIVED_BOUNDARY) == 0
    );
    assert!(send(&fact(11, RSF_GAME_CPU_FRAME_END, 292, 0)));
}
