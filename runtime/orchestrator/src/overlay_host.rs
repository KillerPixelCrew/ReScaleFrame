// SPDX-License-Identifier: MIT
// Owner of the shared D3D11 overlay host behind loader/proxy/src/overlay_host.h.
//
// AC7's embedded bridge, the Runtime overlay service and the D3D11-on-12 path all draw the same
// panel through this one host. It decides lifecycle, owns the panel, renderer and device, runs the
// startup hint, visibility, input and texture sequencing, admits draws and targets, handles
// failures and keeps frame time. The native functions only make the
// loader, COM, input, renderer and Win32 calls.
use crate::com_ref::ComRef;
use crate::graphics_d3d11::rsf_call_native_log;
use crate::overlay_panel::{LoadError, PanelEntries, elapsed_seconds};
#[cfg(test)]
use crate::sys::rsf_overlay_host_panel;
use crate::sys::{RSF_OVERLAY_ABI_VERSION, rsf_overlay, rsf_overlay_draw_data, rsf_overlay_input};
use crate::sys::{rsf_overlay_intent, rsf_overlay_stats, rsf_overlay_texture_update};
use std::ffi::{CString, c_char, c_void};
use std::fmt::Display;
use std::ptr::{self, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};

type Handle = *mut c_void;

// Keep the fixture's injected raw loader and its original refusal cases.
#[cfg(test)]
const RSF_OVERLAY_HOST_PANEL_LOADED: i32 = 0;
#[cfg(test)]
const RSF_OVERLAY_HOST_PANEL_NO_PATH: i32 = 1;
#[cfg(test)]
const RSF_OVERLAY_HOST_PANEL_NOT_LOADED: i32 = 2;
#[cfg(test)]
const RSF_OVERLAY_HOST_PANEL_MISSING_EXPORTS: i32 = 3;

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
#[derive(Copy, Clone)]
struct Platform {
    #[cfg(test)]
    load_panel: unsafe extern "C" fn(out: *mut rsf_overlay_host_panel, error: *mut u32) -> u32,
    chain_device: unsafe extern "C" fn(chain: *mut c_void, device: *mut *mut c_void) -> i32,
    chain_buffer: unsafe extern "C" fn(chain: *mut c_void, texture: *mut *mut c_void) -> i32,
    chain_window: unsafe extern "C" fn(chain: *mut c_void) -> *mut c_void,
    same_object: unsafe extern "C" fn(first: *mut c_void, second: *mut c_void) -> u32,
    immediate_context: unsafe extern "C" fn(device: *mut c_void) -> *mut c_void,
    texture_device: unsafe extern "C" fn(texture: *mut c_void) -> *mut c_void,
    texture_desc: unsafe extern "C" fn(texture: *mut c_void, out: *mut TextureDesc),
    create_view: unsafe extern "C" fn(
        device: *mut c_void,
        texture: *mut c_void,
        view: *mut *mut c_void,
    ) -> i32,
    save_targets: unsafe extern "C" fn(context: *mut c_void, out: *mut Targets),
    bind_target: unsafe extern "C" fn(context: *mut c_void, view: *mut c_void),
    restore_targets: unsafe extern "C" fn(context: *mut c_void, saved: *const Targets),
    renderer_create: unsafe extern "C" fn(
        device: *mut c_void,
        log: Log,
        user: *mut c_void,
        out: *mut *mut c_void,
    ) -> i32,
    renderer_upload: unsafe extern "C" fn(
        renderer: *mut c_void,
        context: *mut c_void,
        update: *const rsf_overlay_texture_update,
    ) -> i32,
    renderer_free: unsafe extern "C" fn(renderer: *mut c_void, id: u64),
    renderer_draw: unsafe extern "C" fn(
        renderer: *mut c_void,
        context: *mut c_void,
        data: *const rsf_overlay_draw_data,
        width: u32,
        height: u32,
    ) -> i32,
    renderer_destroy: unsafe extern "C" fn(renderer: *mut c_void),
    input_install: unsafe extern "C" fn(
        window: *mut c_void,
        toggle_key: u32,
        log: Log,
        user: *mut c_void,
    ) -> i32,
    input_uninstall: unsafe extern "C" fn(),
    input_visible: unsafe extern "C" fn() -> u32,
    input_set_visible: unsafe extern "C" fn(visible: u32),
    input_collect: unsafe extern "C" fn(
        out: *mut rsf_overlay_input,
        width: u32,
        height: u32,
        delta_seconds: f32,
    ),
    tick: unsafe extern "C" fn() -> u64,
    counter: unsafe extern "C" fn(value: *mut i64) -> u32,
    frequency: unsafe extern "C" fn(value: *mut i64) -> u32,
    thread_id: unsafe extern "C" fn() -> u32,
}

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
    targets: Targets,
}

