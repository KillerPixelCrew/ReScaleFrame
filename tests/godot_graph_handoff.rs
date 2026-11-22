// SPDX-License-Identifier: MIT
use core::ffi::c_void;
use core::{mem::size_of, ptr};

use crate::packet_abi::{RSF_GODOT_VK_HANDOFF_ABI_VERSION, rsf_godot_vk_handoff};

unsafe extern "C" {
    fn rsf_fixture_graph_scope(context: *mut c_void, active: u32);
    fn rsf_fixture_graph_handoff(context: *mut c_void, foreign: u32, command: u64) -> u32;
    fn rsf_fixture_graph_header(
        context: *mut c_void,
        driver: *mut *mut c_void,
    ) -> *const rsf_godot_vk_handoff;
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
    fn rsf_fixture_graph_pool(context: *mut c_void, out: *mut [u64; 4]);
    fn rsf_fixture_graph_run(
        state: *mut c_void,
        foreign: *mut c_void,
        reuse: u32,
        refusals: u32,
    ) -> i32;
    fn rsf_fixture_graph_failure(line: u32);
}

#[derive(Clone, Copy, Default)]
struct BufferState {
    begun: bool,
    ended: bool,
    timeline: u64,
    wait: u64,
    signal: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault {
    None,
    TimelineDisabled,
    Allocate,
    Semaphore,
    Begin,
}

struct DriverState {
    buffers: [BufferState; 16],
    allocated: u64,
    semaphores: u64,
    freed: u64,
    begins: u64,
    splits: u64,
    closes: u64,
    opens: u64,
    labels_valid: bool,
    fault: Fault,
}

impl DriverState {
    fn new(reuse: bool) -> Self {
        Self {
            buffers: [BufferState::default(); 16],
            allocated: if reuse { 2 } else { 1 },
            semaphores: 0,
            freed: 0,
            begins: 0,
            splits: 0,
            closes: 0,
            opens: 0,
            labels_valid: true,
            fault: Fault::None,
        }
    }
}

// The C++ stubs borrow opaque pointers to these stack-owned states while GraphScope
// is active. Each stub operation is synchronous and exclusively updates its own state.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fixture_vk_graph_info(state: *mut c_void, id: u64) -> u32 {
    let state = unsafe { &*state.cast::<DriverState>() };
    if state.fault != Fault::TimelineDisabled
        && id != 0
        && state
            .buffers
            .get(id as usize)
            .is_some_and(|buffer| buffer.timeline == 0)
    {
        3
    } else {
        0
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_vk_command(_state: *mut c_void, id: u64) -> *mut c_void {
    (id as usize * 16) as *mut c_void
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fixture_vk_allocate(state: *mut c_void) -> u64 {
    let state = unsafe { &mut *state.cast::<DriverState>() };
    if state.fault == Fault::Allocate {
        return 0;
    }
    state.allocated += 1;
    state.allocated
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fixture_vk_semaphore(state: *mut c_void) -> u64 {
    let state = unsafe { &mut *state.cast::<DriverState>() };
    if state.fault == Fault::Semaphore {
        return 0;
    }
    state.semaphores += 1;
    state.semaphores
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fixture_vk_free_semaphore(state: *mut c_void) {
    unsafe { (*state.cast::<DriverState>()).freed += 1 };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fixture_vk_begin(state: *mut c_void, id: u64) -> u32 {
    let state = unsafe { &mut *state.cast::<DriverState>() };
    state.begins += 1;
    if state.fault == Fault::Begin {
        return 0;
    }
    let Some(buffer) = state.buffers.get_mut(id as usize) else {
        return 0;
    };
    *buffer = BufferState {
        begun: true,
        ..BufferState::default()
    };
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fixture_vk_split(
    state: *mut c_void,
    before: u64,
    after: u64,
    timeline: u64,
    previous: u64,
    ready: u64,
    done: u64,
) {
    let state = unsafe { &mut *state.cast::<DriverState>() };
    state.splits += 1;
    if let Some(buffer) = state.buffers.get_mut(before as usize) {
        buffer.ended = true;
        buffer.timeline = timeline;
        buffer.wait = previous;
        buffer.signal = ready;
    }
    if let Some(buffer) = state.buffers.get_mut(after as usize) {
        buffer.timeline = timeline;
        buffer.wait = done;
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fixture_vk_labels(state: *mut c_void, command: u64, reopen: u32) {
    let state = unsafe { &mut *state.cast::<DriverState>() };
    if reopen != 0 {
        state.opens += 1;
        state.labels_valid &= command == 2 && state.buffers[1].ended;
    } else {
        state.closes += 1;
        state.labels_valid &= command == 1 && !state.buffers[1].ended && state.buffers[2].begun;
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_graph_main() -> i32 {
    let mut failures = 0;
    for (reuse, refusals) in [(false, false), (true, false), (false, true)] {
        let mut state = DriverState::new(reuse);
        let mut foreign = DriverState::new(false);
        // SAFETY: both states outlive the C++ Fixture and its scope.
        failures += unsafe {
            rsf_fixture_graph_run(
                (&raw mut state).cast(),
                (&raw mut foreign).cast(),
                u32::from(reuse),
                u32::from(refusals),
            )
        };
    }
    i32::from(failures != 0)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_graph_cases(
    context: *mut c_void,
    state: *mut c_void,
    reuse: u32,
    refusals: u32,
) -> i32 {
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
        let valid_header = || {
            let mut driver = ptr::null_mut();
            let header = &*rsf_fixture_graph_header(context, &mut driver);
            header.user == driver
                && header.queue_family == 3
                && header.timeline_enabled == 1
                && header.struct_size as usize == size_of::<rsf_godot_vk_handoff>()
                && header.abi_version == RSF_GODOT_VK_HANDOFF_ABI_VERSION
                && header.split_record.is_some()
        };
        let split = |foreign, command, timeline, previous, ready, done, out| {
            rsf_fixture_graph_split(
                context, foreign, command, timeline, previous, ready, done, out,
            )
        };
        let state = state.cast::<DriverState>();
        let snapshot = || {
            let mut pool = [0; 4];
            rsf_fixture_graph_pool(context, &mut pool);
            let [command, used, buffers, semaphores] = pool;
            let state = &*state;
            let [source, continuation] = [state.buffers[1], state.buffers[2]];
            [
                command,
                used,
                buffers,
                semaphores,
                state.allocated,
                state.begins,
                state.splits,
                state.freed,
                source.timeline,
                source.wait,
                source.signal,
                continuation.timeline,
                continuation.wait,
                continuation.signal,
                state.closes,
                state.opens,
                u64::from(state.labels_valid),
            ]
        };
        let mut next = 0;
        check!(!handoff(0, 0));
        check!(snapshot()[2] == u64::from(reuse));
        rsf_fixture_graph_scope(context, 1);
        if refusals == 0 {
            check!(handoff(0, 0));
            check!(valid_header());
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
            (*state).fault = Fault::TimelineDisabled;
            check!(!handoff(0, 0));
            (*state).fault = Fault::None;
            check!(handoff(0, 0));
            check!(valid_header());
            check!(split(1, 0, 77, 4, 5, 6, &mut next) != 0);
            check!(split(0, 17, 77, 4, 5, 6, &mut next) != 0);
            check!(split(0, 0, 0, 4, 5, 6, &mut next) != 0);
            check!(split(0, 0, 77, 5, 5, 6, &mut next) != 0);
            check!(split(0, 0, 77, 4, 5, 5, &mut next) != 0);
            check!(split(0, 0, 77, 4, 5, 6, ptr::null_mut()) != 0);
            (*state).fault = Fault::Allocate;
            check!(split(0, 0, 77, 4, 5, 6, &mut next) == -1);
            check!(snapshot() == [1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
            (*state).fault = Fault::Semaphore;
            check!(split(0, 0, 77, 4, 5, 6, &mut next) == -1);
            check!(snapshot() == [1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
            (*state).fault = Fault::Begin;
            check!(split(0, 0, 77, 4, 5, 6, &mut next) == -3);
            check!(next == 0);
            check!(snapshot() == [1, 0, 1, 1, 2, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
            rsf_fixture_graph_scope(context, 0);
        }
    }
    failures
}
