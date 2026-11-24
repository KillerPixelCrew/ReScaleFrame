// SPDX-License-Identifier: MIT
//! Anomaly motion-stage policy. The native leaf supplies guarded producer facts,
//! the SEH-protected engine include calls, and the original engine and D3D11 trampolines.

pub(super) mod abi;
mod capture;
mod d3d11;
mod draw;
mod native;
mod options;
mod report;
pub(super) mod shaders;

use self::abi::*;
use self::capture::Capture;
use self::options::Settings;
use self::report::Counters;
use self::shaders::{Chain, Loader, Pack, Registry};
use super::{foliage, window, *};
use crate::engine_guards::{Image, guarded_copy, motion_guard_failure};
use crate::geometry_registry::RetirementQueue;
use crate::host_log::Log;
use crate::lease::Lease;
use crate::lock;
use crate::motion_device::*;
use crate::motion_profile::MotionProfile;
use crate::motion_target::{rsf_anomaly_motion_bind_close, rsf_anomaly_motion_bind_draw};
use crate::native_hooks::Site as HookSite;
use crate::sr_stage::DX10_MSAA;
use crate::sys::{
    RSF_ANOMALY_MOTION_BIND_ALREADY_BOUND as ALREADY_BOUND, RSF_ANOMALY_MOTION_BIND_BOUND as BOUND,
    RSF_ANOMALY_MOTION_BIND_CONFLICT as CONFLICT, RSF_ANOMALY_MOTION_BIND_FAULT as FAULT,
    RSF_ANOMALY_MOTION_BIND_UNBOUND as UNBOUND, rsf_anomaly_motion_bind_state as BindState,
};
use crate::sys::{
    RSF_MOTION_CREATED, RSF_MOTION_FAILED, RSF_MOTION_MULTISAMPLED, RSF_MOTION_READY,
    rsf_anomaly_motion_device_view as DeviceView,
};
use rsf_game_sys::rsf_game_log_fn;
use std::ffi::{CStr, CString};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{
    AtomicBool, AtomicU8,
    Ordering::{Acquire, Relaxed, Release},
};
use std::sync::{Mutex, OnceLock};

const MAIN: u8 = 0;

const NO_DRAW_ACTION: u8 = 0;
const RESTORE_AFTER_DRAW: u8 = 1;
const REPLAY_AFTER_DRAW: u8 = 2;

// The native detour owns the storage and calls finish on every exit.
struct DrawFlight {
    stage: &'static Stage,
    context: Raw,
    action: ActiveDraw,
}

enum ActiveDraw {
    Restore {
        stock: ShaderObjects,
        chain: Chain,
        restore_after: bool,
    },
    Replay {
        view: DeviceView,
        _leases: [Option<Lease>; 4],
    },
}

const _: () = {
    assert!(size_of::<DrawFlight>() <= size_of::<DrawScope>());
    assert!(std::mem::align_of::<DrawFlight>() <= std::mem::align_of::<DrawScope>());
};

struct WindowFacts {
    device: Raw,
    context: Raw,
    backend: Raw,
    position_rtv: Raw,
    position_surface: Raw,
    generation: u64,
    frame: u32,
    view: u8,
}

impl WindowFacts {
    fn read(target: Raw) -> Option<Self> {
        let device = native::shader_device();
        let context = native::context();
        if target.is_null()
            || device.is_null()
            || context.is_null()
            || native::render_target() != target
        {
            return None;
        }
        let position = native::position_views(target)?;
        let frame = native::native_frame()?;
        let view = native::render_view()?;
        let generation = crate::native_observer::resets();
        Some(Self {
            device,
            context,
            backend: native::backend(),
            position_rtv: position.rtv,
            position_surface: position.surface,
            generation,
            frame,
            view,
        })
    }
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain([0]).collect()
}

#[derive(Clone, Copy)]
struct Build {
    image: usize,
    /// native_profile.rs index; the motion and SR records share it.
    index: usize,
    size: u32,
    motion: &'static MotionProfile,
    engine: native::Engine,
}

#[derive(Default)]
enum DrawHooks {
    #[default]
    Cold,
    Installed,
    Refused,
}

#[derive(Default)]
struct Render {
    window: window::Window,
    draw_hooks: DrawHooks,
    history: MotionHistoryState,
    foliage: foliage::History,
    binder: BindState,
    device: Option<Owner>,
    capture: Option<Capture>,
    counters: Counters,
}

// Engine pointers in the window are borrowed by render callbacks. Access to
// render-owned state is serialized, and quiesce follows observer admission.
unsafe impl Send for Render {}

