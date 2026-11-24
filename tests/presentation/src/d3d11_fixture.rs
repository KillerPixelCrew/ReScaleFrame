// SPDX-License-Identifier: MIT
//! The linked AC7 presentation fixture: cold COM and status contracts, the shared D3D12 SR, the
//! generic marker admission sequences and, when enabled, queued AC7 render scopes driving real FG
//! frames over a synthetic native command list.
use crate::d3d11_device_checks::*;
use crate::present_fixture_startup::*;
use crate::present_fixture_time::{frequency, milliseconds, stamp};
use crate::sys::*;
use rsf_game_sys::backend_result::{ABI_MISMATCH, INVALID_ARGUMENT, NOT_READY};
use rsf_game_sys::{RSF_D3D11_DISPATCH_RELEASE, rsf_d3d11_chain_dispatch};
use rsf_test_support::{Window, enable_d3d12_debug_layer, warp_d3d11};
use std::ffi::{CStr, c_char, c_void};
use std::ptr::{self, null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering};
use std::time::Instant;
use windows::Win32::Foundation::{E_FAIL, HINSTANCE, HMODULE, HWND, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Direct3D12::ID3D12Device;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Storage::FileSystem::CreateDirectoryW;
use windows::Win32::System::Diagnostics::Debug::{
    EXCEPTION_POINTERS, RtlCaptureStackBackTrace, SetUnhandledExceptionFilter,
};
use windows::Win32::System::Environment::{GetEnvironmentVariableA, SetEnvironmentVariableW};
use windows::Win32::System::LibraryLoader::{
    GetModuleFileNameA, GetModuleFileNameW, GetModuleHandleW, GetProcAddress, LoadLibraryA,
};
use windows::Win32::System::Memory::{MEMORY_BASIC_INFORMATION, VirtualQuery};
use windows::Win32::System::Threading::Sleep;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_SYSTEM_AWARE, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, IUnknown, Interface, PCSTR, PCWSTR, s, w};

const SESSION: u64 = 42;
// RSF_DLSS_ERROR_NOT_READY (dlss.h), a cast macro bindgen does not evaluate.
const DLSS_NOT_READY: i32 = -8;

// The render scope exports of rsf-ac7-plugin (linked with this crate's ac7-render-scope feature) and
// the C create call of ReScaleFrame::AC7RenderScope; the AC7 bindings leave them to their definitions.
unsafe extern "C" {
    fn rsf_ac7_render_scopes_create_passes(
        session: u64,
        capacity: u32,
        state: rsf_ac7_render_scope_fn,
        pass: rsf_ac7_render_scope_fn,
        user: Raw,
        out: *mut *mut rsf_ac7_render_scopes,
    ) -> i32;
    fn rsf_ac7_render_scope_open(
        scopes: *mut rsf_ac7_render_scopes,
        commands: Raw,
        pass: *const rsf_game_render_pass,
        out: *mut *mut rsf_ac7_render_ticket,
    ) -> i32;
    fn rsf_ac7_render_scope_close(ticket: *mut rsf_ac7_render_ticket, commands: Raw) -> i32;
    fn rsf_ac7_render_scopes_quiesce(scopes: *mut rsf_ac7_render_scopes);
    fn rsf_ac7_render_scopes_destroy(scopes: *mut rsf_ac7_render_scopes) -> i32;
}

/// The engine's command record and list, which the render scopes queue their markers into.
#[repr(C)]
struct NativeCommand {
    next: *mut NativeCommand,
    execute: Option<unsafe extern "C" fn(Raw, *mut NativeCommand)>,
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

impl NativeList {
    /// An empty list; `tail` points into the list, so it must stay where it is once reset.
    fn reset(&mut self) {
        self.root = null_mut();
        self.tail = &raw mut self.root;
        self.executing = 0;
        self.count = 0;
    }

    fn append<T>(&mut self, command: &mut T) {
        // Every queued command starts with its NativeCommand.
        let command = ptr::from_mut(command).cast::<NativeCommand>();
        unsafe {
            self.tail.write(command);
            self.tail = &raw mut (*command).next;
        }
        self.count = self.count.wrapping_add(1);
    }

    fn run(&mut self) {
        let owner = ptr::from_mut(self).cast();
        self.executing = 1;
        let mut command = self.root;
        while !command.is_null() {
            // A command may consume or release itself; read the next link first.
            let next = unsafe { (*command).next };
            if let Some(execute) = unsafe { (*command).execute } {
                unsafe { execute(owner, command) };
            }
            command = next;
        }
        self.reset();
        self.uid = self.uid.wrapping_add(1);
    }

    fn raw(&mut self) -> Raw {
        ptr::from_mut(self).cast()
    }
}

static QUEUED_PASSES_OK: AtomicBool = AtomicBool::new(true);
// Optional: hold one frame's scene submission open, to exercise the render hitch sampler.
static SUBMISSION_STALL_MS: AtomicU32 = AtomicU32::new(0);
static EXPECTED_PATH: AtomicU32 = AtomicU32::new(RSF_NATIVE_SCENE_PRESENT_NONE);
static FINAL_CONTEXT: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static RENDERER_STARTUP_OBSERVED: AtomicBool = AtomicBool::new(false);
static REENTRY_SOURCE: AtomicU64 = AtomicU64::new(0);
static REENTRIES: AtomicU32 = AtomicU32::new(0);

fn passes(accepted: bool) {
    if !accepted {
        QUEUED_PASSES_OK.store(false, Ordering::Relaxed);
    }
}

fn token_live(id: u64) -> bool {
    !unsafe { rsf_streamline_host_token(rsf_d3d11_present_host(), id) }.is_null()
}

unsafe extern "C" fn queued_pass(_: Raw, _: Raw, pass: *const rsf_game_render_pass, begin: u32) {
    let pass = unsafe { &*pass };
    match pass.role {
        RSF_GAME_RENDER_SUBMISSION => unsafe { rsf_native_fg_submission(pass, begin) },
        RSF_GAME_RENDER_FRAME => unsafe { rsf_native_fg_frame(pass, begin) },
        RSF_GAME_RENDER_FINAL_SCENE => {
            passes(unsafe { rsf_native_scene_pass(pass, begin) } != 0);
            let context = FINAL_CONTEXT.load(Ordering::Relaxed);
            if begin == 0 && !context.is_null() {
                unsafe { rsf_native_fg_final(context, pass) };
            }
        }
        RSF_GAME_RENDER_TEXTURE_BINDING if begin == 0 => {
            passes(unsafe { rsf_native_scene_texture_binding(pass) } != 0);
        }
        RSF_GAME_RENDER_WINDOW => {
            if begin != 0 {
                let expected =
                    if EXPECTED_PATH.load(Ordering::Relaxed) == RSF_NATIVE_SCENE_PRESENT_DIRECT {
                        RSF_NATIVE_SCENE_PRESENT_DIRECT
                    } else {
                        RSF_NATIVE_SCENE_PRESENT_NONE
                    };
                passes(unsafe { rsf_native_scene_present_path(pass) } == expected);
                let mut stale = *pass;
                stale.source_frame_id = stale.source_frame_id.wrapping_add(1);
                passes(
                    unsafe { rsf_native_scene_present_path(&stale) }
                        == RSF_NATIVE_SCENE_PRESENT_NONE,
                );
                let mut other = *pass;
                other.viewport_key = other.viewport_key.wrapping_add(1);
                passes(
                    unsafe { rsf_native_scene_present_path(&other) }
                        == RSF_NATIVE_SCENE_PRESENT_NONE,
                );
            } else {
                unsafe { rsf_native_fg_window_end(pass) };
            }
            passes(unsafe { rsf_native_window_scope(pass, begin) } != 0);
        }
        _ => {}
    }
}

#[repr(C)]
struct PresentCommand {
    base: NativeCommand,
    chain: Raw,
    simulation_pending: bool,
    result: i32,
}

unsafe extern "C" fn present_execute(_: Raw, command: *mut NativeCommand) {
    let present = unsafe { &mut *command.cast::<PresentCommand>() };
    let mut window = rsf_game_render_pass {
        struct_size: size_of::<rsf_game_render_pass>() as u32,
        ..Default::default()
    };
    if unsafe { rsf_native_window_match(present.chain, &mut window) } == 0
        || unsafe { rsf_native_scene_present_path(&window) }
            != EXPECTED_PATH.load(Ordering::Relaxed)
    {
        return;
    }
    let chain = unsafe { IDXGISwapChain::from_raw_borrowed(&present.chain) }.unwrap();
    present.result = unsafe { chain.Present(0, DXGI_PRESENT(0)) }.0;
    // RenderSubmitEnd precedes PresentStart, so PresentEnd retires the token unless the game
    // thread's SimulationEnd is still outstanding.
    passes(token_live(window.source_frame_id) == present.simulation_pending);
}

#[repr(C)]
struct EvaluateCommand {
    base: NativeCommand,
    context: Raw,
    source: *const rsf_game_render_pass,
    frame: *const rsf_dlss_frame,
    result: i32,
}

unsafe extern "C" fn evaluate_execute(_: Raw, command: *mut NativeCommand) {
    let evaluate = unsafe { &mut *command.cast::<EvaluateCommand>() };
    let stall = SUBMISSION_STALL_MS.swap(0, Ordering::Relaxed);
    if stall != 0 {
        unsafe { Sleep(stall) };
    }
    unsafe {
        rsf_native_fg_scene(evaluate.source);
        evaluate.result = rsf_native_fg_evaluate(evaluate.context, evaluate.frame);
        rsf_native_fg_scene(null());
    }
}

#[repr(C)]
struct SimulationEndCommand {
    base: NativeCommand,
    source: u64,
}

unsafe extern "C" fn simulation_end_execute(_: Raw, command: *mut NativeCommand) {
    cpu_event(
        unsafe { (*command.cast::<SimulationEndCommand>()).source },
        RSF_GAME_CPU_SIMULATION_END,
    );
}

fn command(execute: unsafe extern "C" fn(Raw, *mut NativeCommand)) -> NativeCommand {
    NativeCommand {
        next: null_mut(),
        execute: Some(execute),
    }
}

unsafe extern "C" fn log(_: Raw, message: *const c_char) {
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
            // A synchronous logger reentry must not open a second interval.
            unsafe { rsf_native_latency_render(source, 1) };
        }
    }
}

