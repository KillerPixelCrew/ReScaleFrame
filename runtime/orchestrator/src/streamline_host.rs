// SPDX-License-Identifier: MIT
// The first-party D3D12 Streamline host behind rescaleframe/streamline_host.h, formerly
// runtime/backends/dlss/src/dlss_generation.cpp. One owner holds the interposer, its entry
// points, the proxy and native graphics objects, the frame tokens and the Reflex/PCL pacing state.
use crate::diagnostic_output::Output;
use crate::input_receipts::{CONTROLLER_SAMPLE, streamline_input};
use crate::mfg_state::{self, Owner as Mfg};
use crate::reflex_audit::{rsf_reflex_audit_stop, rsf_reflex_driver_sleep};
use crate::sdk_module::SdkModule;
use crate::sr_session::has_capacity;
use crate::streamline_generation_records as records;
use crate::streamline_log;
use crate::streamline_sdk::{self as sdk, Entries};
pub(crate) use crate::sys::rsf_streamline_host_setup as Setup;
use crate::sys::rsf_streamline_latency_status as LatencyStatus;
use crate::sys::{
    RSF_PACING_NONE, RSF_PACING_REFLEX, RSF_REFLEX_BOOST, RSF_REFLEX_OFF,
    rsf_backend_log_fn as Log, rsf_fg_frame as Frame, rsf_fg_status as Status,
};
use rsf_streamline_vk::sdk_api;
use std::ffi::{CStr, CString, c_char, c_void};
use std::mem::{transmute, zeroed};
use std::ptr::{NonNull, null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use windows::Win32::Graphics::Direct3D12::{
    D3D12_FENCE_FLAG_NONE, ID3D12CommandQueue, ID3D12Device, ID3D12Fence,
};
use windows::Win32::Graphics::Dxgi::IDXGIFactory2;
use windows::core::{IUnknown, Interface};
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::CreateEventW;

mod generation;
pub(crate) use generation::rsf_generation_dlss;
mod startup;

use startup::FeatureList;

type Handle = *mut c_void;

const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const LOAD_FAILED: i32 = -4;
const MISSING_ENTRY: i32 = -5;
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

// rsf_latency_marker values, which are the sl::PCLMarker values.
const PRESENT_END: u32 = 5;
const INPUT_SAMPLE: u32 = 6;

const REQUIRED_MARKERS: u32 = (1 << (PRESENT_END + 1)) - 1;

// sl::MAX_FRAMES_IN_FLIGHT of the pinned SDK.
const TOKEN_SLOTS: usize = 6;

const LOG_SLOT: u32 = 1; // RSF_STREAMLINE_LOG_D3D12_HOST
const DRAIN_TIMEOUT_MS: u32 = 10_000;

/// A required core entry. Startup publishes a host only with every one of them resolved.
fn core<T>(entry: Option<T>) -> T {
    entry.expect("required Streamline entry")
}

#[derive(Clone, Copy)]
struct Token {
    id: u64,
    handle: NonNull<c_void>,
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
    tokens: [Option<Token>; TOKEN_SLOTS],
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
            tokens: [None; TOKEN_SLOTS],
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

    fn slot(&self, id: u64) -> Option<usize> {
        self.tokens
            .iter()
            .position(|slot| slot.is_some_and(|token| token.id == id))
    }

    fn token_mut(&mut self, id: u64) -> Option<&mut Token> {
        let slot = self.slot(id)?;
        self.tokens[slot].as_mut()
    }
}

// The DLSS-G feature functions, replaced when the plugin is reloaded.
#[derive(Clone, Copy, Default)]
struct Generation {
    get_state: Option<sdk_api::GetFgState<sdk::DlssgState, sdk::FgOptions>>,
    set_options: Option<sdk_api::SetOptions<sdk::FgOptions>>,
}

pub(crate) struct Host {
    // Borrowed by the SDK from slInit until shutdown, as are the plugin path array and texts.
    directory: Vec<u16>,
    paths: [*const u16; 1],
    engine_version: [c_char; 128],
    project_id: [c_char; 256],
    profile: u32,
    engine_type: u32,
    features: FeatureList,
    signature_verified: bool,
    development_runtime: bool,
    log: Log,
    log_user: Handle,
    initialized: bool,
    core: Entries,
    device: Option<ID3D12Device>,
    native_device: Option<ID3D12Device>,
    queue: Option<ID3D12CommandQueue>,
    native_queue: Option<ID3D12CommandQueue>,
    factory: Option<IDXGIFactory2>,
    generation: Mutex<Generation>,
    reflex_options: Option<sdk_api::Apply<sdk::ReflexOptions>>,
    reflex_state: Option<sdk_api::Read<sdk::ReflexState>>,
    sleep: Option<sdk_api::ReflexSleep>,
    pcl_marker: Option<sdk_api::PclSetMarker>,
    pcl_options: Option<sdk_api::Apply<sdk::PclOptions>>,
    pcl_state: Option<sdk_api::Read<sdk::PclState>>,
    mfg: *mut Mfg,
    borrowers: AtomicU32,
    generation_loaded: AtomicBool,
    frames: Mutex<Frames>,
    common_present: Mutex<Option<[NonNull<c_void>; 2]>>,
    // Held across the SDK sleep. Option changes share it; Present-side markers do not.
    reflex: Mutex<()>,
    // Declared last, so it is unloaded after every other field.
    module: SdkModule,
}

impl Drop for Host {
    fn drop(&mut self) {
        rsf_reflex_audit_stop();
        self.queue = None;
        self.native_queue = None;
        self.factory = None;
        self.device = None;
        self.native_device = None;
        if self.initialized {
            unsafe { core(self.core.shutdown)() };
        }
        unsafe { mfg_state::rsf_core_mfg_release(self.mfg) };
        streamline_log::clear(LOG_SLOT, std::ptr::from_ref(self) as usize);
        unsafe { mfg_state::rsf_core_mfg_destroy(self.mfg) };
    }
}

impl Host {
    fn frames(&self) -> MutexGuard<'_, Frames> {
        self.frames.lock().unwrap()
    }

    fn generation(&self) -> Generation {
        *self.generation.lock().unwrap()
    }

    fn say(&self, text: &str) {
        if let (Some(log), Ok(text)) = (self.log, CString::new(text)) {
            unsafe { log(self.log_user, text.as_ptr()) };
        }
    }

    fn raw<T: Interface>(object: &Option<T>) -> Handle {
        object.as_ref().map_or(null_mut(), Interface::as_raw)
    }

    fn native_device_raw(&self) -> Handle {
        Self::raw(&self.native_device)
    }

    /// One feature function, or None when the SDK refuses or returns null.
    unsafe fn feature(&self, id: u32, name: &CStr) -> Option<Handle> {
        let mut pointer = null_mut();
        let function = core(self.core.get_feature_function);
        let found = unsafe { function(id, name.as_ptr(), &mut pointer) } == sdk::OK;
        (found && !pointer.is_null()).then_some(pointer)
    }

    /// Borrowed native alias, valid through the supplied proxy object's lifetime.
    unsafe fn alias(&self, proxy: Handle) -> Handle {
        let mut native = null_mut();
        let native_interface = core(self.core.get_native_interface);
        if unsafe { native_interface(proxy, &mut native) } != sdk::OK || native.is_null() {
            return null_mut();
        }
        drop(unsafe { IUnknown::from_raw(native) });
        native
    }

    /// Recognizes an already upgraded factory-created interface, else upgrades the owned one.
    unsafe fn upgrade_chain(&self, chain: &mut Handle) -> i32 {
        let native = unsafe { self.alias(*chain) };
        if native.is_null() {
            return FEATURE_FAILED;
        }
        let upgraded = native != *chain;
        if upgraded || unsafe { core(self.core.upgrade_interface)(chain) } == sdk::OK {
            OK
        } else {
            FEATURE_FAILED
        }
    }

    fn report_driver_sleep(&self) {
        unsafe { rsf_reflex_driver_sleep(self.native_device_raw(), self.log, self.log_user) };
    }

    // slReflexGetState: the published capabilities, with an optional driver report.
    fn read_reflex(&self, report: bool, inputs: &[u64; 4]) -> Option<(u32, u32)> {
        let get = self.reflex_state?;
        let mut state = sdk::reflex_state();
        if unsafe { get(&mut *state) } != sdk::OK {
            return None;
        }
        let low_latency = u32::from(state.low_latency_available != 0);
        let report_available = u32::from(state.latency_report_available != 0);
        if report && self.log.is_some() {
            records::reflex_summary(&state, inputs, report_available, self.log, self.log_user);
            self.report_driver_sleep();
        }
        Some((low_latency, report_available))
    }

    fn pcl_message(&self) -> Option<u32> {
        let get = self.pcl_state?;
        let mut state = sdk::pcl_state();
        (unsafe { get(&mut state) } == sdk::OK).then_some(state.window_message)
    }

    fn emit_marker(&self, token: NonNull<c_void>, marker: u32) -> bool {
        self.pcl_marker
            .is_some_and(|set| unsafe { set(marker, token.as_ptr()) } == sdk::OK)
    }
}

