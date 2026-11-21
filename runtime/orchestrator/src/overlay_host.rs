// SPDX-License-Identifier: MIT
// Owner of the shared D3D11 overlay host behind loader/proxy/src/overlay_host.h.
//
// AC7's embedded bridge, the Runtime overlay service and the D3D11-on-12 path all draw the same
// panel through this one host. It decides lifecycle, owns the panel, renderer and device, runs the
// startup hint, visibility, input and texture sequencing, admits draws and targets, handles
// failures and keeps frame time. The native leaves in overlay_host_platform.h only make the
// loader, COM, input, renderer and Win32 calls.
use crate::com_ref::ComRef;
use crate::overlay_abi::{rsf_overlay_intent, rsf_overlay_stats, rsf_overlay_texture_update};
use crate::overlay_host_abi::{
    RSF_OVERLAY_ABI_VERSION, RSF_OVERLAY_HOST_PANEL_LOADED, RSF_OVERLAY_HOST_PANEL_MISSING_EXPORTS,
    RSF_OVERLAY_HOST_PANEL_NO_PATH, RSF_OVERLAY_HOST_PANEL_NOT_LOADED, rsf_overlay,
    rsf_overlay_draw_data, rsf_overlay_host_log, rsf_overlay_host_panel,
    rsf_overlay_host_platform as Platform, rsf_overlay_host_targets, rsf_overlay_host_texture_desc,
    rsf_overlay_input,
};
use std::ffi::{CString, c_char, c_void};
use std::fmt::Display;
use std::ptr::{self, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};

type Handle = *mut c_void;

/* The key that opens the panel. F6 through F11 are already claimed by the bridge, and the input
module's own default is F7, which is the debug display. Holding it does not toggle again; the
input module owns that contract. */
const TOGGLE_KEY: u32 = 0x2D; // VK_INSERT

/* Atlas changes are drained in batches rather than one at a time. egui sends the font atlas whole
once and then a patch per glyph first used, so a frame that introduces a word can produce
several at once and none of them can be skipped. */
const UPDATE_BATCH: usize = 8;

/* How many drawn frames still announce each step before taking it. The first frames lay out egui,
upload a whole font atlas and draw into a game's frame; the last line in the log names the step
that did not survive. */
const TRACE_FRAMES: u32 = 4;

const STARTUP_HINT_MS: u64 = 8000;
const STARTUP_FADE_MS: u64 = 1000;
const DEFAULT_DELTA: f32 = 1.0 / 60.0;
/* A frame that took longer is a mission load or a breakpoint, and handing egui that as a delta
makes every animation jump. */
const LONGEST_DELTA: f64 = 0.25;

fn leaf<T>(entry: Option<T>) -> T {
    entry.expect("overlay host platform entry")
}

struct Logger {
    log: rsf_overlay_host_log,
    user: usize,
}

/* Kept apart from the host so the renderer and the input subclass can announce lines from any
thread, including while the host is held. */
static LOGGER: Mutex<Logger> = Mutex::new(Logger { log: None, user: 0 });

fn set_logger(log: rsf_overlay_host_log, user: Handle) {
    *LOGGER.lock().unwrap_or_else(PoisonError::into_inner) = Logger {
        log,
        user: user as usize,
    };
}

fn say(message: impl Display) {
    let (log, user) = {
        let logger = LOGGER.lock().unwrap_or_else(PoisonError::into_inner);
        (logger.log, logger.user)
    };
    let Some(log) = log else {
        return;
    };
    if let Ok(line) = CString::new(message.to_string()) {
        unsafe { log(user as Handle, line.as_ptr()) };
    }
}

unsafe extern "C" fn log_from_module(_user: Handle, message: *const c_char) {
    if message.is_null() {
        return;
    }
    say(unsafe { std::ffi::CStr::from_ptr(message) }.to_string_lossy());
}

/* MSVC's %p, which the existing log lines used. */
struct Pointer(*const c_void);

impl Display for Pointer {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "{:016X}", self.0 as usize)
    }
}

fn pointer(value: Option<&ComRef>) -> Pointer {
    Pointer(value.map_or(ptr::null(), |value| value.as_ptr()))
}