impl<'a> SavedTargets<'a> {
    fn bind(platform: &'a Platform, context: Handle, view: Handle) -> Self {
        let mut targets = Targets::default();
        unsafe { (platform.save_targets)(context, &mut targets) };
        unsafe { (platform.bind_target)(context, view) };
        Self {
            platform,
            context,
            targets,
        }
    }
}

impl Drop for SavedTargets<'_> {
    fn drop(&mut self) {
        unsafe { (self.platform.restore_targets)(self.context, &self.targets) };
        for saved in self.targets.views.into_iter().chain([self.targets.depth]) {
            drop(unsafe { ComRef::from_owned(saved) });
        }
    }
}

struct Host {
    api: Option<PanelEntries>,
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

    fn panel_api(&mut self, _platform: &Platform) -> Option<PanelEntries> {
        if self.api.is_none() {
            #[cfg(not(test))]
            let loaded = crate::overlay_panel::open_panel().map(|panel| panel.publish());
            #[cfg(test)]
            let loaded = {
                let mut api = rsf_overlay_host_panel::default();
                let mut error = 0;
                let status = unsafe { (_platform.load_panel)(&mut api, &mut error) } as i32;
                match status {
                    RSF_OVERLAY_HOST_PANEL_LOADED => {
                        PanelEntries::admit(api).ok_or(LoadError::MissingExports)
                    }
                    RSF_OVERLAY_HOST_PANEL_NO_PATH => Err(LoadError::NoPath),
                    RSF_OVERLAY_HOST_PANEL_NOT_LOADED => Err(LoadError::CouldNotLoad(error)),
                    RSF_OVERLAY_HOST_PANEL_MISSING_EXPORTS => Err(LoadError::MissingExports),
                    _ => {
                        say("overlay: the panel DLL loader returned an unknown status");
                        return None;
                    }
                }
            };
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
        p: &Platform,
        device: ComRef,
        window: impl FnOnce() -> Handle,
        refusals: Refusals,
    ) -> i32 {
        let Some(api) = self.panel_api(p) else {
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
            (p.renderer_create)(
                device.as_ptr(),
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
            let installed =
                unsafe { (p.input_install)(window, TOGGLE_KEY, Some(log_from_module), null_mut()) };
            if installed != 0 {
                refusals.say_input(installed);
            }
            Some(installed)
        };
        if installed != Some(0) {
            unsafe { (p.renderer_destroy)(renderer) };
            unsafe { (api.destroy)(panel) };
            self.failed = true;
            return 0;
        }

        let mut frequency = 0;
        if unsafe { (p.frequency)(&mut frequency) } == 0 {
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
        let got = unsafe { (p.chain_device)(chain, &mut raw) };
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
        let window = || unsafe { (p.chain_window)(chain) };
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
        log: Log,
        user: Handle,
    ) -> i32 {
        if self.started {
            return i32::from(self.owns_device(device));
        }
        if device.is_null() || window.is_null() || self.failed {
            return 0;
        }
        set_logger(log, user);
        let device = unsafe { ComRef::retain(device) };
        if self.build(p, device, || window, Refusals::DEVICE) == 0 {
            // The wrapped caller owns retry admission for a refused device start.
            self.failed = false;
            return 0;
        }
        self.hint_begin = unsafe { (p.tick)() };
        say("overlay: shared panel ready on supplied device; Insert opens it");
        1
    }

    fn seconds_since_last_frame(&mut self, p: &Platform) -> f32 {
        let mut now = 0;
        if self.frequency == 0 || unsafe { (p.counter)(&mut now) } == 0 {
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
    fn upload_textures(&self, p: &Platform, api: &PanelEntries, context: Handle) {
        let mut updates = [rsf_overlay_texture_update::default(); UPDATE_BATCH];
        loop {
            let count = unsafe {
                (api.texture_updates)(self.panel, updates.as_mut_ptr(), UPDATE_BATCH as u32)
            } as usize;
            for update in &updates[..count.min(UPDATE_BATCH)] {
                if unsafe { (p.renderer_upload)(self.renderer, context, update) } != 0 {
                    say("overlay: an atlas update was refused, so some text will be missing");
                }
            }
            if count < UPDATE_BATCH {
                break;
            }
        }
    }

    fn free_textures(&self, p: &Platform, api: &PanelEntries) {
        let mut finished = [0; UPDATE_BATCH];
        loop {
            let count = unsafe {
                (api.textures_to_free)(self.panel, finished.as_mut_ptr(), UPDATE_BATCH as u32)
            } as usize;
            for &id in &finished[..count.min(UPDATE_BATCH)] {
                unsafe { (p.renderer_free)(self.renderer, id) };
            }
            if count < UPDATE_BATCH {
                break;
            }
        }
    }

    fn layout(
        &mut self,
        p: &Platform,
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
        let delta = self.seconds_since_last_frame(p);
        unsafe { (p.input_collect)(&mut input, width, height, delta) };
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
            self.close_after_failure(p);
            return None;
        }
        if decided.close_requested != 0 {
            unsafe { (p.input_set_visible)(0) };
        }
        Some((data, decided))
    }

    fn paint(
        &self,
        p: &Platform,
        api: &PanelEntries,
        context: Handle,
        target: Handle,
        dimensions: (u32, u32),
        data: &rsf_overlay_draw_data,
    ) -> i32 {
        self.upload_textures(p, api, context);
        let (width, height) = dimensions;
        let result = {
            let _targets = SavedTargets::bind(p, context, target);
            unsafe { (p.renderer_draw)(self.renderer, context, data, width, height) }
        };
        // egui may use a retiring texture in this frame's draw data.
        self.free_textures(p, api);
        result
    }

    fn admit_target(&self, p: &Platform, hud: bool) -> Option<DrawAdmission> {
        if !self.started || self.failed || self.api.is_none() {
            return None;
        }
        let visible = unsafe { (p.input_visible)() } != 0;
        let elapsed = unsafe { (p.tick)() }.wrapping_sub(self.hint_begin);
        (visible || hud || elapsed < STARTUP_HINT_MS).then(|| DrawAdmission {
            visible,
            hint: if visible { 0.0 } else { hint_alpha(elapsed) },
        })
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
                unsafe { (p.thread_id)() }
            ));
        }
        let dimensions = (width, height);
        let Some((data, decided)) = self.layout(p, &api, dimensions, stats, hint) else {
            return 0;
        };
        if trace {
            say(format_args!(
                "D3D11-on-12 overlay: drawing {} vertices, {} indices",
                data.vertex_count, data.index_count
            ));
        }
        let result = self.paint(p, &api, context, target, dimensions, &data);
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

    fn close_after_failure(&mut self, p: &Platform) -> i32 {
        self.failed = true;
        unsafe { (p.input_set_visible)(0) };
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
        let visible = unsafe { (p.input_visible)() } != 0;
        let now = unsafe { (p.tick)() };
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
                unsafe { (p.thread_id)() }
            ));
        }
        let mut raw = null_mut();
        let got = unsafe { (p.chain_device)(chain, &mut raw) };
        let chain_device = unsafe { ComRef::from_owned(raw) };
        let same = |first: Option<&ComRef>, second: Option<&ComRef>| match (first, second) {
            (Some(first), Some(second)) => {
                (unsafe { (p.same_object)(first.as_ptr(), second.as_ptr()) }) != 0
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
            (unsafe { ComRef::from_owned((p.immediate_context)(chain_device.as_ptr())) })
        else {
            return 0;
        };
        let mut raw = null_mut();
        let got = unsafe { (p.chain_buffer)(chain, &mut raw) };
        let buffer = unsafe { ComRef::from_owned(raw) };
        let Some(buffer) = buffer.filter(|_| got >= 0) else {
            if trace {
                say(format_args!("overlay: GetBuffer failed, hr {}", Hr(got)));
                self.trace_frames -= 1;
            }
            return 0;
        };
        let owner = unsafe { ComRef::from_owned((p.texture_device)(buffer.as_ptr())) };
        if !same(Some(&chain_device), owner.as_ref()) {
            say(format_args!(
                "overlay: back buffer {} belongs to device {}, chain device {}. Closing panel",
                Pointer(buffer.as_ptr()),
                pointer(owner.as_ref()),
                Pointer(chain_device.as_ptr())
            ));
            return self.close_after_failure(p);
        }
        let mut desc = TextureDesc::default();
        unsafe { (p.texture_desc)(buffer.as_ptr(), &mut desc) };
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
        let made = unsafe { (p.create_view)(chain_device.as_ptr(), buffer.as_ptr(), &mut raw) };
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
        let Some((data, decided)) = self.layout(p, &api, dimensions, stats, hint) else {
            return 0;
        };
        if trace {
            say(format_args!(
                "overlay frame: laid out, {} vertices, {} indices, {} calls. Calling the renderer",
                data.vertex_count, data.index_count, data.call_count
            ));
        }
        let drawn = self.paint(p, &api, context.as_ptr(), view.as_ptr(), dimensions, &data);
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

    fn stop(&mut self, p: &Platform) {
        // A deliberate stop and restart gets a fresh attempt.
        self.failed = false;
        if !self.started {
            return;
        }
        self.started = false;
        unsafe { (p.input_set_visible)(0) };
        unsafe { (p.input_uninstall)() };
        if !self.renderer.is_null() {
            unsafe { (p.renderer_destroy)(self.renderer) };
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
            .is_some_and(|current| current.as_ptr() == device)
    }

    fn stop_device(&mut self, p: &Platform, device: Handle) {
        if self.owns_device(device) {
            self.stop(p);
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

unsafe fn rsf_core_overlay_host_start(
    platform: &Platform,
    chain: Handle,
    log: Log,
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

unsafe fn rsf_core_overlay_host_start_device(
    platform: &Platform,
    device: Handle,
    window: Handle,
    log: Log,
    user: Handle,
) -> i32 {
    let mut host = hold();
    let result = host.start_device(platform, device, window, log, user);
    publish(&host);
    result
}

#[allow(clippy::too_many_arguments)]
unsafe fn rsf_core_overlay_host_draw_target(
    platform: &Platform,
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
    let Some(admission) =
        admission.or_else(|| host.admit_target(platform, stats.show_performance_hud != 0))
    else {
        return 0;
    };
    let result = host.draw_target(
        platform, context, target, width, height, stats, intent, admission,
    );
    publish(&host);
    result
}

fn rsf_core_overlay_host_visible(platform: &Platform) -> u32 {
    if !STARTED.load(Ordering::Acquire) {
        return 0;
    }
    unsafe { (platform.input_visible)() }
}

fn rsf_core_overlay_host_toggle(platform: &Platform) {
    if !STARTED.load(Ordering::Acquire) {
        say("overlay: not running, so there is nothing to open");
        return;
    }
    let visible = unsafe { (platform.input_visible)() };
    unsafe { (platform.input_set_visible)((visible == 0) as u32) };
}

unsafe fn rsf_core_overlay_host_present(
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
            unsafe { (platform.thread_id)() }
        ));
        return 0;
    };
    let result = host.present(platform, chain, unsafe { &*stats }, intent);
    publish(&host);
    result
}

fn rsf_core_overlay_host_stop(platform: &Platform) {
    let mut host = hold();
    host.stop(platform);
    publish(&host);
}

fn rsf_core_overlay_host_stop_device(platform: &Platform, device: Handle) {
    let mut host = hold();
    host.stop_device(platform, device);
    publish(&host);
}

#[cfg(test)]
#[path = "overlay_host_tests.rs"]
mod tests;

#[cfg(not(test))]
#[path = "overlay_host_native.rs"]
pub(super) mod native;
