// SPDX-License-Identifier: MIT
// OpenGL pipeline: GL scene surfaces shared with a D3D12 device, a native SR session and a DXGI
// child window whose presentation the FG owner intercepts. This owner decides session and frame
// identity, surface and SR session lifetime, configuration admission, SR backend switching and
// fallback, the FG provider request and teardown order. `native` (linked.rs; fakes under test)
// connects the runtime owners. Graphics work stays on the caller's GL context and graphics thread.
use crate::com_ref;
use crate::owned_handle::OwnedHandle as Owned;
use crate::sharpening_policy;
use crate::sr_session::has_capacity;
use crate::sys::RSF_OVERLAY_NOTIFY_DEVICE_RESET;
use crate::sys::rsf_fg12_setup;
use crate::sys::rsf_native_cpu_frame;
use crate::sys::*;
use crate::sys::{
    RSF_DLSS_ABI_VERSION, RSF_FG_BACKEND_AUTO, RSF_FG_OFF, RSF_REFLEX_OFF, rsf_backend_log_fn,
    rsf_native_fg_options, rsf_native_fg_status, rsf_sharpening_options,
};
use crate::sys::{
    RSF_SR_AUTO, RSF_SR_DLSS, RSF_SR_FSR2, RSF_SR_FSR3, RSF_SR_FSR4, RSF_SR_NONE, RSF_SR_SPATIAL,
    RSF_SR_XESS,
};
use rsf_game_sys::backend_result::{
    FEATURE_FAILED, INIT_FAILED, INVALID_ARGUMENT, NEEDS_RESTART, NOT_READY, OK,
};
use rsf_game_sys::{
    RSF_GAME_CPU_EVENT_VERSION, RSF_GAME_CPU_INPUT_EVENT, RSF_GAME_CPU_INPUT_KEYED,
    RSF_GAME_CPU_INPUT_UNFRAMED, RSF_GAME_CPU_WINDOW_MESSAGE, RSF_GAME_RENDER_WINDOW,
    rsf_game_cpu_event, rsf_game_render_pass as Pass,
};
use rsf_game_sys::vendor_paths::VendorPaths;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
#[cfg(test)]
use std::mem::MaybeUninit;
use std::ptr::{self};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Mutex, MutexGuard};
use windows::core::{IUnknown, Interface};

#[cfg_attr(test, allow(dead_code))]
mod dxgi;
#[cfg(not(test))]
mod linked;
#[cfg(not(test))]
use linked as native;
mod settings;
mod window;

// Public SR backend IDs (sr_session.h), checked by the native contract.
const SR_CHOICES: u32 = (1 << RSF_SR_DLSS)
    | (1 << RSF_SR_FSR2)
    | (1 << RSF_SR_FSR3)
    | (1 << RSF_SR_FSR4)
    | (1 << RSF_SR_XESS);
const MAX_QUALITY: u32 = 5;
const DLSS_ENGINE_CUSTOM: u32 = 0;
const SR12_ABI_VERSION: u32 = 1;

// D3D12 states and the DXGI back-buffer format, from the Windows SDK.
use dxgi::{
    CHAIN_FORMAT, STATE_COMMON, STATE_COPY_DEST, STATE_COPY_SOURCE, STATE_PRESENT,
    STATE_RENDER_TARGET, STATE_SHADER_INPUT,
};
const CHAIN_BUFFERS: u32 = 3;

// Published GL surfaces, in rsf_gl_pipeline_surfaces order. The first three have the render
// extent, the rest the output extent. Scene, depth, motion and output form the SR transaction.
const SCENE: usize = 0;
const DEPTH: usize = 1;
const MOTION: usize = 2;
const OUTPUT: usize = 3;
const HUDLESS: usize = 4;
const COMPOSED: usize = 5;
const SURFACES: usize = 6;
const SR_SURFACES: u32 = 4;

type Handle = *mut c_void;
type Log = rsf_backend_log_fn;

fn depth_domains_requested(pass: &Pass) -> bool {
    unsafe { native::depth_domains_requested(pass) != 0 }
}

fn valid_options(options: &rsf_gl_pipeline_options, width: u32, height: u32) -> bool {
    width != 0
        && height != 0
        && options.sr_backend <= RSF_SR_AUTO
        && options.sr_backend != RSF_SR_SPATIAL
        && options.quality <= MAX_QUALITY
        && unsafe { native::dlss_preset_valid(options.dlss_preset) } != 0
        && options.generation.struct_size as usize >= size_of::<rsf_native_fg_options>()
        && (options.sharpening.struct_size == 0 || sharpening_policy::valid(&options.sharpening))
}
fn failed(result: i32) -> bool {
    result < 0
}

// Strings, the caller's log and the presentation window that SDK and presentation hooks reach
// through their `user` pointer. Once FG is installed its hooks keep this for the process lifetime.
struct Shared {
    logger: Mutex<(Log, usize)>,
    window: AtomicPtr<c_void>,
    root: CString,
    vendors: VendorPaths,
    version: CString,
    project: CString,
}

#[cfg(not(test))]
static IDLE_REGISTRATION: AtomicPtr<Shared> = AtomicPtr::new(ptr::null_mut());
#[cfg(test)]
thread_local! {
    static IDLE_REGISTRATION: AtomicPtr<Shared> = const { AtomicPtr::new(ptr::null_mut()) };
}

