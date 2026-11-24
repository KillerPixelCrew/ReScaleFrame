// SPDX-License-Identifier: MIT
//! Lifecycle and facts from the guarded native renderer boundaries.

mod report;
#[cfg(test)]
mod tests;

// The native installs, the observer hook set and the guarded reads; the tests' fakes under test.
#[cfg(not(test))]
mod leaves {
    use super::{Engine, Frame, HOOK_COUNT, PROFILES};
    use crate::engine_guards::Image;
    pub(super) use crate::engine_guards::guarded_copy;
    use crate::native_hooks::{self, HookSet};
    use std::ffi::c_void;
    use std::ptr;

    unsafe extern "C" {
        // native_observer_platform.c: the detours in RSF_ANOMALY_OBSERVER_* order and their trampoline slots.
        static rsf_anomaly_observer_detours: [*mut c_void; HOOK_COUNT];
        static mut rsf_anomaly_observer_trampolines: [*mut c_void; HOOK_COUNT];
        // native_geometry_platform.c.
        fn rsf_anomaly_geometry_install_native(image: *mut c_void, build: usize) -> bool;
        fn rsf_anomaly_geometry_disable_native() -> bool;
    }

    static HOOKS: HookSet = HookSet::new(ptr::null_mut());

    pub(super) fn enable_hooks(engine: &Engine) -> bool {
        if native_hooks::created(&HOOKS) {
            return native_hooks::set_enabled(&HOOKS, true);
        }
        let trampolines = (&raw mut rsf_anomaly_observer_trampolines).cast::<*mut c_void>();
        let detours = unsafe { &*(&raw const rsf_anomaly_observer_detours) };
        super::sites(engine.build, detours, trampolines)
            .is_some_and(|(size, sites)| native_hooks::install(&HOOKS, engine.image, size, &sites))
    }

    pub(super) fn disable_hooks() -> bool {
        native_hooks::set_enabled(&HOOKS, false)
    }

    pub(super) fn install_geometry(engine: &Engine) -> bool {
        unsafe { rsf_anomaly_geometry_install_native(engine.image, engine.build) }
    }

    pub(super) fn disable_geometry() -> bool {
        unsafe { rsf_anomaly_geometry_disable_native() }
    }

    pub(super) fn install_sr(engine: &Engine) -> bool {
        crate::sr_stage::native::rsf_anomaly_sr_stage_install_build(engine.image, engine.build)
    }

    pub(super) fn install_motion(engine: &Engine) -> bool {
        crate::motion::stage::install(engine.image, engine.build)
    }

    pub(super) fn enable_input() -> bool {
        crate::engine_slots::rsf_anomaly_slot_set(crate::engine_slots::INPUT as u32, true)
    }

    pub(super) fn disable_input() -> bool {
        crate::engine_slots::rsf_anomaly_slot_set(crate::engine_slots::INPUT as u32, false)
    }

    pub(super) fn read_frame(engine: &Engine) -> Option<Frame> {
        let profile = PROFILES.get(engine.build)?;
        let image = Image::new(engine.image.cast(), profile.image_size, guarded_copy);
        Some(Frame {
            id: u32::from_le_bytes(image.copy(profile.device_frame_rva)?),
            width: u32::from_le_bytes(image.copy(profile.device_width_rva)?),
            height: u32::from_le_bytes(image.copy(profile.device_height_rva)?),
        })
    }
}
#[cfg(test)]
use tests::leaves;
#[cfg(not(test))]
mod stages {
    pub(super) use crate::fg_stage::{
        rsf_anomaly_fg_stage_camera as camera, rsf_anomaly_fg_stage_device_reset as device_reset,
        rsf_anomaly_fg_stage_end_after as end_after, rsf_anomaly_fg_stage_end_before as end_before,
        rsf_anomaly_fg_stage_render_begin as render_begin,
        rsf_anomaly_fg_stage_scope as scope_frame, rsf_anomaly_fg_stage_ui_boundary as ui_boundary,
    };
    pub(super) use crate::input::rsf_anomaly_input_idle as input_idle;
    pub(super) use crate::motion::stage::rsf_anomaly_motion_stage_quiesce as quiesce_motion;
    pub(super) use crate::sr_stage::{
        rsf_anomaly_sr_stage_frame_end as sr_frame_end, rsf_anomaly_sr_stage_quiesce as quiesce_sr,
    };
}
#[cfg(test)]
use tests::stages;