impl Render {
    fn reset_history(&mut self) {
        self.history.views = Default::default();
        self.foliage.reset();
    }

    fn device_view(&self) -> DeviceView {
        self.device
            .as_ref()
            .map_or_else(DeviceView::default, Owner::view)
    }

    fn install_draw_hooks(&mut self, context: Raw, log: Log) -> bool {
        match self.draw_hooks {
            DrawHooks::Installed => true,
            DrawHooks::Refused => false,
            DrawHooks::Cold => {
                let result = native::install_draw_hooks(context);
                self.draw_hooks = if result {
                    DrawHooks::Installed
                } else {
                    DrawHooks::Refused
                };
                if result {
                    log.text(c"Anomaly motion: immediate-context Draw/DrawIndexed/DrawInstanced/DrawIndexedInstanced detours installed.");
                } else {
                    log.text(c"Anomaly motion refused: the immediate context's draw entry points could not be hooked; shaders stay stock.");
                }
                result
            }
        }
    }

    fn close_binding(&mut self, context: Raw, backend: Raw) -> bool {
        let rtv = self.device_view().rtv;
        let result = rsf_anomaly_motion_bind_close(
            &mut self.binder,
            &native::BIND,
            context,
            &native::BIND_LAYOUT,
            backend.cast(),
            rtv,
        );
        match result {
            UNBOUND => self.counters.close_unbinds += 1,
            FAULT => self.counters.binder_faults += 1,
            _ => {}
        }
        result != FAULT
    }

    fn bind_draw(&mut self, backend: Raw, wanted: bool) {
        let rtv = self.device_view().rtv;
        let action = rsf_anomaly_motion_bind_draw(
            &mut self.binder,
            &native::BIND,
            self.window.context,
            &native::BIND_LAYOUT,
            backend.cast(),
            wanted.into(),
            if self.window.open {
                self.window.position_rtv
            } else {
                ptr::null_mut()
            },
            rtv,
        );
        match action {
            BOUND => self.counters.rebinds += 1,
            ALREADY_BOUND => self.counters.already_bound += 1,
            UNBOUND => self.counters.unbinds += 1,
            CONFLICT => self.counters.conflicts += 1,
            FAULT => self.counters.binder_faults += 1,
            _ => {}
        }
    }

    fn upload<const N: usize>(&mut self, context: Raw, kind: i32, bytes: &[u8; N]) -> bool {
        self.device
            .as_mut()
            .is_some_and(|device| device.upload(context, kind, bytes))
    }
}

struct Stage {
    lifecycle: Mutex<()>,
    build: Mutex<Option<Build>>,
    settings: Mutex<Settings>,
    logger: Mutex<Log>,
    render: Mutex<Render>,
    retirements: RetirementQueue,
    loader: Mutex<Loader>,
    registry: Mutex<Registry>,
    previous_report: Mutex<Counters>,
    active: AtomicBool,
    enabled: AtomicBool,
    foliage: AtomicBool,
    invalidate: AtomicBool,
    reload: AtomicBool,
    dump_shaders: AtomicBool,
    pack_status: AtomicU8,
}

static STAGE: OnceLock<Stage> = OnceLock::new();

pub(crate) fn record_retirement(key: InstanceKey) {
    if let Some(stage) = STAGE.get().filter(|stage| stage.active.load(Acquire)) {
        let mut queue = lock(&stage.retirements);
        if stage.active.load(Acquire) {
            queue.enqueue(key);
        }
    }
}

