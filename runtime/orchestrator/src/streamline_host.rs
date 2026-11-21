// SPDX-License-Identifier: MIT
// The first-party D3D12 Streamline host behind rescaleframe/streamline_host.h. Rust owns its
// lifetime, interposer admission, feature routing, frame tokens, marker ordering and Reflex/PCL
// pacing state. The native table holds every typed SDK, COM and Win32 call.
use crate::diagnostic_output::Output;
use crate::native_fg_cpu_abi::rsf_streamline_latency_status as LatencyStatus;
use crate::render_abi::{
    RSF_PACING_NONE, RSF_PACING_REFLEX, RSF_REFLEX_BOOST, RSF_REFLEX_OFF,
    rsf_backend_log_fn as Log, rsf_fg_frame as Frame, rsf_fg_status as Status,
    rsf_fg_swapchain_desc as ChainDesc,
};
use crate::sr_session::has_capacity;
use std::ffi::{CStr, CString, c_char, c_void};
use std::mem::{offset_of, zeroed};
use std::ptr::{NonNull, null_mut};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

mod generation;
#[cfg(test)]
mod tests;

use generation::{Observed, Request};

type Handle = *mut c_void;

const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const LOAD_FAILED: i32 = -4;
const INIT_FAILED: i32 = -6;
const NOT_SUPPORTED: i32 = -7;
const NOT_READY: i32 = -8;
const FEATURE_FAILED: i32 = -9;
const NEEDS_RESTART: i32 = -11;
const WRONG_API: i32 = -12;

const ABI_VERSION: u32 = 2;
const PROFILE_DLSS_FG: u32 = 0;
const PROFILE_PCL: u32 = 2;
const REFLEX_ON: u32 = 1;

// rsf_latency_marker values and the sl::PCLMarker values the native table asserts.
const PRESENT_END: u32 = 5;
const INPUT_SAMPLE: u32 = 6;
const PCL_PING: u32 = 8;
const CONTROLLER_SAMPLE: u32 = 13;
const REQUIRED_MARKERS: u32 = (1 << (PRESENT_END + 1)) - 1;
const CONTROLLER: u32 = 4;
const INPUT_KINDS: u32 = 7;

// sl::MAX_FRAMES_IN_FLIGHT of the pinned SDK, asserted natively.
const TOKEN_SLOTS: usize = 6;

// Features requested from slInit. The native table keeps Streamline's load order.
const FEATURE_DLSS: u32 = 1;
const FEATURE_PCL: u32 = 2;
const FEATURE_REFLEX: u32 = 4;
const FEATURE_GENERATION: u32 = 8;

// Feature functions the native table resolved.
const GET_STATE: u32 = 1;
const SET_OPTIONS: u32 = 2;
const REFLEX_OPTIONS: u32 = 4;
const REFLEX_STATE: u32 = 8;
const SLEEP: u32 = 16;
const PCL_MARKER: u32 = 32;
const PCL_STATE: u32 = 64;
const GENERATION: u32 = GET_STATE | SET_OPTIONS;

// rsf_streamline_host_setup, ABI 2.
#[allow(dead_code)]
#[repr(C)]
struct Setup {
    struct_size: u32,
    abi_version: u32,
    runtime_directory_utf8: *const c_char,
    dxgi_adapter: Handle,
    engine_type: u32,
    engine_version_utf8: *const c_char,
    project_id_utf8: *const c_char,
    require_signature: u32,
    development_runtime: u32,
    log: Log,
    log_user: Handle,
    profile: u32,
    load_dlss_sr: u32,
}

#[repr(C)]
struct Opened {
    native: Handle,
    entries: u32,
    pcl_message: u32,
}

#[repr(C)]
#[derive(Default)]
struct Reflex {
    low_latency_available: u32,
    latency_report_available: u32,
}

