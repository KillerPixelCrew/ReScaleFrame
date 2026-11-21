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

const LEAVES_VERSION: u32 = 1;
// Native subsystem operations, RSF_SKYRIM_* in session_platform.h.
const ADMIT_OBSERVERS: u32 = 0;
const OBSERVERS_BUSY: u32 = 1;
const RESET_OBSERVATIONS: u32 = 2;
const CPU_PREPARE: u32 = 3;
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

/// `rsf_motion_decode_params` from `motion_decode.h`.
#[allow(dead_code)] // The Runtime reads every field.
#[repr(C)]
struct MotionDecode {
    struct_size: u32,
    scale_x: f32,
    scale_y: f32,
    bias_x: f32,
    bias_y: f32,
    output_scale_x: f32,
    output_scale_y: f32,
    invalid_value: f32,
    zero_means_unwritten: u32,
    validity_enabled: u32,
    validity_channel: u32,
    validity_threshold: f32,
}

/// `rsf_d3d11_host_setup` from `d3d11_host.h`.
#[allow(dead_code)] // The Runtime reads every field.
#[repr(C)]
pub(crate) struct HostSetup {
    struct_size: u32,
    abi_version: u32,
    configuration_path: *const u16,
    configuration_section: *const u16,
    runtime_directory: *const u16,
    identity_utf8: *const c_char,
    executable_sha256: *const c_char,
    engine_type: u32,
    engine_version_utf8: *const c_char,
    project_id_utf8: *const c_char,
    units_to_meters: f32,
    depth_inverted: u32,
    motion: MotionDecode,
    quality_choices: u32,
    presentation_bridge: u32,
    accept_window: Option<AcceptWindow>,
    presentation_user: *mut c_void,
    capabilities: u32,
    colour_transport_default: u32,
}

/// `rsf_skyrim_session_leaves`, lent by `plugin.cpp` for the process.
#[repr(C)]
pub(crate) struct Leaves {
    _struct_size: u32,
    _version: u32,
    platform: *const Platform,
    observers: *const observers::Leaves,
    subsystem: unsafe extern "C" fn(u32, *const rsf_game_host_services, *mut i32) -> bool,
    checkpoint: unsafe extern "C" fn(*mut *const c_char, bool) -> bool,
    compatibility_status: unsafe extern "C" fn() -> *const c_char,
    log: unsafe extern "C" fn(*const rsf_game_host_services, *const c_char) -> bool,
    module_path: unsafe extern "C" fn(usize, *mut u16, u32) -> u32,
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
    assert!(size_of::<MotionDecode>() == 48);
    assert!(size_of::<HostSetup>() == 160);
    assert!(std::mem::offset_of!(HostSetup, motion) == 80);
    assert!(std::mem::offset_of!(HostSetup, quality_choices) == 128);
    assert!(std::mem::offset_of!(HostSetup, accept_window) == 136);
    assert!(std::mem::offset_of!(HostSetup, colour_transport_default) == 156);
    assert!(size_of::<Leaves>() == 8 + 11 * size_of::<usize>());
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