struct Hr(i32);

impl Display for Hr {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "0x{:08x}", self.0 as u32)
    }
}

fn hint_alpha(elapsed: u64) -> f32 {
    if elapsed < STARTUP_HINT_MS - STARTUP_FADE_MS {
        1.0
    } else {
        STARTUP_HINT_MS.saturating_sub(elapsed) as f32 / STARTUP_FADE_MS as f32
    }
}

/* Targets bound before the overlay binds its own, put back on every path out of a draw. The
saved references are released after they are rebound. */
struct SavedTargets<'a> {
    platform: &'a Platform,
    context: Handle,
    targets: rsf_overlay_host_targets,
}

impl<'a> SavedTargets<'a> {
    fn bind(platform: &'a Platform, context: Handle, view: Handle) -> Self {
        let mut targets = rsf_overlay_host_targets::default();
        unsafe { leaf(platform.save_targets)(context, &mut targets) };
        unsafe { leaf(platform.bind_target)(context, view) };
        Self {
            platform,
            context,
            targets,
        }
    }
}

impl Drop for SavedTargets<'_> {
    fn drop(&mut self) {
        unsafe { leaf(self.platform.restore_targets)(self.context, &self.targets) };
        for saved in self.targets.views.into_iter().chain([self.targets.depth]) {
            drop(unsafe { ComRef::from_owned(saved) });
        }
    }
}

struct Host {
    api: Option<rsf_overlay_host_panel>,
    panel: *mut rsf_overlay,
    renderer: Handle,
    device: Option<ComRef>,
    started: bool,
    /* Set when a start or frame failed in a way that will fail again every frame. The panel is
    closed and left closed rather than reporting the same line sixty times a second. A
    deliberate stop clears it. */
    failed: bool,
    trace_frames: u32,
    frequency: i64,
    last_frame: i64,
    hint_begin: u64,
    hint_dismissed: bool,
    shared_visible: bool,
}

// The panel, renderer and device are only used while the host is held. The device is
// free-threaded; its immediate context is acquired per draw on the drawing thread.
unsafe impl Send for Host {}

impl Host {
    const fn new() -> Self {
        Self {
            api: None,
            panel: null_mut(),
            renderer: null_mut(),
            device: None,
            started: false,
            failed: false,
            trace_frames: TRACE_FRAMES,
            frequency: 0,
            last_frame: 0,
            hint_begin: 0,
            hint_dismissed: false,
            shared_visible: false,
        }
    }

    fn panel_api(&mut self, p: &Platform) -> Option<rsf_overlay_host_panel> {
        if self.api.is_none() {
            let mut api = rsf_overlay_host_panel::default();
            let mut error = 0;
            let status = unsafe { leaf(p.load_panel)(&mut api, &mut error) } as i32;
            let complete = api.create.is_some()
                && api.destroy.is_some()
                && api.frame.is_some()
                && api.texture_updates.is_some()
                && api.textures_to_free.is_some();
            match status {
                RSF_OVERLAY_HOST_PANEL_LOADED if complete => self.api = Some(api),
                RSF_OVERLAY_HOST_PANEL_NO_PATH => {
                    say("overlay: cannot work out where the panel DLL is");
                }
                RSF_OVERLAY_HOST_PANEL_NOT_LOADED => say(format_args!(
                    "overlay: the panel DLL did not load, error {error}. Set RSF_OVERLAY_DLL to point at rescaleframe_overlay.dll"
                )),
                RSF_OVERLAY_HOST_PANEL_LOADED | RSF_OVERLAY_HOST_PANEL_MISSING_EXPORTS => {
                    say("overlay: the panel DLL loaded but does not export the five entry points");
                }
                _ => say("overlay: the panel DLL loader returned an unknown status"),
            }
        }
        self.api
    }

