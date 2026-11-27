// SPDX-License-Identifier: MIT
//! The GameSDK lifecycle and its process-lived Runtime session.
//!
//! Prepare discovers the native sites and admits the observer sites, arms the Community Shaders
//! checkpoint, requests per-monitor-v2 DPI once per process, creates the observer hooks and the
//! CPU hooks, then creates the Runtime's scoped D3D11 host once per process. A later prepare must
//! present the original session. Start enables native creation, observers, sampling,
//! presentation inputs and CPU events in that order and retires the started ones when a later
//! stage refuses or throws. The host setup carries this adapter's facts; the Runtime owns every vendor
//! and pacing decision.
//!
//! One lock serializes the lifecycle, the borrowed services lent to the native detours and the
//! compatibility withdraw callback. The detours never hold it across native execution.
use crate::creation;
use crate::observers::Control;
use crate::platform::{self, Platform};
use crate::plugin::INVALID_ARGUMENT;
use rsf_game_sys::call_header::validate;
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_game_sys::rsf_motion_decode_params as MotionDecode;
use rsf_game_sys::win32::module_path;
use rsf_game_sys::{
    RSF_ERROR_BUSY as BUSY, RSF_ERROR_NATIVE_REFUSED as NATIVE_REFUSED,
    RSF_ERROR_NOT_READY as NOT_READY, RSF_OK as OK,
};
use rsf_game_sys::{rsf_d3d11_host_setup as HostSetup, same_upstream_identity};
use rsf_game_sys::{
    rsf_game_control_args, rsf_game_host_services, rsf_game_prepare_args, rsf_game_renderer_status,
    rsf_game_start_args, rsf_result,
};
use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

const RUNTIME_MODULE: &str = "ReScaleFrame.Runtime.dll";
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

const NOT_PREPARED: &CStr = c"Native hooks not prepared; native-resolution SR is experimental; FG and latency remain incomplete.";
const OBSERVER_REFUSED: &CStr =
    c"Skyrim native observer refused: executable name or native hook bytes differ.";
const CPU_REFUSED: &CStr =
    c"Skyrim CPU event hooks refused: expected bytes, hook ownership or clock unavailable.";
const PROCESS_LIVED: &CStr =
    c"Experimental D3D11 host is process-lived; a different session requires process restart.";
const CREATION_REFUSED: &CStr = c"Native creation root refused: cold owner, expected thunk/caller bytes or runtime service unavailable.";
const PREPARED: &CStr =
    c"Native pre-HDR SR prepared at native resolution; FG and latency ownership remain incomplete.";
const START_IDENTITY: &CStr =
    c"Skyrim start refused: executable or a native observer site changed before enable.";
const ACTIVE: &CStr = c"Experimental native-resolution SR active with native temporal fallback; FG and latency ownership remain incomplete.";
const STOPPED: &CStr = c"Native observer stopped; no host services retained.";

/// The subsystems the lifecycle drives, called directly. Unit tests swap in recording fakes
/// (`tests::subsystems`).
#[cfg(not(test))]
mod subsystems {
    use super::HostSetup;
    use rsf_game_sys::rsf_game_host_services;
    use std::ffi::{c_char, c_void};

    pub(super) use crate::compatibility::checkpoint;
    pub(super) use crate::cpu_events::start as cpu_start;
    pub(super) use crate::cpu_hooks::{
        bind as cpu_bind, busy as cpu_busy, disable as cpu_disable, prepare as cpu_prepare,
        quiesce as cpu_quiesce, stop as cpu_stop,
    };
    pub(super) use crate::creation::{start as creation_start, stop as creation_stop};
    pub(super) use crate::discovery::{executable_sha256, refusal as discovery_refusal};
    pub(super) use crate::observers::busy as observers_busy;
    pub(super) use crate::plugin::admit_observers;
    pub(super) use crate::postprocess::cleanup_pending as auxiliaries_pending;
    pub(super) use crate::presentation::{
        busy as presentation_busy, quiesce as presentation_quiesce, start as presentation_start,
        stop as presentation_stop,
    };
    pub(super) use crate::render_observer::{accept_window, reset_observations};
    pub(super) use crate::resolution::owned as resolution_owned;
    pub(super) use crate::sampling::{
        busy as sampling_busy, quiesce as sampling_quiesce, start as sampling_start,
        stop as sampling_stop,
    };