// Host creation and destruction, and the interposer check in between, are serialized.
static OWNER: Mutex<()> = Mutex::new(());
// One driver ring snapshot is written at a time by every host.
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

unsafe fn create(setup: *const Setup, graphics: [Handle; 2], out: *mut *mut Host) -> i32 {
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
    match unsafe { startup::create_host(setup, graphics) } {
        Ok(host) => {
            unsafe { out.write(Box::into_raw(host)) };
            OK
        }
        Err(result) => result,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_create(setup: *const Setup, out: *mut *mut Host) -> i32 {
    unsafe { create(setup, [null_mut(); 2], out) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_adopt(
    setup: *const Setup,
    device: Handle,
    queue: Handle,
    out: *mut *mut Host,
) -> i32 {
    if device.is_null() || queue.is_null() {
        return INVALID;
    }
    unsafe { create(setup, [device, queue], out) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_destroy(host: *mut Host) -> i32 {
    if Output::active_here() {
        return NOT_READY;
    }
    let Some(owner) = (unsafe { host.as_ref() }) else {
        return INVALID;
    };
    let Ok(_owner) = OWNER.try_lock() else {
        return NOT_READY;
    };
    if owner.borrowers.load(Relaxed) != 0 || owner.frames().tokens.iter().any(Option::is_some) {
        return NOT_READY;
    }
    drop(unsafe { Box::from_raw(host) });
    OK
}

use crate::sys::rsf_streamline_graphics as Graphics;

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_streamline_host_graphics(
    host: *mut Host,
    out: *mut Graphics,
) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return INVALID;
    };
    if !unsafe { has_capacity(out) } {
        return INVALID;
    }
    let out = unsafe { &mut *out };
    out.device = Host::raw(&host.device);
    out.queue = Host::raw(&host.queue);
    out.factory = Host::raw(&host.factory);
    out.native_device = Host::raw(&host.native_device);
    out.native_queue = Host::raw(&host.native_queue);
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_module(host: *mut Host) -> Handle {
    unsafe { host.as_ref() }.map_or(null_mut(), |host| host.module.as_ptr())
}

/// What an SR route borrows from a live host: its interposer, entry table and native device.
pub(crate) struct Shared {
    pub(crate) module: Handle,
    pub(crate) entries: Entries,
    pub(crate) native_device: Handle,
}

/// The session a live host lends to DLSS SR. None for a null host.
pub(crate) unsafe fn shared(host: *mut Host) -> Option<Shared> {
    let host = unsafe { host.as_ref() }?;
    Some(Shared {
        module: host.module.as_ptr(),
        entries: host.core,
        native_device: host.native_device_raw(),
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_drain(host: *mut Host) -> i32 {
    unsafe { drain(host) }
}

/// Waits for the host's proxy queue to finish its submitted work.
pub(crate) unsafe fn drain(host: *mut Host) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return INVALID;
    };
    let (Some(queue), Some(device)) = (host.queue.as_ref(), host.device.as_ref()) else {
        return INVALID;
    };
    let Ok(fence) = (unsafe { device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) }) else {
        return FEATURE_FAILED;
    };
    let event = unsafe { CreateEventW(null(), 0, 0, null()) };
    if event.is_null() {
        return FEATURE_FAILED;
    }
    let ready = unsafe { queue.Signal(&fence, 1) }.is_ok()
        && unsafe { crate::d3d12_fence::wait_fence(fence.as_raw(), 1, event, DRAIN_TIMEOUT_MS) }
            >= 0;
    unsafe { CloseHandle(event) };
    if ready { OK } else { FEATURE_FAILED }
}

// Only the presentation owner calls this around physical Presents that bypass the proxy.
// No lock spans the vendor lookup or either hook.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_streamline_host_present_common(
    host: *mut Host,
    chain: Handle,
    interval: u32,
    flags: u32,
    begin: u32,
) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return INVALID;
    };
    if chain.is_null() || begin > 1 {
        return INVALID;
    }
    let cached = *host.common_present.lock().unwrap();
    let [before, after] = if let Some(entries) = cached {
        entries
    } else {
        let hooks = unsafe {
            [
                host.feature(sdk::FEATURE_COMMON, c"slHookPresent"),
                host.feature(sdk::FEATURE_COMMON, c"slHookAfterPresent"),
            ]
        };
        let [Some(before), Some(after)] = hooks.map(|hook| hook.and_then(NonNull::new)) else {
            return MISSING_ENTRY;
        };
        let entries = [before, after];
        *host.common_present.lock().unwrap() = Some(entries);
        entries
    };
    let ok = if begin != 0 {
        let hook = unsafe { transmute::<Handle, sdk::HookPresent>(before.as_ptr()) };
        let mut skip = false;
        (unsafe { hook(chain, interval, flags, &mut skip) }) >= 0 && !skip
    } else {
        let hook = unsafe { transmute::<Handle, sdk::HookAfterPresent>(after.as_ptr()) };
        (unsafe { hook(flags) }) >= 0
    };
    if ok { OK } else { FEATURE_FAILED }
}

/// The SDK token of source frame `id`, or null.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_streamline_host_token(host: *mut Host, id: u64) -> Handle {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return null_mut();
    };
    host.frames()
        .tokens
        .iter()
        .flatten()
        .find(|token| token.id == id)
        .map_or(null_mut(), |token| token.handle.as_ptr())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_begin(host: *mut Host, id: u64) -> i32 {
    unsafe { host.as_ref() }.map_or(INVALID, |host| begin(host, id))
}

fn begin(host: &Host, id: u64) -> i32 {
    let acquired = acquire(host, id);
    if acquired != OK {
        return acquired;
    }
    let result = sleep(host, id);
    if result != OK {
        abort(host, id);
    }
    result
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_streamline_host_acquire(host: *mut Host, id: u64) -> i32 {
    unsafe { host.as_ref() }.map_or(INVALID, |host| acquire(host, id))
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_streamline_host_sleep(host: *mut Host, id: u64) -> i32 {
    unsafe { host.as_ref() }.map_or(INVALID, |host| sleep(host, id))
}

fn acquire(host: &Host, id: u64) -> i32 {
    if id == 0 {
        return INVALID;
    }
    let mut frames = host.frames();
    let cursor = frames.cursor;
    if id <= frames.last_begin || frames.tokens[cursor].is_some() {
        return NOT_READY;
    }
    if frames.native_index == u32::MAX {
        return NEEDS_RESTART;
    }
    // SDK token indices stay monotonic even when minting fails.
    frames.native_index += 1;
    let index = frames.native_index;
    let mut token = null();
    let new_token = core(host.core.get_new_frame_token);
    if unsafe { new_token(&mut token, &index) } != sdk::OK {
        return FEATURE_FAILED;
    }
    let Some(handle) = NonNull::new(token.cast_mut()) else {
        return FEATURE_FAILED;
    };
    frames.cursor = (cursor + 1) % TOKEN_SLOTS;
    frames.last_begin = id;
    frames.tokens[cursor] = Some(Token {
        id,
        handle,
        index,
        common: [false; 2],
        markers: 0,
        controller_sampled: false,
        ping_sampled: false,
        sleep_claimed: false,
    });
    OK
}

fn milliseconds(span: Duration) -> f64 {
    span.as_secs_f64() * 1000.0
}

fn sleep(host: &Host, id: u64) -> i32 {
    if id == 0 {
        return INVALID;
    }
    let entered = Instant::now();
    let mut frames = host.frames();
    let locked = Instant::now();
    let Some(token) = frames.token_mut(id) else {
        return NOT_READY;
    };
    if token.sleep_claimed {
        return INVALID;
    }
    let (handle, index) = (token.handle, token.index);
    token.sleep_claimed = true;
    let minted = Instant::now();
    drop(frames);
    let mut slow = None;
    if host.profile != PROFILE_PCL {
        // Present markers for the previous frame must remain able to advance while the
        // current CPU frame sleeps. Only option changes share the sleeper's owner lock.
        let _sleeper = host.reflex.lock().unwrap();
        let sleeping = Instant::now();
        if !host
            .sleep
            .is_some_and(|sleep| unsafe { sleep(handle.as_ptr()) } == sdk::OK)
        {
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
        host.report_driver_sleep();
        let ring = {
            let mut frames = host.frames();
            let ring = frames.stall_rings < 8;
            frames.stall_rings += u32::from(ring);
            ring
        };
        if ring {
            latency_ring(host, c"stall", id);
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
            latency_ring(host, c"baseline", id);
        }
    }
    OK
}

// The driver's own record of its last 64 frames: marker times as it received them and its
// queue and GPU spans. Written when a sleep stalls, and a few times while it does not, to show
// what the driver saw before it chose its interval. Bounded; never per frame.
fn latency_ring(host: &Host, reason: &CStr, source: u64) {
    let Some(get) = host.reflex_state.filter(|_| host.log.is_some()) else {
        return;
    };
    let _ring = RING.lock().unwrap();
    let mut state = sdk::reflex_state();
    let captured = {
        let _frames = host.frames();
        (unsafe { get(&mut *state) }) == sdk::OK && state.latency_report_available != 0
    };
    if captured {
        records::reflex_ring(&state, reason, source, host.log, host.log_user);
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_streamline_host_marker(
    host: *mut Host,
    id: u64,
    marker: u32,
    controller: u32,
) -> i32 {
    unsafe { host.as_ref() }.map_or(INVALID, |host| marker_for(host, id, marker, controller))
}

fn marker_for(host: &Host, id: u64, marker: u32, controller: u32) -> i32 {
    if id == 0 || marker > INPUT_SAMPLE || controller > 1 {
        return INVALID;
    }
    if host.pcl_marker.is_none() {
        return NOT_SUPPORTED;
    }
    let mut frames = host.frames();
    let Some(slot) = frames.slot(id) else {
        return NOT_READY;
    };
    let token = frames.tokens[slot].as_mut().unwrap();
    // PCL removed ordinary input sample marker 6. Marker 13 is for controllers only.
    if marker == INPUT_SAMPLE {
        if controller == 0 {
            return OK;
        }
        return controller_sample(host, &mut frames, slot);
    }
    if token.markers & (1 << marker) != 0 {
        return INVALID;
    }
    if !host.emit_marker(token.handle, marker) {
        return FEATURE_FAILED;
    }
    token.markers |= 1 << marker;
    // UE's native EndFrame follows Present. Keep the token until every required
    // marker has arrived, including configurations where simulation overlaps render.
    if token.markers == REQUIRED_MARKERS {
        frames.tokens[slot].take();
    }
    frames.marker_calls += 1;
    OK
}

fn controller_sample(host: &Host, frames: &mut Frames, slot: usize) -> i32 {
    let token = frames.tokens[slot].as_mut().unwrap();
    if token.controller_sampled {
        return OK;
    }
    if !host.emit_marker(token.handle, CONTROLLER_SAMPLE) {
        return FEATURE_FAILED;
    }
    token.controller_sampled = true;
    frames.marker_calls += 1;
    OK
}

/// Shared D3D11 DLSS SR sets this frame's common constants. Each viewport is set once per frame.
pub(crate) unsafe fn common_set(
    host: *mut Host,
    id: u64,
    viewport: u32,
    constants: &sdk::Constants,
) -> bool {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return false;
    };
    if id == 0 || viewport >= 2 {
        return false;
    }
    let mut frames = host.frames();
    let Some(token) = frames.token_mut(id) else {
        return false;
    };
    let slot = sdk::viewport(viewport);
    let set_constants = core(host.core.set_constants);
    if token.common[viewport as usize]
        || unsafe { set_constants(constants, token.handle.as_ptr(), &slot) } != sdk::OK
    {
        return false;
    }
    token.common[viewport as usize] = true;
    true
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_streamline_host_presented(host: *mut Host, id: u64) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return 0;
    };
    host.frames()
        .tokens
        .iter()
        .flatten()
        .any(|token| token.id == id && token.markers & (1 << PRESENT_END) != 0)
        .into()
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_streamline_host_input(
    host: *mut Host,
    id: u64,
    kinds: u32,
    message: u32,
) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return INVALID;
    };
    if id == 0 {
        return INVALID;
    }
    let mut guard = host.frames();
    let frames = &mut *guard;
    // The host API accepts physical input independently of an unrelated message.
    let Some(input) = streamline_input(kinds, message, frames.pcl_message) else {
        return INVALID;
    };
    let Some(slot) = frames.slot(id) else {
        return NOT_READY;
    };
    for (kind, count) in frames.input_counts.iter_mut().take(3).enumerate() {
        if kinds & (1 << kind) != 0 {
            *count += 1;
        }
    }
    if input.controller.is_some() {
        let result = controller_sample(host, frames, slot);
        if result != OK {
            return result;
        }
    }
    let token = frames.tokens[slot].as_mut().unwrap();
    if let Some(marker) = input.ping
        && !token.ping_sampled
    {
        if !host.emit_marker(token.handle, marker) {
            return FEATURE_FAILED;
        }
        token.ping_sampled = true;
        frames.marker_calls += 1;
        frames.input_counts[3] += 1;
    }
    // Keyboard/mouse timing uses real dequeue records and registered pings, never deprecated marker 6.
    OK
}

fn abort(host: &Host, id: u64) -> i32 {
    if id == 0 {
        return INVALID;
    }
    let mut frames = host.frames();
    if let Some(slot) = frames.slot(id) {
        frames.tokens[slot].take();
    }
    OK
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_streamline_host_abort(host: *mut Host, id: u64) -> i32 {
    unsafe { host.as_ref() }.map_or(INVALID, |host| abort(host, id))
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_streamline_host_reset_sources(host: *mut Host) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return INVALID;
    };
    let mut frames = host.frames();
    if frames.tokens.iter().any(Option::is_some) {
        return NOT_READY;
    }
    frames.last_begin = 0;
    OK
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_streamline_host_retire_through(
    host: *mut Host,
    id: u64,
) -> i32 {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return INVALID;
    };
    if id == 0 {
        return INVALID;
    }
    for token in &mut host.frames().tokens {
        if token.is_some_and(|entry| entry.id <= id) {
            token.take();
        }
    }
    OK
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_streamline_host_reflex(
    host: *mut Host,
    mode: u32,
    limit: u32,
) -> i32 {
    unsafe { host.as_ref() }.map_or(INVALID, |host| reflex(host, mode, limit))
}

fn reflex(host: &Host, mode: u32, limit: u32) -> i32 {
    if mode > RSF_REFLEX_BOOST {
        return INVALID;
    }
    // Present can request new options while the next CPU source sleeps.
    // Keep this frame moving and let its caller retry the uncommitted options.
    let Ok(_reflex) = host.reflex.try_lock() else {
        return NOT_READY;
    };
    let mut frames = host.frames();
    let Some(set) = host.reflex_options.filter(|_| host.profile != PROFILE_PCL) else {
        return NOT_SUPPORTED;
    };
    let Some((low_latency, _)) = host.read_reflex(false, &frames.input_counts) else {
        return FEATURE_FAILED;
    };
    if (mode != RSF_REFLEX_OFF || limit != 0) && low_latency == 0 {
        return NOT_SUPPORTED;
    }
    let options = records::pack_reflex(mode, limit);
    if unsafe { set(&options) } != sdk::OK {
        return FEATURE_FAILED;
    }
    if host.log.is_some() {
        records::report_reflex_options(
            mode,
            limit,
            options.optimize_markers.into(),
            host.log,
            host.log_user,
        );
        host.report_driver_sleep();
    }
    frames.reflex_mode = mode;
    frames.frame_limit_us = limit;
    OK
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_streamline_host_latency_status(
    host: *mut Host,
    out: *mut LatencyStatus,
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
    if reflex.is_some() && host.reflex_state.is_some() {
        let Some((low_latency, report)) = host.read_reflex(false, &frames.input_counts) else {
            return FEATURE_FAILED;
        };
        frames.low_latency_available = low_latency;
        frames.latency_report_available = report;
    }
    if reflex.is_some() && host.pcl_state.is_some() {
        let Some(message) = host.pcl_message() else {
            return FEATURE_FAILED;
        };
        frames.pcl_message = message;
    }
    out.low_latency_available = frames.low_latency_available;
    out.latency_report_available = frames.latency_report_available;
    out.pcl_message_id = frames.pcl_message;
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_upgrade_chain(host: *mut Host, chain: *mut Handle) -> i32 {
    let Some(chain) = (unsafe { chain.as_mut() }).filter(|chain| !chain.is_null()) else {
        return INVALID;
    };
    let Some(host) = (unsafe { host.as_ref() }) else {
        return INVALID;
    };
    unsafe { host.upgrade_chain(chain) }
}

// Present owner only, with all graphics callers quiescent. FEATURE_FAILED means the plugin
// state did not change; after any other result the old feature functions are gone.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_streamline_host_generation_load(
    host: *mut Host,
    enabled: u32,
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
    if enabled {
        unsafe { startup::prime(host, host.native_device_raw()) };
    }
    let loaded = core(host.core.set_feature_loaded);
    if unsafe { loaded(sdk::FEATURE_DLSS_G, enabled) } != sdk::OK {
        return FEATURE_FAILED;
    }
    host.generation_loaded.store(enabled, Relaxed);
    let mut generation = Generation::default();
    // Set options is looked up only after get state resolved.
    let found = !enabled
        || unsafe {
            generation.get_state = host
                .feature(sdk::FEATURE_DLSS_G, c"slDLSSGGetState")
                .and_then(|entry| sdk_api::function(entry));
            generation.get_state.is_some() && {
                generation.set_options = host
                    .feature(sdk::FEATURE_DLSS_G, c"slDLSSGSetOptions")
                    .and_then(|entry| sdk_api::function(entry));
                generation.set_options.is_some()
            }
        };
    *host.generation.lock().unwrap() = generation;
    if !found {
        return MISSING_ENTRY;
    }
    if enabled {
        unsafe { startup::prepare_mfg(host) };
    }
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_streamline_host_native(host: *mut Host, proxy: Handle) -> Handle {
    if proxy.is_null() {
        return null_mut();
    }
    unsafe { host.as_ref() }.map_or(null_mut(), |host| unsafe { host.alias(proxy) })
}
