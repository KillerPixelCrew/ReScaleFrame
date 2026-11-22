// SPDX-License-Identifier: MIT
//! The GameSDK lifecycle and its process-lived Runtime session.
//!
//! Prepare admits the exact build and the observer sites, arms the Community Shaders
//! checkpoint, requests per-monitor-v2 DPI once per process, creates the observer hooks and the
//! CPU hooks, then creates the Runtime's scoped D3D11 host once per process. A later prepare must
//! present the original session. Start enables observers, sampling, presentation inputs, CPU
//! events and native creation in that order and retires the started ones when a later stage
//! refuses or throws. The host setup carries this adapter's facts; the Runtime owns every vendor
//! and pacing decision.
//!
//! One lock serializes the lifecycle, the borrowed services lent to the native detours and the
//! compatibility withdraw callback. The detours never hold it across native execution.
use crate::creation;
use crate::observers::{self, Control};
use crate::platform::{self, Platform};
use crate::plugin::{ABI_MISMATCH, INVALID_ARGUMENT, SHA256};
use rsf_game_sys::d3d11_host_abi::{rsf_d3d11_host_setup as HostSetup, same_upstream_identity};
use rsf_game_sys::sr_abi::rsf_motion_decode_params as MotionDecode;
use rsf_game_sys::{
    RSF_GAME_ABI_VERSION, rsf_game_control_args, rsf_game_host_services, rsf_game_prepare_args,
    rsf_game_renderer_status, rsf_game_start_args, rsf_result,
};
use std::ffi::{CStr, c_char, c_int, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

const OK: rsf_result = 0;
const NOT_READY: rsf_result = -3;
const BUSY: rsf_result = -4;
const NATIVE_REFUSED: rsf_result = -5;

const LEAVES_VERSION: u32 = 4;
// Subsystem operation tags also drive the injectable lifecycle fixture.
const ADMIT_OBSERVERS: u32 = 0;
const OBSERVERS_BUSY: u32 = 1;
const RESET_OBSERVATIONS: u32 = 2;
const CPU_PREPARE: u32 = 3;
const CPU_BIND: u32 = 19;
const CPU_START: u32 = 4;
const CPU_QUIESCE: u32 = 5;
const CPU_DISABLE: u32 = 6;
const CPU_STOP: u32 = 7;
const CPU_BUSY: u32 = 8;
const SAMPLING_START: u32 = 9;
const SAMPLING_QUIESCE: u32 = 10;
const SAMPLING_STOP: u32 = 11;
const SAMPLING_BUSY: u32 = 12;
const PRESENTATION_START: u32 = 13;
const PRESENTATION_QUIESCE: u32 = 14;
const PRESENTATION_STOP: u32 = 15;
const PRESENTATION_BUSY: u32 = 16;
const RESOLUTION_OWNED: u32 = 17;
const AUXILIARIES_PENDING: u32 = 18;

const RUNTIME_MODULE: &str = "ReScaleFrame.Runtime.dll";
const PATH_CAPACITY: usize = 32_768;
const ENGINE_VERSION: &CStr = c"1.7.104";
const PROJECT_ID: &CStr = c"668392a0-ecfe-4d0c-9f58-c2838c5ca308";
const UNITS_TO_METERS: f32 = 0.014_287_5;
const QUALITY_CHOICES: u32 = 0x1f;
// Scope inner creation before external wrappers capture the chain; retain their outer result.
const PRESENTATION_BRIDGE_SCOPED: u32 = 3;
// Native colour by default; a saved overlay choice wins.
const COLOUR_TRANSPORT_NATIVE: u32 = 1;
// Native prospective-poll ownership connects input and simulation reports; the Runtime owns
// pacing, marker mapping and provider admission policy.
const CAP_FG_INPUTS: u32 = 1;
const CAP_INPUT_TIMELINE: u32 = 2;
// DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2.
const PER_MONITOR_V2: *mut c_void = -4isize as *mut c_void;

const NOT_PREPARED: &CStr = c"Native hooks not prepared; native-resolution SR is experimental; FG and latency remain incomplete.";
const OBSERVER_REFUSED: &CStr =
    c"Skyrim native observer refused: executable identity or native hook bytes differ.";
const CPU_REFUSED: &CStr =
    c"Skyrim CPU event hooks refused: expected bytes, hook ownership or clock unavailable.";
const PROCESS_LIVED: &CStr =
    c"Experimental D3D11 host is process-lived; a different session requires process restart.";
const CREATION_REFUSED: &CStr = c"Native creation root refused: cold owner, expected thunk/caller bytes or runtime service unavailable.";
const PREPARED: &CStr =
    c"Native pre-HDR SR prepared at native resolution; FG and latency ownership remain incomplete.";
const START_IDENTITY: &CStr =
    c"Skyrim start refused: executable identity or jitter101E360 guard changed before enable.";
const ACTIVE: &CStr = c"Experimental native-resolution SR active with native temporal fallback; FG and latency ownership remain incomplete.";
const STOPPED: &CStr = c"Native observer stopped; no host services retained.";

type AcceptWindow = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;
type SetDpi = unsafe extern "system" fn(*mut c_void) -> i32;

/// The process-lived native services and injectable fixture callbacks.
#[repr(C)]
pub(crate) struct Leaves {
    _struct_size: u32,
    _version: u32,
    platform: *const Platform,
    observers: *const observers::Leaves,
    admit_observers: unsafe extern "C" fn() -> i32,
    observers_busy: unsafe extern "C" fn() -> bool,
    cpu_prepare: unsafe extern "C" fn() -> bool,
    cpu_start: unsafe extern "C" fn() -> bool,
    #[cfg(test)]
    subsystem: unsafe extern "C" fn(u32, *const rsf_game_host_services, *mut i32) -> bool,
    checkpoint: unsafe extern "C" fn(*mut *const c_char, bool) -> bool,
    compatibility_status: unsafe extern "C" fn() -> *const c_char,
    log: unsafe extern "C" fn(*const rsf_game_host_services, *const c_char) -> bool,
    host_create: unsafe extern "C" fn(
        usize,
        *const HostSetup,
        *mut rsf_game_host_services,
        *mut i32,
    ) -> bool,
    host_set_active: unsafe extern "C" fn(usize, *mut c_void, u32, *mut i32) -> bool,
    accept_window: AcceptWindow,
}

const _: () = {
    assert!(size_of::<Leaves>() == 8 + (12 + cfg!(test) as usize) * size_of::<usize>());
};

// The production record borrows only immutable process-lived tables.
#[cfg(not(test))]
unsafe impl Sync for Leaves {}

#[cfg(not(test))]
unsafe extern "C" {
    fn rsf_skyrim_cpu_prepare() -> bool;
    fn rsf_skyrim_call_log(host: *const rsf_game_host_services, message: *const c_char) -> bool;
    fn rsf_skyrim_host_create_guarded(
        create: usize,
        setup: *const HostSetup,
        services: *mut rsf_game_host_services,
        result: *mut i32,
    ) -> bool;
    fn rsf_skyrim_host_active_guarded(
        activate: usize,
        user: *mut c_void,
        active: u32,
        result: *mut i32,
    ) -> bool;
}

#[cfg(not(test))]
pub(crate) static NATIVE_LEAVES: Leaves = Leaves {
    _struct_size: size_of::<Leaves>() as u32,
    _version: LEAVES_VERSION,
    platform: &platform::NATIVE_PLATFORM,
    observers: &observers::NATIVE_LEAVES,
    admit_observers: crate::plugin::rsf_skyrim_observer_admit,
    observers_busy: observers::busy,
    cpu_prepare: rsf_skyrim_cpu_prepare,
    cpu_start: crate::cpu_events::rsf_skyrim_cpu_start,
    checkpoint: crate::compatibility::checkpoint,
    compatibility_status: crate::compatibility::status,
    log: rsf_skyrim_call_log,
    host_create: rsf_skyrim_host_create_guarded,
    host_set_active: rsf_skyrim_host_active_guarded,
    accept_window: crate::render_observer::rsf_skyrim_render_accept_window,
};

/// A native leaf threw. A lifecycle entry reports RSF_ERROR_NOT_READY as the former C++ catch did.
struct Threw;

impl Leaves {
    fn platform(&self) -> &'static Platform {
        // SAFETY: `publish` accepted the process-lived table.
        unsafe { &*self.platform }
    }

    fn observers(&self) -> &observers::Leaves {
        // SAFETY: `publish` refused a null table; it lives for the process.
        unsafe { &*self.observers }
    }

    #[cfg(test)]
    fn run(&self, operation: u32, services: Option<&rsf_game_host_services>) -> Result<i32, Threw> {
        let services = services.map_or(ptr::null(), ptr::from_ref);
        let mut value = 0;
        if unsafe { (self.subsystem)(operation, services, &mut value) } {
            Ok(value)
        } else {
            Err(Threw)
        }
    }

    #[cfg(not(test))]
    fn run(&self, operation: u32, services: Option<&rsf_game_host_services>) -> Result<i32, Threw> {
        use crate::{cpu_hooks, postprocess, presentation, render_observer, resolution, sampling};

        let result = match operation {
            ADMIT_OBSERVERS => return Ok(unsafe { (self.admit_observers)() }),
            OBSERVERS_BUSY => unsafe { (self.observers_busy)() },
            RESET_OBSERVATIONS => {
                render_observer::rsf_skyrim_reset_render_observations();
                true
            }
            CPU_PREPARE => unsafe { (self.cpu_prepare)() },
            CPU_BIND => services.is_some_and(|host| cpu_hooks::rsf_skyrim_cpu_rust_bind(host)),
            CPU_START => unsafe { (self.cpu_start)() },
            CPU_QUIESCE => {
                cpu_hooks::rsf_skyrim_cpu_rust_quiesce();
                true
            }
            CPU_DISABLE => cpu_hooks::rsf_skyrim_cpu_rust_disable(),
            CPU_STOP => cpu_hooks::rsf_skyrim_cpu_rust_stop(),
            CPU_BUSY => cpu_hooks::rsf_skyrim_cpu_rust_busy(),
            SAMPLING_START => {
                services.is_some_and(|host| sampling::rsf_skyrim_sampling_start(host))
            }
            SAMPLING_QUIESCE => {
                sampling::rsf_skyrim_sampling_quiesce();
                true
            }
            SAMPLING_STOP => sampling::rsf_skyrim_sampling_stop(),
            SAMPLING_BUSY => sampling::rsf_skyrim_sampling_busy(),
            PRESENTATION_START => {
                services.is_some_and(|host| presentation::rsf_skyrim_presentation_start(host))
            }
            PRESENTATION_QUIESCE => {
                presentation::rsf_skyrim_presentation_quiesce();
                true
            }
            PRESENTATION_STOP => presentation::rsf_skyrim_presentation_stop(),
            PRESENTATION_BUSY => presentation::rsf_skyrim_presentation_busy(),
            RESOLUTION_OWNED => resolution::rsf_skyrim_resolution_owned(),
            AUXILIARIES_PENDING => postprocess::rsf_skyrim_render_cleanup_pending(),
            _ => false,
        };
        Ok(i32::from(result))
    }

    fn check(&self, operation: u32) -> Result<bool, Threw> {
        Ok(self.run(operation, None)? != 0)
    }

    fn check_with(&self, operation: u32, services: &rsf_game_host_services) -> Result<bool, Threw> {
        Ok(self.run(operation, Some(services))? != 0)
    }

    fn checkpoint(&self, reason: &mut *const c_char, arm: bool) -> bool {
        unsafe { (self.checkpoint)(reason, arm) }
    }

    fn compatibility_status(&self) -> *const c_char {
        unsafe { (self.compatibility_status)() }
    }

    fn log(&self, host: &rsf_game_host_services, message: *const c_char) -> Result<(), Threw> {
        if unsafe { (self.log)(host, message) } {
            Ok(())
        } else {
            Err(Threw)
        }
    }

    /// Logs formatted text; it never contains an interior NUL.
    fn log_text(&self, host: &rsf_game_host_services, message: &str) -> Result<(), Threw> {
        let mut text = Vec::with_capacity(message.len() + 1);
        text.extend_from_slice(message.as_bytes());
        text.push(0);
        match CStr::from_bytes_with_nul(&text) {
            Ok(text) => self.log(host, text.as_ptr()),
            Err(_) => Ok(()),
        }
    }

    fn host_create(
        &self,
        create: usize,
        setup: &HostSetup,
        services: &mut rsf_game_host_services,
    ) -> Result<rsf_result, Threw> {
        let mut result = NOT_READY;
        if unsafe { (self.host_create)(create, setup, services, &mut result) } {
            Ok(result)
        } else {
            Err(Threw)
        }
    }

    fn set_active(
        &self,
        activate: usize,
        user: *mut c_void,
        active: u32,
    ) -> Result<rsf_result, Threw> {
        let mut result = NOT_READY;
        if unsafe { (self.host_set_active)(activate, user, active, &mut result) } {
            Ok(result)
        } else {
            Err(Threw)
        }
    }
}