#[repr(C)]
struct Platform {
    interposer_loaded: unsafe extern "C" fn() -> u32,
    admit: unsafe extern "C" fn(&Setup) -> i32,
    open: unsafe extern "C" fn(
        &Setup,
        Handle,
        Handle,
        u32,
        *const c_char,
        *const c_char,
        &mut Opened,
    ) -> i32,
    dispose: unsafe extern "C" fn(Handle),
    token: unsafe extern "C" fn(Handle, u32, &mut Handle) -> i32,
    sleep: unsafe extern "C" fn(Handle, Handle) -> i32,
    marker: unsafe extern "C" fn(Handle, Handle, u32) -> i32,
    common: unsafe extern "C" fn(Handle, Handle, u32, *const c_void) -> i32,
    reflex_state: unsafe extern "C" fn(Handle, u32, &[u64; 4], &mut Reflex) -> i32,
    reflex_options: unsafe extern "C" fn(Handle, u32, u32) -> i32,
    pcl_message: unsafe extern "C" fn(Handle, &mut u32) -> i32,
    driver_sleep: unsafe extern "C" fn(Handle),
    ring_capture: unsafe extern "C" fn(Handle) -> u32,
    ring_write: unsafe extern "C" fn(Handle, *const c_char, u64),
    generation_load: unsafe extern "C" fn(Handle, u32, u32, &mut u32) -> i32,
    same_graphics: unsafe extern "C" fn(Handle, Handle, Handle) -> u32,
    support: unsafe extern "C" fn(Handle, Log, Handle) -> i32,
    version: unsafe extern "C" fn(Handle, &mut Status),
    chain: unsafe extern "C" fn(Handle, &ChainDesc, &mut Handle) -> i32,
    options: unsafe extern "C" fn(Handle, &Request) -> i32,
    off: unsafe extern "C" fn(Handle),
    observe: unsafe extern "C" fn(Handle, &mut Observed) -> i32,
    frame: unsafe extern "C" fn(Handle, *const Frame) -> i32,
    command: unsafe extern "C" fn(Handle, Handle) -> i32,
    constants: unsafe extern "C" fn(Handle, Handle, &Frame, u32, u32) -> i32,
    tags: unsafe extern "C" fn(Handle, Handle, Handle, &Frame) -> i32,
}

const _: () = {
    assert!(size_of::<Setup>() == 80);
    assert!(offset_of!(Setup, engine_type) == 24);
    assert!(offset_of!(Setup, engine_version_utf8) == 32);
    assert!(offset_of!(Setup, log) == 56);
    assert!(offset_of!(Setup, profile) == 72);
    assert!(size_of::<Opened>() == 16);
    assert!(size_of::<Reflex>() == 8);
    assert!(size_of::<Platform>() == 26 * size_of::<Handle>());
};

#[derive(Clone, Copy, Default)]
struct Token {
    id: u64,
    handle: Option<NonNull<c_void>>,
    // The index this token was minted with, which the SDK token reports back.
    index: u32,
    common: [bool; 2],
    markers: u32,
    controller_sampled: bool,
    ping_sampled: bool,
    sleep_claimed: bool,
}

// Everything the original marker lock guarded: the token ring, pacing counters and the
// published Reflex/PCL capabilities.
struct Frames {
    tokens: [Token; TOKEN_SLOTS],
    cursor: usize,
    native_index: u32,
    last_begin: u64,
    sleep_calls: u64,
    marker_calls: u64,
    stall_rings: u32,
    baseline_rings: u32,
    reflex_mode: u32,
    frame_limit_us: u32,
    pcl_message: u32,
    low_latency_available: u32,
    latency_report_available: u32,
    // Keyboard, mouse, controller and PCL ping counts for the driver report.
    input_counts: [u64; 4],
    last_report: Option<Instant>,
    last_slow_log: Option<Instant>,
    cached_capability: Status,
}

impl Frames {
    fn new(pcl_message: u32) -> Self {
        Self {
            tokens: [Token::default(); TOKEN_SLOTS],
            cursor: 0,
            native_index: 0,
            last_begin: 0,
            sleep_calls: 0,
            marker_calls: 0,
            stall_rings: 0,
            baseline_rings: 0,
            reflex_mode: RSF_REFLEX_OFF,
            frame_limit_us: 0,
            pcl_message,
            low_latency_available: 0,
            latency_report_available: 0,
            input_counts: [0; 4],
            last_report: None,
            last_slow_log: None,
            cached_capability: unsafe { zeroed() },
        }
    }

    // The in-flight slot carrying `id`. Zero marks an empty slot and never matches.
    fn slot(&self, id: u64) -> Option<usize> {
        if id == 0 {
            return None;
        }
        self.tokens.iter().position(|token| token.id == id)
    }