    unsafe extern "C" {
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

    /// False when the host log threw.
    pub(super) fn log(host: &rsf_game_host_services, message: *const c_char) -> bool {
        unsafe { crate::render_native::log_host(host, message) }
    }

    /// False when the Runtime export at `create` threw.
    pub(super) fn host_create(
        create: usize,
        setup: &HostSetup,
        services: &mut rsf_game_host_services,
        result: &mut i32,
    ) -> bool {
        unsafe { rsf_skyrim_host_create_guarded(create, setup, services, result) }
    }

    /// False when the Runtime export at `activate` threw.
    pub(super) fn host_set_active(
        activate: usize,
        user: *mut c_void,
        active: u32,
        result: &mut i32,
    ) -> bool {
        unsafe { rsf_skyrim_host_active_guarded(activate, user, active, result) }
    }
}

#[cfg(test)]
use tests::subsystems;

/// A native leaf threw. A lifecycle entry reports RSF_ERROR_NOT_READY.
struct Threw;

fn log(host: &rsf_game_host_services, message: *const c_char) -> Result<(), Threw> {
    if subsystems::log(host, message) {
        Ok(())
    } else {
        Err(Threw)
    }
}

/// Logs formatted text; it never contains an interior NUL.
fn log_text(host: &rsf_game_host_services, message: &str) -> Result<(), Threw> {
    let mut text = Vec::with_capacity(message.len() + 1);
    text.extend_from_slice(message.as_bytes());
    text.push(0);
    match CStr::from_bytes_with_nul(&text) {
        Ok(text) => log(host, text.as_ptr()),
        Err(_) => Ok(()),
    }
}

fn host_create(
    create: usize,
    setup: &HostSetup,
    services: &mut rsf_game_host_services,
) -> Result<rsf_result, Threw> {
    let mut result = NOT_READY;
    if subsystems::host_create(create, setup, services, &mut result) {
        Ok(result)
    } else {
        Err(Threw)
    }
}

fn set_active(activate: usize, user: *mut c_void, active: u32) -> Result<rsf_result, Threw> {
    let mut result = NOT_READY;
    if subsystems::host_set_active(activate, user, active, &mut result) {
        Ok(result)
    } else {
        Err(Threw)
    }
}

// Read without the lock by the detours. Only start sets it, under the lock. Quiesce and the
// withdraw callback clear it before taking the lock; every other clear happens under it.
static ACCEPTING: AtomicBool = AtomicBool::new(false);
static SESSION: Mutex<Session> = Mutex::new(Session::new());

fn session() -> MutexGuard<'static, Session> {
    lock_or_recover(&SESSION)
}

const fn empty_services() -> rsf_game_host_services {
    // SAFETY: the services table is plain data; all-zero is its empty state.
    unsafe { std::mem::zeroed() }
}

use rsf_game_sys::ffi_util::wide_path as terminated;
/// Quiesces the CPU, sampling and presentation-input subsystems and native creation.
fn quiesce_subsystems(creation_first: bool) {
    if creation_first {
        creation::quiesce();
    }
    subsystems::cpu_quiesce();
    subsystems::sampling_quiesce();
    subsystems::presentation_quiesce();
    if !creation_first {
        creation::quiesce();
    }
}

/// The per-monitor-v2 startup request, once per process before any observer hook exists. A
/// request that does not verify is logged; prepare continues.
fn request_dpi(host: &rsf_game_host_services) -> Result<(), Threw> {
    let startup = rsf_game_sys::dpi::request_per_monitor_v2(true);
    if host.log.is_some() {
        log_text(
            host,
            &format!(
                "Skyrim DPI startup per-monitor-v2 requested={} accepted={} prior_compatible={} verified={} process_windows={} error={}; window creation must use physical pixels.",
                u32::from(startup.requested),
                u32::from(startup.accepted),
                u32::from(startup.prior_compatible),
                u32::from(startup.verified),
                startup.process_windows,
                startup.error,
            ),
        )?;
    }
    Ok(())
}

/// The started subsystems, in start order. The plugin carrier holds the game's device creation
/// until start has returned (loader creation_gate.rs). Native creation goes first: it refuses a
/// device that already exists before any other hook is enabled.
#[derive(Clone, Copy)]
enum Stage {
    Creation,
    Observers,
    Sampling,
    Presentation,
    Cpu,
}

const STAGES: [Stage; 5] = [
    Stage::Creation,
    Stage::Observers,
    Stage::Sampling,
    Stage::Presentation,
    Stage::Cpu,
];

impl Stage {
    fn start(self, observers: &mut Control, services: &rsf_game_host_services) -> bool {
        match self {
            Self::Creation => subsystems::creation_start(),
            Self::Observers => observers.start(services),
            Self::Sampling => subsystems::sampling_start(services),
            Self::Presentation => subsystems::presentation_start(services),
            Self::Cpu => subsystems::cpu_start(),
        }
    }

    /// A stage retires only when it acknowledges cleanup.
    fn retire(self, observers: &mut Control) -> bool {
        match self {
            Self::Creation => subsystems::creation_stop(),
            Self::Observers => observers.disable(true),
            Self::Sampling => subsystems::sampling_stop(),
            Self::Presentation => subsystems::presentation_stop(),
            Self::Cpu => subsystems::cpu_disable(),
        }
    }

