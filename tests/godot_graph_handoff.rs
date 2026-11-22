// SPDX-License-Identifier: MIT
use core::ffi::c_void;
use core::ptr;

unsafe extern "C" {
    fn rsf_fixture_graph_scope(context: *mut c_void, active: u32);
    fn rsf_fixture_graph_handoff(context: *mut c_void, foreign: u32, command: u64) -> u32;
    fn rsf_fixture_graph_header(context: *mut c_void) -> u32;
    fn rsf_fixture_graph_split(
        context: *mut c_void,
        foreign: u32,
        command: usize,
        timeline: u64,
        previous: u64,
        ready: u64,
        done: u64,
        out: *mut usize,
    ) -> i32;
    fn rsf_fixture_graph_fault(context: *mut c_void, fault: u32);
    fn rsf_fixture_graph_snapshot(context: *mut c_void, out: *mut u64);
    fn rsf_fixture_graph_failure(line: u32);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_graph_cases(context: *mut c_void, reuse: u32, refusals: u32) -> i32 {
    let mut failures = 0;
    macro_rules! check {
        ($condition:expr) => {
            if !$condition {
                rsf_fixture_graph_failure(line!());
                failures += 1;
            }
        };
    }
    // SAFETY: the C++ runner gives us a single live Fixture per scenario. The
    // output and snapshot storage live through the corresponding calls.
    unsafe {
        let handoff = |foreign, command| rsf_fixture_graph_handoff(context, foreign, command) != 0;
        let split = |foreign, command, timeline, previous, ready, done, out| {
            rsf_fixture_graph_split(
                context, foreign, command, timeline, previous, ready, done, out,
            )
        };
        let snapshot = || {
            // Engine command and pool, driver counters, source and continuation timelines,
            // then label order. Match the export in godot_graph_handoff.cpp.
            let mut values = [0_u64; 17];
            rsf_fixture_graph_snapshot(context, values.as_mut_ptr());
            values
        };
        let mut next = 0;
        check!(!handoff(0, 0));
        check!(snapshot()[2] == u64::from(reuse));
        rsf_fixture_graph_scope(context, 1);
        if refusals == 0 {
            check!(handoff(0, 0));
            check!(rsf_fixture_graph_header(context) != 0);
            check!(split(0, 0, 77, 4, 5, 6, &mut next) == 0);
            check!(next == 2 * 16);
            check!(snapshot() == [2, 1, 1, 1, 2, 1, 1, 0, 77, 4, 5, 77, 6, 0, 1, 1, 1]);
            // Provider fallback still records on the graph-owned continuation.
            check!(!handoff(0, 0));
            rsf_fixture_graph_scope(context, 0);
            next = 1;
            check!(split(0, 0, 77, 6, 7, 8, &mut next) != 0);
            check!(next == 0 && snapshot()[..2] == [2, 1]);
        } else {
            check!(!handoff(1, 0));
            check!(!handoff(0, 2));
            rsf_fixture_graph_fault(context, 1);
            check!(!handoff(0, 0));
            rsf_fixture_graph_fault(context, 0);
            check!(handoff(0, 0));
            check!(rsf_fixture_graph_header(context) != 0);
            check!(split(1, 0, 77, 4, 5, 6, &mut next) != 0);
            check!(split(0, 17, 77, 4, 5, 6, &mut next) != 0);
            check!(split(0, 0, 0, 4, 5, 6, &mut next) != 0);
            check!(split(0, 0, 77, 5, 5, 6, &mut next) != 0);
            check!(split(0, 0, 77, 4, 5, 5, &mut next) != 0);
            check!(split(0, 0, 77, 4, 5, 6, ptr::null_mut()) != 0);
            rsf_fixture_graph_fault(context, 2);
            check!(split(0, 0, 77, 4, 5, 6, &mut next) == -1);
            check!(snapshot() == [1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
            rsf_fixture_graph_fault(context, 3);
            check!(split(0, 0, 77, 4, 5, 6, &mut next) == -1);
            check!(snapshot() == [1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
            rsf_fixture_graph_fault(context, 4);
            check!(split(0, 0, 77, 4, 5, 6, &mut next) == -3);
            check!(next == 0);
            check!(snapshot() == [1, 0, 1, 1, 2, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
            rsf_fixture_graph_scope(context, 0);
        }
    }
    failures
}