fn event(id: u64, stage: u32) -> rsf_game_cpu_event {
    rsf_test_support::cpu_event(SESSION, id, stage, stamp(), frequency())
}

fn cpu_event(id: u64, stage: u32) {
    let event = event(id, stage);
    if unsafe { rsf_native_cpu_event(&event) } != 0 {
        unsafe { rsf_native_fg_cpu(&event) };
    }
}

unsafe extern "C" fn dispatch_target(_: Raw, fact: *const rsf_game_cpu_event) {
    unsafe { rsf_native_fg_cpu(fact) };
}

fn latency() -> (i32, rsf_streamline_latency_status) {
    let mut status = rsf_streamline_latency_status {
        struct_size: size_of::<rsf_streamline_latency_status>() as u32,
        ..Default::default()
    };
    let result =
        unsafe { rsf_streamline_host_latency_status(rsf_d3d11_present_host(), &mut status) };
    (result, status)
}

fn fg_status() -> Option<rsf_native_fg_status> {
    let mut status = rsf_native_fg_status {
        struct_size: size_of::<rsf_native_fg_status>() as u32,
        ..Default::default()
    };
    (unsafe { rsf_native_fg_status_get(&mut status) } != 0).then_some(status)
}

fn observer_status() -> (i32, rsf_observer_status) {
    let mut status = rsf_observer_status {
        struct_size: size_of::<rsf_observer_status>() as u32,
        ..Default::default()
    };
    let result = unsafe { rsf_observer_get_status(&mut status) };
    (result, status)
}

const FALLBACK: u64 = 10_000;
const LEGACY: u64 = 10_001;
const KEYED: u64 = 10_002;
const FAILED: u64 = 10_003;
const PROSPECTIVE: u64 = 10_004;
const KEYED_FLAGS: u32 = RSF_GAME_CPU_INPUT_KEYED;

/// The generic marker admission sequences over the real CPU ledger, native FG owner and provider,
/// without fake simulation facts for fallback sources. Plain mode only.
struct MarkerFixture {
    chain: Raw,
    success: bool,
}

impl MarkerFixture {
    fn check(&mut self, condition: bool, message: &str) {
        if !condition {
            self.success = false;
            eprintln!("FAIL marker admission: {message}");
        }
    }

    fn header(id: u64, stage: u32, flags: u32, version: u32) -> rsf_game_cpu_event {
        rsf_game_cpu_event {
            event_version: version,
            flags,
            ..event(id, stage)
        }
    }

    fn dispatch(event: *const rsf_game_cpu_event) -> bool {
        unsafe { rsf_native_cpu_dispatch(event, Some(dispatch_target), null_mut()) != 0 }
    }

    fn send(id: u64, stage: u32, flags: u32, version: u32) -> bool {
        Self::dispatch(&Self::header(id, stage, flags, version))
    }

    fn modern(id: u64, stage: u32, flags: u32) -> bool {
        Self::send(id, stage, flags, RSF_GAME_CPU_EVENT_VERSION)
    }

    fn read(&mut self, id: u64) -> rsf_native_cpu_frame {
        let mut facts = rsf_native_cpu_frame {
            struct_size: size_of::<rsf_native_cpu_frame>() as u32,
            ..Default::default()
        };
        let read = unsafe { rsf_native_cpu_read(SESSION, id, &mut facts) } != 0;
        self.check(read, "source CPU facts retained");
        facts
    }

    fn close(&mut self, id: u64, flags: u32) {
        self.check(Self::modern(id, RSF_GAME_CPU_FRAME_END, flags), "CPU close");
        let frame = rsf_game_render_pass {
            struct_size: size_of::<rsf_game_render_pass>() as u32,
            role: RSF_GAME_RENDER_FRAME,
            session_id: SESSION,
            source_frame_id: id,
            ..Default::default()
        };
        unsafe { rsf_native_fg_frame(&frame, 0) };
    }

    fn render(id: u64, begin: u32) {
        unsafe { rsf_native_latency_render(id, begin) };
    }

    fn present(&self, id: u64, begin: u32) {
        unsafe { rsf_native_latency_present(id, self.chain, begin) };
    }

    fn counters(&mut self) -> rsf_native_cpu_counters {
        let mut counters = rsf_native_cpu_counters {
            struct_size: size_of::<rsf_native_cpu_counters>() as u32,
            ..Default::default()
        };
        let copied = unsafe { rsf_native_cpu_counters_get(&mut counters) } != 0;
        self.check(copied, "CPU counters copied");
        counters
    }

    fn sleeps(&mut self, message: &str) -> u64 {
        let (result, status) = latency();
        self.check(result == 0, message);
        status.sleep_calls
    }