static LEAVES: AtomicPtr<Leaves> = AtomicPtr::new(ptr::null_mut());
// Read without the lock by the detours. Only start sets it, under the lock. Quiesce and the
// withdraw callback clear it before taking the lock; every other clear happens under it.
static ACCEPTING: AtomicBool = AtomicBool::new(false);
static SESSION: Mutex<Session> = Mutex::new(Session::new());

/// Accepts the adapter's table when its size, version and nested tables match this crate.
///
/// # Safety
/// `leaves` is null or points at a table readable for at least its leading header that, when
/// accepted, lives for the process with every entry present.
pub(crate) unsafe fn publish(leaves: *const Leaves) -> bool {
    if leaves.is_null() {
        return false;
    }
    let header = leaves.cast::<u32>();
    if (unsafe { header.read() } as usize) < size_of::<Leaves>()
        || unsafe { header.add(1).read() } != LEAVES_VERSION
    {
        return false;
    }
    let table = unsafe { &*leaves };
    if table.observers.is_null() || unsafe { platform::register(table.platform) }.is_none() {
        return false;
    }
    LEAVES.store(leaves.cast_mut(), Ordering::Release);
    true
}

fn leaves() -> Option<&'static Leaves> {
    // Only `publish` stores, and only a process-lived table.
    unsafe { LEAVES.load(Ordering::Acquire).as_ref() }
}

