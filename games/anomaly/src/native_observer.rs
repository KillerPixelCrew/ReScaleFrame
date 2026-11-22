// SPDX-License-Identifier: MIT
//! Lifecycle and facts from the guarded native renderer boundaries.

#[cfg(test)]
mod tests;

use crate::engine_guards::{Image, NativeSite, Read, UniqueGuard};
use crate::executable::observer_build;
use crate::native_hooks::ImageHeaders;
use crate::native_observer_abi::{
    RSF_ANOMALY_OBSERVATION_ABI, rsf_anomaly_native_observation as Observation,
    rsf_anomaly_observer_camera as Camera, rsf_anomaly_observer_engine as Engine,
    rsf_anomaly_observer_frame as Frame, rsf_anomaly_observer_platform as Platform,
};
use crate::{ABI_MISMATCH, BuildIdentity, INVALID_ARGUMENT};
use rsf_game_sys::{
    RSF_GAME_ABI_VERSION, rsf_game_control_args, rsf_game_host_services, rsf_game_log_fn,
    rsf_game_prepare_args, rsf_game_renderer_status, rsf_game_start_args, rsf_result,
};
use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};
use std::{ptr, slice};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN, GetModuleHandleExW,
    GetModuleHandleW,
};

const OK: rsf_result = 0;
const NOT_READY: rsf_result = -3;
const BUSY: rsf_result = -4;
const NATIVE_REFUSED: rsf_result = -5;

#[repr(C)]
struct ObserverProfile {
    label: *const c_char,
    image_size: u32,
    sites: *const NativeSite,
    site_count: usize,
    unique_guards: *const UniqueGuard,
    unique_guard_count: usize,
}