    fn handle(&self, slot: usize) -> Handle {
        self.tokens[slot].handle.map_or(null_mut(), NonNull::as_ptr)
    }
}

pub(crate) struct Host {
    native: Handle,
    profile: u32,
    engine_type: u32,
    // Borrowed by the SDK from slInit until shutdown.
    engine_version: [c_char; 128],
    project_id: [c_char; 256],
    signature_verified: bool,
    development_runtime: bool,
    log: Log,
    log_user: Handle,
    entries: AtomicU32,
    borrowers: AtomicU32,
    generation_loaded: AtomicBool,
    frames: Mutex<Frames>,
    // Held across the SDK sleep. Option changes share it; Present-side markers do not.
    reflex: Mutex<()>,
}

impl Host {
    fn new(setup: &Setup) -> Self {
        Self {
            native: null_mut(),
            profile: setup.profile,
            engine_type: setup.engine_type,
            engine_version: [0; 128],
            project_id: [0; 256],
            signature_verified: setup.require_signature != 0,
            development_runtime: setup.development_runtime != 0,
            log: setup.log,
            log_user: setup.log_user,
            entries: AtomicU32::new(0),
            borrowers: AtomicU32::new(0),
            generation_loaded: AtomicBool::new(true),
            frames: Mutex::new(Frames::new(0)),
            reflex: Mutex::new(()),
        }
    }

    fn frames(&self) -> MutexGuard<'_, Frames> {
        self.frames.lock().unwrap()
    }

    fn has(&self, entries: u32) -> bool {
        self.entries.load(Relaxed) & entries == entries
    }

    fn say(&self, text: &str) {
        if let (Some(log), Ok(text)) = (self.log, CString::new(text)) {
            unsafe { log(self.log_user, text.as_ptr()) };
        }
    }
}

// Host creation and destruction, and the interposer check in between, are serialized.
static OWNER: Mutex<()> = Mutex::new(());
// The native ring snapshot is one buffer shared by every host.
static RING: Mutex<()> = Mutex::new(());