    /* Panel, renderer and input subclass in that order. Every refusal is final until a stop, so
    a caller retrying each frame does not reload the DLL and rebuild the renderer each time. */
    fn build(
        &mut self,
        p: &Platform,
        device: ComRef,
        window: impl FnOnce() -> Handle,
        refusals: Refusals,
    ) -> i32 {
        let Some(api) = self.panel_api(p) else {
            self.failed = true;
            return 0;
        };
        let panel = unsafe { leaf(api.create)(RSF_OVERLAY_ABI_VERSION) };
        if panel.is_null() {
            refusals.say_panel();
            self.failed = true;
            return 0;
        }
        let mut renderer = null_mut();
        let built = unsafe {
            leaf(p.renderer_create)(
                device.as_ptr(),
                Some(log_from_module),
                null_mut(),
                &mut renderer,
            )
        };
        if built != 0 {
            refusals.say(refusals.renderer);
            unsafe { leaf(api.destroy)(panel) };
            self.failed = true;
            return 0;
        }
        let window = window();
        let installed = if window.is_null() {
            refusals.say(refusals.window);
            None
        } else {
            let installed = unsafe {
                leaf(p.input_install)(window, TOGGLE_KEY, Some(log_from_module), null_mut())
            };
            if installed != 0 {
                refusals.say_input(installed);
            }
            Some(installed)
        };
        if installed != Some(0) {
            unsafe { leaf(p.renderer_destroy)(renderer) };
            unsafe { leaf(api.destroy)(panel) };
            self.failed = true;
            return 0;
        }

        let mut frequency = 0;
        if unsafe { leaf(p.frequency)(&mut frequency) } == 0 {
            frequency = 0;
        }
        self.panel = panel;
        self.renderer = renderer;
        self.device = Some(device);
        self.frequency = frequency;
        self.last_frame = 0;
        self.hint_dismissed = false;
        self.started = true;
        1
    }

    fn start(&mut self, p: &Platform, chain: Handle) -> i32 {
        if self.started {
            return 1;
        }
        if self.failed || chain.is_null() {
            return 0;
        }
        let mut raw = null_mut();
        let got = unsafe { leaf(p.chain_device)(chain, &mut raw) };
        let device = unsafe { ComRef::from_owned(raw) };
        let Some(device) = device.filter(|_| got >= 0) else {
            say(format_args!(
                "overlay: presenting swap chain has no D3D11 device, hr {}",
                Hr(got)
            ));
            return 0;
        };
        say(format_args!(
            "overlay: swap chain {} selects device {}",
            Pointer(chain),
            Pointer(device.as_ptr())
        ));
        let window = || unsafe { leaf(p.chain_window)(chain) };
        if self.build(p, device, window, Refusals::CHAIN) == 0 {
            return 0;
        }
        self.hint_begin = 0;
        say(
            "overlay: ready. Insert opens it, and it draws over the finished frame without taking part in the reconstruction",
        );
        1
    }

    fn start_device(
        &mut self,
        p: &Platform,
        device: Handle,
        window: Handle,
        log: rsf_overlay_host_log,
        user: Handle,
    ) -> i32 {
        if self.started {
            return 1;
        }
        if device.is_null() || window.is_null() || self.failed {
            return 0;
        }
        set_logger(log, user);
        let device = unsafe { ComRef::retain(device) };
        if self.build(p, device, || window, Refusals::DEVICE) == 0 {
            return 0;
        }
        self.hint_begin = unsafe { leaf(p.tick)() };
        say("overlay: shared panel ready on supplied device; Insert opens it");
        1
    }

    fn seconds_since_last_frame(&mut self, p: &Platform) -> f32 {
        let mut now = 0;
        if self.frequency == 0 || unsafe { leaf(p.counter)(&mut now) } == 0 {
            return DEFAULT_DELTA;
        }
        if self.last_frame == 0 {
            self.last_frame = now;
            return DEFAULT_DELTA;
        }
        let elapsed = (now - self.last_frame) as f64 / self.frequency as f64;
        self.last_frame = now;
        if elapsed <= 0.0 || elapsed > LONGEST_DELTA {
            DEFAULT_DELTA
        } else {
            elapsed as f32
        }
    }