fn session() -> MutexGuard<'static, Session> {
    SESSION.lock().unwrap_or_else(PoisonError::into_inner)
}

const fn empty_services() -> rsf_game_host_services {
    // SAFETY: the services table is plain data; all-zero is its empty state.
    unsafe { std::mem::zeroed() }
}

fn terminated(units: &[u16], suffix: &str) -> Vec<u16> {
    let mut path = units.to_vec();
    path.extend(suffix.encode_utf16());
    path.push(0);
    path
}

/// Quiesces the CPU, sampling and presentation-input subsystems and native creation. Every step
/// runs even when an earlier one threw; returns false when any step threw.
fn quiesce_subsystems(leaves: &Leaves, creation_first: bool) -> bool {
    if creation_first {
        creation::quiesce();
    }
    let mut completed = true;
    for operation in [CPU_QUIESCE, SAMPLING_QUIESCE, PRESENTATION_QUIESCE] {
        completed &= leaves.run(operation, None).is_ok();
    }
    if !creation_first {
        creation::quiesce();
    }
    completed
}

/// The per-monitor-v2 startup request, once per process before any observer hook exists.
fn request_dpi(leaves: &Leaves, host: &rsf_game_host_services) -> Result<(), Threw> {
    let platform = leaves.platform();
    let user32 = platform.module_handle("user32.dll");
    let set = platform.export_address(user32, c"SetProcessDpiAwarenessContext");
    platform.set_last_error(0);
    // SAFETY: the user32 export has this signature.
    let accepted =
        set != 0 && unsafe { std::mem::transmute::<usize, SetDpi>(set)(PER_MONITOR_V2) } != 0;
    let error = if accepted { 0 } else { platform.last_error() };
    if host.log.is_some() {
        leaves.log_text(
            host,
            &format!(
                "Skyrim DPI startup per-monitor-v2 accepted={} error={error}; window creation must use physical pixels.",
                u32::from(accepted)
            ),
        )?;
    }
    Ok(())
}