unsafe fn copy_text<const N: usize>(destination: &mut [c_char; N], source: *const c_char) -> bool {
    if source.is_null() {
        return false;
    }
    let text = unsafe { CStr::from_ptr(source) }.to_bytes_with_nul();
    if text.len() == 1 || text.len() > N {
        return false;
    }
    for (byte, &value) in destination.iter_mut().zip(text) {
        *byte = value as c_char;
    }
    true
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_slhost_create(
    setup: *const Setup,
    device: Handle,
    queue: Handle,
    out: *mut *mut Host,
    platform: &Platform,
) -> i32 {
    if out.is_null() || !unsafe { has_capacity(setup) } {
        return INVALID;
    }
    unsafe { out.write(null_mut()) };
    if Output::active_here() {
        return NOT_READY;
    }
    let setup = unsafe { &*setup };
    if setup.abi_version != ABI_VERSION {
        return ABI_MISMATCH;
    }
    if setup.dxgi_adapter.is_null()
        || setup.runtime_directory_utf8.is_null()
        || setup.profile > PROFILE_PCL
    {
        return INVALID;
    }
    // Engine identity range, then an NVIDIA adapter. Another vendor is a clean refusal.
    let admitted = unsafe { (platform.admit)(setup) };
    if admitted != OK {
        return admitted;
    }
    let _owner = OWNER.lock().unwrap();
    // Never start a second SDK or device registration over an existing interposer.
    if unsafe { (platform.interposer_loaded)() } != 0 {
        return NEEDS_RESTART;
    }
    let raw = crate::allocate_handle(Host::new(setup));
    if raw.is_null() {
        return INIT_FAILED;
    }
    let mut host = unsafe { Box::from_raw(raw) };
    if !unsafe { copy_text(&mut host.engine_version, setup.engine_version_utf8) }
        || !unsafe { copy_text(&mut host.project_id, setup.project_id_utf8) }
    {
        return INVALID;
    }
    // PCL excludes Reflex and DLSS-G, so telemetry has no sleeper, and requests no NGX
    // feature unless shared DLSS SR asks for one.
    let mut features = FEATURE_PCL;
    if setup.profile != PROFILE_PCL || setup.load_dlss_sr != 0 {
        features |= FEATURE_DLSS;
    }
    if setup.profile != PROFILE_PCL {
        features |= FEATURE_REFLEX;
    }
    if setup.profile == PROFILE_DLSS_FG {
        features |= FEATURE_GENERATION;
    }
    let mut opened = Opened {
        native: null_mut(),
        entries: 0,
        pcl_message: 0,
    };
    let result = unsafe {
        (platform.open)(
            setup,
            device,
            queue,
            features,
            host.engine_version.as_ptr(),
            host.project_id.as_ptr(),
            &mut opened,
        )
    };
    if result != OK {
        return result;
    }
    if opened.native.is_null() {
        return INIT_FAILED;
    }
    host.native = opened.native;
    host.entries = AtomicU32::new(opened.entries);
    host.frames = Mutex::new(Frames::new(opened.pcl_message));
    unsafe { out.write(Box::into_raw(host)) };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_slhost_destroy(host: *mut Host, platform: &Platform) -> i32 {
    if Output::active_here() {
        return NOT_READY;
    }
    let Some(owner) = (unsafe { host.as_ref() }) else {
        return INVALID;
    };
    let _owner = OWNER.lock().unwrap();
    if owner.borrowers.load(Relaxed) != 0 || owner.frames().tokens.iter().any(|token| token.id != 0)
    {
        return NOT_READY;
    }
    unsafe { (platform.dispose)(owner.native) };
    drop(unsafe { Box::from_raw(host) });
    OK
}

// The native state of a live host, for the native-only graphics and presentation entries.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_slhost_native(host: *mut Host) -> Handle {
    unsafe { host.as_ref() }.map_or(null_mut(), |host| host.native)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_slhost_token(host: *mut Host, id: u64) -> Handle {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return null_mut();
    };
    let frames = host.frames();
    frames
        .slot(id)
        .map_or(null_mut(), |slot| frames.handle(slot))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_slhost_begin(host: *mut Host, id: u64, platform: &Platform) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return INVALID;
    };
    let result = acquire(host, id, platform);
    if result == OK {
        sleep(host, id, platform)
    } else {
        result
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_slhost_acquire(host: *mut Host, id: u64, platform: &Platform) -> i32 {
    unsafe { host.as_ref() }.map_or(INVALID, |host| acquire(host, id, platform))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_slhost_sleep(host: *mut Host, id: u64, platform: &Platform) -> i32 {
    unsafe { host.as_ref() }.map_or(INVALID, |host| sleep(host, id, platform))
}

fn acquire(host: &Host, id: u64, platform: &Platform) -> i32 {
    if id == 0 {
        return INVALID;
    }
    let mut frames = host.frames();
    let cursor = frames.cursor;
    if id <= frames.last_begin || frames.tokens[cursor].id != 0 {
        return NOT_READY;
    }
    if frames.native_index == u32::MAX {
        return NEEDS_RESTART;
    }
    // SDK token indices stay monotonic even when minting fails.
    frames.native_index += 1;
    let index = frames.native_index;
    let mut handle = null_mut();
    if unsafe { (platform.token)(host.native, index, &mut handle) } != OK {
        return FEATURE_FAILED;
    }
    let Some(handle) = NonNull::new(handle) else {
        return FEATURE_FAILED;
    };
    frames.cursor = (cursor + 1) % TOKEN_SLOTS;
    frames.last_begin = id;
    frames.tokens[cursor] = Token {
        id,
        handle: Some(handle),
        index,
        ..Token::default()
    };
    OK
}

fn milliseconds(span: Duration) -> f64 {
    span.as_secs_f64() * 1000.0
}

fn sleep(host: &Host, id: u64, platform: &Platform) -> i32 {
    if id == 0 {
        return INVALID;
    }
    let entered = Instant::now();
    let mut frames = host.frames();
    let locked = Instant::now();
    let Some(slot) = frames.slot(id) else {
        return NOT_READY;
    };
    let token = &mut frames.tokens[slot];
    if token.sleep_claimed {
        return INVALID;
    }
    let (Some(handle), index) = (token.handle, token.index) else {
        return NOT_READY;
    };
    token.sleep_claimed = true;
    let minted = Instant::now();
    drop(frames);
    let mut slow = None;
    if host.profile != PROFILE_PCL {
        // Present markers for the previous frame must remain able to advance while the
        // current CPU frame sleeps. Only option changes share the sleeper's owner lock.
        let _sleeper = host.reflex.lock().unwrap();
        let sleeping = Instant::now();
        if !host.has(SLEEP) || unsafe { (platform.sleep)(host.native, handle.as_ptr()) } != OK {
            return FEATURE_FAILED;
        }
        let slept = Instant::now();
        let mut frames = host.frames();
        frames.sleep_calls += 1;
        let finished = Instant::now();
        // A requested limit makes long sleeps normal: with generation the driver waits up to
        // the limit times the presents per frame. Only sleeps well beyond that are stalls.
        let expected = Duration::from_micros(u64::from(frames.frame_limit_us) * 5 / 2);
        let threshold = expected.max(Duration::from_millis(8));
        if host.log.is_some()
            && finished - entered >= threshold
            && frames.last_slow_log.is_none_or(|last| {
                finished.saturating_duration_since(last) >= Duration::from_secs(1)
            })
        {
            frames.last_slow_log = Some(finished);
            slow = Some(format!(
                "Reflex slow begin source={id} SDKframe={index} mode={} limit_us={} marker_lock_ms={:.2} token_ms={:.2} options_lock_ms={:.2} sleep_ms={:.2} completion_lock_ms={:.2}",
                frames.reflex_mode,
                frames.frame_limit_us,
                milliseconds(locked - entered),
                milliseconds(minted - locked),
                milliseconds(sleeping - minted),
                milliseconds(slept - sleeping),
                milliseconds(finished - slept),
            ));
        }
    }
    if let Some(text) = slow {
        host.say(&text);
        unsafe { (platform.driver_sleep)(host.native) };
        let ring = {
            let mut frames = host.frames();
            let ring = frames.stall_rings < 8;
            frames.stall_rings += u32::from(ring);
            ring
        };
        if ring {
            latency_ring(host, platform, c"stall", id);
        }
    } else if host.log.is_some() {
        // Unstalled reference for the same screen, about every 2048 frames, plus one at startup.
        let ring = {
            let mut frames = host.frames();
            let calls = frames.sleep_calls;
            let ring = frames.baseline_rings < 10 && ((calls & 2047) == 1024 || calls == 16);
            frames.baseline_rings += u32::from(ring);
            ring
        };
        if ring {
            latency_ring(host, platform, c"baseline", id);
        }
    }
    OK
}

// The driver's own record of its last 64 frames. Bounded; never per frame.
fn latency_ring(host: &Host, platform: &Platform, reason: &CStr, source: u64) {
    if host.log.is_none() || !host.has(REFLEX_STATE) {
        return;
    }
    let _ring = RING.lock().unwrap();
    let captured = {
        let _frames = host.frames();
        unsafe { (platform.ring_capture)(host.native) }
    };
    if captured != 0 {
        unsafe { (platform.ring_write)(host.native, reason.as_ptr(), source) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_slhost_marker(
    host: *mut Host,
    id: u64,
    marker: u32,
    controller: u32,
    platform: &Platform,
) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return INVALID;
    };
    marker_for(host, id, marker, controller, platform)
}

fn marker_for(host: &Host, id: u64, marker: u32, controller: u32, platform: &Platform) -> i32 {
    if id == 0 || marker > INPUT_SAMPLE || controller > 1 {
        return INVALID;
    }
    if !host.has(PCL_MARKER) {
        return NOT_SUPPORTED;
    }
    let mut frames = host.frames();
    let Some(slot) = frames
        .slot(id)
        .filter(|&slot| frames.tokens[slot].handle.is_some())
    else {
        return NOT_READY;
    };
    // PCL removed ordinary input sample marker 6. Marker 13 is for controllers only.
    if marker == INPUT_SAMPLE && controller == 0 {
        return OK;
    }
    let mapped = if marker == INPUT_SAMPLE {
        CONTROLLER_SAMPLE
    } else {
        marker
    };
    let required = marker <= PRESENT_END;
    if required && frames.tokens[slot].markers & (1 << marker) != 0 {
        return INVALID;
    }
    if unsafe { (platform.marker)(host.native, frames.handle(slot), mapped) } != OK {
        return FEATURE_FAILED;
    }
    frames.marker_calls += 1;
    if required {
        let token = &mut frames.tokens[slot];
        token.markers |= 1 << marker;
        // UE's native EndFrame follows Present. Keep the token until every required
        // marker has arrived, including configurations where simulation overlaps render.
        if token.markers == REQUIRED_MARKERS {
            token.id = 0;
        }
    }
    OK
}

// SR's shared constants for this frame's token. Each viewport is set once per frame.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_slhost_common(
    host: *mut Host,
    id: u64,
    viewport: u32,
    constants: *const c_void,
    platform: &Platform,
) -> u32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return 0;
    };
    common_for(host, id, viewport, constants, platform).into()
}

fn common_for(
    host: &Host,
    id: u64,
    viewport: u32,
    constants: *const c_void,
    platform: &Platform,
) -> bool {
    if id == 0 || viewport >= 2 {
        return false;
    }
    let mut frames = host.frames();
    let Some(slot) = frames.slot(id) else {
        return false;
    };
    if frames.tokens[slot].common[viewport as usize]
        || unsafe { (platform.common)(host.native, frames.handle(slot), viewport, constants) } != OK
    {
        return false;
    }
    frames.tokens[slot].common[viewport as usize] = true;
    true
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_slhost_presented(host: *mut Host, id: u64) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return 0;
    };
    let frames = host.frames();
    frames
        .slot(id)
        .is_some_and(|slot| frames.tokens[slot].markers & (1 << PRESENT_END) != 0)
        .into()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_slhost_input(
    host: *mut Host,
    id: u64,
    kinds: u32,
    message: u32,
    platform: &Platform,
) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return INVALID;
    };
    input_for(host, id, kinds, message, platform)
}

