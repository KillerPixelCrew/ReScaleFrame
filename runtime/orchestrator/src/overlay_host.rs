// SPDX-License-Identifier: MIT
// Owner of the shared D3D11 overlay host behind loader/proxy/src/overlay_host.h.
//
// AC7's embedded bridge, the Runtime overlay service and the D3D11-on-12 path all draw the same
// panel through this one host. It decides lifecycle, owns the panel, renderer and device, runs the
// startup hint, visibility, input and texture sequencing, admits draws and targets, handles
// failures and keeps frame time. The native functions only make the
// loader, COM, input, renderer and Win32 calls.
use crate::com_ref;
use crate::graphics_d3d11::rsf_call_native_log;
use crate::overlay_panel::{LoadError, PanelEntries, elapsed_seconds};
use crate::sync::lock;
use crate::sys::{RSF_OVERLAY_ABI_VERSION, rsf_overlay, rsf_overlay_draw_data, rsf_overlay_input};
use crate::sys::{rsf_overlay_intent, rsf_overlay_stats, rsf_overlay_texture_update};
use std::ffi::{CString, c_char, c_void};
use std::fmt::Display;
use std::ptr::{self, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use windows::core::{IUnknown, Interface};

type Handle = *mut c_void;

type Log = Option<unsafe extern "C" fn(user: *mut c_void, message: *const c_char)>;
#[derive(Default, Copy, Clone)]
struct TextureDesc {
    width: u32,
    height: u32,
    format: u32,
    bind_flags: u32,
}
#[derive(Copy, Clone)]
struct Targets {
    views: [*mut c_void; 8usize],
    depth: *mut c_void,
}
impl Default for Targets {
    fn default() -> Self {
        Self {
            views: [null_mut(); 8],
            depth: null_mut(),
        }
    }
}
// Production calls the D3D11, renderer, input and clock leaves in `native`; unit tests call
// same-named fakes.
#[cfg(not(test))]
use native as leaf;
#[cfg(test)]
use tests as leaf;

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
#[derive(Clone, Copy)]
struct DrawAdmission {
    visible: bool,
    hint: f32,
}

struct Logger {
    log: Log,
    user: usize,
}

/* Kept apart from the host so the renderer and the input subclass can announce lines from any
thread, including while the host is held. */
static LOGGER: Mutex<Logger> = Mutex::new(Logger { log: None, user: 0 });

fn set_logger(log: Log, user: Handle) {
    *lock(&LOGGER) = Logger {
        log,
        user: user as usize,
    };
}

fn say(message: impl Display) {
    let (log, user) = {
        let logger = lock(&LOGGER);
        (logger.log, logger.user)
    };
    if log.is_none() {
        return;
    }
    if let Ok(line) = CString::new(message.to_string()) {
        unsafe { rsf_call_native_log(log, user as Handle, line.as_ptr()) };
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

fn pointer(value: Option<&IUnknown>) -> Pointer {
    Pointer(value.map_or(ptr::null(), |value| value.as_raw()))
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
struct SavedTargets {
    context: Handle,
    targets: Targets,
}

impl SavedTargets {
    fn bind(context: Handle, view: Handle) -> Self {
        let mut targets = Targets::default();
        unsafe { leaf::save_targets(context, &mut targets) };
        unsafe { leaf::bind_target(context, view) };
        Self { context, targets }
    }
}

impl Drop for SavedTargets {
    fn drop(&mut self) {
        unsafe { leaf::restore_targets(self.context, &self.targets) };
        for saved in self.targets.views.into_iter().chain([self.targets.depth]) {
            drop(unsafe { com_ref::owned(saved) });
        }
    }
}

struct Host {
    api: Option<PanelEntries>,
    panel: *mut rsf_overlay,
    renderer: Handle,
    device: Option<IUnknown>,
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

    fn panel_api(&mut self) -> Option<PanelEntries> {
        if self.api.is_none() {
            #[cfg(not(test))]
            let loaded = crate::overlay_panel::open_panel().map(|panel| panel.publish());
            #[cfg(test)]
            let loaded = leaf::load_panel();
            match loaded {
                Ok(entries) => self.api = Some(entries),
                Err(LoadError::NoPath) => say("overlay: cannot work out where the panel DLL is"),
                Err(LoadError::CouldNotLoad(error)) => say(format_args!(
                    "overlay: the panel DLL did not load, error {error}. Set RSF_OVERLAY_DLL to point at rescaleframe_overlay.dll"
                )),
                Err(LoadError::MissingExports) => {
                    say("overlay: the panel DLL loaded but does not export the five entry points");
                }
            }
        }
        self.api
    }

    /* Panel, renderer and input subclass in that order. Every refusal is final until a stop, so
    a caller retrying each frame does not reload the DLL and rebuild the renderer each time. */
    fn build(
        &mut self,
        device: IUnknown,
        window: impl FnOnce() -> Handle,
        refusals: Refusals,
    ) -> i32 {
        let Some(api) = self.panel_api() else {
            self.failed = true;
            return 0;
        };
        let panel = unsafe { (api.create)(RSF_OVERLAY_ABI_VERSION) };
        if panel.is_null() {
            refusals.say_panel();
            self.failed = true;
            return 0;
        }
        let mut renderer = null_mut();
        let built = unsafe {
            leaf::renderer_create(
                device.as_raw(),
                Some(log_from_module),
                null_mut(),
                &mut renderer,
            )
        };
        if built != 0 {
            refusals.say(refusals.renderer);
            unsafe { (api.destroy)(panel) };
            self.failed = true;
            return 0;
        }
        let window = window();
        let installed = if window.is_null() {
            refusals.say(refusals.window);
            None
        } else {
            let installed = unsafe {
                leaf::input_install(window, TOGGLE_KEY, Some(log_from_module), null_mut())
            };
            if installed != 0 {
                refusals.say_input(installed);
            }
            Some(installed)
        };
        if installed != Some(0) {
            unsafe { leaf::renderer_destroy(renderer) };
            unsafe { (api.destroy)(panel) };
            self.failed = true;
            return 0;
        }

        let mut frequency = 0;
        if unsafe { leaf::frequency(&mut frequency) } == 0 {
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

    fn start(&mut self, chain: Handle) -> i32 {
        if self.started {
            return 1;
        }
        if self.failed || chain.is_null() {
            return 0;
        }
        let mut raw = null_mut();
        let got = unsafe { leaf::chain_device(chain, &mut raw) };
        let device = unsafe { com_ref::owned(raw) };
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
            Pointer(device.as_raw())
        ));
        let window = || unsafe { leaf::chain_window(chain) };
        if self.build(device, window, Refusals::CHAIN) == 0 {
            return 0;
        }
        self.hint_begin = 0;
        say(
            "overlay: ready. Insert opens it, and it draws over the finished frame without taking part in the reconstruction",
        );
        1
    }

    fn start_device(&mut self, device: Handle, window: Handle, log: Log, user: Handle) -> i32 {
        if self.started {
            return i32::from(self.owns_device(device));
        }
        if device.is_null() || window.is_null() || self.failed {
            return 0;
        }
        set_logger(log, user);
        let device = unsafe { com_ref::retain(device) };
        if self.build(device, || window, Refusals::DEVICE) == 0 {
            // The wrapped caller owns retry admission for a refused device start.
            self.failed = false;
            return 0;
        }
        self.hint_begin = unsafe { leaf::tick() };
        say("overlay: shared panel ready on supplied device; Insert opens it");
        1
    }

    fn seconds_since_last_frame(&mut self) -> f32 {
        let mut now = 0;
        if self.frequency == 0 || unsafe { leaf::counter(&mut now) } == 0 {
            return elapsed_seconds(0.0);
        }
        if self.last_frame == 0 {
            self.last_frame = now;
            return elapsed_seconds(0.0);
        }
        let elapsed = (now - self.last_frame) as f64 / self.frequency as f64;
        self.last_frame = now;
        elapsed_seconds(elapsed)
    }

    /* Drain each queue until a batch comes back short. */
    fn upload_textures(&self, api: &PanelEntries, context: Handle) {
        let mut updates = [rsf_overlay_texture_update::default(); UPDATE_BATCH];
        loop {
            let count = unsafe {
                (api.texture_updates)(self.panel, updates.as_mut_ptr(), UPDATE_BATCH as u32)
            } as usize;
            for update in &updates[..count.min(UPDATE_BATCH)] {
                if unsafe { leaf::renderer_upload(self.renderer, context, update) } != 0 {
                    say("overlay: an atlas update was refused, so some text will be missing");
                }
            }
            if count < UPDATE_BATCH {
                break;
            }
        }
    }

    fn free_textures(&self, api: &PanelEntries) {
        let mut finished = [0; UPDATE_BATCH];
        loop {
            let count = unsafe {
                (api.textures_to_free)(self.panel, finished.as_mut_ptr(), UPDATE_BATCH as u32)
            } as usize;
            for &id in &finished[..count.min(UPDATE_BATCH)] {
                unsafe { leaf::renderer_free(self.renderer, id) };
            }
            if count < UPDATE_BATCH {
                break;
            }
        }
    }

    fn layout(
        &mut self,
        api: &PanelEntries,
        dimensions: (u32, u32),
        stats: &rsf_overlay_stats,
        hint: f32,
    ) -> Option<(rsf_overlay_draw_data, rsf_overlay_intent)> {
        let (width, height) = dimensions;
        let mut input = rsf_overlay_input {
            struct_size: size_of::<rsf_overlay_input>() as u32,
            ..Default::default()
        };
        let delta = self.seconds_since_last_frame();
        unsafe { leaf::input_collect(&mut input, width, height, delta) };
        input.startup_hint_alpha = hint;
        let mut data = rsf_overlay_draw_data {
            struct_size: size_of::<rsf_overlay_draw_data>() as u32,
            ..Default::default()
        };
        let mut decided = rsf_overlay_intent {
            struct_size: size_of::<rsf_overlay_intent>() as u32,
            ..Default::default()
        };
        let result = unsafe { (api.frame)(self.panel, &input, stats, &mut data, &mut decided) };
        if result != 0 {
            say(format_args!(
                "overlay: the panel failed to lay out a frame, result {result}. Closing it"
            ));
            self.close_after_failure();
            return None;
        }
        if decided.close_requested != 0 {
            leaf::input_set_visible(0);
        }
        Some((data, decided))
    }

    fn paint(
        &self,
        api: &PanelEntries,
        context: Handle,
        target: Handle,
        dimensions: (u32, u32),
        data: &rsf_overlay_draw_data,
    ) -> i32 {
        self.upload_textures(api, context);
        let (width, height) = dimensions;
        let result = {
            let _targets = SavedTargets::bind(context, target);
            unsafe { leaf::renderer_draw(self.renderer, context, data, width, height) }
        };
        // egui may use a retiring texture in this frame's draw data.
        self.free_textures(api);
        result
    }

    fn admit_target(&self, hud: bool) -> Option<DrawAdmission> {
        if !self.started || self.failed || self.api.is_none() {
            return None;
        }
        let visible = leaf::input_visible() != 0;
        let elapsed = unsafe { leaf::tick() }.wrapping_sub(self.hint_begin);
        (visible || hud || elapsed < STARTUP_HINT_MS).then(|| DrawAdmission {
            visible,
            hint: if visible { 0.0 } else { hint_alpha(elapsed) },
        })
    }

    /* For renderers that already own a device, window and target. */
    #[allow(clippy::too_many_arguments)]
    fn draw_target(
        &mut self,
        context: Handle,
        target: Handle,
        width: u32,
        height: u32,
        stats: &rsf_overlay_stats,
        intent: *mut rsf_overlay_intent,
        admission: DrawAdmission,
    ) -> i32 {
        let Some(api) = self.api.filter(|_| self.started && !self.failed) else {
            return 0;
        };
        if context.is_null() || target.is_null() || width == 0 || height == 0 {
            return 0;
        }
        let DrawAdmission { visible, hint } = admission;
        if visible && !self.shared_visible {
            self.trace_frames = TRACE_FRAMES;
        }
        self.shared_visible = visible;
        let trace = visible && self.trace_frames > 0;
        if trace {
            say(format_args!(
                "D3D11-on-12 overlay: laying out panel on thread {}",
                unsafe { leaf::thread_id() }
            ));
        }
        let dimensions = (width, height);
        let Some((data, decided)) = self.layout(&api, dimensions, stats, hint) else {
            return 0;
        };
        if trace {
            say(format_args!(
                "D3D11-on-12 overlay: drawing {} vertices, {} indices",
                data.vertex_count, data.index_count
            ));
        }
        let result = self.paint(&api, context, target, dimensions, &data);
        if !intent.is_null() {
            unsafe { intent.write(decided) };
        }
        if trace {
            say(format_args!(
                "D3D11-on-12 overlay: draw completed result={result}"
            ));
            self.trace_frames -= 1;
        }
        (result == 0) as i32
    }

    fn close_after_failure(&mut self) -> i32 {
        self.failed = true;
        leaf::input_set_visible(0);
        0
    }

    /* Lay out and draw one frame into the presenting chain's back buffer. */
    fn present(
        &mut self,
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
        let visible = leaf::input_visible() != 0;
        let now = unsafe { leaf::tick() };
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
                unsafe { leaf::thread_id() }
            ));
        }
        let mut raw = null_mut();
        let got = unsafe { leaf::chain_device(chain, &mut raw) };
        let chain_device = unsafe { com_ref::owned(raw) };
        let same = |first: Option<&IUnknown>, second: Option<&IUnknown>| match (first, second) {
            (Some(first), Some(second)) => {
                (unsafe { leaf::same_object(first.as_raw(), second.as_raw()) }) != 0
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
            (unsafe { com_ref::owned(leaf::immediate_context(chain_device.as_raw())) })
        else {
            return 0;
        };
        let mut raw = null_mut();
        let got = unsafe { leaf::chain_buffer(chain, &mut raw) };
        let buffer = unsafe { com_ref::owned(raw) };
        let Some(buffer) = buffer.filter(|_| got >= 0) else {
            if trace {
                say(format_args!("overlay: GetBuffer failed, hr {}", Hr(got)));
                self.trace_frames -= 1;
            }
            return 0;
        };
        let owner = unsafe { com_ref::owned(leaf::texture_device(buffer.as_raw())) };
        if !same(Some(&chain_device), owner.as_ref()) {
            say(format_args!(
                "overlay: back buffer {} belongs to device {}, chain device {}. Closing panel",
                Pointer(buffer.as_raw()),
                pointer(owner.as_ref()),
                Pointer(chain_device.as_raw())
            ));
            return self.close_after_failure();
        }
        let mut desc = TextureDesc::default();
        unsafe { leaf::texture_desc(buffer.as_raw(), &mut desc) };
        if desc.width == 0 || desc.height == 0 {
            return 0;
        }
        if trace {
            say(format_args!(
                "overlay view: chain {}, device {}, context {}, buffer {}, owner {}, {}x{} format {} bind 0x{:x}. Creating view",
                Pointer(chain),
                Pointer(chain_device.as_raw()),
                Pointer(context.as_raw()),
                Pointer(buffer.as_raw()),
                pointer(owner.as_ref()),
                desc.width,
                desc.height,
                desc.format,
                desc.bind_flags
            ));
        }
        let mut raw = null_mut();
        let made = unsafe { leaf::create_view(chain_device.as_raw(), buffer.as_raw(), &mut raw) };
        let view = unsafe { com_ref::owned(raw) };
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
            return self.close_after_failure();
        };

        if self.hint_begin == 0 {
            self.hint_begin = now;
        }
        let hint = if !visible && !self.hint_dismissed {
            hint_alpha(now.wrapping_sub(self.hint_begin))
        } else {
            0.0
        };
        let dimensions = (desc.width, desc.height);
        if trace {
            say("overlay frame: laying out the panel");
        }
        let Some((data, decided)) = self.layout(&api, dimensions, stats, hint) else {
            return 0;
        };
        if trace {
            say(format_args!(
                "overlay frame: laid out, {} vertices, {} indices, {} calls. Calling the renderer",
                data.vertex_count, data.index_count, data.call_count
            ));
        }
        let drawn = self.paint(&api, context.as_raw(), view.as_raw(), dimensions, &data);
        if trace {
            say(format_args!(
                "overlay frame: drawn into the back buffer, result {drawn}"
            ));
            self.trace_frames -= 1;
        }
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

    fn stop(&mut self) {
        // A deliberate stop and restart gets a fresh attempt.
        self.failed = false;
        if !self.started {
            return;
        }
        self.started = false;
        leaf::input_set_visible(0);
        unsafe { leaf::input_uninstall() };
        if !self.renderer.is_null() {
            unsafe { leaf::renderer_destroy(self.renderer) };
            self.renderer = null_mut();
        }
        if !self.panel.is_null() {
            if let Some(api) = self.api {
                unsafe { (api.destroy)(self.panel) };
            }
            self.panel = null_mut();
        }
        self.device = None;
        // The panel DLL stays loaded: a window message already inside the input hook can still
        // be on its way into its code.
    }

    fn owns_device(&self, device: Handle) -> bool {
        self.device
            .as_ref()
            .is_some_and(|current| current.as_raw() == device)
    }

    fn stop_device(&mut self, device: Handle) {
        if self.owns_device(device) {
            self.stop();
        }
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
                "overlay: the panel refused ABI {RSF_OVERLAY_ABI_VERSION}. Use rescaleframe_overlay.dll and the RSF host from the same build. An allocation failure is also possible."
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
    lock(&HOST)
}

/* A second drawing thread skips its frame rather than waiting: waiting on a render thread to draw
a diagnostic is a worse trade than missing one frame of it. */
fn try_hold() -> Option<MutexGuard<'static, Host>> {
    crate::sync::try_lock(&HOST)
}

fn publish(host: &Host) {
    STARTED.store(host.started, Ordering::Release);
    PRESENTABLE.store(host.started && !host.failed, Ordering::Release);
}

unsafe fn rsf_core_overlay_host_start(chain: Handle, log: Log, user: Handle) -> i32 {
    set_logger(log, user);
    if STARTED.load(Ordering::Acquire) {
        return 1;
    }
    let mut host = hold();
    let result = host.start(chain);
    publish(&host);
    result
}

unsafe fn rsf_core_overlay_host_start_device(
    device: Handle,
    window: Handle,
    log: Log,
    user: Handle,
) -> i32 {
    let mut host = hold();
    let result = host.start_device(device, window, log, user);
    publish(&host);
    result
}

#[allow(clippy::too_many_arguments)]
unsafe fn rsf_core_overlay_host_draw_target(
    context: Handle,
    target: Handle,
    width: u32,
    height: u32,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
    admission: Option<DrawAdmission>,
) -> i32 {
    if stats.is_null() || !STARTED.load(Ordering::Acquire) {
        return 0;
    }
    let Some(mut host) = try_hold() else {
        return 0;
    };
    let stats = unsafe { &*stats };
    let Some(admission) = admission.or_else(|| host.admit_target(stats.show_performance_hud != 0))
    else {
        return 0;
    };
    let result = host.draw_target(context, target, width, height, stats, intent, admission);
    publish(&host);
    result
}

fn rsf_core_overlay_host_visible() -> u32 {
    if !STARTED.load(Ordering::Acquire) {
        return 0;
    }
    leaf::input_visible()
}

fn rsf_core_overlay_host_toggle() {
    if !STARTED.load(Ordering::Acquire) {
        say("overlay: not running, so there is nothing to open");
        return;
    }
    let visible = leaf::input_visible();
    leaf::input_set_visible((visible == 0) as u32);
}

unsafe fn rsf_core_overlay_host_present(
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
            unsafe { leaf::thread_id() }
        ));
        return 0;
    };
    let result = host.present(chain, unsafe { &*stats }, intent);
    publish(&host);
    result
}

fn rsf_core_overlay_host_stop() {
    let mut host = hold();
    host.stop();
    publish(&host);
}

fn rsf_core_overlay_host_stop_device(device: Handle) {
    let mut host = hold();
    host.stop_device(device);
    publish(&host);
}

#[cfg(test)]
#[path = "overlay_host_tests.rs"]
mod tests;

#[cfg(not(test))]
#[path = "overlay_host_native.rs"]
pub(super) mod native;