fn with_idle_registration<T>(operation: impl FnOnce(&AtomicPtr<Shared>) -> T) -> T {
    #[cfg(not(test))]
    {
        operation(&IDLE_REGISTRATION)
    }
    #[cfg(test)]
    {
        IDLE_REGISTRATION.with(operation)
    }
}

fn put_registration(shared: &'static Shared) {
    with_idle_registration(|idle| idle.store(ptr::from_ref(shared).cast_mut(), Ordering::Release));
}

impl Shared {
    fn log(&self, message: &str) {
        if let Ok(text) = CString::new(message) {
            unsafe { hook_log(ptr::from_ref(self).cast_mut().cast(), text.as_ptr()) };
        }
    }

    fn detach_log(&self) {
        *crate::sync::lock(&self.logger) = (None, 0);
    }
}

unsafe extern "C" fn hook_log(user: Handle, message: *const c_char) {
    let shared = unsafe { &*user.cast::<Shared>() };
    if let Ok(logger) = shared.logger.try_lock()
        && let (Some(log), user) = *logger
    {
        unsafe { log(user as Handle, message) };
    }
}

unsafe extern "C" fn hook_accept(user: Handle, window: Handle) -> c_int {
    let shared = unsafe { &*user.cast::<Shared>() };
    c_int::from(!window.is_null() && shared.window.load(Ordering::Acquire) == window)
}

// What the published SR session and surfaces were built for. A request that differs rebuilds.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Shape {
    width: u32,
    height: u32,
    backend: u32,
    quality: u32,
    dlss_preset: u32,
}

// One SR session and its six GL surfaces. Fields drop in declaration order: the SR session releases
// before the surfaces it was evaluated with.
struct Surfaces {
    sr: Owned,
    textures: Vec<Owned>,
    handles: [Handle; SURFACES],
    info: [rsf_gl_texture_info; SURFACES],
    render_width: u32,
    render_height: u32,
    shape: Shape,
    selection_result: i32,
}

impl Surfaces {
    fn resource(&self, slot: usize) -> Handle {
        self.info[slot].resource
    }
}

struct State {
    sharing: Handle,
    device: Handle,
    queue: Handle,
    window: Handle,
    chain: Option<IUnknown>,
    shown: bool,
    closed: bool,
    retry_registration: bool,
    window_dirty: bool,
    width: u32,
    height: u32,
    generation: u32,
    status: rsf_gl_pipeline_status,
    evaluation_result: i32,
    evaluated_frame: u64,
    overlay: bool,
    frame_view: Option<(u64, Option<u64>)>,
    active: Option<Surfaces>,
}

impl State {
    fn record_view(&mut self, pass: &Pass) {
        self.frame_view = Some(match self.frame_view {
            Some((frame, view)) if frame == pass.source_frame_id => {
                (frame, view.filter(|key| *key == pass.view_key))
            }
            _ => (pass.source_frame_id, Some(pass.view_key)),
        });
    }
}

pub(crate) struct Pipeline {
    shared: &'static Shared,
    session: u64,
    own_window: bool,
    ready: AtomicBool,
    parent: Handle,
    settings: settings::SettingsSlot,
    // Graphics-owner state. CPU, input, abort and PCL calls never take it.
    state: Mutex<State>,
}

fn new_status() -> rsf_gl_pipeline_status {
    rsf_gl_pipeline_status {
        struct_size: size_of::<rsf_gl_pipeline_status>() as u32,
        ..Default::default()
    }
}

// Frees the hook state on an open failure that happens before any hook could retain it.
struct SharedGuard(*mut Shared);

impl SharedGuard {
    fn get(&self) -> &Shared {
        unsafe { &*self.0 }
    }

    fn keep(mut self) -> &'static Shared {
        let shared = std::mem::replace(&mut self.0, ptr::null_mut());
        unsafe { &*shared }
    }
}

impl Drop for SharedGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            drop(unsafe { Box::from_raw(self.0) });
        }
    }
}

fn empty_info() -> rsf_gl_texture_info {
    rsf_gl_texture_info {
        struct_size: size_of::<rsf_gl_texture_info>() as u32,
        ..Default::default()
    }
}

fn surface_format(slot: usize) -> u32 {
    match slot {
        DEPTH => RSF_GL_R32F,
        MOTION => RSF_GL_RG16F,
        HUDLESS | COMPOSED => RSF_GL_RGBA8,
        _ => RSF_GL_RGBA16F,
    }
}

fn qpc() -> (u64, u64) {
    use windows_sys::Win32::System::Performance::{
        QueryPerformanceCounter, QueryPerformanceFrequency,
    };
    let (mut now, mut frequency) = (0i64, 0i64);
    unsafe {
        QueryPerformanceCounter(&mut now);
        QueryPerformanceFrequency(&mut frequency);
    }
    (now as u64, frequency as u64)
}

fn session_seed() -> u64 {
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }.wrapping_add(1)
}

// Reads a caller structure only after its leading struct_size shows the whole structure.
unsafe fn read_sized<T: Copy>(value: *const T) -> Option<T> {
    unsafe { has_capacity(value) }.then(|| unsafe { value.read() })
}


fn lock(pipeline: &Pipeline) -> Option<MutexGuard<'_, State>> {
    // Graphics-owner calls are serialized by contract. A concurrent or reentrant call is refused
    // instead of waiting on, or racing, the graphics owner.
    crate::sync::try_lock(&pipeline.state)
}