/// Subsystems that start after the observers, in start order.
#[derive(Clone, Copy)]
enum Stage {
    Sampling,
    Presentation,
    Cpu,
    Creation,
}

const STAGES: [Stage; 4] = [
    Stage::Sampling,
    Stage::Presentation,
    Stage::Cpu,
    Stage::Creation,
];

impl Stage {
    fn start(self, leaves: &Leaves, services: &rsf_game_host_services) -> Result<bool, Threw> {
        match self {
            Self::Sampling => leaves.check_with(SAMPLING_START, services),
            Self::Presentation => leaves.check_with(PRESENTATION_START, services),
            Self::Cpu => leaves.check(CPU_START),
            Self::Creation => Ok(creation::start()),
        }
    }

    /// A stage retires only when it acknowledges cleanup.
    fn retire(self, leaves: &Leaves) -> bool {
        match self {
            Self::Sampling => matches!(leaves.check(SAMPLING_STOP), Ok(true)),
            Self::Presentation => matches!(leaves.check(PRESENTATION_STOP), Ok(true)),
            Self::Cpu => matches!(leaves.check(CPU_DISABLE), Ok(true)),
            Self::Creation => creation::stop(),
        }
    }

    fn refusal(self) -> &'static CStr {
        match self {
            Self::Sampling => {
                c"Skyrim start refused: sampler guard, binding service or hook enable failed."
            }
            Self::Presentation => {
                c"Skyrim start refused: presentation-input guard or hook enable failed."
            }
            Self::Cpu => c"Skyrim start refused: CPU guard or hook enable failed.",
            Self::Creation => {
                c"Skyrim start refused: native creation guard or hook enable failed; native presentation retained."
            }
        }
    }
}