    fn refusal(self) -> &'static CStr {
        match self {
            Self::Observers => c"Skyrim start refused: observer hook enable failed.",
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

    fn suspend_host(&self) -> Result<rsf_result, Threw> {
        if self.activate != 0 && !self.bound_services.user.is_null() {
            set_active(self.activate, self.bound_services.user, 0)
        } else {
            Ok(OK)
        }
    }

    fn prepare(&mut self, host: &rsf_game_host_services) -> Result<rsf_result, Threw> {
        if self.prepared
            || self.retirement_pending
            || subsystems::observers_busy()
            || subsystems::cpu_busy()
        {
            return Ok(BUSY);
        }
        let admitted = subsystems::admit_observers();
        if admitted < 0 {
            return Ok(NOT_READY);
        }
        if admitted == 0 {
            // A site discovery refusal names the site.
            let refusal = subsystems::discovery_refusal();
            self.reason = if refusal.is_null() {
                OBSERVER_REFUSED.as_ptr()
            } else {
                refusal
            };
            log(host, self.reason)?;
            return Ok(NATIVE_REFUSED);
        }
        if !subsystems::checkpoint(&mut self.reason, true) {
            log(host, self.reason)?;
            return Ok(NATIVE_REFUSED);
        }
        if !self.dpi_requested {
            self.dpi_requested = true;
            request_dpi(host)?;
        }
        if !self.observers.prepare() {
            return Ok(NATIVE_REFUSED);
        }
        if !subsystems::cpu_prepare() {
            self.reason = CPU_REFUSED.as_ptr();
            return Ok(NATIVE_REFUSED);
        }
        self.services = *host;
        let platform = &Platform;
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
        let path = (runtime != 0)
            .then(|| unsafe { module_path(runtime as _) })
            .flatten()
            .filter(|_| create != 0 && self.activate != 0 && self.bind != 0);
        let Some(path) = path else {
            return Ok(NOT_READY);
        };
        let Some(separator) = path
            .units()
            .iter()
            .rposition(|&unit| unit == u16::from(b'\\'))
        else {
            return Ok(NOT_READY);
        };
        let directory = terminated(&path.units()[..separator], "");
        let ini = terminated(&path.units()[..separator], "\\ReScaleFrame.ini");
        let section = platform::wide("Rendering");
        let setup = HostSetup {
            struct_size: size_of::<HostSetup>() as u32,
            abi_version: rsf_game_sys::RSF_D3D11_HOST_ABI_VERSION,
            configuration_path: ini.as_ptr(),
            configuration_section: section.as_ptr(),
            runtime_directory: directory.as_ptr(),
            identity_utf8: c"skyrim-se".as_ptr(),
            executable_sha256: subsystems::executable_sha256().as_ptr(),
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
            accept_window: Some(subsystems::accept_window),
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
            let result = host_create(create, &setup, &mut self.services)?;
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
            log(&services, self.reason)?;
            return Ok(NATIVE_REFUSED);
        }
        if !subsystems::cpu_bind(&services) {
            return Ok(NATIVE_REFUSED);
        }
        subsystems::reset_observations();
        self.prepared = true;
        self.reason = PREPARED.as_ptr();
        Ok(OK)
    }

    fn refuse(&mut self, message: &'static CStr) -> Result<rsf_result, Threw> {
        self.reason = message.as_ptr();
        log(&self.services, self.reason)?;
        Ok(NATIVE_REFUSED)
    }

    fn rollback(&mut self) -> bool {
        for (index, stage) in STAGES.into_iter().enumerate().rev() {
            let bit = 1 << index;
            if self.rollback_stages & bit != 0 && stage.retire(&mut self.observers) {
                self.rollback_stages &= !bit;
            }
        }
        self.rollback_stages == 0
    }

    fn start(&mut self) -> Result<rsf_result, Threw> {
        if !self.prepared {
            return Ok(NOT_READY);
        }
        if self.retirement_pending {
            return Ok(BUSY);
        }
        if !subsystems::checkpoint(&mut self.reason, false) {
            log(&self.services, self.reason)?;
            return Ok(NATIVE_REFUSED);
        }
        if self.enabled {
            return Ok(if accepting() { OK } else { BUSY });
        }
        self.reason = START_IDENTITY.as_ptr();
        let admitted = subsystems::admit_observers();
        if admitted < 0 {
            return Ok(NOT_READY);
        }
        if admitted == 0 {
            log(&self.services, self.reason)?;
            return Ok(NATIVE_REFUSED);
        }
        let services = self.services;
        for (index, stage) in STAGES.into_iter().enumerate() {
            if stage.start(&mut self.observers, &services) {
                continue;
            }
            // Start can refuse after acquiring a subset of its hooks. Retire it too.
            self.rollback_stages = (1 << (index + 1)) - 1;
            let retired = self.rollback();
            self.retirement_pending = !retired;
            if !retired {
                return Ok(BUSY);
            }
            return self.refuse(stage.refusal());
        }
        self.enabled = true;
        if self.activate != 0 {
            let activated = set_active(self.activate, self.services.user, 1);
            if !matches!(activated, Ok(OK)) {
                self.retirement_pending = true;
                let _ = self.suspend_host();
                quiesce_subsystems(false);
                return activated;
            }
        }
        ACCEPTING.store(true, Ordering::Release);
        // Close a refusal racing start: do not republish a live engine source after the
        // compatibility callback already withdrew it on another thread.
        if !subsystems::checkpoint(&mut self.reason, false) {
            ACCEPTING.store(false, Ordering::Release);
            let suspended = self.suspend_host();
            quiesce_subsystems(false);
            self.retirement_pending = true;
            return match suspended {
                Ok(OK) => Ok(NATIVE_REFUSED),
                failure => failure,
            };
        }
        self.reason = ACTIVE.as_ptr();
        Ok(OK)
    }

    /// Every step runs; a host suspension that threw reports RSF_ERROR_NOT_READY after the rest.
    fn quiesce(&mut self) -> Result<rsf_result, Threw> {
        // A start holding the lock could have published after the public entry cleared it.
        ACCEPTING.store(false, Ordering::Release);
        self.retirement_pending |= self.prepared;
        let suspended = self.suspend_host();
        quiesce_subsystems(true);
        let suspended = suspended?;
        if suspended != OK {
            return Ok(suspended);
        }
        let busy = subsystems::observers_busy()
            || subsystems::cpu_busy()
            || subsystems::resolution_owned()
            || subsystems::auxiliaries_pending()
            || subsystems::sampling_busy()
            || subsystems::presentation_busy()
            || creation::busy();
        // The Runtime cannot call stop until quiesce succeeds. Retry failed start cleanup here.
        Ok(if busy || !self.rollback() { BUSY } else { OK })
    }

    /// Each refusal is retryable: a later stop resumes from the first stage that remains.
    fn stop(&mut self) -> Result<rsf_result, Threw> {
        ACCEPTING.store(false, Ordering::Release);
        self.retirement_pending |= self.prepared;
        let suspended = self.suspend_host()?;
        if suspended != OK {
            return Ok(suspended);
        }
        if subsystems::resolution_owned() || subsystems::auxiliaries_pending() {
            return Ok(BUSY);
        }
        if !subsystems::creation_stop()
            || !subsystems::presentation_stop()
            || !subsystems::sampling_stop()
        {
            return Ok(BUSY);
        }
        let cpu_stopped = subsystems::cpu_disable();
        if !self.observers.disable(false) {
            return Ok(BUSY);
        }
        self.enabled = false;
        if !cpu_stopped || subsystems::observers_busy() || !subsystems::cpu_stop() {
            return Ok(BUSY);
        }
        self.services = empty_services();
        self.prepared = false;
        self.retirement_pending = false;
        self.rollback_stages = 0;
        self.reason = STOPPED.as_ptr();
        Ok(OK)
    }
}

fn run(entry: impl FnOnce(&mut Session) -> Result<rsf_result, Threw>) -> rsf_result {
    entry(&mut session()).unwrap_or(NOT_READY)
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
    run(|session| session.prepare(&host))
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
    // Withdraw first, so a lifecycle call holding the lock cannot delay it.
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
    let session = session();
    let output = unsafe { &mut *output };
    output.prepared = u32::from(session.prepared);
    output.active = u32::from(ACCEPTING.load(Ordering::Acquire));
    output.rendering_ready = 0;
    output.reason = session.reason;
    if !crate::compatibility::render_allowed() {
        let compatibility = crate::compatibility::status();
        if !compatibility.is_null() {
            output.reason = compatibility;
        }
    }
    OK
}

pub(crate) fn accepting() -> bool {
    ACCEPTING.load(Ordering::Acquire)
}

/// Lends the services to a detour for its synchronous call: while accepting when `live`,
/// otherwise as retained. Returns the Runtime presentation binding export, or 0.
///
/// # Safety
/// `out` is null or writable for one services table.
pub(crate) unsafe fn lease(live: bool, out: *mut rsf_game_host_services) -> usize {
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
pub(crate) fn withdraw() {
    let was_active = ACCEPTING.swap(false, Ordering::AcqRel);
    quiesce_subsystems(false);
    // A successful start publishes the host services before accepting. A refusal while the
    // session is not accepting has no live host to suspend and must not take the lock, which a
    // lifecycle call on this thread may hold.
    if was_active {
        let session = session();
        if session.activate != 0 && !session.services.user.is_null() {
            let _ = set_active(session.activate, session.services.user, 0);
        }
    }
}

#[cfg(test)]
mod tests;