impl Pipeline {
    fn log(&self, message: &str) {
        self.shared.log(message);
    }

    fn create_sr(
        &self,
        state: &State,
        backend: u32,
        options: &rsf_gl_pipeline_options,
        width: u32,
        height: u32,
    ) -> (i32, Option<Owned>) {
        let shared = self.shared;
        let user = ptr::from_ref(shared).cast_mut().cast::<c_void>();
        let setup = rsf_sr12_setup {
            struct_size: size_of::<rsf_sr12_setup>() as u32,
            abi_version: SR12_ABI_VERSION,
            device: state.device,
            backend,
            quality: options.quality,
            output_width: width,
            output_height: height,
            inverted_depth: 0,
            fsr2_directory_utf8: shared.vendors.fidelityfx.as_ptr(),
            fsr3_directory_utf8: shared.vendors.fidelityfx.as_ptr(),
            fsr4_directory_utf8: shared.vendors.fidelityfx.as_ptr(),
            xess_directory_utf8: shared.vendors.xess.as_ptr(),
            dlss: rsf_dlss_setup {
                struct_size: size_of::<rsf_dlss_setup>() as u32,
                abi_version: RSF_DLSS_ABI_VERSION,
                interposer_path_utf8: shared.vendors.interposer.as_ptr(),
                plugin_directory_utf8: shared.vendors.streamline.as_ptr(),
                log_directory_utf8: shared.root.as_ptr(),
                application_id: 0,
                engine: DLSS_ENGINE_CUSTOM,
                engine_version_utf8: shared.version.as_ptr(),
                project_id_utf8: shared.project.as_ptr(),
                require_signature: 1,
                log: Some(hook_log),
                log_user: user,
                preset: options.dlss_preset,
            },
            log: Some(hook_log),
            user,
            render_width: 0,
            render_height: 0,
            streamline_host: native::present_host(),
            // OpenGL scenes are SDR.
            input_sdr: 1,
            engine_input_extents: 0,
            external_exposure: 0,
            dlss_registration: ptr::null_mut(),
        };
        let mut sr = ptr::null_mut();
        let result = unsafe { native::sr_create(&setup, &mut sr) };
        let sr = Owned::new(sr, native::sr_destroy);
        if result == OK && sr.is_none() {
            (INIT_FAILED, None)
        } else if result == OK {
            (result, sr)
        } else {
            (result, None)
        }
    }

    // Selects and creates the SR session. A failed vendor session falls back to SR off, which still
    // normalizes inputs and supplies the output surface.
    fn select_sr(
        &self,
        state: &mut State,
        options: &rsf_gl_pipeline_options,
        width: u32,
        height: u32,
    ) -> Result<(u32, Owned), i32> {
        let mut selected = options.sr_backend;
        let (mut result, mut sr) = (INVALID_ARGUMENT, None);
        if selected == RSF_SR_AUTO {
            let (vendor, software) = unsafe { native::adapter_identity(state.device) };
            let mut choices = SR_CHOICES;
            loop {
                let candidate = crate::rsf_gpu_select_sr_backend(vendor, software, choices);
                if candidate == RSF_SR_NONE {
                    break;
                }
                (result, sr) = self.create_sr(state, candidate, options, width, height);
                if result == OK && sr.is_some() {
                    selected = candidate;
                    break;
                }
                choices &= !(1 << candidate);
            }
        } else {
            (result, sr) = self.create_sr(state, selected, options, width, height);
        }
        state.status.sr_result = result;
        if sr.is_none() {
            selected = RSF_SR_NONE;
            (result, sr) = self.create_sr(state, RSF_SR_NONE, options, width, height);
        }
        match sr {
            Some(sr) if result == OK => Ok((selected, sr)),
            _ => Err(if result == OK { INIT_FAILED } else { result }),
        }
    }