struct Session {
    prepared: bool,
    // The observers and every stage are enabled; accepting is separate.
    enabled: bool,
    retirement_pending: bool,
    rollback_stages: u8,
    rollback_observers: bool,
    dpi_requested: bool,
    // Borrowed by the detours through the lease while accepting, and by the renderer-end
    // detour as retained.
    services: rsf_game_host_services,
    // The Runtime services created for this process and the prepare host that created them.
    bound_services: rsf_game_host_services,
    bound_upstream: rsf_game_host_services,
    // Runtime rsf_d3d11_host_set_active and rsf_d3d11_host_bind_presentation, or 0.
    activate: usize,
    bind: usize,
    // A static string: this crate's or the compatibility owner's.
    reason: *const c_char,
    observers: Control,
}
// SAFETY: the services are process-lived Runtime and host state, used under the session lock;
// the reason is a static string.
unsafe impl Send for Session {}

impl Session {
    const fn new() -> Self {
        Self {
            prepared: false,
            enabled: false,
            retirement_pending: false,
            rollback_stages: 0,
            rollback_observers: false,
            dpi_requested: false,
            services: empty_services(),
            bound_services: empty_services(),
            bound_upstream: empty_services(),
            activate: 0,
            bind: 0,
            reason: NOT_PREPARED.as_ptr(),
            observers: Control::new(),
        }
    }

    fn suspend_host(&self, leaves: &Leaves) -> Result<rsf_result, Threw> {
        if self.activate != 0 && !self.bound_services.user.is_null() {
            leaves.set_active(self.activate, self.bound_services.user, 0)
        } else {
            Ok(OK)
        }
    }