    /* Hand the renderer everything the last layout changed, then everything it has finished
    with. Both drain until a batch comes back short. */
    fn carry_textures(&self, p: &Platform, api: &rsf_overlay_host_panel, context: Handle) {
        let mut updates = [rsf_overlay_texture_update::default(); UPDATE_BATCH];
        loop {
            let count = unsafe {
                leaf(api.texture_updates)(self.panel, updates.as_mut_ptr(), UPDATE_BATCH as u32)
            } as usize;
            for update in &updates[..count.min(UPDATE_BATCH)] {
                if unsafe { leaf(p.renderer_upload)(self.renderer, context, update) } != 0 {
                    say("overlay: an atlas update was refused, so some text will be missing");
                }
            }
            if count < UPDATE_BATCH {
                break;
            }
        }

        let mut finished = [0; UPDATE_BATCH];
        loop {
            let count = unsafe {
                leaf(api.textures_to_free)(self.panel, finished.as_mut_ptr(), UPDATE_BATCH as u32)
            } as usize;
            for &id in &finished[..count.min(UPDATE_BATCH)] {
                unsafe { leaf(p.renderer_free)(self.renderer, id) };
            }
            if count < UPDATE_BATCH {
                break;
            }
        }
    }

    /* For renderers that already own a device, window and target. */
    #[allow(clippy::too_many_arguments)]
    fn draw_target(
        &mut self,
        p: &Platform,
        context: Handle,
        target: Handle,
        width: u32,
        height: u32,
        stats: &rsf_overlay_stats,
        intent: *mut rsf_overlay_intent,
    ) -> i32 {
        let Some(api) = self.api.filter(|_| self.started) else {
            return 0;
        };
        if context.is_null() || target.is_null() || width == 0 || height == 0 {
            return 0;
        }
        let visible = unsafe { leaf(p.input_visible)() } != 0;
        if visible && !self.shared_visible {
            self.trace_frames = TRACE_FRAMES;
        }
        self.shared_visible = visible;
        let trace = visible && self.trace_frames > 0;
        let elapsed = unsafe { leaf(p.tick)() }.wrapping_sub(self.hint_begin);
        if !visible && elapsed >= STARTUP_HINT_MS && stats.show_performance_hud == 0 {
            return 0;
        }
        let mut input = rsf_overlay_input {
            struct_size: size_of::<rsf_overlay_input>() as u32,
            ..Default::default()
        };
        let delta = self.seconds_since_last_frame(p);
        unsafe { leaf(p.input_collect)(&mut input, width, height, delta) };
        input.startup_hint_alpha = if !visible && elapsed < STARTUP_HINT_MS {
            hint_alpha(elapsed)
        } else {
            0.0
        };
        let mut data = rsf_overlay_draw_data {
            struct_size: size_of::<rsf_overlay_draw_data>() as u32,
            ..Default::default()
        };
        let mut decided = rsf_overlay_intent {
            struct_size: size_of::<rsf_overlay_intent>() as u32,
            ..Default::default()
        };
        if trace {
            say(format_args!(
                "Unity overlay: laying out panel on thread {}",
                unsafe { leaf(p.thread_id)() }
            ));
        }
        if unsafe { leaf(api.frame)(self.panel, &input, stats, &mut data, &mut decided) } != 0 {
            return 0;
        }
        // The close button and a tap outside the sheet. Visibility is input state, so it is
        // handled here for every host rather than by each one.
        if decided.close_requested != 0 {
            unsafe { leaf(p.input_set_visible)(0) };
        }
        if trace {
            say("Unity overlay: uploading textures");
        }
        self.carry_textures(p, &api, context);
        let result = {
            let _targets = SavedTargets::bind(p, context, target);
            if trace {
                say(format_args!(
                    "Unity overlay: drawing {} vertices, {} indices",
                    data.vertex_count, data.index_count
                ));
            }
            unsafe { leaf(p.renderer_draw)(self.renderer, context, &data, width, height) }
        };
        if !intent.is_null() {
            unsafe { intent.write(decided) };
        }
        if trace {
            say(format_args!(
                "Unity overlay: draw completed result={result}"
            ));
            self.trace_frames -= 1;
        }
        (result == 0) as i32
    }

    fn close_after_failure(&mut self, p: &Platform) -> i32 {
        self.failed = true;
        unsafe { leaf(p.input_set_visible)(0) };
        0
    }

