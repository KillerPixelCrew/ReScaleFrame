// SPDX-License-Identifier: MIT
use crate::render_abi::{
    RSF_FG_ABI_VERSION, RSF_FG_DYNAMIC, RSF_FG_OFF, RSF_REFLEX_BOOST, rsf_backend_log_fn,
    rsf_fg_frame as Frame, rsf_fg_options as Options, rsf_fg_retirement as Retirement,
    rsf_fg_status as Status, rsf_generation_provider as Provider, rsf_generation_setup as Setup,
};
use crate::sdk_module::SdkModule;
use crate::sr_session::has_capacity;
use rsf_game_sys::rsf_frame_record as Record;
use std::ffi::{CString, c_char, c_void};
use std::fmt::Arguments;
use std::ptr::null_mut;
use std::sync::Mutex;

mod admission;
mod disabled;

use disabled::DISABLED_PROVIDER;
mod fsr;
mod xess;

fn chain_description(
    desc: &crate::render_abi::rsf_fg_swapchain_desc,
) -> windows::Win32::Graphics::Dxgi::DXGI_SWAP_CHAIN_DESC1 {
    use windows::Win32::Graphics::Dxgi::{Common::*, *};
    DXGI_SWAP_CHAIN_DESC1 {
        Width: desc.width,
        Height: desc.height,
        Format: DXGI_FORMAT(desc.format as i32),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: desc.buffer_count,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        Flags: if desc.allow_tearing != 0 {
            DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING.0 as u32
        } else {
            0
        },
        ..Default::default()
    }
}

type Raw = *mut c_void;

const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const LOAD_FAILED: i32 = -4;
const INIT_FAILED: i32 = -6;
const UNSUPPORTED: i32 = -7;
const NOT_READY: i32 = -8;
const FEATURE_FAILED: i32 = -9;
const STALE: i32 = -10;
const NEEDS_RESTART: i32 = -11;

const UI_NONE: u32 = 1;
const UI_LAYER: u32 = 2;
const HUDLESS: u32 = 4;
const HUDLESS_UI: u32 = 8;

#[repr(C)]
struct Checks {
    setup: unsafe extern "C" fn(*const Setup, *mut Raw, *mut Raw) -> i32,
    frame: unsafe extern "C" fn(*const Frame, Raw) -> i32,
    command: unsafe extern "C" fn(Raw, Raw) -> i32,
    streamline_loaded: unsafe extern "C" fn() -> u32,
    load_library: unsafe extern "C" fn(&Setup, *const u16) -> Raw,
}

impl Checks {
    fn preflight(&self, setup: &Setup, supported: bool) -> Result<(), i32> {
        if setup.streamline_host.is_null() && unsafe { (self.streamline_loaded)() } != 0 {
            return Err(NEEDS_RESTART);
        }
        if !supported {
            return Err(UNSUPPORTED);
        }
        Ok(())
    }