    fn fallback_and_legacy(&mut self) {
        self.check(
            Self::modern(FALLBACK, RSF_GAME_CPU_FRAME_BEGIN, 0),
            "unkeyed fallback begin",
        );
        for _ in 0..2 {
            Self::render(FALLBACK, 1);
            Self::render(FALLBACK, 0);
            // No accepted start, including an unmatched end.
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

        // Legacy threaded order permits Present before a late SimulationEnd/RenderSubmitEnd.
        unsafe { rsf_native_fg_reflex_ordering_set(1) };
        self.check(
            Self::send(LEGACY, RSF_GAME_CPU_FRAME_BEGIN, 0, 0)
                && Self::send(LEGACY, RSF_GAME_CPU_INPUT_SAMPLE, 0, 0),
            "legacy input",
        );
        Self::render(LEGACY, 1);
        self.present(LEGACY, 1);
        self.present(LEGACY, 0);
        self.check(
            Self::send(LEGACY, RSF_GAME_CPU_SIMULATION_BEGIN, 0, 0)
                && Self::send(LEGACY, RSF_GAME_CPU_SIMULATION_END, 0, 0),
            "legacy late simulation",
        );
        Self::render(LEGACY, 0);
        Self::render(LEGACY, 0);
        let facts = self.read(LEGACY);
        self.check(
            facts.marker_mask == 63,
            "legacy threaded marker order retained",
        );
        self.close(LEGACY, 0);
    }

    fn keyed_and_failed(&mut self) {
        unsafe { rsf_native_fg_reflex_ordering_set(0) };
        self.check(
            Self::modern(KEYED, RSF_GAME_CPU_FRAME_BEGIN, KEYED_FLAGS)
                && Self::modern(KEYED, RSF_GAME_CPU_INPUT_SAMPLE, KEYED_FLAGS)
                && Self::modern(KEYED, RSF_GAME_CPU_SIMULATION_BEGIN, KEYED_FLAGS),
            "keyed input and simulation",
        );
        REENTRY_SOURCE.store(KEYED, Ordering::Release);
        Self::render(KEYED, 1);
        Self::render(KEYED, 1);
        self.check(
            REENTRIES.load(Ordering::Relaxed) == 1,
            "accepted start tolerates synchronous reentry",
        );
        let facts = self.read(KEYED);
        self.check(
            facts.stage_mask == 15
                && facts.flags & RSF_GAME_CPU_DERIVED_BOUNDARY != 0
                && facts.flags & RSF_GAME_CPU_INPUT_SUBSTITUTE == 0,
            "existing keyed submit derives only simulation end",
        );
        Self::render(KEYED, 0);
        Self::render(KEYED, 0);
        self.present(KEYED, 1);
        self.present(KEYED, 1);
        self.present(KEYED, 0);
        self.present(KEYED, 0);
        let facts = self.read(KEYED);
        self.check(facts.marker_mask == 63, "keyed begin/end deduplicated");
        self.close(KEYED, KEYED_FLAGS);

        self.check(
            Self::modern(FAILED, RSF_GAME_CPU_FRAME_BEGIN, KEYED_FLAGS)
                && Self::modern(FAILED, RSF_GAME_CPU_INPUT_SAMPLE, KEYED_FLAGS)
                && Self::modern(FAILED, RSF_GAME_CPU_SIMULATION_BEGIN, KEYED_FLAGS)
                && Self::modern(FAILED, RSF_GAME_CPU_SIMULATION_END, KEYED_FLAGS),
            "failed-start input facts",
        );
        self.check(
            unsafe { rsf_d3d11_present_abort(FAILED) } == 0,
            "withdraw provider token before submission start",
        );
        Self::render(FAILED, 1);
        Self::render(FAILED, 1);
        Self::render(FAILED, 0);
        Self::render(FAILED, 0);
        self.present(FAILED, 1);
        self.present(FAILED, 0);
        let facts = self.read(FAILED);
        self.check(
            facts.marker_mask == 7,
            "provider-refused start cannot emit a matching end or Present",
        );
        let mut state = rsf_native_latency_frame {
            struct_size: size_of::<rsf_native_latency_frame>() as u32,
            version: 1,
            source: FAILED,
            ..Default::default()
        };
        self.check(
            unsafe { rsf_native_latency_frame_get(&mut state) } != 0 && state.failed != 0,
            "provider failure retained rather than hidden",
        );
        self.close(FAILED, KEYED_FLAGS);
    }

    fn source_event(stage: u32, poll_begin_qpc: u64) -> bool {
        let flags = if stage == RSF_GAME_CPU_SOURCE_SAMPLED {
            RSF_GAME_CPU_SOURCE_PRODUCED
        } else {
            0
        };
        let mut header = Self::header(PROSPECTIVE, stage, flags, RSF_GAME_CPU_EVENT_VERSION_SOURCE);
        header.struct_size = size_of::<rsf_game_cpu_source_event>() as u32;
        header.render_expected = 0;
        let event = rsf_game_cpu_source_event {
            header,
            predecessor_source_id: 0,
            native_generation: unsafe { rsf_d3d11_present_generation() },
            reason: 0,
            reserved: 0,
            poll_begin_qpc,
        };
        Self::dispatch(ptr::from_ref(&event).cast())
    }

    /// A real pre-poll reservation with actual poll entry and accepted BIND keeps the canonical
    /// derivation; no second pacing token is acquired at submission.
    fn reserved_poll(&mut self) {
        self.check(
            Self::source_event(RSF_GAME_CPU_SOURCE_RESERVE, 0),
            "pre-poll reservation",
        );
        let token = unsafe { rsf_streamline_host_token(rsf_d3d11_present_host(), PROSPECTIVE) };
        let reserved_sleeps = self.sleeps("reserved latency counters");
        let poll_begin = stamp();
        self.check(
            Self::source_event(RSF_GAME_CPU_SOURCE_SAMPLED, poll_begin)
                && Self::modern(PROSPECTIVE, RSF_GAME_CPU_FRAME_BEGIN, 0)
                && Self::source_event(RSF_GAME_CPU_SOURCE_BIND, 0),
            "real poll adopted and bound",
        );
        let submission = rsf_game_render_pass {
            struct_size: size_of::<rsf_game_render_pass>() as u32,
            role: RSF_GAME_RENDER_SUBMISSION,
            session_id: SESSION,
            source_frame_id: PROSPECTIVE,
            submission_id: PROSPECTIVE,
            view_key: PROSPECTIVE,
            flags: RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION,
            ..Default::default()
        };
        unsafe {
            rsf_native_fg_submission(&submission, 1);
            rsf_native_fg_submission(&submission, 0);
        }
        let (result, submitted) = latency();
        self.check(
            !token.is_null()
                && unsafe { rsf_streamline_host_token(rsf_d3d11_present_host(), PROSPECTIVE) }
                    == token
                && result == 0
                && submitted.sleep_calls == reserved_sleeps,
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

    fn run(chain: Raw) -> bool {
        let mut fixture = MarkerFixture {
            chain,
            success: true,
        };
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
            rsf_native_fg_set_log(Some(log), null_mut());
            rsf_native_fg_reflex_ordering_set(0);
        }
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
        fixture.success
    }
}

/// Loads the AC7 carrier by path and checks its DirectInput exports, then starts D3D11 beside it.
unsafe fn proxy_smoke(proxy_path: *const c_char) -> i32 {
    let mut logs = [0_u16; 260];
    let path_length = unsafe { GetModuleFileNameW(None, &mut logs) } as usize;
    if path_length == 0 || path_length >= logs.len() {
        return 1;
    }
    let Some(leaf) = logs[..path_length]
        .iter()
        .rposition(|&unit| unit == u16::from(b'\\'))
    else {
        return 1;
    };
    let suffix = w!("\\proxy-smoke-logs");
    let suffix = unsafe { suffix.as_wide() };
    if leaf + suffix.len() + 1 > logs.len() {
        return 1;
    }
    logs[leaf..leaf + suffix.len()].copy_from_slice(suffix);
    logs[leaf + suffix.len()] = 0;
    let dir = PCWSTR(logs.as_ptr());
    let _ = unsafe { CreateDirectoryW(dir, None) };
    if unsafe { SetEnvironmentVariableW(w!("RSF_DUMP_DIR"), dir) }.is_err() {
        return 1;
    }
    let proxy = match unsafe { LoadLibraryA(PCSTR(proxy_path.cast())) } {
        Ok(proxy) => proxy,
        Err(error) => {
            eprintln!("proxy load error={}", error.code().0 as u32 & 0xffff);
            return 1;
        }
    };
    let named = unsafe { GetProcAddress(proxy, s!("DirectInput8Create")) };
    let ordinal = unsafe { GetProcAddress(proxy, PCSTR(ptr::without_provenance(1))) };
    if !matches!((named, ordinal), (Some(named), Some(ordinal)) if ptr::fn_addr_eq(named, ordinal))
        || unsafe { GetProcAddress(proxy, s!("GetFileVersionInfoW")) }.is_some()
    {
        return 1;
    }
    // Let this isolated process exercise the carrier's observer startup. Carrier hooks and its worker
    // are process-owned, as in AC7, so the DLL stays loaded.
    unsafe { Sleep(250) };
    if unsafe { CreateDXGIFactory1::<IDXGIFactory2>() }.is_err()
        || rsf_test_support::d3d11_device(
            D3D_DRIVER_TYPE_HARDWARE,
            D3D11_CREATE_DEVICE_FLAG(0),
            None,
        )
        .or_else(|_| warp_d3d11(D3D11_CREATE_DEVICE_FLAG(0), None))
        .is_err()
    {
        return 1;
    }
    println!(
        "PASS: AC7-only carrier loads, DirectInput ordinal 1 matches, D3D11 observer/device startup succeeds"
    );
    0
}

unsafe extern "system" fn exception_filter(exception: *const EXCEPTION_POINTERS) -> i32 {
    let record = unsafe { &*(*exception).ExceptionRecord };
    eprintln!(
        "fixture exception 0x{:08x} at {:p}",
        record.ExceptionCode.0 as u32, record.ExceptionAddress
    );
    let mut frames = [null_mut(); 24];
    let count = unsafe { RtlCaptureStackBackTrace(0, &mut frames, None) };
    for &frame in &frames[..usize::from(count)] {
        let mut region = MEMORY_BASIC_INFORMATION::default();
        unsafe {
            VirtualQuery(
                Some(frame.cast_const()),
                &mut region,
                size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        let mut module = [0_u8; 260];
        let length =
            unsafe { GetModuleFileNameA(Some(HMODULE(region.AllocationBase)), &mut module) };
        eprintln!(
            "  {} + 0x{:x}",
            String::from_utf8_lossy(&module[..length as usize]),
            frame as usize - region.AllocationBase as usize
        );
    }
    1 // EXCEPTION_EXECUTE_HANDLER
}

unsafe extern "C" fn before_present(_: Raw, chain: Raw) {
    let (result, status) = observer_status();
    if result != 0 || status.present_width == 0 || status.present_height == 0 {
        return;
    }
    let (mut device, mut context) = (null_mut(), null_mut());
    let acquired = unsafe { rsf_observer_acquire_device(&mut device, &mut context) };
    // Adopt even a partial observer result so every returned reference is released.
    let _device = (!device.is_null()).then(|| unsafe { IUnknown::from_raw(device) });
    let _context = (!context.is_null()).then(|| unsafe { IUnknown::from_raw(context) });
    let chain = unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }.unwrap();
    if acquired == 0
        && unsafe { chain.GetDevice::<ID3D11Device>() }
            .is_ok_and(|actual| actual.as_raw() == device)
    {
        RENDERER_STARTUP_OBSERVED.store(true, Ordering::Release);
    }
}

unsafe extern "C" fn accept_window(user: Raw, window: Raw) -> i32 {
    i32::from(user == window)
}

unsafe extern "C" fn present_event(_: Raw, event: *const rsf_observer_present_event) {
    unsafe { rsf_native_window_present(event) };
}

unsafe extern "C" fn latency_event(_: Raw, event: *const rsf_observer_present_event) {
    unsafe { rsf_native_fg_present(event) };
}

fn cold_status() -> bool {
    let mut status = rsf_d3d11_present_status {
        struct_size: size_of::<rsf_d3d11_present_status>() as u32,
        version: RSF_PRESENT_STATUS_VERSION,
        ..Default::default()
    };
    if unsafe { rsf_d3d11_present_status_get(&mut status) } != NOT_READY || status.valid != 0 {
        return false;
    }
    let mut short = status;
    short.struct_size = size_of::<rsf_d3d11_present_status>() as u32 - 1;
    if unsafe { rsf_d3d11_present_status_get(&mut short) } != INVALID_ARGUMENT {
        return false;
    }
    status.version += 1;
    if unsafe { rsf_d3d11_present_status_get(&mut status) } != ABI_MISMATCH {
        return false;
    }
    println!("PASS: cold provider status refusals");
    true
}

// X-Ray: an ANSI class, WS_EX_TOPMOST and an off-origin starting position.
fn xray_window(instance: HINSTANCE) -> Result<HWND, i32> {
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 640,
        bottom: 480,
    };
    let _ = unsafe { AdjustWindowRect(&mut rect, style, false) };
    let class = s!("_XRAY_FIXTURE");
    let xray = WNDCLASSA {
        hInstance: instance,
        lpfnWndProc: Some(default_window_procedure),
        lpszClassName: class,
        ..Default::default()
    };
    if unsafe { RegisterClassA(&xray) } == 0 {
        return Err(1);
    }
    unsafe {
        CreateWindowExA(
            WS_EX_TOPMOST,
            class,
            s!("Presentation bridge fixture"),
            style,
            190,
            190,
            rect.right - rect.left,
            rect.bottom - rect.top,
            None,
            None,
            Some(instance),
            None,
        )
    }
    .map_err(|_| 1)
}

// The windows crate wraps DefWindowProcA in a Rust function; the class needs the system ABI.
unsafe extern "system" fn default_window_procedure(
    window: HWND,
    message: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    unsafe { DefWindowProcA(window, message, wparam, lparam) }
}

fn limit(name: PCSTR) -> u32 {
    let mut bytes = [0u8; 16];
    let read = unsafe { GetEnvironmentVariableA(name, Some(&mut bytes)) };
    if read != 0 && (read as usize) < bytes.len() {
        unsafe { number(bytes.as_ptr().cast()) }
    } else {
        0
    }
}

fn resize(chain: &IDXGISwapChain, width: u32, height: u32) -> Result<(), i32> {
    unsafe {
        chain.ResizeBuffers(
            1,
            width,
            height,
            DXGI_FORMAT_UNKNOWN,
            DXGI_SWAP_CHAIN_FLAG(0),
        )
    }
    .map_err(|_| 1)
}

/// Evaluates the shared D3D12 SR once through `host` on the nine fixture `textures` (four SR inputs
/// and outputs, then AC7's five translucency hints) and returns the frame it evaluated. This is
/// synthetic GPU evidence for the host FG uses: it catches a second Streamline owner or token and
/// D3D11/12 state errors, not a game scene or a latency measurement.
unsafe fn shared_sr(
    host: Raw,
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    textures: &[ID3D11Texture2D],
) -> Option<rsf_dlss_frame> {
    if unsafe { rsf_dlss_share_host(host, Some(log), null_mut()) } != 0 {
        return None;
    }
    let mut support = rsf_dlss_support {
        struct_size: size_of::<rsf_dlss_support>() as u32,
        ..Default::default()
    };
    if unsafe { rsf_dlss_query_support(&mut support) } != 0 {
        return None;
    }
    let raw = |index: usize| textures[index].as_raw();
    let mut frame = rsf_dlss_frame {
        struct_size: size_of::<rsf_dlss_frame>() as u32,
        abi_version: RSF_DLSS_ABI_VERSION,
        color_in: raw(0),
        depth: raw(1),
        motion: raw(2),
        color_out: raw(3),
        render_width: 512,
        render_height: 512,
        output_width: 512,
        output_height: 512,
        quality: 0, // RSF_DLSS_QUALITY_NATIVE.
        near_plane: 0.1,
        vertical_fov: 1.0,
        aspect_ratio: 1.0,
        motion_scale_x: 1.0 / 512.0,
        motion_scale_y: 1.0 / 512.0,
        camera_motion_included: 1,
        depth_inverted: 1,
        reset: 1,
        camera_forward: [0.0, 0.0, 1.0],
        camera_up: [0.0, 1.0, 0.0],
        camera_right: [1.0, 0.0, 0.0],
        color_before_transparency: raw(4),
        transparency_layer: raw(5),
        reactive_mask: raw(6),
        transparency_hint: raw(7),
        bias_current_color: raw(8),
        // AC7 runs DLSS on the display-range colour transport with HDR input off.
        color_encoded: 1,
        ..Default::default()
    };
    frame.camera_view_to_clip[0] = 2.0;
    frame.camera_view_to_clip[5] = 2.0;
    frame.camera_view_to_clip[11] = 1.0;
    frame.camera_view_to_clip[14] = 0.1;
    frame.clip_to_camera_view[0] = 0.5;
    frame.clip_to_camera_view[5] = 0.5;
    frame.clip_to_camera_view[11] = 10.0;
    frame.clip_to_camera_view[14] = 1.0;
    for index in [0, 5, 10, 15] {
        frame.clip_to_prev_clip[index] = 1.0;
        frame.prev_clip_to_clip[index] = 1.0;
    }
    let pass = rsf_game_render_pass {
        struct_size: size_of::<rsf_game_render_pass>() as u32,
        source_frame_id: 1,
        ..Default::default()
    };
    if unsafe { rsf_streamline_host_begin(host.cast(), 1) } != 0 {
        return None;
    }
    unsafe { rsf_native_fg_scene(&pass) };
    let evaluated = unsafe { rsf_native_fg_evaluate(context.as_raw(), &frame) };
    unsafe { rsf_native_fg_scene(null()) };
    let sample = if evaluated == 0 {
        red_half(device, context, &textures[3])
    } else {
        0
    };
    unsafe { rsf_streamline_host_abort(host.cast(), 1) };
    if unsafe { rsf_dlss_release_resources() } != 0
        || unsafe { rsf_dlss_shutdown() } != 0
        || sample == 0
        || sample >= 0x7c00
    {
        return None;
    }
    println!("shared D3D12 SR output red half={sample:04x}, one existing CPU token");
    Some(frame)
}

fn open(
    scopes: *mut rsf_ac7_render_scopes,
    commands: &mut NativeList,
    pass: &rsf_game_render_pass,
) -> Result<*mut rsf_ac7_render_ticket, i32> {
    let mut ticket = null_mut();
    if unsafe { rsf_ac7_render_scope_open(scopes, commands.raw(), pass, &mut ticket) } == 0 {
        return Err(1);
    }
    Ok(ticket)
}

fn close(ticket: *mut rsf_ac7_render_ticket, commands: &mut NativeList) -> Result<(), i32> {
    if unsafe { rsf_ac7_render_scope_close(ticket, commands.raw()) } == 0 {
        return Err(1);
    }
    Ok(())
}

/// The enabled fixture: queued AC7 render scopes drive sampled and then direct FG frames across
/// the flight, hangar and briefing screens, then the abandoned-window and pacing-join checks.
#[allow(clippy::too_many_arguments)]
fn run_enabled(
    chain: &IDXGISwapChain,
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    window: HWND,
    gpu: Raw,
    frame: &mut rsf_dlss_frame,
    format: DXGI_FORMAT,
    (switching, pacing, ordering_only, visible): (bool, bool, bool, bool),
) -> Result<(), i32> {
    // Match the SR fixture's output extent for both sampled and direct paths.
    resize(chain, 512, 512)?;
    if visible {
        show(window, true);
    }
    println!(
        "fixture window={:p} foreground={:p}",
        window.0,
        unsafe { GetForegroundWindow() }.0
    );
    if unsafe { rsf_dlss_share_host(rsf_d3d11_present_host().cast(), Some(log), null_mut()) } != 0 {
        return Err(1);
    }
    let mut options = rsf_native_fg_options {
        struct_size: size_of::<rsf_native_fg_options>() as u32,
        mode: RSF_FG_FIXED,
        generated_frames: 1,
        reflex_mode: RSF_REFLEX_ON,
        // Optional: a limit on rendered frames, to measure what the driver's limiter does with it.
        frame_limit_us: limit(s!("RSF_TEST_FRAME_LIMIT_US")),
        debug: 1,
    };
    SUBMISSION_STALL_MS.store(limit(s!("RSF_TEST_SUBMISSION_STALL_MS")), Ordering::Relaxed);
    unsafe {
        rsf_native_fg_options_set(&options);
        rsf_native_fg_set_log(Some(log), null_mut());
    }
    FINAL_CONTEXT.store(context.as_raw(), Ordering::Relaxed);
    let description = D3D11_TEXTURE2D_DESC {
        Width: 512,
        Height: 512,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32,
        ..Default::default()
    };
    let mut hudless = None;
    unsafe { device.CreateTexture2D(&description, None, Some(&mut hudless)) }.map_err(|_| 1)?;
    let hudless = hudless.ok_or(1)?;
    let mut hudless_target: Option<ID3D11RenderTargetView> = None;
    unsafe { device.CreateRenderTargetView(&hudless, None, Some(&mut hudless_target)) }
        .map_err(|_| 1)?;
    let hudless_target = hudless_target.ok_or(1)?;
    let mut source = rsf_game_render_pass {
        struct_size: size_of::<rsf_game_render_pass>() as u32,
        session_id: SESSION,
        family_key: 1,
        view_key: 2,
        viewport_key: 3,
        screen: RSF_SCREEN_FLIGHT,
        flags: RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION,
        ..Default::default()
    };
    let camera = &mut source.camera;
    camera.struct_size = size_of::<rsf_camera_frame>() as u32;
    camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    camera.render_width = 512;
    camera.render_height = 512;
    camera.output_width = 512;
    camera.output_height = 512;
    camera.near_plane = 0.1;
    camera.vertical_fov_radians = 1.0;
    camera.depth_inverted = 1;
    camera.view_to_clip = frame.camera_view_to_clip;
    camera.clip_to_view = frame.clip_to_camera_view;
    camera.clip_to_previous_clip = frame.clip_to_prev_clip;
    for index in [0, 5, 10, 15] {
        camera.view_to_world[index] = 1.0;
        camera.world_to_view[index] = 1.0;
    }
    camera.frame_time_seconds = 1.0 / 60.0;
    source.output_rect[2] = 512;
    source.output_rect[3] = 512;
    let mut scopes = null_mut();
    if unsafe {
        rsf_ac7_render_scopes_create_passes(
            SESSION,
            8,
            None,
            Some(queued_pass),
            null_mut(),
            &mut scopes,
        )
    } == 0
    {
        return Err(1);
    }

    let frame_count = if pacing {
        240
    } else if switching {
        120
    } else {
        16
    };
    let mut paths = [false; 2];
    let mut providers = [0; 6];
    let mut sleeps_expected = 1;
    let (mut active_ms, mut active_frames, mut inactive_ms, mut inactive_frames) = (0.0, 0, 0.0, 0);
    let qpc_frequency = frequency();
    for id in 2..2 + frame_count {
        let provider = unsafe { rsf_d3d11_present_backend() };
        let direct = id >= 2 + frame_count / 2;
        EXPECTED_PATH.store(
            if direct {
                RSF_NATIVE_SCENE_PRESENT_DIRECT
            } else {
                RSF_NATIVE_SCENE_PRESENT_SAMPLED
            },
            Ordering::Relaxed,
        );
        let (screen, reflex) = if id < 2 + frame_count / 3 {
            (RSF_SCREEN_FLIGHT, RSF_REFLEX_OFF)
        } else if id < 2 + 2 * frame_count / 3 {
            (RSF_SCREEN_HANGAR, RSF_REFLEX_ON)
        } else {
            (RSF_SCREEN_BRIEFING, RSF_REFLEX_BOOST)
        };
        source.screen = screen;
        options.reflex_mode = reflex;
        // Requests beyond this GPU's capability must clamp without disabling generation.
        if switching {
            options.generated_frames = if id % 12 < 6 { 1 } else { 3 };
        }
        unsafe { rsf_native_fg_options_set(&options) };
        let started = stamp();
        cpu_event(id, RSF_GAME_CPU_FRAME_BEGIN);
        let (reserved_result, reserved) = latency();
        if reserved_result != 0 || reserved.sleep_calls != sleeps_expected {
            println!(
                "sleep count source={id} actual={} expected={sleeps_expected}",
                reserved.sleep_calls
            );
            return Err(1);
        }
        source.source_frame_id = id;
        source.submission_id = id;
        source.native_frame = id;
        source.scope_id = id;
        let mut commands = NativeList {
            root: null_mut(),
            tail: null_mut(),
            executing: 0,
            reserved: [0; 3],
            count: 0,
            uid: 1,
        };
        commands.reset();
        let full_frame = rsf_game_render_pass {
            role: RSF_GAME_RENDER_FRAME,
            family_key: 0,
            view_key: 0,
            ..source
        };
        let frame_ticket = open(scopes, &mut commands, &full_frame)?;
        commands.run(); // Native BeginFrame can execute before the game thread's Reflex sleep.
        cpu_event(id, RSF_GAME_CPU_PACING);
        let pacing_status = fg_status().ok_or(1)?;
        // Pacing is a core choice, independent of which FG provider is selected.
        if pacing_status.vendor.pacing_owner == RSF_PACING_REFLEX {
            sleeps_expected += 1;
        }
        let (paced_result, paced) = latency();
        if paced_result != 0
            || paced.sleep_calls != sleeps_expected
            || (!switching
                && unsafe { rsf_streamline_host_sleep(rsf_d3d11_present_host(), id) }
                    != INVALID_ARGUMENT)
        {
            return Err(1);
        }
        let mut message = MSG::default();
        let _ = unsafe { PeekMessageW(&mut message, Some(window), 0, 0, PM_NOREMOVE) };
        cpu_event(id, RSF_GAME_CPU_INPUT_SAMPLE);
        let mut input = event(id, RSF_GAME_CPU_INPUT_EVENT);
        input.input_kind = RSF_GAME_INPUT_MOUSE | RSF_GAME_INPUT_KEYBOARD;
        input.message_id = WM_MOUSEMOVE;
        if unsafe { rsf_native_cpu_event(&input) } == 0 {
            return Err(1);
        }
        unsafe { rsf_native_fg_cpu(&input) };
        input.input_kind = RSF_GAME_INPUT_CONTROLLER;
        input.message_id = 0;
        if unsafe { rsf_native_cpu_event(&input) } == 0 {
            return Err(1);
        }
        // The SDK controller marker is once per frame.
        unsafe {
            rsf_native_fg_cpu(&input);
            rsf_native_fg_cpu(&input);
        }
        let mut record = rsf_native_cpu_frame {
            struct_size: size_of::<rsf_native_cpu_frame>() as u32,
            ..Default::default()
        };
        if unsafe { rsf_native_cpu_read(SESSION, id, &mut record) } == 0
            || record.stage_mask != 3
            || record.input_mask != 7
            || record.input_events[..3] != [1, 1, 1]
        {
            return Err(1);
        }
        cpu_event(id, RSF_GAME_CPU_SIMULATION_BEGIN);
        let late_simulation_end = id & 1 != 0;
        if !late_simulation_end {
            cpu_event(id, RSF_GAME_CPU_SIMULATION_END);
        }
        let submission = rsf_game_render_pass {
            role: RSF_GAME_RENDER_SUBMISSION,
            ..source
        };
        let submission_ticket = open(scopes, &mut commands, &submission)?;
        frame.reset = u32::from(id == 2);
        let mut evaluate = EvaluateCommand {
            base: command(evaluate_execute),
            context: context.as_raw(),
            source: &source,
            frame,
            result: DLSS_NOT_READY,
        };
        commands.append(&mut evaluate);
        let backbuffer = unsafe { chain.GetBuffer::<ID3D11Texture2D>(0) }.map_err(|_| 1)?;
        let mut target: Option<ID3D11RenderTargetView> = None;
        unsafe { device.CreateRenderTargetView(&backbuffer, None, Some(&mut target)) }
            .map_err(|_| 1)?;
        let target = target.ok_or(1)?;
        let color = [0.25, 0.2, 0.6, 1.0];
        unsafe {
            context.ClearRenderTargetView(&target, &color);
            context.ClearRenderTargetView(&hudless_target, &color);
        }
        let final_scene = rsf_game_render_pass {
            role: RSF_GAME_RENDER_FINAL_SCENE,
            scene_surface: if direct {
                backbuffer.as_raw()
            } else {
                hudless.as_raw()
            },
            ..source
        };
        let final_ticket = open(scopes, &mut commands, &final_scene)?;
        close(final_ticket, &mut commands)?;
        close(submission_ticket, &mut commands)?;
        // Native Slate packets carry source and window ownership, never copied scene-view keys.
        let output = rsf_game_render_pass {
            struct_size: size_of::<rsf_game_render_pass>() as u32,
            session_id: source.session_id,
            source_frame_id: id,
            viewport_key: source.viewport_key,
            pass_key: id,
            flags: source.flags,
            role: RSF_GAME_RENDER_WINDOW,
            window_key: window.0 as usize as u64,
            rhi_viewport_key: 4,
            swapchain: chain.as_raw(),
            ..Default::default()
        };
        let window_ticket = open(scopes, &mut commands, &output)?;
        if !direct {
            let binding = rsf_game_render_pass {
                role: RSF_GAME_RENDER_TEXTURE_BINDING,
                sampled_texture: hudless.as_raw(),
                rhi_viewport_key: 0,
                swapchain: null_mut(),
                ..output
            };
            let binding_ticket = open(scopes, &mut commands, &binding)?;
            close(binding_ticket, &mut commands)?;
        }
        let mut present = PresentCommand {
            base: command(present_execute),
            chain: chain.as_raw(),
            simulation_pending: late_simulation_end,
            result: E_FAIL.0,
        };
        commands.append(&mut present);
        close(window_ticket, &mut commands)?;
        let mut simulation_end = SimulationEndCommand {
            base: command(simulation_end_execute),
            source: id,
        };
        if late_simulation_end {
            commands.append(&mut simulation_end);
        }
        close(frame_ticket, &mut commands)?;
        commands.run();
        drop((target, backbuffer));
        cpu_event(id, RSF_GAME_CPU_FRAME_END);
        let passes_ok = QUEUED_PASSES_OK.load(Ordering::Relaxed);
        if !passes_ok || evaluate.result != 0 || present.result < 0 || token_live(id) {
            let device = unsafe { ID3D12Device::from_raw_borrowed(&gpu) }.unwrap();
            let reason = unsafe { device.GetDeviceRemovedReason() }
                .map_or_else(|error| error.code().0, |()| 0);
            println!(
                "AC7 adapter failed source={id} SR={} Present={:08x} device={:08x} passes={}",
                evaluate.result,
                present.result as u32,
                reason as u32,
                u32::from(passes_ok)
            );
            validation_clean(device);
            return Err(1);
        }
        let activity = fg_status().ok_or(1)?;
        let vendor = &activity.vendor;
        // Present may complete a queued provider switch: historical frame eligibility then belongs to
        // the old provider, while latency status describes the new owner.
        let expected_reflex = if vendor.effective_mode == RSF_FG_OFF || reflex != RSF_REFLEX_OFF {
            reflex
        } else {
            RSF_REFLEX_ON
        };
        if provider == RSF_FG_BACKEND_DLSS
            && unsafe { rsf_d3d11_present_backend() } == provider
            && vendor.effective_reflex != expected_reflex
        {
            println!(
                "Reflex mismatch source={id} effective={} requested={reflex} mode={}",
                vendor.effective_reflex, vendor.effective_mode
            );
            return Err(1);
        }
        if switching {
            println!(
                "adapter source={id} backend={provider} mode={} active={} result={} generated={} max={}",
                vendor.effective_mode,
                vendor.active,
                activity.last_result,
                vendor.effective_generated_frames,
                vendor.max_generated_frames
            );
            if vendor.active != 0 {
                *providers.get_mut(provider as usize).ok_or(1)? += 1;
                if vendor.effective_generated_frames
                    != options.generated_frames.min(vendor.max_generated_frames)
                {
                    println!(
                        "multiplier mismatch source={id} effective={} requested={} max={}",
                        vendor.effective_generated_frames,
                        options.generated_frames,
                        vendor.max_generated_frames
                    );
                    return Err(1);
                }
            }
            let next = match id {
                13 | 53 => Some(RSF_FG_BACKEND_FSR3),
                25 | 65 => Some(RSF_FG_BACKEND_XESS),
                37 => Some(0),
                43 | 75 => Some(RSF_FG_BACKEND_DLSS),
                _ => None,
            };
            if next.is_some_and(|next| unsafe { rsf_d3d11_present_request(next) } != 0)
                || activity.last_result != 0
            {
                return Err(1);
            }
        }
        paths[usize::from(direct)] |= vendor.active != 0;
        let elapsed = milliseconds(started, qpc_frequency);
        if vendor.active != 0 {
            active_ms += elapsed;
            active_frames += 1;
        } else {
            inactive_ms += elapsed;
            inactive_frames += 1;
        }
        if !pacing {
            unsafe { Sleep(16) };
        }
    }
    let mean = |sum: f64, frames: u32| {
        if frames != 0 {
            sum / f64::from(frames)
        } else {
            0.0
        }
    };
    println!(
        "frame limit: requested {} us between rendered frames; generating {active_frames} frames mean {:.2} ms, not generating {inactive_frames} frames mean {:.2} ms",
        options.frame_limit_us,
        mean(active_ms, active_frames),
        mean(inactive_ms, inactive_frames)
    );
    unsafe { rsf_ac7_render_scopes_quiesce(scopes) };
    if unsafe { rsf_ac7_render_scopes_destroy(scopes) } == 0 {
        return Err(1);
    }
    let (result, presenter) = observer_status();
    if result != 0 || u64::from(presenter.frames_presented) != 12 + frame_count {
        return Err(1);
    }
    println!("PASS: application frame count matches source Presents");
    println!(
        "PASS: {frame_count} sampled/direct queued frames across flight/hangar/briefing; wrong source/viewport and missing sampled binding rejected; all CPU tokens retired"
    );
    println!(
        "present paths: sampled active={} direct active={}",
        u32::from(paths[0]),
        u32::from(paths[1])
    );
    if !ordering_only && !paths.into_iter().all(|active| active) {
        return Err(1);
    }
    let observed = fg_status().ok_or(1)?;
    let vendor = observed.vendor;
    println!(
        "synthetic enabled FG: tagged={} mode={} generated={} active={} aggregate presents={} result={} SDK status={}",
        observed.tagged_frames,
        vendor.effective_mode,
        vendor.effective_generated_frames,
        vendor.active,
        vendor.total_presented,
        observed.last_result,
        vendor.vendor_status
    );
    if observed.tagged_frames == 0
        || observed.last_result != 0
        || vendor.vendor_status != 0
        || (!ordering_only && !switching && vendor.active == 0)
    {
        return Err(1);
    }
    let (result, latency) = latency();
    println!(
        "latency sleeps={} markers={}",
        latency.sleep_calls, latency.marker_calls
    );
    if result != 0
        || latency.sleep_calls != sleeps_expected
        || latency.marker_calls != frame_count * 7
    {
        return Err(1);
    }
    if switching {
        let (dlss, fsr, xess) = (providers[1], providers[3], providers[5]);
        println!(
            "AC7 adapter active samples: DLSS={dlss} FSR3={fsr} XeSS={xess}; live multiplier requests 2x/4x clamp to SDK limit"
        );
        if fsr == 0 || xess == 0 {
            return Err(1);
        }
        if dlss == 0 {
            if visible && unsafe { GetForegroundWindow() } == window {
                return Err(1);
            }
            println!(
                "NOT RUN: DLSS activity acceptance requires a foreground game; this fixture was denied foreground ownership"
            );
        }
    }
    let host = unsafe { rsf_d3d11_present_host() };
    let probe = frame_count + 2;
    if latency.pcl_message_id == 0
        || unsafe { rsf_streamline_host_begin(host, probe) } != 0
        || unsafe { rsf_streamline_host_input(host, probe, 0, latency.pcl_message_id) } != 0
        || unsafe { rsf_streamline_host_abort(host, probe) } != 0
    {
        return Err(1);
    }
    println!(
        "PASS: keyboard/mouse dequeue records, controller marker deduplication and SDK-registered PCL ping routing"
    );
    let unpresented = frame_count + 3;
    cpu_event(unpresented, RSF_GAME_CPU_FRAME_BEGIN);
    cpu_event(unpresented, RSF_GAME_CPU_PACING);
    for stage in RSF_GAME_CPU_INPUT_SAMPLE..=RSF_GAME_CPU_FRAME_END {
        cpu_event(unpresented, stage);
    }
    source.source_frame_id = unpresented;
    unsafe {
        rsf_native_fg_submission(&source, 1);
        rsf_native_fg_submission(&source, 0);
    }
    source.role = RSF_GAME_RENDER_WINDOW;
    source.swapchain = chain.as_raw();
    if !token_live(unpresented) {
        return Err(1);
    }
    unsafe { rsf_native_fg_window_end(&source) };
    if token_live(unpresented) {
        return Err(1);
    }
    let abort_started = Instant::now();
    cpu_event(unpresented + 1, RSF_GAME_CPU_FRAME_BEGIN);
    if abort_started.elapsed().as_millis() >= 500 || !token_live(unpresented + 1) {
        return Err(1);
    }
    // The abandoned window also releases the next frame's pacing join.
    let start = stamp();
    cpu_event(unpresented + 1, RSF_GAME_CPU_PACING);
    let released = milliseconds(start, frequency());
    // A frame that never reaches Present or EndFrame holds the next sleep for one bounded wait.
    cpu_event(unpresented + 2, RSF_GAME_CPU_FRAME_BEGIN);
    let start = stamp();
    cpu_event(unpresented + 2, RSF_GAME_CPU_PACING);
    let held = milliseconds(start, frequency());
    println!("pacing join: released {released:.2} ms, bounded wait {held:.2} ms");
    // A requested frame limit puts its own driver sleep into both measurements.
    if options.frame_limit_us == 0 && (released >= 18.0 || !(18.0..200.0).contains(&held)) {
        return Err(1);
    }
    for id in unpresented + 1..=unpresented + 2 {
        if unsafe { rsf_streamline_host_abort(host, id) } != 0 {
            return Err(1);
        }
        cpu_event(id, RSF_GAME_CPU_FRAME_END);
    }
    println!(
        "PASS: an abandoned window releases the next frame's sleep; a frame without completion holds it for one bounded wait"
    );
    options.mode = RSF_FG_OFF;
    unsafe { rsf_native_fg_options_set(&options) };
    if unsafe { rsf_dlss_release_resources() } != 0 || unsafe { rsf_dlss_shutdown() } != 0 {
        return Err(1);
    }
    Ok(())
}

unsafe fn run(argv: &[*const c_char]) -> Result<i32, i32> {
    if !cold_status() {
        return Err(1);
    }
    let argc = argv.len();
    let arg = |i: usize| unsafe { CStr::from_ptr(argv[i]) }.to_bytes();
    if argc == 2 && arg(1) == b"--status-snapshot" {
        return Ok(0);
    }
    if argc == 3 && arg(1) == b"--proxy-smoke" {
        return Ok(unsafe { proxy_smoke(argv[2]) });
    }
    if !matches!(argc, 3 | 4 | 6) {
        println!(
            "SKIP: supply Streamline directory, adapter vendor ID, optional --enable or --switch FSR-directory XeSS-directory"
        );
        return Ok(77);
    }
    let flag = if argc > 3 { arg(3) } else { b"" };
    let switching_visible = argc == 6 && flag == b"--switch-visible";
    // --borderless: X-Ray's borderless sequence (xray-monolith Device_create.cpp:205) on a window whose
    // swap chain the bridge owns. The window must land at the monitor origin, as it does without it.
    let borderless_fixture = argc == 6 && flag == b"--borderless";
    let switching = switching_visible || borderless_fixture || (argc == 6 && flag == b"--switch");
    let pacing = argc == 4 && flag == b"--enable-visible-pacing";
    let ordering_only = argc == 4 && flag == b"--ordering-only";
    let ten_bit = pacing || (argc == 4 && flag == b"--enable-visible-10bit");
    let visible = switching_visible || ten_bit || (argc == 4 && flag == b"--enable-visible");
    let enabled = switching || ordering_only || visible || (argc == 4 && flag == b"--enable");
    let format = if ten_bit {
        DXGI_FORMAT_R10G10B10A2_UNORM
    } else {
        DXGI_FORMAT_R8G8B8A8_UNORM
    };
    unsafe { SetUnhandledExceptionFilter(Some(exception_filter)) };
    // Anomaly runs system-DPI-aware (its window reports awareness 1 at the system DPI); match it.
    if borderless_fixture {
        let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_SYSTEM_AWARE) };
    }
    println!("fixture: creating D3D11 graphics");
    if unsafe { GetEnvironmentVariableA(s!("RSF_TEST_D3D12_DEBUG"), None) } != 0
        && !enable_d3d12_debug_layer()
    {
        return Err(1);
    }
    let (factory, adapter) = adapter::<IDXGIFactory2>(unsafe { number(argv[2]) })?;
    let (device, context) = device11(&adapter).map_err(|_| 77)?;
    let observer = rsf_observer_options {
        struct_size: size_of::<rsf_observer_options>() as u32,
        abi_version: RSF_OBSERVER_ABI_VERSION,
        // AC7's configured filter excludes the facade backbuffer.
        format: DXGI_FORMAT_R16G16_FLOAT.0 as u32,
        capacity: 1,
        ..Default::default()
    };
    if unsafe { rsf_observer_install(&observer) } != 0 {
        return Err(1);
    }
    println!("fixture: D3D11 observer installed");
    let window;
    let hwnd = if borderless_fixture {
        let instance = unsafe { GetModuleHandleW(PCWSTR::null()) }.map_err(|_| 1)?;
        xray_window(HINSTANCE(instance.0))?
    } else {
        window = Window::new(
            w!("RSFPresentBridgeFixture"),
            w!("Presentation bridge fixture"),
            WS_OVERLAPPEDWINDOW,
            1280,
            720,
        )
        .map_err(|_| 1)?;
        window.handle
    };
    if borderless_fixture {
        show(hwnd, false);
    }
    if visible {
        show(hwnd, true);
    }
    let development = arg(1)
        .windows(b"development".len())
        .any(|part| part == b"development");
    let mut setup = rsf_d3d11_present_setup {
        struct_size: size_of::<rsf_d3d11_present_setup>() as u32,
        runtime_directory_utf8: argv[1],
        development_runtime: u32::from(development),
        log: Some(log),
        user: hwnd.0,
        accept_window: Some(accept_window),
        before_present: Some(before_present),
        present_event: Some(present_event),
        latency_event: Some(latency_event),
        retire: Some(rsf_native_fg_retire),
        prepare: Some(rsf_native_fg_prepare),
        debug_timing: 1,
        ui_mode: RSF_UI_MODE_BACKBUFFER_HUDLESS,
        // AC7's conventions and Streamline identity, as its plugin states them.
        depth_inverted: 1,
        depth_infinite: 1,
        units_to_meters: 0.01,
        engine_type: 1,
        engine_version_utf8: c"4.18.3".as_ptr(),
        project_id_utf8: c"a3ed1f08-3542-4698-b85c-e1a9908e861a".as_ptr(),
        ..Default::default()
    };
    if switching {
        setup.backend = RSF_FG_BACKEND_DLSS;
        setup.runtime_switching = 1;
        setup.max_generated_frames = u32::MAX;
        setup.streamline_directory_utf8 = argv[1];
        setup.fsr3_directory_utf8 = argv[4];
        setup.fsr4_directory_utf8 = argv[4];
        setup.xess_directory_utf8 = argv[5];
    }
    if unsafe { rsf_d3d11_present_install(&setup) } == 0 {
        return Err(1);
    }
    println!("fixture: presentation interception installed");
    let desc = DXGI_SWAP_CHAIN_DESC {
        BufferDesc: DXGI_MODE_DESC {
            Width: if borderless_fixture { 640 } else { 1280 },
            Height: if borderless_fixture { 480 } else { 720 },
            Format: format,
            ..Default::default()
        },
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: if borderless_fixture { 2 } else { 1 },
        OutputWindow: hwnd,
        Windowed: BOOL(1),
        SwapEffect: DXGI_SWAP_EFFECT_DISCARD,
        ..Default::default()
    };
    let chain = create_chain(&factory, device.as_raw(), &desc)?;
    if borderless_fixture {
        let result = borderless(&chain, hwnd);
        drop(chain);
        let _ = unsafe { DestroyWindow(hwnd) };
        return Ok(result);
    }
    let host = unsafe { rsf_d3d11_present_host() };
    if host.is_null() {
        if !clear_and_present(&device, &context, &chain, false) {
            return Err(1);
        }
        println!(
            "PASS fallback: original D3D11 buffer, draw and Present after FG startup refusal; FG capability skipped"
        );
        return Ok(77);
    }
    let mut presentation = rsf_streamline_graphics {
        struct_size: size_of::<rsf_streamline_graphics>() as u32,
        ..Default::default()
    };
    let mut transfer = presentation;
    if unsafe { rsf_streamline_host_graphics(host, &mut presentation) } != 0
        || unsafe { rsf_d3d11_present_graphics(&mut transfer) } == 0
        || presentation.queue == transfer.queue
        || presentation.device != transfer.device
    {
        return Err(1);
    }
    println!("PASS: SR/upload use a separate queue on the same presentation device");
    let inputs = create_inputs(&device, &context).ok_or(1)?;
    let mut frame = unsafe { shared_sr(host.cast(), &device, &context, &inputs) }.ok_or(1)?;
    let modern = chain.cast::<IDXGISwapChain4>().map_err(|_| 1)?;
    let identity = chain.cast::<IUnknown>().map_err(|_| 1)?;
    let modern_identity = modern.cast::<IUnknown>().map_err(|_| 1)?;
    let returned = unsafe { chain.GetDevice::<ID3D11Device>() }.map_err(|_| 1)?;
    if modern_identity.as_raw() != identity.as_raw() || returned.as_raw() != device.as_raw() {
        return Err(1);
    }
    if !clear_and_present(&device, &context, &chain, true) {
        return Err(1);
    }
    let (result, observed) = observer_status();
    if !RENDERER_STARTUP_OBSERVED.load(Ordering::Acquire)
        || result != 0
        || observed.frames_presented != 12
        || observed.present_width != 1280
        || observed.present_height != 720
    {
        return Err(1);
    }
    println!(
        "PASS: renderer startup sees the facade's presenting device/output and 12 source Presents"
    );
    if enabled {
        let modes = (switching, pacing, ordering_only, visible);
        run_enabled(
            &chain,
            &device,
            &context,
            hwnd,
            transfer.native_device,
            &mut frame,
            format,
            modes,
        )?;
    } else if !MarkerFixture::run(chain.as_raw()) {
        return Err(1);
    }
    unsafe { context.ClearState() };
    resize(&chain, 1024, 576)?;
    let resized = unsafe { chain.GetBuffer::<ID3D11Texture2D>(0) }.map_err(|_| 1)?;
    let mut size = D3D11_TEXTURE2D_DESC::default();
    unsafe { resized.GetDesc(&mut size) };
    if size.Width != 1024
        || size.Height != 576
        || unsafe { chain.Present(0, DXGI_PRESENT_TEST) }.is_err()
    {
        return Err(1);
    }
    drop((resized, returned, modern_identity, identity, modern));
    // Transfer the original COM reference to the exact owned Release dispatch.
    let mut release = rsf_d3d11_chain_dispatch {
        method: RSF_D3D11_DISPATCH_RELEASE,
        receiver: chain.into_raw(),
        ..Default::default()
    };
    unsafe { rsf_d3d11_chain_dispatch_owned(&mut release) };
    if release.handled == 0
        || unsafe { release.result.references } != 0
        || unsafe { rsf_d3d11_present_has_owner() } != 0
    {
        return Err(1);
    }
    println!(
        "PASS: shared D3D12 DLSS SR and CPU token; D3D11 COM identity/buffers, Off Present, retained-reference resize refusal, resize and retirement"
    );
    Ok(0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_fixture_main(argc: i32, argv: *const *const c_char) -> i32 {
    unsafe { run(args(argc, argv)) }.unwrap_or_else(|status| status)
}