    // Builds the complete replacement while the published GL names stay alive, so a rejected
    // resize or switch leaves the caller with its previous surfaces and SR session.
    fn prepare(
        &self,
        state: &mut State,
        options: &rsf_gl_pipeline_options,
        shape: Shape,
        sharpening: &rsf_sharpening_options,
    ) -> Result<(u32, Surfaces), i32> {
        let (selected, sr) = self.select_sr(state, options, shape.width, shape.height)?;
        let (mut render_width, mut render_height) = (0, 0);
        let planned = unsafe { native::sr_plan(sr.get(), &mut render_width, &mut render_height) };
        if planned != OK {
            return Err(planned);
        }
        if render_width == 0 || render_height == 0 {
            return Err(INIT_FAILED);
        }
        let mut surfaces = Surfaces {
            sr,
            textures: Vec::with_capacity(SURFACES),
            handles: [ptr::null_mut(); SURFACES],
            info: [empty_info(); SURFACES],
            render_width,
            render_height,
            shape,
            selection_result: state.status.sr_result,
        };
        for slot in 0..SURFACES {
            let (width, height) = if slot <= MOTION {
                (render_width, render_height)
            } else {
                (shape.width, shape.height)
            };
            let mut texture = ptr::null_mut();
            let result = unsafe {
                native::gl_texture_create(
                    state.sharing,
                    width,
                    height,
                    surface_format(slot),
                    &mut texture,
                )
            };
            let owned = Owned::new(texture, native::gl_texture_destroy);
            let Some(owned) = owned.filter(|_| result == OK) else {
                return Err(if result == OK { INIT_FAILED } else { result });
            };
            surfaces.handles[slot] = owned.get();
            let result = unsafe { native::gl_texture_get(owned.get(), &mut surfaces.info[slot]) };
            if result != OK {
                return Err(result);
            }
            surfaces.textures.push(owned);
        }
        let sharpened = unsafe { native::sr_set_sharpening(surfaces.sr.get(), sharpening) };
        if sharpened != OK {
            return Err(sharpened);
        }
        if (state.width, state.height) != (shape.width, shape.height) {
            if native::window_resize(state.window, shape.width, shape.height) == 0 {
                self.log("OpenGL: event-thread window resize refused.");
                self.restore_window(state);
                return Err(FEATURE_FAILED);
            }
            let chain = state
                .chain
                .as_ref()
                .map_or(ptr::null_mut(), Interface::as_raw);
            let resized = unsafe {
                native::chain_resize(
                    chain,
                    CHAIN_BUFFERS,
                    shape.width,
                    shape.height,
                    CHAIN_FORMAT,
                )
            };
            if failed(resized) {
                self.log(&format!(
                    "OpenGL: DXGI resize {}x{} failed (0x{:08x})",
                    shape.width, shape.height, resized as u32
                ));
                self.restore_window(state);
                return Err(FEATURE_FAILED);
            }
            state.window_dirty = false;
        }
        Ok((selected, surfaces))
    }

    fn restore_window(&self, state: &mut State) {
        let (width, height) = if state.active.is_some() {
            (state.width, state.height)
        } else {
            (window::INITIAL_EXTENT, window::INITIAL_EXTENT)
        };
        state.window_dirty = native::window_resize(state.window, width, height) == 0;
    }

    fn request_fg(&self, state: &mut State, requested: u32) {
        let mut backend = requested;
        if backend == RSF_FG_BACKEND_AUTO {
            let (vendor, software) = unsafe { native::adapter_identity(state.device) };
            backend = crate::fg_choice::rsf_fg_select_auto_backend(
                native::fg_default_backend(vendor, software),
                native::present_backend_choices(),
            );
        }
        if backend != native::present_backend() {
            state.status.fg_switch_result = native::present_request(backend);
        }
    }

    fn configure(
        &self,
        state: &mut State,
        options: &rsf_gl_pipeline_options,
        width: u32,
        height: u32,
    ) -> i32 {
        let shape = Shape {
            width,
            height,
            backend: options.sr_backend,
            quality: options.quality,
            dlss_preset: options.dlss_preset,
        };
        state.status.requested_sr = options.sr_backend;
        state.status.requested_fg = options.fg_backend;
        if state.overlay {
            let drained = unsafe { native::gl_drain(state.sharing) };
            if drained != OK {
                return drained;
            }
        } else {
            unsafe { native::fg_options(&options.generation) };
            self.request_fg(state, options.fg_backend);
        }
        let sharpening = if options.sharpening.struct_size != 0 {
            options.sharpening
        } else {
            sharpening_policy::defaults()
        };
        if state.window_dirty {
            self.restore_window(state);
            if state.window_dirty {
                return FEATURE_FAILED;
            }
        }
        if let Some(active) = state.active.as_ref().filter(|active| active.shape == shape) {
            state.status.sr_result = active.selection_result;
            return unsafe { native::sr_set_sharpening(active.sr.get(), &sharpening) };
        }
        if !state.overlay {
            let drained = unsafe { native::gl_drain(state.sharing) };
            if drained != OK {
                return drained;
            }
        }
        let (selected, surfaces) = match self.prepare(state, options, shape, &sharpening) {
            Ok(prepared) => prepared,
            Err(result) => return result,
        };
        // The previous session and surfaces are released here, after the drain above.
        state.active = Some(surfaces);
        state.width = width;
        state.height = height;
        state.status.effective_sr = selected;
        state.status.evaluated = 0;
        state.evaluation_result = OK;
        state.evaluated_frame = 0;
        state.frame_view = None;
        state.generation = state.generation.wrapping_add(1);
        OK
    }

    // Opens a GL-to-D3D12 transaction over `handles`. The transaction is always submitted, including
    // when recording refuses, so the GL wait is signalled.
    fn transaction(
        &self,
        state: &State,
        handles: &[Handle],
        record: impl FnOnce(Handle, u32) -> i32,
    ) -> Result<i32, i32> {
        let (mut commands, mut slot) = (ptr::null_mut(), 0);
        let count = handles.len() as u32;
        let begun = unsafe {
            native::gl_begin(
                state.sharing,
                count,
                handles.as_ptr(),
                &mut commands,
                &mut slot,
            )
        };
        if begun != OK {
            return Err(begun);
        }
        let result = record(commands, slot);
        let submitted = unsafe { native::gl_submit(state.sharing, count, handles.as_ptr()) };
        if submitted != OK {
            return Err(submitted);
        }
        Ok(result)
    }