    fn prepare(
        &mut self,
        leaves: &Leaves,
        host: &rsf_game_host_services,
    ) -> Result<rsf_result, Threw> {
        if self.prepared
            || self.retirement_pending
            || leaves.check(OBSERVERS_BUSY)?
            || leaves.check(CPU_BUSY)?
        {
            return Ok(BUSY);
        }
        let admitted = leaves.run(ADMIT_OBSERVERS, None)?;
        if admitted < 0 {
            return Ok(NOT_READY);
        }
        if admitted == 0 {
            self.reason = OBSERVER_REFUSED.as_ptr();
            return Ok(NATIVE_REFUSED);
        }
        if !leaves.checkpoint(&mut self.reason, true) {
            leaves.log(host, self.reason)?;
            return Ok(NATIVE_REFUSED);
        }
        if !self.dpi_requested {
            self.dpi_requested = true;
            request_dpi(leaves, host)?;
        }
        if !self.observers.prepare(leaves.observers()) {
            return Ok(NATIVE_REFUSED);
        }
        if !leaves.check(CPU_PREPARE)? {
            self.reason = CPU_REFUSED.as_ptr();
            return Ok(NATIVE_REFUSED);
        }
        self.services = *host;
        let platform = leaves.platform();
        let runtime = platform.module_handle(RUNTIME_MODULE);
        let export = |name| {
            if runtime == 0 {
                0
            } else {
                platform.export_address(runtime, name)
            }
        };
        let create = export(c"rsf_d3d11_host_create");
        self.activate = export(c"rsf_d3d11_host_set_active");
        self.bind = export(c"rsf_d3d11_host_bind_presentation");
        let mut path = vec![0u16; PATH_CAPACITY];
        let length = if runtime == 0 {
            0
        } else {
            platform::module_path(runtime, &mut path)
        };
        if create == 0
            || self.activate == 0
            || self.bind == 0
            || length == 0
            || length >= PATH_CAPACITY
        {
            return Ok(NOT_READY);
        }
        let Some(separator) = path[..length]
            .iter()
            .rposition(|&unit| unit == u16::from(b'\\'))
        else {
            return Ok(NOT_READY);
        };
        let directory = terminated(&path[..separator], "");
        let ini = terminated(&path[..separator], "\\ReScaleFrame.ini");
        let section = platform::wide("Rendering");
        let setup = HostSetup {
            struct_size: size_of::<HostSetup>() as u32,
            abi_version: platform.d3d11_host_abi,
            configuration_path: ini.as_ptr(),
            configuration_section: section.as_ptr(),
            runtime_directory: directory.as_ptr(),
            identity_utf8: c"skyrim-se".as_ptr(),
            executable_sha256: SHA256.as_ptr(),
            engine_type: 0,
            engine_version_utf8: ENGINE_VERSION.as_ptr(),
            project_id_utf8: PROJECT_ID.as_ptr(),
            units_to_meters: UNITS_TO_METERS,
            depth_inverted: 0,
            motion: MotionDecode {
                struct_size: size_of::<MotionDecode>() as u32,
                scale_x: 1.0,
                scale_y: 1.0,
                bias_x: 0.0,
                bias_y: 0.0,
                output_scale_x: 1.0,
                output_scale_y: 1.0,
                invalid_value: -1000.0,
                zero_means_unwritten: 0,
                validity_enabled: 0,
                validity_channel: 0,
                validity_threshold: 0.0,
            },
            quality_choices: QUALITY_CHOICES,
            // The corrected shared SR transfer has device regression evidence. Runtime
            // capability and per-frame checks still gate generation and latency admission.
            presentation_bridge: PRESENTATION_BRIDGE_SCOPED,
            accept_window: Some(leaves.accept_window),
            presentation_user: ptr::null_mut(),
            capabilities: CAP_FG_INPUTS | CAP_INPUT_TIMELINE,
            colour_transport_default: COLOUR_TRANSPORT_NATIVE,
            depth_far_plane: 0,
        };
        if !self.bound_services.user.is_null() {
            if !same_upstream_identity(&self.bound_upstream, host) {
                self.reason = PROCESS_LIVED.as_ptr();
                return Ok(NATIVE_REFUSED);
            }
            self.services = self.bound_services;
        } else {
            let result = leaves.host_create(create, &setup, &mut self.services)?;
            if result != OK {
                return Ok(result);
            }
            self.bound_services = self.services;
            self.bound_upstream = *host;
        }
        let services = self.services;
        let created = creation::prepare(platform, &services);
        if !created {
            self.reason = CREATION_REFUSED.as_ptr();
            leaves.log(&services, self.reason)?;
            return Ok(NATIVE_REFUSED);
        }
        if !leaves.check_with(CPU_BIND, &services)? {
            return Ok(NATIVE_REFUSED);
        }
        leaves.run(RESET_OBSERVATIONS, None)?;
        self.prepared = true;
        self.reason = PREPARED.as_ptr();
        Ok(OK)
    }

    fn refuse(&mut self, leaves: &Leaves, message: &'static CStr) -> Result<rsf_result, Threw> {
        self.reason = message.as_ptr();
        leaves.log(&self.services, self.reason)?;
        Ok(NATIVE_REFUSED)
    }

    fn rollback(&mut self, leaves: &Leaves) -> bool {
        for (index, stage) in STAGES.into_iter().enumerate().rev() {
            let bit = 1 << index;
            if self.rollback_stages & bit != 0 && stage.retire(leaves) {
                self.rollback_stages &= !bit;
            }
        }
        if self.rollback_observers {
            self.rollback_observers = !self.observers.disable(leaves.observers(), true);
        }
        self.rollback_stages == 0 && !self.rollback_observers
    }