use crate::engine_guards::{Image, guarded_copy};
use crate::executable::observer_build;
use crate::lock;
use crate::native_hooks::{Site, image_matches, read_guarded};
use crate::native_profile::{HOOK_COUNT, PROFILES};
use crate::sys::{
    RSF_ANOMALY_OBSERVATION_ABI, RSF_ANOMALY_OBSERVER_BEGIN, RSF_ANOMALY_OBSERVER_CAMERA,
    RSF_ANOMALY_OBSERVER_END, RSF_ANOMALY_OBSERVER_RESET_BEGIN, RSF_ANOMALY_OBSERVER_RESET_END,
    RSF_ANOMALY_OBSERVER_SCENE, RSF_ANOMALY_OBSERVER_TARGET_COPY, RSF_ANOMALY_OBSERVER_UI,
    rsf_anomaly_native_observation as Observation,
};
use rsf_game_sys::call_header::{validate, validate_abi};
use rsf_game_sys::{
    RSF_ERROR_BUSY as BUSY, RSF_ERROR_NATIVE_REFUSED as NATIVE_REFUSED,
    RSF_ERROR_NOT_READY as NOT_READY, RSF_GAME_ABI_VERSION, RSF_OK as OK, rsf_game_host_services,
    rsf_game_log_fn, rsf_game_renderer_status, rsf_game_start_args, rsf_result,
};
use std::ffi::{CStr, c_void};
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, TryLockError};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN, GetModuleHandleExW,
    GetModuleHandleW,
};

/// The running image and the exact build prepare() selected.
#[derive(Clone, Copy)]
struct Engine {
    #[cfg_attr(test, allow(dead_code))]
    image: *mut c_void,
    build: usize,
}

#[derive(Clone, Copy, Default)]
struct Frame {
    id: u32,
    width: u32,
    height: u32,
}

fn identify() -> Result<Engine, &'static CStr> {
    let build = observer_build()?;
    let profile = PROFILES
        .get(build)
        .ok_or(c"Native observer build index is out of range.")?;
    let image = unsafe { GetModuleHandleW(ptr::null()) };
    // Hook-set installation adds a mapped-region check and an NT header extent check.
    if !image_matches(read_guarded, image, profile.image_size, usize::MAX) {
        return Err(c"Running image size differs from the verified build.");
    }
    let checked = Image::new(image.cast(), profile.image_size, guarded_copy);
    if !unsafe { checked.matches_sites(&profile.sites, &profile.guards) } {
        return Err(c"A native hook's expected instructions do not match; no hook was written.");
    }
    Ok(Engine { image, build })
}