    fn evaluate(&self, state: &mut State, input: &Pass) -> i32 {
        let Some(active) = state.active.as_ref() else {
            return INVALID_ARGUMENT;
        };
        if depth_domains_requested(input) {
            return INVALID_ARGUMENT;
        }
        let mut pass = *input;
        pass.session_id = self.session;
        pass.resource_generation = state.generation;
        pass.color_input = active.resource(SCENE);
        pass.depth = active.resource(DEPTH);
        pass.motion = active.resource(MOTION);
        pass.color_output = active.resource(OUTPUT);
        let target = |slot: usize| {
            if slot == OUTPUT {
                STATE_COPY_DEST
            } else {
                STATE_SHADER_INPUT
            }
        };
        let handles = &active.handles[..SR_SURFACES as usize];
        let result = self.transaction(state, handles, |list, slot| {
            for i in SCENE..=OUTPUT {
                unsafe { native::transition(list, active.resource(i), STATE_COMMON, target(i)) };
            }
            let result = unsafe { native::sr_evaluate(active.sr.get(), list, &pass, slot) };
            for i in SCENE..=OUTPUT {
                unsafe { native::transition(list, active.resource(i), target(i), STATE_COMMON) };
            }
            result
        });
        let result = match result {
            Ok(result) => result,
            Err(result) => return result,
        };
        unsafe { native::fg_submitted(self.session, pass.source_frame_id) };
        state.evaluation_result = result;
        state.record_view(&pass);
        if result == OK && state.status.effective_sr != RSF_SR_NONE {
            state.status.evaluated += 1;
            state.evaluated_frame = pass.source_frame_id;
        }
        result
    }

    fn hudless(&self, state: &mut State, input: &Pass) -> i32 {
        let Some(active) = state.active.as_ref() else {
            return INVALID_ARGUMENT;
        };
        if depth_domains_requested(input) {
            return INVALID_ARGUMENT;
        }
        let mut pass = *input;
        pass.session_id = self.session;
        pass.color_input = active.resource(HUDLESS);
        let result = self.transaction(state, &active.handles[HUDLESS..=HUDLESS], |list, _| {
            unsafe {
                native::transition(list, pass.color_input, STATE_COMMON, STATE_RENDER_TARGET)
            };
            unsafe { native::fg_hudless(list, &pass) };
            unsafe {
                native::transition(list, pass.color_input, STATE_RENDER_TARGET, STATE_COMMON)
            };
            OK
        });
        match result {
            Ok(_) => {
                unsafe { native::fg_submitted(self.session, pass.source_frame_id) };
                state.record_view(&pass);
                OK
            }
            Err(result) => result,
        }
    }

    fn present(&self, state: &mut State, frame: u64, vsync: u32) -> i32 {
        let view = match state.frame_view {
            Some((captured, None)) if captured == frame => return INVALID_ARGUMENT,
            Some((captured, Some(view))) if captured == frame => view,
            _ => 0,
        };
        let (Some(active), Some(chain)) = (state.active.as_ref(), state.chain.as_ref()) else {
            return INVALID_ARGUMENT;
        };
        let Some(buffer) = (unsafe { com_ref::owned(native::chain_back_buffer(chain.as_raw())) })
        else {
            return FEATURE_FAILED;
        };
        let composed = active.resource(COMPOSED);
        let copied = self.transaction(state, &active.handles[COMPOSED..=COMPOSED], |list, _| {
            unsafe {
                native::transition(list, composed, STATE_COMMON, STATE_COPY_SOURCE);
                native::transition(list, buffer.as_raw(), STATE_PRESENT, STATE_COPY_DEST);
                native::copy_resource(list, buffer.as_raw(), composed);
                native::transition(list, buffer.as_raw(), STATE_COPY_DEST, STATE_PRESENT);
                native::transition(list, composed, STATE_COPY_SOURCE, STATE_COMMON);
            }
            OK
        });
        if let Err(result) = copied {
            return result;
        }
        if state.overlay {
            settings::present(self, state, chain.as_raw(), buffer.as_raw(), frame);
        }
        let mut window: Pass = unsafe { std::mem::zeroed() };
        window.struct_size = size_of::<Pass>() as u32;
        window.role = RSF_GAME_RENDER_WINDOW;
        window.session_id = self.session;
        window.source_frame_id = frame;
        window.view_key = view;
        window.swapchain = chain.as_raw();
        unsafe { native::fg_window(&window) };
        let presented = unsafe { native::chain_present(chain.as_raw(), u32::from(vsync != 0)) };
        drop(buffer);
        if failed(presented) {
            return FEATURE_FAILED;
        }
        if !state.shown {
            native::window_show(state.window);
            state.shown = true;
        }
        OK
    }

    fn close(&self, state: &mut State) -> i32 {
        self.ready.store(false, Ordering::Release);
        if state.closed {
            return OK;
        }
        if state.chain.is_some() {
            let drained = unsafe { native::gl_drain(state.sharing) };
            if drained != OK {
                return drained;
            }
            if state.overlay {
                if native::overlay_notify(RSF_OVERLAY_NOTIFY_DEVICE_RESET) != OK {
                    return NOT_READY;
                }
                state.overlay = false;
            }
            // DLSS SR borrows the chain's Streamline host.
            state.active = None;
            state.chain = None;
        }
        if !state.sharing.is_null() {
            let closed = unsafe { native::gl_close(state.sharing) };
            if closed != OK {
                return closed;
            }
            state.sharing = ptr::null_mut();
            state.device = ptr::null_mut();
            state.queue = ptr::null_mut();
            native::latency_end_session(self.session);
            native::dump_cancel(self.session);
        }
        self.shared.window.store(ptr::null_mut(), Ordering::Release);
        self.shared.detach_log();
        if self.own_window && !state.window.is_null() && native::window_destroy(state.window) == 0 {
            return FEATURE_FAILED;
        }
        state.window = ptr::null_mut();
        state.closed = true;
        if std::mem::take(&mut state.retry_registration) {
            put_registration(self.shared);
        }
        OK
    }