    /* Lay out and draw one frame into the presenting chain's back buffer. */
    fn present(
        &mut self,
        p: &Platform,
        chain: Handle,
        stats: &rsf_overlay_stats,
        intent: *mut rsf_overlay_intent,
    ) -> i32 {
        let Some(api) = self.api.filter(|_| self.started && !self.failed) else {
            return 0;
        };
        if chain.is_null() {
            return 0;
        }
        let visible = unsafe { leaf(p.input_visible)() } != 0;
        let now = unsafe { leaf(p.tick)() };
        if visible || (self.hint_begin != 0 && now.wrapping_sub(self.hint_begin) >= STARTUP_HINT_MS)
        {
            self.hint_dismissed = true;
        }
        if !visible && self.hint_dismissed && stats.show_performance_hud == 0 {
            self.last_frame = 0;
            return 0;
        }

        let trace = self.trace_frames > 0;
        if trace {
            say(format_args!(
                "overlay frame: visible on thread {}, acquiring the back buffer",
                unsafe { leaf(p.thread_id)() }
            ));
        }
        let mut raw = null_mut();
        let got = unsafe { leaf(p.chain_device)(chain, &mut raw) };
        let chain_device = unsafe { ComRef::from_owned(raw) };
        let same = |first: Option<&ComRef>, second: Option<&ComRef>| match (first, second) {
            (Some(first), Some(second)) => {
                (unsafe { leaf(p.same_object)(first.as_ptr(), second.as_ptr()) }) != 0
            }
            _ => false,
        };
        if got < 0 || !same(self.device.as_ref(), chain_device.as_ref()) {
            // Shared vtables can also dispatch Present for another device. The renderer belongs
            // to the device selected at start; never submit its resources through another one.
            if trace {
                say(format_args!(
                    "overlay: skipping swap chain {} on another device {} (renderer {})",
                    Pointer(chain),
                    pointer(chain_device.as_ref()),
                    pointer(self.device.as_ref())
                ));
                self.trace_frames -= 1;
            }
            return 0;
        }
        let chain_device = chain_device.expect("matched device");
        let Some(context) =
            (unsafe { ComRef::from_owned(leaf(p.immediate_context)(chain_device.as_ptr())) })
        else {
            return 0;
        };
        let mut raw = null_mut();
        let got = unsafe { leaf(p.chain_buffer)(chain, &mut raw) };
        let buffer = unsafe { ComRef::from_owned(raw) };
        let Some(buffer) = buffer.filter(|_| got >= 0) else {
            if trace {
                say(format_args!("overlay: GetBuffer failed, hr {}", Hr(got)));
                self.trace_frames -= 1;
            }
            return 0;
        };
        let owner = unsafe { ComRef::from_owned(leaf(p.texture_device)(buffer.as_ptr())) };
        if !same(Some(&chain_device), owner.as_ref()) {
            say(format_args!(
                "overlay: back buffer {} belongs to device {}, chain device {}. Closing panel",
                Pointer(buffer.as_ptr()),
                pointer(owner.as_ref()),
                Pointer(chain_device.as_ptr())
            ));
            return self.close_after_failure(p);
        }
        let mut desc = rsf_overlay_host_texture_desc::default();
        unsafe { leaf(p.texture_desc)(buffer.as_ptr(), &mut desc) };
        if desc.width == 0 || desc.height == 0 {
            return 0;
        }
        if trace {
            say(format_args!(
                "overlay view: chain {}, device {}, context {}, buffer {}, owner {}, {}x{} format {} bind 0x{:x}. Creating view",
                Pointer(chain),
                Pointer(chain_device.as_ptr()),
                Pointer(context.as_ptr()),
                Pointer(buffer.as_ptr()),
                pointer(owner.as_ref()),
                desc.width,
                desc.height,
                desc.format,
                desc.bind_flags
            ));
        }
        let mut raw = null_mut();
        let made = unsafe { leaf(p.create_view)(chain_device.as_ptr(), buffer.as_ptr(), &mut raw) };
        let view = unsafe { ComRef::from_owned(raw) };
        if trace {
            say(format_args!(
                "overlay view: CreateRenderTargetView returned hr {}, view {}",
                Hr(made),
                pointer(view.as_ref())
            ));
        }
        let Some(view) = view.filter(|_| made >= 0) else {
            say(format_args!(
                "overlay: no back-buffer view, hr {}. Closing panel",
                Hr(made)
            ));
            return self.close_after_failure(p);
        };

        let mut input = rsf_overlay_input {
            struct_size: size_of::<rsf_overlay_input>() as u32,
            ..Default::default()
        };
        let delta = self.seconds_since_last_frame(p);
        unsafe { leaf(p.input_collect)(&mut input, desc.width, desc.height, delta) };
        if self.hint_begin == 0 {
            self.hint_begin = now;
        }
        if !visible && !self.hint_dismissed {
            input.startup_hint_alpha = hint_alpha(now.wrapping_sub(self.hint_begin));
        }

        let mut data = rsf_overlay_draw_data {
            struct_size: size_of::<rsf_overlay_draw_data>() as u32,
            ..Default::default()
        };
        let mut decided = rsf_overlay_intent {
            struct_size: size_of::<rsf_overlay_intent>() as u32,
            ..Default::default()
        };
        if trace {
            say("overlay frame: laying out the panel");
        }
        let laid_out =
            unsafe { leaf(api.frame)(self.panel, &input, stats, &mut data, &mut decided) };
        if laid_out != 0 {
            /* A panicked panel is poisoned for good and every later frame returns the same
            thing. Closing it alone left the performance HUD and startup hint laying it out,
            and reporting this line, every frame. */
            say(format_args!(
                "overlay: the panel failed to lay out a frame, result {laid_out}. Closing it"
            ));
            return self.close_after_failure(p);
        }
        if decided.close_requested != 0 {
            unsafe { leaf(p.input_set_visible)(0) };
        }
        if trace {
            say(format_args!(
                "overlay frame: laid out, {} vertices, {} indices, {} calls. Carrying textures",
                data.vertex_count, data.index_count, data.call_count
            ));
        }
        self.carry_textures(p, &api, context.as_ptr());

        let drawn = {
            // Restored before the view is released, on every path out of the draw.
            let _targets = SavedTargets::bind(p, context.as_ptr(), view.as_ptr());
            if trace {
                say("overlay frame: calling the renderer");
            }
            let drawn = unsafe {
                leaf(p.renderer_draw)(
                    self.renderer,
                    context.as_ptr(),
                    &data,
                    desc.width,
                    desc.height,
                )
            };
            if trace {
                say(format_args!("overlay frame: the renderer returned {drawn}"));
                say(format_args!(
                    "overlay frame: drawn into the back buffer, result {drawn}"
                ));
                self.trace_frames -= 1;
            }
            drawn
        };
        drop(view);
        if drawn != 0 {
            say(format_args!(
                "overlay: the panel laid out a frame the renderer would not draw, result {drawn}"
            ));
            return 0;
        }
        if !intent.is_null() {
            unsafe { intent.write(decided) };
        }
        1
    }