    fn start(&mut self, leaves: &Leaves) -> Result<rsf_result, Threw> {
        if !self.prepared {
            return Ok(NOT_READY);
        }
        if self.retirement_pending {
            return Ok(BUSY);
        }
        if !leaves.checkpoint(&mut self.reason, false) {
            leaves.log(&self.services, self.reason)?;
            return Ok(NATIVE_REFUSED);
        }
        if self.enabled {
            return Ok(if rsf_skyrim_session_accepting() {
                OK
            } else {
                BUSY
            });
        }
        self.reason = START_IDENTITY.as_ptr();
        let admitted = leaves.run(ADMIT_OBSERVERS, None)?;
        if admitted < 0 {
            return Ok(NOT_READY);
        }
        if admitted == 0 {
            leaves.log(&self.services, self.reason)?;
            return Ok(NATIVE_REFUSED);
        }
        let services = self.services;
        if !self.observers.start(leaves.observers(), &services) {
            self.rollback_observers = true;
            let retired = self.rollback(leaves);
            self.retirement_pending = !retired;
            return Ok(if retired { NATIVE_REFUSED } else { BUSY });
        }
        for (index, stage) in STAGES.into_iter().enumerate() {
            let (started, threw) = match stage.start(leaves, &services) {
                Ok(started) => (started, false),
                Err(Threw) => (false, true),
            };
            if started {
                continue;
            }
            // Start can refuse after acquiring a subset of its hooks. Retire it too.
            self.rollback_stages = (1 << (index + 1)) - 1;
            self.rollback_observers = true;
            let retired = self.rollback(leaves);
            self.retirement_pending = !retired;
            if !retired {
                return Ok(BUSY);
            }
            if threw {
                return Ok(NOT_READY);
            }
            return self.refuse(leaves, stage.refusal());
        }
        self.enabled = true;
        if self.activate != 0 {
            let activated = leaves.set_active(self.activate, self.services.user, 1);
            if !matches!(activated, Ok(OK)) {
                self.retirement_pending = true;
                let _ = self.suspend_host(leaves);
                let _ = quiesce_subsystems(leaves, false);
                return activated;
            }
        }
        ACCEPTING.store(true, Ordering::Release);
        // Close a refusal racing start: do not republish a live engine source after the
        // compatibility callback already withdrew it on another thread.
        if !leaves.checkpoint(&mut self.reason, false) {
            ACCEPTING.store(false, Ordering::Release);
            let suspended = self.suspend_host(leaves);
            let quiesced = quiesce_subsystems(leaves, false);
            self.retirement_pending = true;
            return match suspended {
                Ok(OK) if quiesced => Ok(NATIVE_REFUSED),
                Ok(OK) => Ok(NOT_READY),
                failure => failure,
            };
        }
        self.reason = ACTIVE.as_ptr();
        Ok(OK)
    }

    /// Every step runs; a step that threw reports RSF_ERROR_NOT_READY after the rest.
    fn quiesce(&mut self, leaves: &Leaves) -> Result<rsf_result, Threw> {
        // A start holding the lock could have published after the public entry cleared it.
        ACCEPTING.store(false, Ordering::Release);
        self.retirement_pending |= self.prepared;
        let suspended = self.suspend_host(leaves);
        let quiesced = quiesce_subsystems(leaves, true);
        let suspended = suspended?;
        if suspended != OK {
            return Ok(suspended);
        }
        if !quiesced {
            return Err(Threw);
        }
        let busy = leaves.check(OBSERVERS_BUSY)?
            || leaves.check(CPU_BUSY)?
            || leaves.check(RESOLUTION_OWNED)?
            || leaves.check(AUXILIARIES_PENDING)?
            || leaves.check(SAMPLING_BUSY)?
            || leaves.check(PRESENTATION_BUSY)?
            || creation::busy();
        // The Runtime cannot call stop until quiesce succeeds. Retry failed start cleanup here.
        Ok(if busy || !self.rollback(leaves) {
            BUSY
        } else {
            OK
        })
    }

    /// Each refusal is retryable: a later stop resumes from the first stage that remains.
    fn stop(&mut self, leaves: &Leaves) -> Result<rsf_result, Threw> {
        ACCEPTING.store(false, Ordering::Release);
        self.retirement_pending |= self.prepared;
        let suspended = self.suspend_host(leaves)?;
        if suspended != OK {
            return Ok(suspended);
        }
        if leaves.check(RESOLUTION_OWNED)? || leaves.check(AUXILIARIES_PENDING)? {
            return Ok(BUSY);
        }
        if !creation::stop() || !leaves.check(PRESENTATION_STOP)? || !leaves.check(SAMPLING_STOP)? {
            return Ok(BUSY);
        }
        let cpu_stopped = leaves.check(CPU_DISABLE)?;
        if !self.observers.disable(leaves.observers(), false) {
            return Ok(BUSY);
        }
        self.enabled = false;
        if !cpu_stopped || leaves.check(OBSERVERS_BUSY)? || !leaves.check(CPU_STOP)? {
            return Ok(BUSY);
        }
        self.services = empty_services();
        self.prepared = false;
        self.retirement_pending = false;
        self.rollback_stages = 0;
        self.rollback_observers = false;
        self.reason = STOPPED.as_ptr();
        Ok(OK)
    }
}