const _: () = {
    assert!(size_of::<ObserverProfile>() == 48);
    assert!(std::mem::offset_of!(ObserverProfile, image_size) == 8);
    assert!(std::mem::offset_of!(ObserverProfile, sites) == 16);
    assert!(std::mem::offset_of!(ObserverProfile, unique_guards) == 32);
};

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_observer_identify(
    out: &mut Engine,
    builds: *const BuildIdentity,
    profiles: *const ObserverProfile,
    count: usize,
    headers: &ImageHeaders,
    read: Read,
) -> *const c_char {
    // Both tables borrow the same static C++ build order as the native detours.
    let builds = unsafe { slice::from_raw_parts(builds, count) };
    let build = match observer_build(builds) {
        Ok(build) => build,
        Err(reason) => return reason.as_ptr(),
    };
    let profile = unsafe { &*profiles.add(build) };
    let image = unsafe { GetModuleHandleW(ptr::null()) };
    // HookSet adds a mapped-region check and an NT header extent check on install.
    if !headers.matches(image, profile.image_size, usize::MAX) {
        return c"Running image size differs from the verified build.".as_ptr();
    }
    let checked = Image::new(image.cast(), profile.image_size, read);
    let sites = unsafe { slice::from_raw_parts(profile.sites, profile.site_count) };
    let guards = if profile.unique_guard_count == 0 {
        &[]
    } else {
        unsafe { slice::from_raw_parts(profile.unique_guards, profile.unique_guard_count) }
    };
    if !unsafe { checked.matches_sites(sites, guards) } {
        return c"A native hook's expected instructions do not match; no hook was written."
            .as_ptr();
    }
    *out = Engine {
        image,
        build,
        label: profile.label,
        executable: builds[build].executable,
    };
    ptr::null()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_observer_read_frame(
    image: &Image,
    frame: u32,
    width: u32,
    height: u32,
    out: &mut Frame,
) -> bool {
    let sample = || {
        Some(Frame {
            id: u32::from_le_bytes(image.copy(frame)?),
            width: u32::from_le_bytes(image.copy(width)?),
            height: u32::from_le_bytes(image.copy(height)?),
        })
    };
    let Some(frame) = sample() else {
        return false;
    };
    *out = frame;
    true
}

// One atomic records whether entry was admitted and registers that invocation.
// Inactive invocations also count until they return from the native trampoline.
const ACTIVE: u32 = 1 << 31;

#[derive(Copy, Clone)]
struct Session {
    engine: Engine,
    log: rsf_game_log_fn,
    user: *mut c_void,
}

// SAFETY: engine and profile data retain process lifetime, and the GameSDK
// log and user pointer are used only while a registered invocation holds
// the session lease. The observer never dereferences the host's user pointer.
unsafe impl Send for Session {}

impl Session {
    fn emit(&self, platform: &Platform, message: &CStr) {
        unsafe { (platform.log.unwrap())(self.log, self.user, message.as_ptr()) };
    }
}

struct Control {
    reason: &'static CStr,
    geometry_installed: bool,
    retirement: Retirement,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Retirement {
    Inactive,
    Live,
    Input,
    Overlay,
    Sr,
    Motion,
    ObserverHooks,
    Geometry,
    Callbacks,
}

struct State {
    session: Option<Session>,
    observation: Observation,
}

struct Observer {
    control: Mutex<Control>,
    state: Mutex<State>,
    activity: AtomicU32,
    fg_frame_open: fn() -> bool,
    fg_retire_inputs: fn() -> bool,
}

static OBSERVER: Observer =
    Observer::new(crate::fg_stage::frame_open, crate::fg_stage::retire_inputs);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Observer {
    const fn new(fg_frame_open: fn() -> bool, fg_retire_inputs: fn() -> bool) -> Self {
        Self {
            control: Mutex::new(Control {
                reason: c"Native rendering has not been prepared.",
                geometry_installed: false,
                retirement: Retirement::Inactive,
            }),
            state: Mutex::new(State {
                session: None,
                observation: Observation {
                    struct_size: size_of::<Observation>() as u32,
                    abi_version: RSF_ANOMALY_OBSERVATION_ABI,
                    // All remaining fields are integer counters or floating-point samples.
                    ..unsafe { std::mem::zeroed() }
                },
            }),
            activity: AtomicU32::new(0),
            fg_frame_open,
            fg_retire_inputs,
        }
    }

    fn enter(&self) -> bool {
        self.activity.fetch_add(1, Ordering::AcqRel) & ACTIVE != 0
    }

    fn leave(&self) {
        self.activity.fetch_sub(1, Ordering::Release);
    }

    fn active(&self) -> bool {
        self.activity.load(Ordering::Acquire) & ACTIVE != 0
    }

    fn observe_window(&self, report: impl FnOnce()) {
        if !self.active() {
            return;
        }
        let observing = self.enter();
        let _flight = Flight(self);
        if observing {
            report();
        }
    }

    fn session(&self) -> Option<Session> {
        lock(&self.state).session
    }

    fn try_control(&self) -> Option<MutexGuard<'_, Control>> {
        match self.control.try_lock() {
            Ok(control) => Some(control),
            Err(TryLockError::Poisoned(error)) => Some(error.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        }
    }

    fn prepare(&self, host: &rsf_game_host_services, platform: &Platform) -> rsf_result {
        let Some(mut control) = self.try_control() else {
            return BUSY;
        };
        if self.session().is_some()
            || control.retirement != Retirement::Inactive
            || self.activity.load(Ordering::Acquire) != 0
        {
            return BUSY;
        }
        let mut engine = Engine::default();
        let refusal = unsafe { (platform.identify.unwrap())(&mut engine) };
        if !refusal.is_null() {
            // Admission returns only static string literals.
            control.reason = unsafe { CStr::from_ptr(refusal) };
            return NATIVE_REFUSED;
        }
        lock(&self.state).session = Some(Session {
            engine,
            log: host.log,
            user: host.user,
        });
        control.reason = c"Exact native boundary sites prepared; temporal SR/FG is not validated.";
        OK
    }

    fn start(&self, platform: &Platform) -> rsf_result {
        let Some(mut control) = self.try_control() else {
            return BUSY;
        };
        let Some(session) = self.session() else {
            return NOT_READY;
        };
        if self.active() {
            return OK;
        }
        if !matches!(control.retirement, Retirement::Inactive | Retirement::Live) {
            let result = self.quiesce_locked(&mut control, platform);
            if result != OK {
                return result;
            }
        }
        // MinHook trampolines can still be reached after disable. Pin the
        // adapter before its first installation attempt.
        let mut pinned = ptr::null_mut();
        let flags = GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN;
        if unsafe {
            GetModuleHandleExW(
                flags,
                (rsf_anomaly_observer_start as *const ()).cast(),
                &mut pinned,
            )
        } == 0
        {
            control.reason = c"Could not pin the native adapter.";
            return NATIVE_REFUSED;
        }
        let engine = &session.engine;
        control.retirement = Retirement::Live;
        if !unsafe { (platform.enable_hooks.unwrap())(engine) } {
            control.reason = c"Guarded native detour installation refused.";
            return NATIVE_REFUSED;
        }
        control.geometry_installed = unsafe { (platform.install_geometry.unwrap())(engine) };
        let sr_refused =
            crate::sr_stage::STAGE.selected() && !unsafe { (platform.install_sr.unwrap())(engine) };
        let motion_refused = !unsafe { (platform.install_motion.unwrap())(engine) };
        let input_enabled = unsafe { (platform.enable_input.unwrap())() };
        let geometry_installed = control.geometry_installed;
        if input_enabled {
            crate::overlay::resume();
            self.activity.fetch_or(ACTIVE, Ordering::Release);
            control.reason = if geometry_installed {
                c"Native boundaries and geometry identity hooks active; observation does not establish temporal SR/FG execution."
            } else {
                c"Native boundaries active; geometry identity hooks refused; observation does not establish temporal SR/FG execution."
            };
        } else {
            let _ = self.quiesce_locked(&mut control, platform);
            control.reason = c"Input fact hook enable refused.";
        }

        // Logging may call the host back. Hold the host lease without keeping
        // the control mutex across the callback.
        self.enter();
        let _flight = Flight(self);
        drop(control);
        if sr_refused {
            session.emit(
                platform,
                c"Anomaly SR stage not installed; rendering is stock.",
            );
        }
        if motion_refused {
            session.emit(
                platform,
                c"Anomaly motion stage not installed; shaders stay stock.",
            );
        }
        if !input_enabled {
            return NATIVE_REFUSED;
        }
        let label = unsafe { CStr::from_ptr(engine.label) }.to_string_lossy();
        let executable = unsafe { CStr::from_ptr(engine.executable) }.to_string_lossy();
        let geometry = if geometry_installed {
            ", including geometry identity"
        } else {
            "; geometry identity hooks were refused"
        };
        if let Ok(message) = CString::new(format!(
            "Anomaly guarded native hooks installed on the {label} build ({executable}){geometry}. No executable file was modified."
        )) {
            session.emit(platform, &message);
        }
        OK
    }

    fn quiesce_locked(&self, control: &mut Control, platform: &Platform) -> rsf_result {
        if control.retirement == Retirement::Inactive {
            return OK;
        }
        if control.retirement == Retirement::Live {
            self.activity.fetch_and(!ACTIVE, Ordering::AcqRel);
            control.retirement = Retirement::Input;
        }
        if control.retirement == Retirement::Input {
            if !unsafe { (platform.disable_input.unwrap())() } {
                return NATIVE_REFUSED;
            }
            control.retirement = Retirement::Overlay;
        }
        if control.retirement == Retirement::Overlay {
            if !crate::overlay::suspend() {
                return BUSY;
            }
            control.retirement = Retirement::Sr;
        }
        // End and reset can still use stage resources after observation closes.
        // The inactive detours continue to supply the native geometry scope
        // until SR and motion have finished teardown.
        if !self.idle(platform) || (self.fg_frame_open)() {
            return BUSY;
        }
        if control.retirement == Retirement::Sr {
            if !unsafe { (platform.quiesce_sr.unwrap())() } {
                return NATIVE_REFUSED;
            }
            control.retirement = Retirement::Motion;
        }
        if control.retirement == Retirement::Motion {
            if !unsafe { (platform.quiesce_motion.unwrap())() } {
                return NATIVE_REFUSED;
            }
            control.retirement = Retirement::ObserverHooks;
        }
        if control.retirement == Retirement::ObserverHooks {
            if !unsafe { (platform.disable_hooks.unwrap())() } {
                return NATIVE_REFUSED;
            }
            control.retirement = Retirement::Geometry;
        }
        if control.retirement == Retirement::Geometry {
            if control.geometry_installed && !unsafe { (platform.disable_geometry.unwrap())() } {
                return NATIVE_REFUSED;
            }
            control.geometry_installed = false;
            control.retirement = Retirement::Callbacks;
        }
        if !self.idle(platform) || !(self.fg_retire_inputs)() {
            return BUSY;
        }
        control.retirement = Retirement::Inactive;
        OK
    }

    fn idle(&self, platform: &Platform) -> bool {
        self.activity.load(Ordering::Acquire) == 0 && unsafe { (platform.input_idle.unwrap())() }
    }

    fn control(&self, platform: &Platform, stop: bool) -> rsf_result {
        let Some(mut control) = self.try_control() else {
            return BUSY;
        };
        let result = self.quiesce_locked(&mut control, platform);
        if result == OK && stop {
            if !self.idle(platform) {
                control.retirement = Retirement::Callbacks;
                return BUSY;
            }
            lock(&self.state).session = None;
            control.reason =
                c"Native callbacks inactive; process-lifetime trampolines remain owned by the pinned adapter.";
        }
        result
    }

    fn status(&self) -> Option<rsf_game_renderer_status> {
        let control = self.try_control()?;
        Some(rsf_game_renderer_status {
            struct_size: size_of::<rsf_game_renderer_status>() as u32,
            abi_version: RSF_GAME_ABI_VERSION,
            prepared: u32::from(self.session().is_some()),
            active: u32::from(self.active()),
            rendering_ready: 0,
            reason: control.reason.as_ptr(),
        })
    }

    fn observation(&self) -> Observation {
        let state = lock(&self.state);
        Observation {
            active: u32::from(self.active()),
            ..state.observation
        }
    }

    fn render_begin(&self, platform: &Platform) {
        let Some(session) = self.session() else {
            return;
        };
        let mut frame = Frame::default();
        let read = unsafe { (platform.read_frame.unwrap())(&session.engine, &mut frame) };
        {
            let mut state = lock(&self.state);
            let o = &mut state.observation;
            o.render_frames = o.render_frames.wrapping_add(1);
            let sample = if read { frame } else { Frame::default() };
            o.engine_frame = sample.id;
            o.width = sample.width;
            o.height = sample.height;
        }
        if read {
            unsafe { (platform.render_begin.unwrap())(frame.id, frame.width, frame.height) };
        }
    }

    fn end_before(&self, platform: &Platform, observing: bool) -> bool {
        // A frame can remain open between callbacks after quiesce has closed
        // admission. Its actual End still owns the matching Runtime scope.
        if !observing && !(self.fg_frame_open)() {
            return false;
        }
        unsafe {
            (platform.sr_frame_end.unwrap())();
            (platform.end_before.unwrap())();
        }
        true
    }

    fn end_after(&self, platform: &Platform) {
        unsafe { (platform.end_after.unwrap())() };
        let o = self.observation();
        if o.render_frames != 1 && !o.render_frames.is_multiple_of(120) {
            return;
        }
        let Some(session) = self.session() else {
            return;
        };
        if let Ok(message) = CString::new(format!(
            "Anomaly native boundaries engine_frame={} render_frames={} scene={} hud={} pda={} scope={} camera={} output={}x{} reset={}; SR/FG execution not established",
            o.engine_frame,
            o.render_frames,
            o.scene_passes,
            o.hud_passes,
            o.pda_copies,
            o.scope_copies,
            o.camera_updates,
            o.width,
            o.height,
            o.resets,
        )) {
            session.emit(platform, &message);
        }
        unsafe { (platform.report_stages.unwrap())(session.log, session.user) };
    }

    fn camera(&self, platform: &Platform, view: *const f32, projection: *const f32) {
        if view.is_null() || projection.is_null() {
            return;
        }
        let mut sample = Camera::default();
        let read = unsafe { (platform.read_camera.unwrap())(view, projection, &mut sample) };
        {
            let mut state = lock(&self.state);
            let o = &mut state.observation;
            o.camera_finite = 0;
            if read {
                o.camera_updates = o.camera_updates.wrapping_add(1);
                o.view = sample.view;
                o.projection = sample.projection;
                o.camera_finite = u32::from(
                    sample
                        .view
                        .iter()
                        .chain(&sample.projection)
                        .all(|v| v.is_finite()),
                );
            }
        }
        if read {
            unsafe { (platform.camera.unwrap())(sample.view.as_ptr(), sample.projection.as_ptr()) };
        }
    }

    fn update(&self, f: impl FnOnce(&mut Observation)) {
        f(&mut lock(&self.state).observation);
    }
}

struct Flight<'a>(&'a Observer);

impl Drop for Flight<'_> {
    fn drop(&mut self) {
        self.0.leave();
    }
}