    fn stop(&mut self, p: &Platform) {
        // A deliberate stop and restart gets a fresh attempt.
        self.failed = false;
        if !self.started {
            return;
        }
        self.started = false;
        unsafe { leaf(p.input_set_visible)(0) };
        unsafe { leaf(p.input_uninstall)() };
        if !self.renderer.is_null() {
            unsafe { leaf(p.renderer_destroy)(self.renderer) };
            self.renderer = null_mut();
        }
        if !self.panel.is_null() {
            if let Some(destroy) = self.api.and_then(|api| api.destroy) {
                unsafe { destroy(self.panel) };
            }
            self.panel = null_mut();
        }
        self.device = None;
        // The panel DLL stays loaded: a window message already inside the input hook can still
        // be on its way into its code.
    }
}

/* The lines each start path reports when a step refuses. */
#[derive(Clone, Copy)]
struct Refusals {
    panel: Option<&'static str>,
    renderer: Option<&'static str>,
    window: Option<&'static str>,
    input: bool,
}

impl Refusals {
    const CHAIN: Self = Self {
        panel: None,
        renderer: Some("overlay: the renderer would not build, so there is nothing to draw with"),
        window: Some("overlay: the swap chain would not say which window it presents to"),
        input: true,
    };
    const DEVICE: Self = Self {
        panel: Some("overlay: panel ABI mismatch"),
        renderer: None,
        window: None,
        input: false,
    };