    fn cpu_event(
        &self,
        stage: u32,
        frame: u64,
        kinds: u32,
        message: u32,
        flags: u32,
    ) -> rsf_game_cpu_event {
        let (now, frequency) = qpc();
        rsf_game_cpu_event {
            struct_size: size_of::<rsf_game_cpu_event>() as u32,
            stage,
            session_id: self.session,
            source_frame_id: frame,
            timestamp_qpc: now,
            qpc_frequency: frequency,
            render_expected: 0,
            input_kind: kinds,
            message_id: message,
            event_version: RSF_GAME_CPU_EVENT_VERSION,
            flags,
        }
    }

    fn input(&self, frame: u64, kinds: u32, message: u32) -> i32 {
        if kinds & !7 != 0 || (kinds == 0 && message == 0) {
            return INVALID_ARGUMENT;
        }
        let mut flags = RSF_GAME_CPU_INPUT_KEYED;
        if frame != 0 {
            // Input belongs to an open CPU frame. A closed SDK token is never reused.
            let mut facts: rsf_native_cpu_frame = unsafe { std::mem::zeroed() };
            facts.struct_size = size_of::<rsf_native_cpu_frame>() as u32;
            let known = unsafe { native::cpu_read(self.session, frame, &mut facts) } != 0;
            if !known || facts.ended != 0 || unsafe { native::latency_completed(frame) } != 0 {
                return NOT_READY;
            }
        } else {
            flags |= if message != 0 {
                RSF_GAME_CPU_WINDOW_MESSAGE
            } else {
                RSF_GAME_CPU_INPUT_UNFRAMED
            };
        }
        let event = self.cpu_event(RSF_GAME_CPU_INPUT_EVENT, frame, kinds, message, flags);
        unsafe { native::fg_cpu(&event) };
        OK
    }
}

fn open(setup: &rsf_gl_pipeline_setup, cleanup: &mut Option<Pipeline>) -> Result<Pipeline, i32> {
    if native::window_valid(setup.hwnd) == 0
        || setup.runtime_directory_utf8.is_null()
        || setup.engine_version_utf8.is_null()
        || setup.project_id_utf8.is_null()
    {
        return Err(INVALID_ARGUMENT);
    }
    let root = unsafe { CStr::from_ptr(setup.runtime_directory_utf8) };
    let version = unsafe { CStr::from_ptr(setup.engine_version_utf8) };
    let project = unsafe { CStr::from_ptr(setup.project_id_utf8) };
    let idle = with_idle_registration(|idle| idle.swap(ptr::null_mut(), Ordering::AcqRel));
    let reused = unsafe { idle.as_ref() };
    if let Some(shared) = reused {
        if (&*shared.root, &*shared.version, &*shared.project) != (root, version, project) {
            put_registration(shared);
            return Err(NEEDS_RESTART);
        }
        *crate::sync::lock(&shared.logger) = (setup.log, setup.user as usize);
    }
    let guard = reused.is_none().then(|| {
        SharedGuard(Box::into_raw(Box::new(Shared {
            logger: Mutex::new((setup.log, setup.user as usize)),
            window: AtomicPtr::new(ptr::null_mut()),
            root: root.to_owned(),
            vendors: VendorPaths::new(root),
            version: version.to_owned(),
            project: project.to_owned(),
        })))
    });
    let shared = reused.unwrap_or_else(|| guard.as_ref().unwrap().get());
    let user = ptr::from_ref(shared).cast_mut().cast();
    let mut sharing = ptr::null_mut();
    let result = unsafe { native::gl_open(Some(hook_log), user, &mut sharing) };
    if result != OK {
        if let Some(shared) = reused {
            shared.detach_log();
            put_registration(shared);
        }
        return Err(result);
    }
    let (mut device, mut queue) = (ptr::null_mut(), ptr::null_mut());
    let graphics = unsafe { native::gl_graphics(sharing, &mut device, &mut queue) };
    // Once WGL has presented even a loading screen, DXGI creation on that HWND returns
    // E_ACCESSDENIED. A child gives the native chain its own compositor surface while GLFW keeps
    // the original context, events and window lifecycle.
    let own_window = setup.presentation_hwnd.is_null();
    let window = if own_window {
        native::window_create(setup.hwnd)
    } else {
        setup.presentation_hwnd
    };
    let fg = rsf_fg12_setup {
        struct_size: size_of::<rsf_fg12_setup>() as u32,
        backend: 0,
        max_generated_frames: 15,
        runtime_directory_utf8: shared.vendors.fidelityfx.as_ptr(),
        options: rsf_native_fg_options {
            struct_size: size_of::<rsf_native_fg_options>() as u32,
            mode: RSF_FG_OFF,
            generated_frames: 1,
            reflex_mode: RSF_REFLEX_OFF,
            frame_limit_us: 0,
            debug: 0,
        },
        depth_inverted: 0,
        depth_infinite: 0,
        units_to_meters: 1.0,
        log: Some(hook_log),
        user,
        accept_window: Some(hook_accept),
        runtime_switching: 1,
        ui_mode: RSF_UI_MODE_BACKBUFFER_HUDLESS,
        fsr3_directory_utf8: shared.vendors.fidelityfx.as_ptr(),
        fsr4_directory_utf8: shared.vendors.fidelityfx.as_ptr(),
        xess_directory_utf8: shared.vendors.xess.as_ptr(),
        streamline_directory_utf8: shared.vendors.streamline.as_ptr(),
        engine_type: DLSS_ENGINE_CUSTOM,
        engine_version_utf8: shared.version.as_ptr(),
        project_id_utf8: shared.project.as_ptr(),
    };
    // FG can retain its logger before publishing whether the installation succeeded.
    let shared = reused.unwrap_or_else(|| guard.unwrap().keep());
    let mut pipeline = Pipeline {
        shared,
        session: session_seed(),
        own_window,
        ready: AtomicBool::new(false),
        parent: setup.hwnd,
        settings: settings::SettingsSlot::new(),
        state: Mutex::new(State {
            sharing,
            device,
            queue,
            window,
            chain: None,
            shown: false,
            closed: false,
            retry_registration: reused.is_some(),
            window_dirty: false,
            width: 0,
            height: 0,
            generation: 0,
            status: new_status(),
            evaluation_result: OK,
            evaluated_frame: 0,
            overlay: false,
            frame_view: None,
            active: None,
        }),
    };
    if graphics != OK {
        reject_open(pipeline, cleanup);
        return Err(graphics);
    }
    if window.is_null() || native::window_child_of(window, setup.hwnd) == 0 {
        shared.log("OpenGL: presentation child must belong to the parent event thread.");
        reject_open(pipeline, cleanup);
        return Err(INIT_FAILED);
    }
    shared.window.store(window, Ordering::Release);
    if reused.is_none() {
        let result = unsafe { native::fg_install(&fg) };
        if result != OK {
            reject_open(pipeline, cleanup);
            return Err(result);
        }
        pipeline.state.get_mut().unwrap().retry_registration = true;
    }
    let mut chain = ptr::null_mut();
    let created =
        unsafe { native::chain_create(queue, window, CHAIN_BUFFERS, CHAIN_FORMAT, &mut chain) };
    let chain = unsafe { com_ref::owned(chain) };
    let owner = chain.as_ref().map_or(0, |chain| unsafe {
        native::present_is_owner(chain.as_raw())
    });
    if failed(created) || owner == 0 {
        shared.log(&format!(
            "OpenGL: DXGI creation result=0x{:08x}, intercepted={}",
            created as u32, owner
        ));
        shared.log("OpenGL: native presentation refused; GL presentation must be retained.");
        pipeline.state.get_mut().unwrap().chain = chain;
        reject_open(pipeline, cleanup);
        return Err(INIT_FAILED);
    }
    let chain = chain.expect("created chain");
    unsafe { native::chain_associate(chain.as_raw(), window) };
    let state = pipeline.state.get_mut().unwrap();
    state.chain = Some(chain);
    state.retry_registration = false;
    pipeline.ready.store(true, Ordering::Release);
    Ok(pipeline)
}