pub(crate) fn observe_window(report: impl FnOnce()) {
    OBSERVER.observe_window(report);
}

unsafe fn validate<T>(value: *const T, abi: u32) -> Result<(), rsf_result> {
    let header = value.cast::<u32>();
    if value.is_null() || unsafe { header.read() } < size_of::<T>() as u32 {
        return Err(INVALID_ARGUMENT);
    }
    if unsafe { header.add(1).read() } != abi {
        return Err(ABI_MISMATCH);
    }
    Ok(())
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_anomaly_observer_prepare(
    args: *const rsf_game_prepare_args,
    platform: &Platform,
) -> rsf_result {
    if let Err(error) = unsafe { validate(args, RSF_GAME_ABI_VERSION) } {
        return error;
    }
    let host = unsafe { (*args).host };
    if let Err(error) = unsafe { validate(host, RSF_GAME_ABI_VERSION) } {
        return error;
    }
    OBSERVER.prepare(unsafe { &*host }, platform)
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_anomaly_observer_start(
    args: *const rsf_game_start_args,
    platform: &Platform,
) -> rsf_result {
    match unsafe { validate(args, RSF_GAME_ABI_VERSION) } {
        Ok(()) => OBSERVER.start(platform),
        Err(error) => error,
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_anomaly_observer_quiesce(
    args: *const rsf_game_control_args,
    platform: &Platform,
) -> rsf_result {
    match unsafe { validate(args, RSF_GAME_ABI_VERSION) } {
        Ok(()) => OBSERVER.control(platform, false),
        Err(error) => error,
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_anomaly_observer_stop(
    args: *const rsf_game_control_args,
    platform: &Platform,
) -> rsf_result {
    match unsafe { validate(args, RSF_GAME_ABI_VERSION) } {
        Ok(()) => OBSERVER.control(platform, true),
        Err(error) => error,
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_anomaly_observer_status(
    out: *mut rsf_game_renderer_status,
) -> rsf_result {
    if let Err(error) = unsafe { validate(out, RSF_GAME_ABI_VERSION) } {
        return error;
    }
    let Some(status) = OBSERVER.status() else {
        return BUSY;
    };
    unsafe { out.write(status) };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_observation(out: *mut Observation) -> rsf_result {
    if let Err(error) = unsafe { validate(out, RSF_ANOMALY_OBSERVATION_ABI) } {
        return error;
    }
    unsafe { out.write(OBSERVER.observation()) };
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_observer_enter() -> bool {
    OBSERVER.enter()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_observer_leave() {
    OBSERVER.leave();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_observer_render_begin(platform: &Platform) {
    OBSERVER.render_begin(platform);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_observer_end_before(platform: &Platform, observing: bool) -> bool {
    OBSERVER.end_before(platform, observing)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_observer_end_after(platform: &Platform) {
    OBSERVER.end_after(platform);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_observer_camera_sample(
    platform: &Platform,
    view: *const f32,
    projection: *const f32,
) {
    OBSERVER.camera(platform, view, projection);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_observer_scene() {
    OBSERVER.update(|o| o.scene_passes = o.scene_passes.wrapping_add(1));
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_observer_ui(platform: &Platform) {
    OBSERVER.update(|o| o.hud_passes = o.hud_passes.wrapping_add(1));
    unsafe { (platform.ui_boundary.unwrap())() };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_observer_copy_target(platform: &Platform, target: u32) {
    match target {
        1 => OBSERVER.update(|o| o.pda_copies = o.pda_copies.wrapping_add(1)),
        2 => {
            OBSERVER.update(|o| o.scope_copies = o.scope_copies.wrapping_add(1));
            unsafe { (platform.scope_frame.unwrap())() };
        }
        _ => (),
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_observer_reset(platform: &Platform) {
    OBSERVER.update(|o| {
        o.resets = o.resets.wrapping_add(1);
        o.generation = o.generation.wrapping_add(1);
        o.camera_finite = 0;
    });
    unsafe { (platform.device_reset.unwrap())() };
}