    fn say(self, line: Option<&'static str>) {
        if let Some(line) = line {
            say(line);
        }
    }

    fn say_panel(self) {
        match self.panel {
            Some(line) => say(line),
            None => say(format_args!(
                "overlay: the panel refused ABI {RSF_OVERLAY_ABI_VERSION}. Deploy rescaleframe_overlay.dll and dinput8.dll from the same build; an allocation failure is also possible"
            )),
        }
    }

    fn say_input(self, result: i32) {
        if self.input {
            say(format_args!(
                "overlay: the window would not take the input hook, result {result}. The panel would draw and not answer a mouse, so it is not started"
            ));
        }
    }
}

static HOST: Mutex<Host> = Mutex::new(Host::new());
/* Mirrors the held host's state for visibility and the hotkey, which must not wait on a render
thread that is drawing. */
static STARTED: AtomicBool = AtomicBool::new(false);
/* Started and not stopped by a failure: what a Present needs before it tries for the host. */
static PRESENTABLE: AtomicBool = AtomicBool::new(false);

fn hold() -> MutexGuard<'static, Host> {
    HOST.lock().unwrap_or_else(PoisonError::into_inner)
}

/* A second drawing thread skips its frame rather than waiting: waiting on a render thread to draw
a diagnostic is a worse trade than missing one frame of it. */
fn try_hold() -> Option<MutexGuard<'static, Host>> {
    match HOST.try_lock() {
        Ok(host) => Some(host),
        Err(TryLockError::Poisoned(host)) => Some(host.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

fn publish(host: &Host) {
    STARTED.store(host.started, Ordering::Release);
    PRESENTABLE.store(host.started && !host.failed, Ordering::Release);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_overlay_host_start(
    platform: &Platform,
    chain: Handle,
    log: rsf_overlay_host_log,
    user: Handle,
) -> i32 {
    set_logger(log, user);
    if STARTED.load(Ordering::Acquire) {
        return 1;
    }
    let mut host = hold();
    let result = host.start(platform, chain);
    publish(&host);
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_overlay_host_start_device(
    platform: &Platform,
    device: Handle,
    window: Handle,
    log: rsf_overlay_host_log,
    user: Handle,
) -> i32 {
    if STARTED.load(Ordering::Acquire) {
        return 1;
    }
    let mut host = hold();
    let result = host.start_device(platform, device, window, log, user);
    publish(&host);
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_overlay_host_draw_target(
    platform: &Platform,
    context: Handle,
    target: Handle,
    width: u32,
    height: u32,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
) -> i32 {
    if stats.is_null() || !STARTED.load(Ordering::Acquire) {
        return 0;
    }
    let Some(mut host) = try_hold() else {
        return 0;
    };
    host.draw_target(
        platform,
        context,
        target,
        width,
        height,
        unsafe { &*stats },
        intent,
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay_host_visible(platform: &Platform) -> u32 {
    if !STARTED.load(Ordering::Acquire) {
        return 0;
    }
    unsafe { leaf(platform.input_visible)() }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay_host_toggle(platform: &Platform) {
    if !STARTED.load(Ordering::Acquire) {
        say("overlay: not running, so there is nothing to open");
        return;
    }
    let visible = unsafe { leaf(platform.input_visible)() };
    unsafe { leaf(platform.input_set_visible)((visible == 0) as u32) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_overlay_host_present(
    platform: &Platform,
    chain: Handle,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
) -> i32 {
    if chain.is_null() || stats.is_null() || !PRESENTABLE.load(Ordering::Acquire) {
        return 0;
    }
    let Some(mut host) = try_hold() else {
        say(format_args!(
            "overlay frame: thread {} skipped, another is already drawing the panel",
            unsafe { leaf(platform.thread_id)() }
        ));
        return 0;
    };
    let result = host.present(platform, chain, unsafe { &*stats }, intent);
    publish(&host);
    result
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay_host_stop(platform: &Platform) {
    let mut host = hold();
    host.stop(platform);
    publish(&host);
}

#[cfg(test)]
#[path = "overlay_host_tests.rs"]
mod tests;