/// The GameSDK size and ABI check on a caller structure that leads with struct_size and
/// abi_version.
///
/// # Safety
/// A nonnull `value` points to at least its two leading fields.
unsafe fn validate<T>(value: *const T) -> rsf_result {
    if value.is_null() {
        return INVALID_ARGUMENT;
    }
    let header = value.cast::<u32>();
    if (unsafe { header.read() } as usize) < size_of::<T>() {
        INVALID_ARGUMENT
    } else if unsafe { header.add(1).read() } != RSF_GAME_ABI_VERSION {
        ABI_MISMATCH
    } else {
        OK
    }
}

fn run(entry: impl FnOnce(&mut Session, &Leaves) -> Result<rsf_result, Threw>) -> rsf_result {
    let Some(leaves) = leaves() else {
        return NOT_READY;
    };
    let mut session = session();
    entry(&mut *session, leaves).unwrap_or(NOT_READY)
}

pub(crate) unsafe extern "C" fn prepare(args: *const rsf_game_prepare_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != OK {
        return valid;
    }
    let host = unsafe { (*args).host };
    let valid = unsafe { validate(host) };
    if valid != OK {
        return valid;
    }
    let host = unsafe { *host };
    if host.session_id == 0 {
        return INVALID_ARGUMENT;
    }
    run(|session, leaves| session.prepare(leaves, &host))
}

pub(crate) unsafe extern "C" fn start(args: *const rsf_game_start_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != OK {
        return valid;
    }
    run(Session::start)
}

pub(crate) unsafe extern "C" fn quiesce(args: *const rsf_game_control_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != OK {
        return valid;
    }
    // Withdraw first, as before, so a lifecycle call holding the lock cannot delay it.
    ACCEPTING.store(false, Ordering::Release);
    run(Session::quiesce)
}

pub(crate) unsafe extern "C" fn stop(args: *const rsf_game_control_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != OK {
        return valid;
    }
    run(Session::stop)
}

pub(crate) unsafe extern "C" fn status(output: *mut rsf_game_renderer_status) -> rsf_result {
    let valid = unsafe { validate(output.cast_const()) };
    if valid != OK {
        return valid;
    }
    let Some(leaves) = leaves() else {
        return NOT_READY;
    };
    let session = session();
    let output = unsafe { &mut *output };
    output.prepared = u32::from(session.prepared);
    output.active = u32::from(ACCEPTING.load(Ordering::Acquire));
    output.rendering_ready = 0;
    output.reason = session.reason;
    if !leaves.platform().render_allowed() {
        let compatibility = leaves.compatibility_status();
        if !compatibility.is_null() {
            output.reason = compatibility;
        }
    }
    OK
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_skyrim_session_accepting() -> bool {
    ACCEPTING.load(Ordering::Acquire)
}

/// Lends the services to a detour for its synchronous call: while accepting when `live`,
/// otherwise as retained. Returns the Runtime presentation binding export, or 0.
///
/// # Safety
/// `out` is null or writable for one services table.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_skyrim_session_lease(
    live: bool,
    out: *mut rsf_game_host_services,
) -> usize {
    let session = session();
    if let Some(out) = unsafe { out.as_mut() } {
        *out = if !live || ACCEPTING.load(Ordering::Relaxed) {
            session.services
        } else {
            empty_services()
        };
    }
    session.bind
}

/// The compatibility withdraw callback, from any thread.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_skyrim_session_withdraw() {
    let was_active = ACCEPTING.swap(false, Ordering::AcqRel);
    let Some(leaves) = leaves() else {
        return;
    };
    let _ = quiesce_subsystems(leaves, false);
    // A successful start publishes the host services before accepting. A refusal while the
    // session is not accepting has no live host to suspend and must not take the lock, which a
    // lifecycle call on this thread may hold.
    if was_active {
        let session = session();
        if session.activate != 0 && !session.services.user.is_null() {
            let _ = leaves.set_active(session.activate, session.services.user, 0);
        }
    }
}

#[cfg(test)]
mod tests;
