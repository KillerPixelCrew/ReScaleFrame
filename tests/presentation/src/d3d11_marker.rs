// SPDX-License-Identifier: MIT
//! Drive the linked CPU ledger and presentation owner with the original marker
//! admission sequences. The native device fixture supplies the production calls.
use crate::present_fixture_time::{frequency, stamp};
use crate::sys::*;
use crate::sys::{
    RSF_FG_OFF, RSF_REFLEX_OFF, rsf_d3d11_present_host, rsf_native_fg_options,
    rsf_native_fg_options_set, rsf_native_fg_set_log, rsf_native_latency_render,
    rsf_streamline_host_latency_status, rsf_streamline_host_token, rsf_streamline_latency_status,
};
use std::ffi::{CStr, c_char, c_void};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

static REENTRY_SOURCE: AtomicU64 = AtomicU64::new(0);
static REENTRIES: AtomicU32 = AtomicU32::new(0);

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_ac7_fixture_log(_: *mut c_void, message: *const c_char) {
    let message = unsafe { CStr::from_ptr(message) };
    println!("SDK: {}", message.to_string_lossy());
    const SUBMIT: &[u8] = b"actual graphics submit begin";
    if REENTRY_SOURCE.load(Ordering::Acquire) != 0
        && message
            .to_bytes()
            .windows(SUBMIT.len())
            .any(|part| part == SUBMIT)
    {
        let source = REENTRY_SOURCE.swap(0, Ordering::AcqRel);
        if source != 0 {
            REENTRIES.fetch_add(1, Ordering::Relaxed);
            // Consume the request before the linked logger can synchronously reenter.
            unsafe { rsf_native_latency_render(source, 1) };
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_fixture_marker_configure() {
    let options = rsf_native_fg_options {
        struct_size: size_of::<rsf_native_fg_options>() as u32,
        mode: RSF_FG_OFF,
        generated_frames: 1,
        reflex_mode: RSF_REFLEX_OFF,
        frame_limit_us: 0,
        debug: 1,
    };
    unsafe {
        rsf_native_fg_options_set(&options);
        rsf_native_fg_set_log(Some(rsf_ac7_fixture_log), null_mut());
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_fixture_marker_stamp() -> u64 {
    stamp()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_fixture_marker_frequency() -> u64 {
    frequency()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_fixture_marker_token(id: u64) -> *mut c_void {
    unsafe { rsf_streamline_host_token(rsf_d3d11_present_host(), id) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_fixture_marker_sleep_count(count: &mut u64) -> i32 {
    let mut status = rsf_streamline_latency_status {
        struct_size: size_of::<rsf_streamline_latency_status>() as u32,
        ..Default::default()
    };
    let result =
        unsafe { rsf_streamline_host_latency_status(rsf_d3d11_present_host(), &mut status) };
    *count = status.sleep_calls;
    result
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_fixture_marker_reenter(id: u64) {
    REENTRY_SOURCE.store(id, Ordering::Release);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_fixture_marker_reentries() -> u32 {
    REENTRIES.load(Ordering::Relaxed)
}

const SESSION: u64 = 42;
const FALLBACK: u64 = 10_000;
const LEGACY: u64 = 10_001;
const KEYED: u64 = 10_002;
const FAILED: u64 = 10_003;
const PROSPECTIVE: u64 = 10_004;
const KEYED_FLAGS: u32 = RSF_GAME_CPU_INPUT_KEYED;

macro_rules! call {
    ($ops:expr, $name:ident $(, $arg:expr)*) => {
        // The linked launcher supplies a complete, static callback table.
        unsafe { ($ops.$name.unwrap())($($arg),*) }
    };
}

struct MarkerFixture<'a> {
    calls: &'a rsf_d3d11_marker_fixture_calls,
    chain: *mut c_void,
    frequency: u64,
    success: bool,
}

impl MarkerFixture<'_> {
    fn check(&mut self, condition: bool, message: &str) {
        if !condition {
            self.success = false;
            eprintln!("FAIL marker admission: {message}");
        }
    }

    fn header(&self, id: u64, stage: u32, flags: u32, version: u32) -> rsf_game_cpu_event {
        rsf_game_cpu_event {
            event_version: version,
            flags,
            ..rsf_test_support::cpu_event(
                SESSION,
                id,
                stage,
                call!(self.calls, stamp),
                self.frequency,
            )
        }
    }

    fn send(&self, id: u64, stage: u32, flags: u32, version: u32) -> bool {
        call!(
            self.calls,
            dispatch,
            &self.header(id, stage, flags, version)
        ) != 0
    }

    fn modern(&self, id: u64, stage: u32, flags: u32) -> bool {
        self.send(id, stage, flags, RSF_GAME_CPU_EVENT_VERSION)
    }

    fn read(&mut self, id: u64) -> rsf_native_cpu_frame {
        let mut facts: rsf_native_cpu_frame = unsafe { std::mem::zeroed() };
        facts.struct_size = size_of::<rsf_native_cpu_frame>() as u32;
        self.check(
            call!(self.calls, read, SESSION, id, &mut facts) != 0,
            "source CPU facts retained",
        );
        facts
    }

    fn close(&mut self, id: u64, flags: u32) {
        self.check(self.modern(id, RSF_GAME_CPU_FRAME_END, flags), "CPU close");
        let mut frame: rsf_game_render_pass = unsafe { std::mem::zeroed() };
        frame.struct_size = size_of::<rsf_game_render_pass>() as u32;
        frame.role = RSF_GAME_RENDER_FRAME;
        frame.session_id = SESSION;
        frame.source_frame_id = id;
        call!(self.calls, frame, &frame, 0);
    }

    fn render(&self, id: u64, begin: u32) {
        call!(self.calls, render, id, begin);
    }

    fn present(&self, id: u64, begin: u32) {
        call!(self.calls, present, id, self.chain, begin);
    }

    fn counters(&mut self) -> rsf_native_cpu_counters {
        let mut counters: rsf_native_cpu_counters = unsafe { std::mem::zeroed() };
        counters.struct_size = size_of::<rsf_native_cpu_counters>() as u32;
        self.check(
            call!(self.calls, counters, &mut counters) != 0,
            "CPU counters copied",
        );
        counters
    }

    fn fallback_and_legacy(&mut self) {
        self.check(
            self.modern(FALLBACK, RSF_GAME_CPU_FRAME_BEGIN, 0),
            "unkeyed fallback begin",
        );
        for _ in 0..2 {
            self.render(FALLBACK, 1);
            self.render(FALLBACK, 0);
            self.present(FALLBACK, 0);
            self.present(FALLBACK, 1);
            self.present(FALLBACK, 0);
        }
        let facts = self.read(FALLBACK);
        self.check(
            facts.stage_mask == 1
                && facts.flags == 0
                && facts.marker_mask == 0
                && facts.timestamps_qpc[1] == 0
                && facts.pacing_qpc == 0,
            "fallback cannot promote CPU input or markers",
        );
        self.close(FALLBACK, 0);

        call!(self.calls, ordering, 1);
        self.check(
            self.send(LEGACY, RSF_GAME_CPU_FRAME_BEGIN, 0, 0)
                && self.send(LEGACY, RSF_GAME_CPU_INPUT_SAMPLE, 0, 0),
            "legacy input",
        );
        self.render(LEGACY, 1);
        self.present(LEGACY, 1);
        self.present(LEGACY, 0);
        self.check(
            self.send(LEGACY, RSF_GAME_CPU_SIMULATION_BEGIN, 0, 0)
                && self.send(LEGACY, RSF_GAME_CPU_SIMULATION_END, 0, 0),
            "legacy late simulation",
        );
        self.render(LEGACY, 0);
        self.render(LEGACY, 0);
        let facts = self.read(LEGACY);
        self.check(
            facts.marker_mask == 63,
            "legacy threaded marker order retained",
        );
        self.close(LEGACY, 0);
    }

    fn keyed_and_failed(&mut self) {
        call!(self.calls, ordering, 0);
        self.check(
            self.modern(KEYED, RSF_GAME_CPU_FRAME_BEGIN, KEYED_FLAGS)
                && self.modern(KEYED, RSF_GAME_CPU_INPUT_SAMPLE, KEYED_FLAGS)
                && self.modern(KEYED, RSF_GAME_CPU_SIMULATION_BEGIN, KEYED_FLAGS),
            "keyed input and simulation",
        );
        call!(self.calls, reenter, KEYED);
        self.render(KEYED, 1);
        self.render(KEYED, 1);
        self.check(
            call!(self.calls, reentries) == 1,
            "accepted start tolerates synchronous reentry",
        );
        let facts = self.read(KEYED);
        self.check(
            facts.stage_mask == 15
                && facts.flags & RSF_GAME_CPU_DERIVED_BOUNDARY != 0
                && facts.flags & RSF_GAME_CPU_INPUT_SUBSTITUTE == 0,
            "existing keyed submit derives only simulation end",
        );
        self.render(KEYED, 0);
        self.render(KEYED, 0);
        self.present(KEYED, 1);
        self.present(KEYED, 1);
        self.present(KEYED, 0);
        self.present(KEYED, 0);
        let facts = self.read(KEYED);
        self.check(facts.marker_mask == 63, "keyed begin/end deduplicated");
        self.close(KEYED, KEYED_FLAGS);

        self.check(
            self.modern(FAILED, RSF_GAME_CPU_FRAME_BEGIN, KEYED_FLAGS)
                && self.modern(FAILED, RSF_GAME_CPU_INPUT_SAMPLE, KEYED_FLAGS)
                && self.modern(FAILED, RSF_GAME_CPU_SIMULATION_BEGIN, KEYED_FLAGS)
                && self.modern(FAILED, RSF_GAME_CPU_SIMULATION_END, KEYED_FLAGS),
            "failed-start input facts",
        );
        self.check(
            call!(self.calls, abort_frame, FAILED) == 0,
            "withdraw provider token before submission start",
        );
        self.render(FAILED, 1);
        self.render(FAILED, 1);
        self.render(FAILED, 0);
        self.render(FAILED, 0);
        self.present(FAILED, 1);
        self.present(FAILED, 0);
        let facts = self.read(FAILED);
        self.check(
            facts.marker_mask == 7,
            "provider-refused start cannot emit a matching end or Present",
        );
        let mut state: rsf_native_latency_frame = unsafe { std::mem::zeroed() };
        state.struct_size = size_of::<rsf_native_latency_frame>() as u32;
        state.version = 1;
        state.source = FAILED;
        self.check(
            call!(self.calls, latency_frame, &mut state) != 0 && state.failed != 0,
            "provider failure retained rather than hidden",
        );
        self.close(FAILED, KEYED_FLAGS);
    }

    fn source_event(&self, stage: u32, poll_begin_qpc: u64) -> bool {
        let flags = if stage == RSF_GAME_CPU_SOURCE_SAMPLED {
            RSF_GAME_CPU_SOURCE_PRODUCED
        } else {
            0
        };
        let mut header = self.header(PROSPECTIVE, stage, flags, RSF_GAME_CPU_EVENT_VERSION_SOURCE);
        header.struct_size = size_of::<rsf_game_cpu_source_event>() as u32;
        header.render_expected = 0;
        let event = rsf_game_cpu_source_event {
            header,
            predecessor_source_id: 0,
            native_generation: call!(self.calls, generation),
            reason: 0,
            reserved: 0,
            poll_begin_qpc,
        };
        let packet = std::ptr::from_ref(&event).cast::<rsf_game_cpu_event>();
        call!(self.calls, dispatch, packet) != 0
    }

    fn reserved_poll(&mut self) {
        self.check(
            self.source_event(RSF_GAME_CPU_SOURCE_RESERVE, 0),
            "pre-poll reservation",
        );
        let token = call!(self.calls, token, PROSPECTIVE);
        let mut reserved_sleeps = 0;
        self.check(
            call!(self.calls, sleep_count, &mut reserved_sleeps) == 0,
            "reserved latency counters",
        );
        let poll_begin = call!(self.calls, stamp);
        self.check(
            self.source_event(RSF_GAME_CPU_SOURCE_SAMPLED, poll_begin)
                && self.modern(PROSPECTIVE, RSF_GAME_CPU_FRAME_BEGIN, 0)
                && self.source_event(RSF_GAME_CPU_SOURCE_BIND, 0),
            "real poll adopted and bound",
        );

        let mut submission: rsf_game_render_pass = unsafe { std::mem::zeroed() };
        submission.struct_size = size_of::<rsf_game_render_pass>() as u32;
        submission.role = RSF_GAME_RENDER_SUBMISSION;
        submission.session_id = SESSION;
        submission.source_frame_id = PROSPECTIVE;
        submission.submission_id = PROSPECTIVE;
        submission.view_key = PROSPECTIVE;
        submission.flags = RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION;
        call!(self.calls, submit, &submission, 1);
        call!(self.calls, submit, &submission, 0);
        let mut submitted_sleeps = 0;
        self.check(
            !token.is_null()
                && call!(self.calls, token, PROSPECTIVE) == token
                && call!(self.calls, sleep_count, &mut submitted_sleeps) == 0
                && submitted_sleeps == reserved_sleeps,
            "submission retains one pre-poll token without a second sleep",
        );
        self.present(PROSPECTIVE, 1);
        self.present(PROSPECTIVE, 0);
        let facts = self.read(PROSPECTIVE);
        self.check(
            facts.marker_mask == 63
                && facts.flags & KEYED_FLAGS != 0
                && facts.flags & RSF_GAME_CPU_INPUT_SUBSTITUTE == 0
                && facts.timestamps_qpc[1] == poll_begin,
            "bound poll retains real input and canonical order",
        );
        self.close(PROSPECTIVE, 0);
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_d3d11_marker_fixture(
    chain: *mut c_void,
    calls: &rsf_d3d11_marker_fixture_calls,
) -> i32 {
    let mut fixture = MarkerFixture {
        calls,
        chain,
        frequency: call!(calls, frequency),
        success: true,
    };
    call!(calls, configure);
    call!(calls, ordering, 0);
    let before = fixture.counters();
    fixture.fallback_and_legacy();
    fixture.keyed_and_failed();
    fixture.reserved_poll();
    let after = fixture.counters();
    fixture.check(
        after.order_violations == before.order_violations
            && after.substitutes == before.substitutes
            && after.duplicates == before.duplicates,
        "admission emits no invalid/duplicate marker attempts or substitute input",
    );
    if fixture.success {
        println!(
            "PASS: generic marker admission, fallback refusal, keyed/bound derivation, failed start and repeated ends"
        );
    }
    i32::from(fixture.success)
}

#[repr(C)]
struct NativeCommand {
    next: *mut NativeCommand,
    // The fixture needs the storage but invokes it through the guarded C++ callback.
    execute: Option<unsafe extern "C" fn(*mut c_void, *mut NativeCommand)>,
}

#[repr(C)]
struct NativeList {
    root: *mut NativeCommand,
    tail: *mut *mut NativeCommand,
    executing: u8,
    reserved: [u8; 3],
    count: u32,
    uid: u32,
}

const _: () = {
    assert!(size_of::<NativeCommand>() == 16);
    assert!(size_of::<NativeList>() == 32);
    assert!(std::mem::offset_of!(NativeList, count) == 0x14);
    assert!(std::mem::offset_of!(NativeList, uid) == 0x18);
};

pub(super) unsafe fn append_native(list: *mut c_void, command: *mut c_void) {
    let list = list.cast::<NativeList>();
    let command = command.cast::<NativeCommand>();
    unsafe {
        (*list).tail.write(command);
        (*list).tail = &raw mut (*command).next;
        (*list).count = (*list).count.wrapping_add(1);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_fixture_run(
    list: *mut NativeList,
    execute: unsafe extern "C" fn(*mut NativeList, *mut NativeCommand),
) {
    unsafe {
        (*list).executing = 1;
        let mut command = (*list).root;
        while !command.is_null() {
            // A callback may consume or release its command. Save the next link first.
            let next = (*command).next;
            execute(list, command);
            command = next;
        }
        (*list).executing = 0;
        (*list).root = std::ptr::null_mut();
        (*list).tail = &raw mut (*list).root;
        (*list).count = 0;
        (*list).uid = (*list).uid.wrapping_add(1);
    }
}