fn input_for(host: &Host, id: u64, kinds: u32, message: u32, platform: &Platform) -> i32 {
    if id == 0 || kinds & !INPUT_KINDS != 0 {
        return INVALID;
    }
    let mut guard = host.frames();
    let frames = &mut *guard;
    let Some(slot) = frames.slot(id) else {
        return NOT_READY;
    };
    for (kind, count) in frames.input_counts.iter_mut().take(3).enumerate() {
        if kinds & (1 << kind) != 0 {
            *count += 1;
        }
    }
    let token = frames.handle(slot);
    let emit = |frames: &mut Frames, marker: u32| {
        if !host.has(PCL_MARKER) || unsafe { (platform.marker)(host.native, token, marker) } != OK {
            return false;
        }
        frames.marker_calls += 1;
        true
    };
    if kinds & CONTROLLER != 0 && !frames.tokens[slot].controller_sampled {
        if !emit(&mut *frames, CONTROLLER_SAMPLE) {
            return FEATURE_FAILED;
        }
        frames.tokens[slot].controller_sampled = true;
    }
    if message != 0 && message == frames.pcl_message && !frames.tokens[slot].ping_sampled {
        if !emit(&mut *frames, PCL_PING) {
            return FEATURE_FAILED;
        }
        frames.tokens[slot].ping_sampled = true;
        frames.input_counts[3] += 1;
    }
    // Keyboard/mouse timing uses real dequeue records and registered pings, never deprecated marker 6.
    OK
}