impl Stage {
    fn get() -> &'static Self {
        STAGE.get_or_init(|| Self {
            lifecycle: Default::default(),
            build: Default::default(),
            settings: Default::default(),
            logger: Default::default(),
            render: Default::default(),
            retirements: Default::default(),
            loader: Default::default(),
            registry: Default::default(),
            previous_report: Default::default(),
            active: AtomicBool::new(false),
            enabled: AtomicBool::new(false),
            foliage: AtomicBool::new(true),
            invalidate: AtomicBool::new(true),
            reload: AtomicBool::new(false),
            dump_shaders: AtomicBool::new(false),
            pack_status: AtomicU8::new(0),
        })
    }

    fn log(&self) -> Log {
        *lock(&self.logger)
    }

    fn install(&self, build: Build, hash: *const c_char) -> bool {
        let _lifecycle = lock(&self.lifecycle);
        if self.active.load(Acquire) {
            return true;
        }
        let log = self.log();
        let size = build.size;
        if build.image == 0 || size < 16 {
            log.text(c"Anomaly motion refused: image size differs from the verified build.");
            return false;
        }
        let old = *lock(&self.build);
        if old.is_some_and(|old| old.index != build.index || old.image != build.image) {
            log.text(c"Anomaly motion refused: a different build than the installed hooks.");
            return false;
        }
        if !native::hooks_created() {
            let image = Image::new(build.image as *const u8, size, guarded_copy);
            if let Some(site) = motion_guard_failure(&image, build.motion) {
                log.say(format!(
                    "Anomaly motion refused, shaders stay stock: guard mismatch at {}",
                    site.to_string_lossy()
                ));
                return false;
            }
        }
        if !crate::native_geometry::rsf_anomaly_geometry_active() {
            log.text(c"Anomaly motion refused: geometry hooks (draw-time site) are not active.");
            return false;
        }
        if !self.open_pack(build) {
            return false;
        }
        // Keep the same process-lifetime identity for the installed trampolines.
        *lock(&self.build) = Some(build);
        let foliage = self.foliage.load(Relaxed);
        native::observe_flush(true);
        crate::native_geometry::rsf_anomaly_geometry_set_foliage(foliage);
        self.active.store(true, Release);
        let sites = build.motion.sites.each_ref().map(|site| HookSite {
            name: site.name,
            rva: site.rva,
            expected: site.expected,
            replacement: ptr::null_mut(),
            original: ptr::null_mut(),
        });
        let installed = native::install_hooks(build.image, size, build.engine, &sites);
        if !installed {
            self.stop_observing();
            lock(&self.render).reset_history();
            log.text(
                c"Anomaly motion refused: guarded detour installation failed; shaders stay stock.",
            );
            return false;
        }
        self.attach();
        let settings = lock(&self.settings).capture;
        if settings.enabled {
            lock(&self.render).capture = Capture::bind(settings, hash, log);
        }
        log.say(format!(
            "Anomaly motion hooks installed: create_shader, phase_scene_begin/end, constants.flush slot; MotionInvalidate={} \
             MotionCapture={} (start={} count={} interval={}) MotionFoliage={}. The target is available to SR and SR-off FG.",
            u8::from(self.invalidate.load(Relaxed)), u8::from(settings.enabled), settings.start,
            settings.count, settings.interval, u8::from(foliage)));
        true
    }

    fn quiesce(&self) -> bool {
        let _lifecycle = lock(&self.lifecycle);
        if lock(&self.build).is_none() {
            return true;
        }
        self.stop_observing();
        let disabled = native::disable_hooks();
        self.cancel_draw();
        let mut render = lock(&self.render);
        window::reset(&mut render.window);
        // Models can retire while motion is inactive. Restart without borrowing old history.
        render.reset_history();
        if let Some(mut capture) = render.capture.take() {
            capture.cancel(&mut render.counters);
        }
        // Trampolines and cached shader/device leases survive quiesce, as
        // the engine can still have a draw callback in flight.
        disabled
    }

    fn stop_observing(&self) {
        self.active.store(false, Release);
        drop(lock(&self.retirements).take());
        native::observe_flush(false);
        crate::native_geometry::rsf_anomaly_geometry_set_foliage(false);
    }

    fn attach(&self) {
        if self.active.load(Acquire) {
            let loader = lock(&self.loader);
            if let Some(pack) = &loader.pack {
                let device = native::shader_device();
                if !device.is_null() {
                    unsafe { pack.attach(device) };
                }
            }
        }
    }

    fn open_window(&self, target: Raw) {
        if !self.active.load(Acquire) {
            return;
        }
        let mut render = lock(&self.render);
        if !self.active.load(Acquire) {
            return;
        }
        self.check_pack_requests(&mut render);
        if !self.enabled.load(Relaxed) {
            return;
        }
        let Some(facts) = WindowFacts::read(target) else {
            return;
        };
        if !render.install_draw_hooks(facts.context, self.log()) {
            return;
        }
        let current = render.device_view();
        if render.binder.cache_may_hold_motion != 0
            && !render.close_binding(facts.context, facts.backend)
        {
            window::reset(&mut render.window);
            return;
        }
        if current.device != facts.device {
            window::reset(&mut render.window);
            render.reset_history();
            lock(&self.registry).clear_links();
            let runtime = lock(&self.loader).pack.as_ref().map(Pack::constant_ops);
            let owner = render.device.get_or_insert_with(Owner::new);
            let log = self.log();
            if !owner.create(facts.device, runtime, log.callback, log.user as Raw) {
                log.text(c"Anomaly motion: device resources could not be created.");
                return;
            }
        }
        let fresh = window::begin(
            &mut render.window,
            facts.context,
            facts.position_rtv,
            facts.position_surface,
            facts.frame,
            facts.view,
            facts.generation,
        );
        let mut target_ok = render.window.target_ok;
        if fresh {
            let retired = lock(&self.retirements).take();
            render.counters.retirements += retired.len() as u64;
            for key in retired {
                render.history.retire(key);
            }
            let mut desc = MaybeUninit::uninit();
            let options = native::options_word3();
            if options.is_none() {
                render.counters.options_unreadable += 1;
            } else if options.is_some_and(|options| options & DX10_MSAA != 0) {
                render.counters.msaa_frames += 1;
            } else if !unsafe { d3d11::target_facts(facts.position_surface, desc.as_mut_ptr()) } {
                render.counters.descriptor_unreadable += 1;
            } else {
                let desc = unsafe { desc.assume_init() };
                match render.device.as_mut().map_or(RSF_MOTION_FAILED, |device| {
                    device.target(desc.width, desc.height, desc.samples, desc.layers)
                }) {
                    RSF_MOTION_CREATED => {
                        render.counters.target_created += 1;
                        target_ok = true;
                    }
                    RSF_MOTION_READY => target_ok = true,
                    RSF_MOTION_MULTISAMPLED => render.counters.target_samples_or_layers += 1,
                    _ => render.counters.target_failed += 1,
                }
            }
            if target_ok {
                let view = render.device_view();
                unsafe { crate::motion_device::clear_target(facts.context, view.rtv) };
                render.counters.clears += 1;
                let identity = FrameIdentity {
                    source: u64::from(facts.frame),
                    generation: facts.generation,
                    width: view.width,
                    height: view.height,
                };
                render.history.begin(lane(facts.view), identity);
                render.foliage.begin(facts.view, identity);
                render.counters.foliage_table_overflows = render.foliage.overflows;
            }
            render.counters.frames += 1;
            if facts.view == MAIN {
                let view = render.device_view();
                let Render {
                    capture, counters, ..
                } = &mut *render;
                if let Some(capture) = capture {
                    capture.frame(&facts, &view, target_ok, counters);
                }
            }
        }
        window::open(&mut render.window, target_ok);
        render.counters.windows += 1;
    }

    fn close_window(&self) {
        self.cancel_draw();
        let mut render = lock(&self.render);
        if !window::close(&mut render.window) {
            return;
        }
        let context = render.window.context;
        render.close_binding(context, native::backend());
        let texture = render.device_view().texture;
        let Render {
            capture,
            counters,
            window,
            ..
        } = &mut *render;
        if let Some(capture) = capture {
            capture.finish(window, texture, counters);
        }
    }

    fn frame_target(&self) -> FrameTarget {
        let render = lock(&self.render);
        if !self.active.load(Acquire) || !self.enabled.load(Relaxed) || !render.window.have_frame {
            return FrameTarget::default();
        }
        let view = render.device_view();
        FrameTarget {
            texture: view.texture,
            width: view.width,
            height: view.height,
            frame: render.window.frame,
            main: render.window.view == MAIN,
            written: render.window.target_ok && !view.texture.is_null(),
        }
    }

    fn cancel_draw(&self) {
        if let Some(draw) = window::take(ptr::null_mut()) {
            if let window::Action::Restore { stock, chain } = draw.action {
                unsafe { d3d11::set_shaders(draw.context, &stock, chain.tessellated()) };
            }
            lock(&self.render).counters.stale_restores += 1;
        }
    }

    fn begin_draw(&'static self, context: Raw) -> Option<DrawFlight> {
        let draw = window::take(context)?;
        let live = self.active.load(Acquire);
        let action = match draw.action {
            window::Action::Restore { stock, chain } => {
                if !live {
                    unsafe { d3d11::set_shaders(context, &stock, chain.tessellated()) };
                }
                ActiveDraw::Restore {
                    stock,
                    chain,
                    restore_after: live,
                }
            }
            window::Action::Replay if !live => return None,
            window::Action::Replay => {
                // Hold the replay resources through both draws without holding the owner lock.
                let render = lock(&self.render);
                let view = render.device_view();
                let leases =
                    [view.rtv, view.zero_shader, view.no_depth, view.equal_depth].map(|raw| {
                        Lease::new(raw, crate::motion_device::release).inspect(|lease| {
                            unsafe { crate::motion_device::retain(lease.as_ptr()) };
                        })
                    });
                ActiveDraw::Replay {
                    view,
                    _leases: leases,
                }
            }
        };
        Some(DrawFlight {
            stage: self,
            context,
            action,
        })
    }
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) unsafe fn configure(ini: *const u16, log: rsf_game_log_fn) {
    Stage::get().configure(
        ini,
        Log {
            callback: log,
            user: 0,
        },
    );
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn install(image: Raw, build: usize) -> bool {
    let stage = Stage::get();
    let records = (
        crate::discovery::native(build),
        MotionProfile::of(build),
        native::Engine::of(build),
    );
    let (Some(native), Some(motion), Some(engine)) = records else {
        stage
            .log()
            .text(c"Anomaly motion refused: its sites were not discovered (see the adapter log).");
        return false;
    };
    let selected = Build {
        image: image as usize,
        index: build,
        size: native.image_size,
        motion,
        engine,
    };
    stage.install(selected, native.sha256.as_ptr())
}
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn rsf_anomaly_motion_stage_quiesce() -> bool {
    STAGE.get().is_none_or(Stage::quiesce)
}

pub(crate) fn rsf_anomaly_motion_stage_report(emit: rsf_game_log_fn, user: Raw) {
    if let Some(stage) = STAGE
        .get()
        .filter(|s| s.active.load(Acquire) && emit.is_some())
    {
        stage.report(Log {
            callback: emit,
            user: user as usize,
        });
    }
}

pub(crate) fn frame_target() -> FrameTarget {
    STAGE
        .get()
        .map_or_else(FrameTarget::default, Stage::frame_target)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_stage_cancel_draw() {
    if let Some(stage) = STAGE.get() {
        stage.cancel_draw();
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_stage_draw_begin(
    context: Raw,
    scope: *mut DrawScope,
) -> u8 {
    let Some(flight) = STAGE.get().and_then(|stage| stage.begin_draw(context)) else {
        return NO_DRAW_ACTION;
    };
    let route = match &flight.action {
        ActiveDraw::Restore { .. } => RESTORE_AFTER_DRAW,
        ActiveDraw::Replay { .. } => REPLAY_AFTER_DRAW,
    };
    unsafe { scope.cast::<DrawFlight>().write(flight) };
    route
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_stage_draw_end(scope: *mut DrawScope, completed: bool) {
    let flight = unsafe { scope.cast::<DrawFlight>().read() };
    if let ActiveDraw::Restore {
        stock,
        chain,
        restore_after: true,
    } = flight.action
    {
        unsafe { d3d11::set_shaders(flight.context, &stock, chain.tessellated()) };
        if completed {
            lock(&flight.stage.render).counters.restores += 1;
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_stage_replay_begin(
    scope: *const DrawScope,
    replay: *mut ReplayScope,
) -> bool {
    let flight = unsafe { &*scope.cast::<DrawFlight>() };
    let ActiveDraw::Replay { view, .. } = &flight.action else {
        return false;
    };
    let stage = flight.stage;
    if !stage.active.load(Acquire) {
        return false;
    }
    let result = unsafe { rsf_anomaly_motion_replay_begin(view, flight.context, replay) };
    match result {
        REPLAY_READY => return true,
        REPLAY_NO_DEPTH_WRITE => lock(&stage.render).counters.replay_skipped_no_depth_write += 1,
        _ => lock(&stage.render).counters.replay_failed += 1,
    }
    false
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_stage_replay_end(
    scope: *const DrawScope,
    replay: *mut ReplayScope,
    completed: bool,
) {
    unsafe { rsf_anomaly_motion_replay_end(replay) };
    if completed {
        let flight = unsafe { &*scope.cast::<DrawFlight>() };
        lock(&flight.stage.render).counters.replays += 1;
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_stage_open(target: Raw) {
    if let Some(stage) = STAGE.get() {
        stage.open_window(target);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_stage_close() {
    if let Some(stage) = STAGE.get() {
        stage.close_window();
    }
}

fn rsf_anomaly_motion_stage_flush(facts: &DrawFacts) {
    if let Some(stage) = STAGE.get() {
        stage.flush(facts);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_stage_attach() {
    if let Some(stage) = STAGE.get() {
        stage.attach();
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_stage_shader(creation: &ShaderCreation) {
    if creation.status >= 0
        && let Some(stage) = STAGE.get().filter(|stage| stage.active.load(Acquire))
    {
        stage.admit_shader_creation(creation);
    }
}