    fn run(&self, operation: u32, services: Option<&rsf_game_host_services>) -> Result<i32, Threw> {
        let services = services.map_or(ptr::null(), ptr::from_ref);
        let mut value = 0;
        if unsafe { (self.subsystem)(operation, services, &mut value) } {
            Ok(value)
        } else {
            Err(Threw)
        }
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

    fn module_path(&self, module: usize, path: &mut [u16]) -> usize {
        let capacity = u32::try_from(path.len()).unwrap_or(u32::MAX);
        unsafe { (self.module_path)(module, path.as_mut_ptr(), capacity) as usize }
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

    fn set_active(&self, activate: usize, user: *mut c_void, active: u32) -> Result<(), Threw> {
        let mut result = NOT_READY;
        // The activation result was never consumed by the lifecycle.
        if unsafe { (self.host_set_active)(activate, user, active, &mut result) } {
            Ok(())
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
    if table.observers.is_null() || unsafe { platform::inspect(table.platform) }.is_none() {
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

fn same_session(upstream: &rsf_game_host_services, host: &rsf_game_host_services) -> bool {
    upstream.session_id == host.session_id
        && upstream.user == host.user
        && upstream.log.map(|log| log as *const ()) == host.log.map(|log| log as *const ())
        && upstream.cpu_event.map(|event| event as *const ())
            == host.cpu_event.map(|event| event as *const ())
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

    /// Rollback of a started stage, or of one whose start threw. Returns false when it threw.
    fn retire(self, leaves: &Leaves) -> bool {
        match self {
            Self::Sampling => leaves.check(SAMPLING_STOP).is_ok(),
            Self::Presentation => leaves.check(PRESENTATION_STOP).is_ok(),
            Self::Cpu => leaves.check(CPU_DISABLE).is_ok(),
            Self::Creation => {
                let _ = creation::stop();
                true
            }
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

    fn suspend_host(&self, leaves: &Leaves) -> Result<(), Threw> {
        if self.activate != 0 {
            leaves.set_active(self.activate, self.services.user, 0)?;
        }
        Ok(())
    }

    fn prepare(
        &mut self,
        leaves: &Leaves,
        host: &rsf_game_host_services,
    ) -> Result<rsf_result, Threw> {
        if self.prepared || leaves.check(OBSERVERS_BUSY)? || leaves.check(CPU_BUSY)? {
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
        if !leaves.check_with(CPU_PREPARE, host)? {
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
            leaves.module_path(runtime, &mut path)
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
        };
        if !self.bound_services.user.is_null() {
            if !same_session(&self.bound_upstream, host) {
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
        let created = unsafe { platform::register(leaves.platform) }
            .is_some_and(|platform| creation::prepare(platform, &services));
        if !created {
            self.reason = CREATION_REFUSED.as_ptr();
            leaves.log(&services, self.reason)?;
            return Ok(NATIVE_REFUSED);
        }
        if !leaves.check_with(CPU_PREPARE, &services)? {
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

    fn start(&mut self, leaves: &Leaves) -> Result<rsf_result, Threw> {
        if !self.prepared {
            return Ok(NOT_READY);
        }
        if !leaves.checkpoint(&mut self.reason, false) {
            leaves.log(&self.services, self.reason)?;
            return Ok(NATIVE_REFUSED);
        }
        if self.enabled {
            return Ok(OK);
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
            return Ok(NATIVE_REFUSED);
        }
        for (index, stage) in STAGES.into_iter().enumerate() {
            let (started, threw) = match stage.start(leaves, &services) {
                Ok(started) => (started, false),
                Err(Threw) => (false, true),
            };
            if started {
                continue;
            }
            // Retire in reverse start order. A stage that threw has unknown state and is
            // retired as well; one that refused is not, as before.
            let retired_from = if threw { index + 1 } else { index };
            let mut retired = true;
            for earlier in STAGES[..retired_from].iter().rev() {
                retired &= earlier.retire(leaves);
            }
            // Observers whose disable refuses stay recorded for stop.
            let _ = self.observers.disable(leaves.observers(), true);
            if threw || !retired {
                return Ok(NOT_READY);
            }
            return self.refuse(leaves, stage.refusal());
        }
        self.enabled = true;
        if self.activate != 0
            && leaves
                .set_active(self.activate, self.services.user, 1)
                .is_err()
        {
            // Enabled but never accepting: suspend what started; stop retires the hooks.
            let _ = self.suspend_host(leaves);
            let _ = quiesce_subsystems(leaves, false);
            return Ok(NOT_READY);
        }
        ACCEPTING.store(true, Ordering::Release);
        // Close a refusal racing start: do not republish a live engine source after the
        // compatibility callback already withdrew it on another thread.
        if !leaves.checkpoint(&mut self.reason, false) {
            ACCEPTING.store(false, Ordering::Release);
            let suspended = self.suspend_host(leaves).is_ok();
            let quiesced = quiesce_subsystems(leaves, false);
            return Ok(if suspended && quiesced {
                NATIVE_REFUSED
            } else {
                NOT_READY
            });
        }
        self.reason = ACTIVE.as_ptr();
        Ok(OK)
    }

    /// Every step runs; a step that threw reports RSF_ERROR_NOT_READY after the rest.
    fn quiesce(&mut self, leaves: &Leaves) -> Result<rsf_result, Threw> {
        let suspended = self.suspend_host(leaves).is_ok();
        let quiesced = quiesce_subsystems(leaves, true);
        if !suspended || !quiesced {
            return Err(Threw);
        }
        let busy = leaves.check(OBSERVERS_BUSY)?
            || leaves.check(CPU_BUSY)?
            || leaves.check(RESOLUTION_OWNED)?
            || leaves.check(AUXILIARIES_PENDING)?
            || leaves.check(SAMPLING_BUSY)?
            || leaves.check(PRESENTATION_BUSY)?
            || creation::busy();
        Ok(if busy { BUSY } else { OK })
    }

    /// Each refusal is retryable: a later stop resumes from the first stage that remains.
    fn stop(&mut self, leaves: &Leaves) -> Result<rsf_result, Threw> {
        ACCEPTING.store(false, Ordering::Release);
        self.suspend_host(leaves)?;
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
extern "C" fn rsf_skyrim_session_accepting() -> bool {
    ACCEPTING.load(Ordering::Acquire)
}

/// Lends the services to a detour for its synchronous call: while accepting when `live`,
/// otherwise as retained. Returns the Runtime presentation binding export, or 0.
///
/// # Safety
/// `out` is null or writable for one services table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_session_lease(
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
extern "C" fn rsf_skyrim_session_withdraw() {
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