/// The profile's observer sites with the native detours and their trampoline slots, and the image size.
fn sites(
    build: usize,
    detours: &[*mut c_void; HOOK_COUNT],
    trampolines: *mut *mut c_void,
) -> Option<(u32, [Site; HOOK_COUNT])> {
    let profile = PROFILES.get(build)?;
    let sites = std::array::from_fn(|index| {
        let site = &profile.sites[index];
        Site {
            name: site.name,
            rva: site.rva,
            expected: site.expected,
            replacement: detours[index],
            original: trampolines.wrapping_add(index),
        }
    });
    Some((profile.image_size, sites))
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
    fn emit(&self, message: &str) {
        unsafe { rsf_game_sys::log_guarded(self.log, self.user, message) };
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

    fn prepare(&self, host: &rsf_game_host_services) -> rsf_result {
        let Some(mut control) = self.try_control() else {
            return BUSY;
        };
        if self.session().is_some()
            || control.retirement != Retirement::Inactive
            || self.activity.load(Ordering::Acquire) != 0
        {
            return BUSY;
        }
        let engine = match identify() {
            Ok(engine) => engine,
            Err(reason) => {
                control.reason = reason;
                return NATIVE_REFUSED;
            }
        };
        lock(&self.state).session = Some(Session {
            engine,
            log: host.log,
            user: host.user,
        });
        control.reason = c"Exact native boundary sites prepared; temporal SR/FG is not validated.";
        OK
    }

    fn start(&self) -> rsf_result {
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
            let result = self.quiesce_locked(&mut control);
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
        if !leaves::enable_hooks(engine) {
            control.reason = c"Guarded native detour installation refused.";
            return NATIVE_REFUSED;
        }
        control.geometry_installed = leaves::install_geometry(engine);
        let sr_refused = crate::sr_stage::STAGE.selected() && !leaves::install_sr(engine);
        let motion_refused = !leaves::install_motion(engine);
        let input_enabled = leaves::enable_input();
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
            let _ = self.quiesce_locked(&mut control);
            control.reason = c"Input fact hook enable refused.";
        }

        // Logging may call the host back. Hold the host lease without keeping
        // the control mutex across the callback.
        self.enter();
        let _flight = Flight(self);
        drop(control);
        if sr_refused {
            session.emit("Anomaly SR stage not installed; rendering is stock.");
        }
        if motion_refused {
            session.emit("Anomaly motion stage not installed; shaders stay stock.");
        }
        if !input_enabled {
            return NATIVE_REFUSED;
        }
        let profile = &PROFILES[engine.build];
        let label = profile.label.to_string_lossy();
        let executable = profile.executable.to_string_lossy();
        let geometry = if geometry_installed {
            ", including geometry identity"
        } else {
            "; geometry identity hooks were refused"
        };
        session.emit(&format!(
            "Anomaly guarded native hooks installed on the {label} build ({executable}){geometry}. No executable file was modified."
        ));
        OK
    }

    fn quiesce_locked(&self, control: &mut Control) -> rsf_result {
        if control.retirement == Retirement::Inactive {
            return OK;
        }
        if control.retirement == Retirement::Live {
            self.activity.fetch_and(!ACTIVE, Ordering::AcqRel);
            control.retirement = Retirement::Input;
        }
        if control.retirement == Retirement::Input {
            if !leaves::disable_input() {
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
        if !self.idle() || (self.fg_frame_open)() {
            return BUSY;
        }
        if control.retirement == Retirement::Sr {
            if !stages::quiesce_sr() {
                return NATIVE_REFUSED;
            }
            control.retirement = Retirement::Motion;
        }
        if control.retirement == Retirement::Motion {
            if !stages::quiesce_motion() {
                return NATIVE_REFUSED;
            }
            control.retirement = Retirement::ObserverHooks;
        }
        if control.retirement == Retirement::ObserverHooks {
            if !leaves::disable_hooks() {
                return NATIVE_REFUSED;
            }
            control.retirement = Retirement::Geometry;
        }
        if control.retirement == Retirement::Geometry {
            if control.geometry_installed && !leaves::disable_geometry() {
                return NATIVE_REFUSED;
            }
            control.geometry_installed = false;
            control.retirement = Retirement::Callbacks;
        }
        if !self.idle() || !(self.fg_retire_inputs)() {
            return BUSY;
        }
        control.retirement = Retirement::Inactive;
        OK
    }

    fn idle(&self) -> bool {
        self.activity.load(Ordering::Acquire) == 0 && stages::input_idle()
    }

    fn control(&self, stop: bool) -> rsf_result {
        let Some(mut control) = self.try_control() else {
            return BUSY;
        };
        let result = self.quiesce_locked(&mut control);
        if result == OK && stop {
            if !self.idle() {
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

    fn render_begin(&self) {
        let Some(session) = self.session() else {
            return;
        };
        let frame = leaves::read_frame(&session.engine);
        {
            let mut state = lock(&self.state);
            let o = &mut state.observation;
            o.render_frames = o.render_frames.wrapping_add(1);
            let sample = frame.unwrap_or_default();
            o.engine_frame = sample.id;
            o.width = sample.width;
            o.height = sample.height;
        }
        if let Some(frame) = frame {
            stages::render_begin(frame.id, frame.width, frame.height);
        }
    }

    fn end_before(&self, observing: bool) -> bool {
        // A frame can remain open between callbacks after quiesce has closed
        // admission. Its actual End still owns the matching Runtime scope.
        if !observing && !(self.fg_frame_open)() {
            return false;
        }
        stages::sr_frame_end();
        stages::end_before();
        true
    }

    fn end_after(&self) {
        stages::end_after();
        let o = self.observation();
        if o.render_frames != 1 && !o.render_frames.is_multiple_of(120) {
            return;
        }
        let Some(session) = self.session() else {
            return;
        };
        session.emit(&format!(
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
        ));
        report::stages(session);
    }

    fn camera(&self, view: *const f32, projection: *const f32) {
        if view.is_null() || projection.is_null() {
            return;
        }
        let (mut sample_view, mut sample_projection) = ([0f32; 16], [0f32; 16]);
        let copy = leaves::guarded_copy;
        let read = unsafe {
            copy(
                sample_view.as_mut_ptr().cast(),
                view.cast(),
                size_of_val(&sample_view),
            ) && copy(
                sample_projection.as_mut_ptr().cast(),
                projection.cast(),
                size_of_val(&sample_projection),
            )
        };
        {
            let mut state = lock(&self.state);
            let o = &mut state.observation;
            o.camera_finite = 0;
            if read {
                o.camera_updates = o.camera_updates.wrapping_add(1);
                o.view = sample_view;
                o.projection = sample_projection;
                o.camera_finite = u32::from(
                    sample_view
                        .iter()
                        .chain(&sample_projection)
                        .all(|v| v.is_finite()),
                );
            }
        }
        if read {
            unsafe { stages::camera(sample_view.as_ptr(), sample_projection.as_ptr()) };
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

/// Device resets the observer has counted, for the motion stage's history generation.
pub(crate) fn resets() -> u64 {
    lock(&OBSERVER.state).observation.resets
}

pub(crate) fn observe_window(report: impl FnOnce()) {
    OBSERVER.observe_window(report);
}

#[cfg(feature = "native-entry")]
pub(crate) unsafe fn rsf_anomaly_observer_prepare(
    args: *const rsf_game_sys::rsf_game_prepare_args,
) -> rsf_result {
    let result = unsafe { validate(args) };
    if result != OK {
        return result;
    }
    let host = unsafe { (*args).host };
    let result = unsafe { validate(host) };
    if result != OK {
        return result;
    }
    OBSERVER.prepare(unsafe { &*host })
}

pub(crate) unsafe fn rsf_anomaly_observer_start(args: *const rsf_game_start_args) -> rsf_result {
    match unsafe { validate(args) } {
        OK => OBSERVER.start(),
        error => error,
    }
}

#[cfg(feature = "native-entry")]
pub(crate) unsafe fn rsf_anomaly_observer_quiesce(
    args: *const rsf_game_sys::rsf_game_control_args,
) -> rsf_result {
    match unsafe { validate(args) } {
        OK => OBSERVER.control(false),
        error => error,
    }
}

#[cfg(feature = "native-entry")]
pub(crate) unsafe fn rsf_anomaly_observer_stop(
    args: *const rsf_game_sys::rsf_game_control_args,
) -> rsf_result {
    match unsafe { validate(args) } {
        OK => OBSERVER.control(true),
        error => error,
    }
}

#[cfg(feature = "native-entry")]
pub(crate) unsafe extern "C" fn rsf_anomaly_observer_status(
    out: *mut rsf_game_renderer_status,
) -> rsf_result {
    let result = unsafe { validate(out) };
    if result != OK {
        return result;
    }
    let Some(status) = OBSERVER.status() else {
        return BUSY;
    };
    unsafe { out.write(status) };
    OK
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_anomaly_observation(out: *mut Observation) -> rsf_result {
    let result = unsafe { validate_abi(out, RSF_ANOMALY_OBSERVATION_ABI) };
    if result != OK {
        return result;
    }
    unsafe { out.write(OBSERVER.observation()) };
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_observer_before(boundary: u32) -> bool {
    let observing = OBSERVER.enter();
    if boundary == RSF_ANOMALY_OBSERVER_END {
        // A frame opened before quiesce can still own the matching End.
        return OBSERVER.end_before(observing);
    }
    if !observing || boundary == RSF_ANOMALY_OBSERVER_RESET_END {
        return false;
    }
    match boundary {
        RSF_ANOMALY_OBSERVER_BEGIN
        | RSF_ANOMALY_OBSERVER_CAMERA
        | RSF_ANOMALY_OBSERVER_TARGET_COPY => return true,
        RSF_ANOMALY_OBSERVER_SCENE => {
            OBSERVER.update(|o| o.scene_passes = o.scene_passes.wrapping_add(1))
        }
        RSF_ANOMALY_OBSERVER_UI => {
            OBSERVER.update(|o| o.hud_passes = o.hud_passes.wrapping_add(1));
            stages::ui_boundary();
        }
        RSF_ANOMALY_OBSERVER_RESET_BEGIN => {
            OBSERVER.update(|o| {
                o.resets = o.resets.wrapping_add(1);
                o.generation = o.generation.wrapping_add(1);
                o.camera_finite = 0;
            });
            stages::device_reset();
        }
        _ => {}
    }
    false
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_observer_after(
    boundary: u32,
    view: *const f32,
    projection: *const f32,
    target: u32,
) {
    match boundary {
        RSF_ANOMALY_OBSERVER_BEGIN => OBSERVER.render_begin(),
        RSF_ANOMALY_OBSERVER_END => OBSERVER.end_after(),
        RSF_ANOMALY_OBSERVER_CAMERA => OBSERVER.camera(view, projection),
        RSF_ANOMALY_OBSERVER_TARGET_COPY => match target {
            1 => OBSERVER.update(|o| o.pda_copies = o.pda_copies.wrapping_add(1)),
            2 => {
                OBSERVER.update(|o| o.scope_copies = o.scope_copies.wrapping_add(1));
                stages::scope_frame();
            }
            _ => {}
        },
        _ => {}
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_observer_leave() {
    OBSERVER.leave();
}