fn reject_open(pipeline: Pipeline, cleanup: &mut Option<Pipeline>) {
    pipeline
        .shared
        .window
        .store(ptr::null_mut(), Ordering::Release);
    pipeline.shared.detach_log();
    let result = {
        let mut state = pipeline.state.lock().unwrap();
        pipeline.close(&mut state)
    };
    if result != OK {
        *cleanup = Some(pipeline);
    }
}

/// # Safety
/// `setup` is null or readable for its `struct_size`; `out` is null or writable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_pipeline_open(
    setup: *const rsf_gl_pipeline_setup,
    out: *mut *mut Pipeline,
) -> i32 {
    if out.is_null() {
        return INVALID_ARGUMENT;
    }
    unsafe { out.write(ptr::null_mut()) };
    let Some(setup) = (unsafe { read_sized(setup) }) else {
        return INVALID_ARGUMENT;
    };
    crate::initialize_handle(unsafe { &mut *out }, INIT_FAILED, || {
        let mut cleanup = None;
        match open(&setup, &mut cleanup) {
            Ok(pipeline) => (OK, Some(pipeline)),
            Err(result) => (result, cleanup),
        }
    })
}

fn with_state(pipeline: *mut Pipeline, run: impl FnOnce(&Pipeline, &mut State) -> i32) -> i32 {
    let Some(pipeline) = (unsafe { pipeline.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    let Some(mut state) = lock(pipeline) else {
        return NOT_READY;
    };
    if state.closed {
        return INVALID_ARGUMENT;
    }
    if !pipeline.ready.load(Ordering::Acquire) || state.chain.is_none() {
        return NOT_READY;
    }
    run(pipeline, &mut state)
}

/// Graphics owner, at a drained boundary before the CPU frame begins.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_pipeline_configure(
    pipeline: *mut Pipeline,
    options: *const rsf_gl_pipeline_options,
    width: u32,
    height: u32,
) -> i32 {
    let Some(options) = (unsafe { read_sized(options) }) else {
        return INVALID_ARGUMENT;
    };
    let valid =
        unsafe { pipeline.as_ref() }.is_some_and(|_| valid_options(&options, width, height));
    if !valid {
        return INVALID_ARGUMENT;
    }
    with_state(pipeline, |pipeline, state| {
        let result = pipeline.configure(state, &options, width, height);
        if result != OK {
            state.status.sr_result = result;
        } else if state.overlay {
            unsafe { native::fg_options(&options.generation) };
            pipeline.request_fg(state, options.fg_backend);
        }
        result
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_pipeline_get_surfaces(
    pipeline: *mut Pipeline,
    out: *mut rsf_gl_pipeline_surfaces,
) -> i32 {
    if !unsafe { has_capacity(out) } {
        return INVALID_ARGUMENT;
    }
    with_state(pipeline, |_, state| {
        let Some(active) = state.active.as_ref() else {
            return INVALID_ARGUMENT;
        };
        let name = |slot: usize| active.info[slot].texture;
        unsafe {
            out.write(rsf_gl_pipeline_surfaces {
                struct_size: size_of::<rsf_gl_pipeline_surfaces>() as u32,
                render_width: active.render_width,
                render_height: active.render_height,
                output_width: state.width,
                output_height: state.height,
                scene: name(SCENE),
                depth: name(DEPTH),
                motion: name(MOTION),
                output: name(OUTPUT),
                hudless: name(HUDLESS),
                composed: name(COMPOSED),
            })
        };
        OK
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_pipeline_cpu(pipeline: *mut Pipeline, frame: u64, stage: u32) {
    let Some(pipeline) =
        (unsafe { pipeline.as_ref() }).filter(|pipeline| pipeline.ready.load(Ordering::Acquire))
    else {
        return;
    };
    let event = pipeline.cpu_event(stage, frame, 0, 0, RSF_GAME_CPU_INPUT_KEYED);
    unsafe { native::fg_cpu(&event) };
}

/// Input consumer on the CPU frame owner, or an unframed window message from the event thread.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_pipeline_input(
    pipeline: *mut Pipeline,
    frame: u64,
    kinds: u32,
    message: u32,
) -> i32 {
    match unsafe { pipeline.as_ref() } {
        Some(pipeline) if pipeline.ready.load(Ordering::Acquire) => {
            pipeline.input(frame, kinds, message)
        }
        Some(_) => NOT_READY,
        None => INVALID_ARGUMENT,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_pipeline_pcl_message(pipeline: *mut Pipeline) -> u32 {
    match unsafe { pipeline.as_ref() } {
        Some(pipeline) if pipeline.ready.load(Ordering::Acquire) => unsafe {
            native::pcl_message()
        },
        _ => 0,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_pipeline_abort(pipeline: *mut Pipeline, frame: u64) -> i32 {
    match unsafe { pipeline.as_ref() } {
        Some(pipeline) if frame != 0 && pipeline.ready.load(Ordering::Acquire) => unsafe {
            native::present_abort(frame)
        },
        Some(_) if frame != 0 => NOT_READY,
        _ => INVALID_ARGUMENT,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_pipeline_evaluate(pipeline: *mut Pipeline, input: *const Pass) -> i32 {
    let Some(input) = (unsafe { read_sized(input) }) else {
        return INVALID_ARGUMENT;
    };
    with_state(pipeline, |pipeline, state| pipeline.evaluate(state, &input))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_pipeline_hudless(pipeline: *mut Pipeline, input: *const Pass) -> i32 {
    let Some(input) = (unsafe { read_sized(input) }) else {
        return INVALID_ARGUMENT;
    };
    with_state(pipeline, |pipeline, state| pipeline.hudless(state, &input))
}

/// Every present carries its nonzero CPU frame ID, including menus.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_pipeline_present(
    pipeline: *mut Pipeline,
    frame: u64,
    vsync: u32,
) -> i32 {
    if frame == 0 {
        return INVALID_ARGUMENT;
    }
    with_state(pipeline, |pipeline, state| {
        pipeline.present(state, frame, vsync)
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_pipeline_status_get(
    pipeline: *mut Pipeline,
    out: *mut rsf_gl_pipeline_status,
) -> i32 {
    if !unsafe { has_capacity(out) } {
        return INVALID_ARGUMENT;
    }
    let Some(pipeline) = (unsafe { pipeline.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    let Some(state) = lock(pipeline) else {
        return NOT_READY;
    };
    // A cleanup-only handle from a failed open has no status; a closed one keeps its last status.
    if !pipeline.ready.load(Ordering::Acquire) && !state.closed {
        return NOT_READY;
    }
    let mut status = state.status;
    if status.sr_result == OK {
        status.sr_result = state.evaluation_result;
    }
    drop(state);
    status.struct_size = size_of::<rsf_gl_pipeline_status>() as u32;
    status.effective_fg = native::present_backend();
    status.fg_switch_result = native::present_switch_result();
    status.generation.struct_size = size_of::<rsf_native_fg_status>() as u32;
    unsafe { native::fg_status(&mut status.generation) };
    unsafe { out.write(status) };
    OK
}

/// Releases graphics resources after draining. The handle stays valid for the process lifetime and
/// refuses graphics calls afterwards; a repeated close succeeds.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_gl_pipeline_close(pipeline: *mut Pipeline) -> i32 {
    let Some(pipeline) = (unsafe { pipeline.as_ref() }) else {
        return OK;
    };
    let Some(mut state) = lock(pipeline) else {
        return NOT_READY;
    };
    pipeline.close(&mut state)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
use tests::native;