    fn module(&self, setup: &Setup, name: &str) -> Option<SdkModule> {
        let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
        SdkModule::adopt_system(unsafe { (self.load_library)(setup, name.as_ptr()) })
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Extent {
    width: u32,
    height: u32,
    format: u32,
}

const _: () = {
    assert!(size_of::<Checks>() == 5 * size_of::<Raw>());
    assert!(size_of::<Extent>() == 12);
};

struct Bindings {
    raw: Raw,
    close: unsafe extern "C" fn(Raw),
    module: SdkModule,
    _latency: Option<SdkModule>,
}

type Resolve = unsafe extern "C" fn(&Setup, Raw, Raw, &mut Raw) -> i32;

impl Bindings {
    fn open(
        setup: &Setup,
        checks: &Checks,
        primary: &str,
        latency: Option<&str>,
        resolve: Resolve,
        close: unsafe extern "C" fn(Raw),
    ) -> Result<Self, i32> {
        let module = checks.module(setup, primary);
        let latency = match latency {
            Some(name) => Some(checks.module(setup, name).ok_or(LOAD_FAILED)?),
            None => None,
        };
        let module = module.ok_or(LOAD_FAILED)?;
        let mut raw = null_mut();
        let result = unsafe {
            resolve(
                setup,
                module.as_ptr(),
                latency.as_ref().map_or(null_mut(), SdkModule::as_ptr),
                &mut raw,
            )
        };
        if result != OK {
            if !raw.is_null() {
                unsafe { close(raw) };
            }
            return Err(result);
        }
        if raw.is_null() {
            return Err(INIT_FAILED);
        }
        Ok(Self {
            raw,
            close,
            module,
            _latency: latency,
        })
    }
}

// Old entries can be overwritten. A lookup must match the entire key, since
// an old callback can arrive after the slot is reused.
struct Recent<T, const N: usize>([Option<(u64, T)>; N]);

impl<T: Copy, const N: usize> Recent<T, N> {
    const fn new() -> Self {
        Self([None; N])
    }

    fn slot(&mut self, key: u64) -> &mut Option<(u64, T)> {
        &mut self.0[(key % N as u64) as usize]
    }

    fn insert(&mut self, key: u64, value: T) {
        *self.slot(key) = Some((key, value));
    }

    fn get(&mut self, key: u64) -> Option<&mut T> {
        let (saved, value) = self.slot(key).as_mut()?;
        (*saved == key).then_some(value)
    }
}

impl Drop for Bindings {
    fn drop(&mut self) {
        unsafe { (self.close)(self.raw) };
    }
}

#[derive(Clone, Copy)]
struct Log {
    callback: rsf_backend_log_fn,
    user: Raw,
}

impl Log {
    fn new(setup: &Setup) -> Self {
        Self {
            callback: setup.chain.log,
            user: setup.chain.log_user,
        }
    }

    fn write(self, message: Arguments<'_>) {
        if let Some(log) = self.callback
            && let Ok(message) = CString::new(message.to_string())
        {
            unsafe { log(self.user, message.as_ptr()) };
        }
    }
}

fn version_name(status: &mut Status, name: &str) {
    for (dst, &byte) in status.version_name.iter_mut().zip(name.as_bytes()) {
        *dst = byte as c_char;
    }
}

// Copyable prepare ordering shared by the D3D12 providers. The caller owns the
// mutex and publishes provider-specific observations separately.
#[derive(Clone, Copy, Default)]
pub(crate) struct PrepareOrder {
    pub(crate) revision: u64,
    pub(crate) last_prepare: u64,
    pub(crate) interpolated: Option<u64>,
}

impl PrepareOrder {
    pub(crate) fn reset_for(self, record: &Record) -> bool {
        record.flags & rsf_game_sys::RSF_FRAME_FLAG_RESET != 0
            || self.interpolated.and_then(|id| id.checked_add(1)) != Some(record.frame_id)
    }

    pub(crate) fn stage(&mut self, revision: u64, id: u64) -> bool {
        if revision != self.revision || id <= self.last_prepare {
            return false;
        }
        self.interpolated = None;
        true
    }

    pub(crate) fn complete(&mut self, revision: u64, id: u64, enabled: bool) -> bool {
        // A successfully submitted source is consumed even when abort won the SDK gap.
        self.last_prepare = self.last_prepare.max(id);
        if revision != self.revision {
            return false;
        }
        self.interpolated = enabled.then_some(id);
        true
    }

    pub(crate) fn invalidate(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.interpolated = None;
    }

    pub(crate) fn active(self, revision: u64, sdk_active: bool) -> bool {
        revision == self.revision && self.interpolated.is_some() && sdk_active
    }
}

#[derive(Clone, Copy)]
struct Snapshot {
    status: Status,
    request: Option<Options>,
    order: PrepareOrder,
}

impl Snapshot {
    fn mode(self) -> u32 {
        self.request.map_or(RSF_FG_OFF, |options| options.mode)
    }

    fn effective(&mut self, mode: u32, generated: u32) {
        self.status.effective_mode = mode;
        self.status.effective_generated_frames = generated;
    }
}

#[derive(Default)]
struct Observation {
    vendor_status: i32,
    statistics: u32,
    callbacks: Option<(u64, u64)>,
    presented: u64,
    active: bool,
}

// This lock protects only copyable CPU state. SDK, COM and logging calls stay
// outside it. An abort can invalidate either a prepare or a status poll in flight.
struct State(Mutex<Snapshot>);

impl State {
    fn new(status: Status) -> Self {
        Self(Mutex::new(Snapshot {
            status,
            request: None,
            order: PrepareOrder::default(),
        }))
    }

    fn read(&self) -> Snapshot {
        *self.0.lock().unwrap()
    }

    fn configured(&self, options: Options, mode: u32, generated: u32, reflex: u32, reset: bool) {
        let mut state = self.0.lock().unwrap();
        state.request = Some(options);
        state.status.configured_mode = options.mode;
        state.status.effective_reflex = reflex;
        state.effective(mode, generated);
        if reset {
            state.order.invalidate();
            state.status.active = 0;
        }
    }

    fn reset_history(&self, revision: u64) -> bool {
        let mut state = self.0.lock().unwrap();
        if revision != state.order.revision {
            return false;
        }
        state.order.interpolated = None;
        state.status.active = 0;
        true
    }

    fn stage(&self, revision: u64, id: u64, mode: u32, generated: u32) -> bool {
        let mut state = self.0.lock().unwrap();
        if !state.order.stage(revision, id) {
            return false;
        }
        state.effective(mode, generated);
        state.status.active = 0;
        true
    }

    fn complete(&self, revision: u64, id: u64, enabled: bool, mode: u32, generated: u32) -> bool {
        let mut state = self.0.lock().unwrap();
        if !state.order.complete(revision, id, enabled) {
            return false;
        }
        state.effective(mode, generated);
        state.status.active = 0;
        true
    }

    fn abort(&self) {
        let mut state = self.0.lock().unwrap();
        state.order.invalidate();
        state.status.active = 0;
    }

    fn paced(&self) {
        let mut state = self.0.lock().unwrap();
        state.status.latency_paced_frames = state.status.latency_paced_frames.wrapping_add(1);
    }

    fn observe(&self, revision: u64, sample: Observation) -> Status {
        let mut state = self.0.lock().unwrap();
        let active = state.order.active(revision, sample.active);
        let status = &mut state.status;
        status.vendor_status = sample.vendor_status;
        if let Some((generated, presented)) = sample.callbacks {
            status.generated_callbacks = generated;
            status.present_callbacks = presented;
            status.valid_statistics = sample.statistics;
        } else {
            status.valid_statistics |= sample.statistics;
        }
        status.total_presented = status.total_presented.wrapping_add(sample.presented);
        status.active = u32::from(active);
        *status
    }
}

fn initial_status(max_frames: u32) -> Status {
    let mut status: Status = unsafe { std::mem::zeroed() };
    status.struct_size = size_of::<Status>() as u32;
    status.supported = 1;
    status.max_generated_frames = max_frames;
    status.vsync_supported = 1;
    status
}

pub(crate) unsafe fn read_options(options: *const Options) -> Result<Options, i32> {
    if !unsafe { has_capacity(options) } {
        return Err(INVALID);
    }
    let options = unsafe { *options };
    if options.abi_version != RSF_FG_ABI_VERSION {
        return Err(ABI_MISMATCH);
    }
    if options.mode > RSF_FG_DYNAMIC
        || options.reflex_mode > RSF_REFLEX_BOOST
        || !options.dynamic_target_fps.is_finite()
        || options.dynamic_target_fps < 0.0
    {
        return Err(INVALID);
    }
    Ok(options)
}

unsafe fn read_frame<'a>(
    check: &Checks,
    frame: *const Frame,
    device: Raw,
) -> Result<(&'a Frame, &'a Record), i32> {
    let result = unsafe { (check.frame)(frame, device) };
    if result != OK {
        return Err(result);
    }
    let frame = unsafe { &*frame };
    Ok((frame, unsafe { &*frame.record }))
}

trait Session {
    fn state(&self) -> &State;
    fn configure(&self, options: Options) -> i32;
    fn begin(&self, _id: u64) -> i32 {
        OK
    }
    fn marker(&self, _marker: u32, _id: u64) -> i32 {
        OK
    }
    unsafe fn prepare(&self, command: Raw, frame: *const Frame) -> i32;
    fn after(&self) -> i32;
    // False means the SDK can still access its context or callback userdata.
    fn retire(&self) -> bool;
}

unsafe extern "C" fn configure<S: Session>(session: Raw, options: *const Options) -> i32 {
    let Some(session) = (unsafe { session.cast::<S>().as_ref() }) else {
        return INVALID;
    };
    match unsafe { read_options(options) } {
        Ok(options) => session.configure(options),
        Err(result) => result,
    }
}

unsafe extern "C" fn begin<S: Session>(session: Raw, id: u64) -> i32 {
    match unsafe { session.cast::<S>().as_ref() } {
        Some(session) if id != 0 => session.begin(id),
        _ => INVALID,
    }
}

unsafe extern "C" fn marker<S: Session>(session: Raw, marker: u32, id: u64, _: u32) -> i32 {
    match unsafe { session.cast::<S>().as_ref() } {
        Some(session) if id != 0 => session.marker(marker, id),
        _ => INVALID,
    }
}

unsafe extern "C" fn prepare<S: Session>(session: Raw, command: Raw, frame: *const Frame) -> i32 {
    match unsafe { session.cast::<S>().as_ref() } {
        Some(session) => unsafe { session.prepare(command, frame) },
        None => INVALID,
    }
}

unsafe extern "C" fn after<S: Session>(session: Raw) -> i32 {
    unsafe { session.cast::<S>().as_ref() }.map_or(INVALID, Session::after)
}

unsafe extern "C" fn status<S: Session>(session: Raw, out: *mut Status) -> i32 {
    let Some(session) = (unsafe { session.cast::<S>().as_ref() }) else {
        return INVALID;
    };
    if !unsafe { has_capacity(out) } {
        return INVALID;
    }
    unsafe { out.write(session.state().read().status) };
    OK
}

unsafe extern "C" fn retirement(session: Raw, out: *mut Retirement) -> i32 {
    if session.is_null() || !unsafe { has_capacity(out) } {
        return INVALID;
    }
    // These providers use the game queue for source reads. The caller keeps
    // its submission fence; the provider adds no further per-frame wait.
    unsafe {
        (*out).fence = null_mut();
        (*out).value = 0;
    }
    OK
}

unsafe extern "C" fn destroy<S: Session>(session: Raw) {
    let _ = unsafe { destroy_checked::<S>(session) };
}

unsafe extern "C" fn destroy_checked<S: Session>(session: Raw) -> i32 {
    let session = session.cast::<S>();
    let Some(owner) = (unsafe { session.as_ref() }) else {
        return INVALID;
    };
    if !owner.retire() {
        return NOT_READY;
    }
    drop(unsafe { Box::from_raw(session) });
    OK
}

unsafe extern "C" fn abort<S: Session>(session: Raw, id: u64) -> i32 {
    match unsafe { session.cast::<S>().as_ref() } {
        Some(session) if id != 0 => {
            session.state().abort();
            OK
        }
        _ => INVALID,
    }
}

const fn provider<S: Session>() -> Provider {
    Provider {
        struct_size: size_of::<Provider>() as u32,
        create: None, // The provider accessor supplies the native create entry.
        configure: Some(configure::<S>),
        begin_frame: Some(begin::<S>),
        marker: Some(marker::<S>),
        prepare: Some(prepare::<S>),
        after_present: Some(after::<S>),
        status: Some(status::<S>),
        retirement: Some(retirement),
        destroy: Some(destroy::<S>),
        abort_frame: Some(abort::<S>),
        destroy_checked: Some(destroy_checked::<S>),
    }
}

fn configure_disabled(state: &State, before: Snapshot, frame: u64, staged: bool) -> i32 {
    let complete = state.complete(before.order.revision, frame, false, RSF_FG_OFF, 0);
    if staged && complete { OK } else { NOT_READY }
}

#[cfg(test)]
mod tests;