fn abort_for(host: &Host, id: u64) -> i32 {
    if id == 0 {
        return INVALID;
    }
    let mut frames = host.frames();
    if let Some(slot) = frames.slot(id) {
        frames.tokens[slot].id = 0;
    }
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_slhost_abort(host: *mut Host, id: u64) -> i32 {
    unsafe { host.as_ref() }.map_or(INVALID, |host| abort_for(host, id))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_slhost_reset_sources(host: *mut Host) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return INVALID;
    };
    let mut frames = host.frames();
    if frames.tokens.iter().any(|token| token.id != 0) {
        return NOT_READY;
    }
    frames.last_begin = 0;
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_slhost_retire_through(host: *mut Host, id: u64) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return INVALID;
    };
    if id == 0 {
        return INVALID;
    }
    for token in &mut host.frames().tokens {
        if token.id != 0 && token.id <= id {
            token.id = 0;
        }
    }
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_slhost_reflex(
    host: *mut Host,
    mode: u32,
    limit: u32,
    platform: &Platform,
) -> i32 {
    unsafe { host.as_ref() }.map_or(INVALID, |host| reflex(host, mode, limit, platform))
}

fn reflex(host: &Host, mode: u32, limit: u32, platform: &Platform) -> i32 {
    if mode > RSF_REFLEX_BOOST {
        return INVALID;
    }
    // Present can request new options while the next CPU source sleeps.
    // Keep this frame moving and let its caller retry the uncommitted options.
    let Ok(_reflex) = host.reflex.try_lock() else {
        return NOT_READY;
    };
    let mut frames = host.frames();
    if !host.has(REFLEX_OPTIONS) || host.profile == PROFILE_PCL {
        return NOT_SUPPORTED;
    }
    let mut state = Reflex::default();
    if !host.has(REFLEX_STATE)
        || unsafe { (platform.reflex_state)(host.native, 0, &frames.input_counts, &mut state) }
            != OK
    {
        return FEATURE_FAILED;
    }
    if (mode != RSF_REFLEX_OFF || limit != 0) && state.low_latency_available == 0 {
        return NOT_SUPPORTED;
    }
    if unsafe { (platform.reflex_options)(host.native, mode, limit) } != OK {
        return FEATURE_FAILED;
    }
    frames.reflex_mode = mode;
    frames.frame_limit_us = limit;
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_slhost_latency_status(
    host: *mut Host,
    out: *mut LatencyStatus,
    platform: &Platform,
) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return INVALID;
    };
    if !unsafe { has_capacity(out) } {
        return INVALID;
    }
    let out = unsafe { &mut *out };
    // The Present-side overlay reads this while the next CPU frame may be
    // sleeping. Read published capabilities and current counters in that case.
    let reflex = host.reflex.try_lock().ok();
    let mut frames = host.frames();
    *out = unsafe { zeroed() };
    out.struct_size = size_of::<LatencyStatus>() as u32;
    out.profile = host.profile;
    out.pacing_owner = if host.profile == PROFILE_PCL {
        RSF_PACING_NONE
    } else {
        RSF_PACING_REFLEX
    };
    out.reflex_mode = frames.reflex_mode;
    out.frame_limit_us = frames.frame_limit_us;
    out.sleep_calls = frames.sleep_calls;
    out.marker_calls = frames.marker_calls;
    out.last_begin_id = frames.last_begin;
    if reflex.is_some() && host.has(REFLEX_STATE) {
        let mut state = Reflex::default();
        if unsafe { (platform.reflex_state)(host.native, 0, &frames.input_counts, &mut state) }
            != OK
        {
            return FEATURE_FAILED;
        }
        frames.low_latency_available = state.low_latency_available;
        frames.latency_report_available = state.latency_report_available;
    }
    if reflex.is_some() && host.has(PCL_STATE) {
        let mut message = 0;
        if unsafe { (platform.pcl_message)(host.native, &mut message) } != OK {
            return FEATURE_FAILED;
        }
        frames.pcl_message = message;
    }
    out.low_latency_available = frames.low_latency_available;
    out.latency_report_available = frames.latency_report_available;
    out.pcl_message_id = frames.pcl_message;
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_slhost_generation_load(
    host: *mut Host,
    enabled: u32,
    platform: &Platform,
) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return NOT_READY;
    };
    if host.borrowers.load(Relaxed) != 0 {
        return NOT_READY;
    }
    let enabled = enabled != 0;
    if host.generation_loaded.load(Relaxed) == enabled {
        return OK;
    }
    let mut resolved = 0;
    let result = unsafe {
        (platform.generation_load)(
            host.native,
            enabled.into(),
            host.signature_verified.into(),
            &mut resolved,
        )
    };
    // A refused load changed nothing. After it, the old feature functions are gone.
    if result != FEATURE_FAILED {
        host.generation_loaded.store(enabled, Relaxed);
        let entries = host.entries.load(Relaxed);
        host.entries
            .store((entries & !GENERATION) | (resolved & GENERATION), Relaxed);
    }
    result
}
