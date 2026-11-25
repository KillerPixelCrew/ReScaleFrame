// SPDX-License-Identifier: MIT
use super::*;

pub(super) fn run() -> i32 {
    sample_native_thread();
    platform::rsf_native_fg_reflex_ordering_set(1);
    assert!(send(fact(1, RSF_GAME_CPU_FRAME_BEGIN)));
    assert!(send(fact(1, RSF_GAME_CPU_INPUT_SAMPLE)));
    assert!(send(fact(1, RSF_GAME_CPU_SIMULATION_BEGIN)));
    assert!(send(fact(1, RSF_GAME_CPU_SIMULATION_END)));
    latency_render(1, 1);
    assert!(send(fact(1, RSF_GAME_CPU_FRAME_END)));

    assert!(send(fact(2, RSF_GAME_CPU_FRAME_BEGIN)));
    arm(2);
    std::thread::scope(|scope| {
        let next = scope.spawn(|| send(fact(2, RSF_GAME_CPU_INPUT_SAMPLE)));
        assert!(wait_for_sleep());
        assert!(snapshot(2).paced == 0);
        let duplicate = fact(2, RSF_GAME_CPU_INPUT_SAMPLE);
        unsafe { fg_cpu(&duplicate) };
        assert!(pace_calls() == 2);
        latency_render(1, 0);
        observe_latency_present(1, null_mut(), 1);
        observe_latency_present(1, null_mut(), 0);
        assert!(next.join().unwrap());
    });
    assert!(
        snapshot(2).paced != 0
            && snapshot(2).generation == generation()
            && snapshot(1).completed != 0
    );
    for marker in 0..6 {
        assert_eq!(
            STATE
                .lock()
                .unwrap()
                .markers
                .iter()
                .filter(|&&record| record == (1, marker))
                .count(),
            1
        );
    }

    assert!(bind(PRIMARY_WINDOW) == 0);
    assert!(send(fact(2, RSF_GAME_CPU_SIMULATION_BEGIN)));
    assert!(receipt(1, 0) && receipt_calls() == 1 && routes().matched_thread == 1);
    assert!(!receipt(1, 0) && receipt_calls() == 1);
    assert!(receipt(2, REPLACEMENT_WINDOW) && receipt_calls() == 1 && routes().unmatched == 1);
    assert!(receipt(3, PRIMARY_WINDOW) && receipt_calls() == 2 && routes().matched_window == 1);
    assert!(routes().last_receipt == 3 && routes().last_source == 2 && routes().routed == 2);
    REBIND.store(true, Relaxed);
    assert!(receipt(4, PRIMARY_WINDOW) && receipt_calls() == 3);
    assert!(routes().last_source == 0 && routes().routed == 0);

    // A source admitted by the old provider cannot mark the new provider.
    let before_switch = marker_count();
    GENERATION.fetch_add(1, Relaxed);
    assert!(send(fact(2, RSF_GAME_CPU_SIMULATION_END)));
    assert!(marker_count() == before_switch);
    assert!(send(fact(2, RSF_GAME_CPU_FRAME_END)));

    arm(3);
    std::thread::scope(|scope| {
        let reserved = scope.spawn(|| source_event(3, RSF_GAME_CPU_SOURCE_RESERVE, 2));
        assert!(wait_state(|state| state.waiting_for_predecessor));
        assert!(!STATE.lock().unwrap().sleeping && pace_calls() == 2);
        assert_ne!(
            rsf_native_cpu_present_result(SESSION, 2, RSF_NATIVE_CPU_PRESENT_RETURNED, 0),
            0
        );
        assert!(wait_for_sleep());
        assert!(snapshot(3).paced == 0 && snapshot(3).ready_qpc == 0);
        let aborts = abort_calls();
        assert!(source_event(3, RSF_GAME_CPU_SOURCE_ABORT, 0));
        assert!(snapshot(3).failed != 0 && abort_calls() == aborts);
        wake();
        assert!(reserved.join().unwrap());
        assert!(snapshot(3).paced == 0 && abort_calls() == aborts + 1);
    });

    assert!(send(fact(4, RSF_GAME_CPU_FRAME_BEGIN)));
    let admitted_generation = GENERATION.fetch_add(1, Relaxed);
    assert!(send(fact(4, RSF_GAME_CPU_INPUT_SAMPLE)));
    assert!(
        pace_calls() == 3
            && snapshot(4).failed != 0
            && snapshot(4).generation == admitted_generation
    );
    assert!(send(fact(4, RSF_GAME_CPU_FRAME_END)));

    // A real Present without a reported submission cannot fabricate submit timing.
    assert!(send(fact(5, RSF_GAME_CPU_FRAME_BEGIN)));
    assert!(send(fact(5, RSF_GAME_CPU_INPUT_SAMPLE)));
    assert!(send(fact(5, RSF_GAME_CPU_SIMULATION_BEGIN)));
    assert!(send(fact(5, RSF_GAME_CPU_SIMULATION_END)));
    let before_missing_submit = marker_count();
    observe_latency_present(5, null_mut(), 1);
    observe_latency_present(5, null_mut(), 0);
    assert!(marker_count() == before_missing_submit);
    assert!(send(fact(5, RSF_GAME_CPU_FRAME_END)));

    // A failed simulation marker stays in the producer ledger but cannot
    // authorize any subsequent call into the vendor.
    FAIL_MARKER.store(true, Relaxed);
    assert!(send(fact(6, RSF_GAME_CPU_FRAME_BEGIN)));
    assert!(send(fact(6, RSF_GAME_CPU_INPUT_SAMPLE)));
    assert!(send(fact(6, RSF_GAME_CPU_SIMULATION_BEGIN)));
    assert!(send(fact(6, RSF_GAME_CPU_SIMULATION_END)));
    latency_render(6, 1);
    observe_latency_present(6, null_mut(), 1);
    observe_latency_present(6, null_mut(), 0);
    let mut refused: rsf_native_cpu_frame = unsafe { std::mem::zeroed() };
    refused.struct_size = size_of_val(&refused) as u32;
    assert!(
        unsafe { rsf_native_cpu_read(SESSION, 6, &mut refused) } != 0 && refused.marker_mask == 1
    );
    assert!(snapshot(6).failed != 0);
    assert!(send(fact(6, RSF_GAME_CPU_FRAME_END)));
    FAIL_MARKER.store(false, Relaxed);

    // Unreal's render thread begins the primary submission before the game thread ends the
    // simulation at FrameEndSync. Submit start is delivered at the submission, after the real
    // simulation start and before its end; submit end and Present still follow that end.
    platform::rsf_native_fg_reflex_ordering_set(0);
    let delivered = |source: u64| -> Vec<u32> {
        let state = STATE.lock().unwrap();
        let markers = state.markers.iter().filter(|record| record.0 == source);
        markers.map(|record| record.1).collect()
    };
    let submission = |source: u64, begin: u32| {
        let mut pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
        pass.struct_size = size_of_val(&pass) as u32;
        pass.session_id = SESSION;
        pass.source_frame_id = source;
        pass.flags = RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION;
        unsafe { fg_submission(&pass, begin) };
    };
    assert!(send(fact(7, RSF_GAME_CPU_FRAME_BEGIN)));
    assert!(send(fact(7, RSF_GAME_CPU_INPUT_SAMPLE)));
    assert!(send(fact(7, RSF_GAME_CPU_SIMULATION_BEGIN)));
    submission(7, 1);
    assert_eq!(delivered(7), [SIM_START, SUBMIT_START]);
    assert!(send(fact(7, RSF_GAME_CPU_SIMULATION_END)));
    observe_latency_present(7, null_mut(), 1);
    observe_latency_present(7, null_mut(), 0);
    submission(7, 0);
    assert!(send(fact(7, RSF_GAME_CPU_FRAME_END)));
    let ordered = [
        SIM_START,
        SUBMIT_START,
        SIM_END,
        SUBMIT_END,
        PRESENT_START,
        PRESENT_END,
    ];
    assert_eq!(delivered(7), ordered);
    assert!(snapshot(7).failed == 0 && snapshot(7).completed != 0);

    // A submission without a real simulation start sends no submit timing, also once a
    // substitute start is normalized in after it.
    assert!(send(fact(8, RSF_GAME_CPU_FRAME_BEGIN)));
    submission(8, 1);
    assert!(send(fact(8, RSF_GAME_CPU_SIMULATION_END)));
    observe_latency_present(8, null_mut(), 1);
    observe_latency_present(8, null_mut(), 0);
    submission(8, 0);
    assert!(send(fact(8, RSF_GAME_CPU_FRAME_END)));
    assert!(delivered(8).is_empty());
    platform::rsf_native_fg_reflex_ordering_set(1);

    latency_end_session(SESSION + 1);
    assert!(snapshot(3).failed != 0 && rsf_native_latency_has_claim(1) != 0);
    latency_end_session(SESSION);
    assert!(rsf_native_latency_has_claim(1) == 0);
    let mut retired = request_frame(3);
    assert_eq!(unsafe { latency_frame_get(&mut retired) }, 0);

    let paced_before_rendered = pace_calls();
    rsf_native_fg_rendered_source_set(1);
    platform::rsf_native_fg_reflex_ordering_set(0);
    let previous_markers = marker_count();
    assert!(send(fact(1, RSF_GAME_CPU_FRAME_BEGIN)));
    assert!(!send(fact(1, RSF_GAME_CPU_INPUT_SAMPLE)));
    assert!(send(fact(1, RSF_GAME_CPU_SIMULATION_END)));
    let mut pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
    pass.struct_size = size_of_val(&pass) as u32;
    pass.role = RSF_GAME_RENDER_FRAME;
    pass.session_id = SESSION;
    pass.source_frame_id = 1;
    pass.native_frame = 1;
    unsafe { fg_frame(&pass, 1) };

    // A previous integration must not retire the current SESSION's SDK tokens or advance its completion.
    RETIRE_HOST.store(true, Relaxed);
    assert!(snapshot(1).completed == 0);
    latency_retire(SESSION + 1, 1);
    latency_retire(SESSION, 0);
    assert!(snapshot(1).completed == 0 && RETIRE_CALLS.load(Relaxed) == 0);
    RETIRE_HOST.store(false, Relaxed);

    latency_render(1, 1);
    observe_latency_present(1, null_mut(), 1);
    observe_latency_present(1, null_mut(), 0);
    unsafe { fg_frame(&pass, 0) };
    assert!(send(fact(1, RSF_GAME_CPU_FRAME_END)));
    assert!(snapshot(1).paced == 0 && snapshot(1).failed == 0 && snapshot(1).completed != 0);
    assert!(
        pace_calls() == paced_before_rendered
            && marker_count() == previous_markers
            && rsf_native_latency_has_claim(1) == 0
    );
    latency_end_session(SESSION);
    assert_eq!(FAILURES.load(Relaxed), 0);
    0
}
