// SPDX-License-Identifier: MIT
//! Pre-HDR identity, HUD-less handoff and matching window pass for Skyrim.

use crate::platform::{self, Platform};
use crate::render_hook::RenderHook;
use rsf_game_sys::{
    RSF_GAME_RENDER_FG_INPUTS, RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_NO_FG,
    RSF_GAME_RENDER_WINDOW, rsf_game_host_services, rsf_game_render_pass,
};
use std::cell::RefCell;
use std::ffi::{c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};

const DRAW_INTERFACE: usize = 0x116_ac80;
const INTERFACE_BYTES: [u8; 15] = [
    0x4c, 0x8b, 0xdc, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41, 0x57, 0x48, 0x83, 0xec, 0x50,
];
const PRIMARY_UI: usize = 0x219_e5c0;
const NATIVE_FRAME: usize = 0x333_63f4;
const ACTIVE_WINDOW: usize = 0x333_0198;
const IMMEDIATE_CONTEXT: usize = 0x333_1f30;

#[repr(C)]
struct Capture {
    struct_size: u32,
    abi_version: u32,
    session_id: u64,
    native_frame: u64,
    view_key: u64,
    name: *const c_char,
    description: *const c_char,
    texture: *mut c_void,
    view: u32,
    scale: f32,
}

#[repr(C)]
struct Buffer {
    chain: *mut c_void,
    context: *mut c_void,
    texture: *mut c_void,
    width: u32,
    height: u32,
    samples: u32,
}

type BufferCallback = unsafe extern "C" fn(*const c_void, &Buffer);

#[repr(C)]
#[derive(Clone, Copy)]
struct Leaves {
    identity: unsafe extern "C" fn(&mut rsf_game_render_pass) -> bool,
    render: unsafe extern "C" fn(
        &rsf_game_host_services,
        *mut c_void,
        &rsf_game_render_pass,
        u32,
    ) -> bool,
    diagnostic: unsafe extern "C" fn(*const c_char, &rsf_game_render_pass),
    buffer: unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void, BufferCallback),
    retain: unsafe extern "C" fn(*mut c_void, *mut c_void),
    release: unsafe extern "C" fn(*mut c_void, *mut c_void),
    capture: unsafe extern "C" fn(usize, *mut c_void, *mut c_void, &Capture),
}

const _: () = {
    assert!(size_of::<Capture>() == 64);
    assert!(std::mem::offset_of!(Capture, texture) == 48);
    assert!(size_of::<Buffer>() == 40);
    assert!(size_of::<Leaves>() == 56);
};

#[derive(Clone, Copy)]
struct Binding {
    platform: &'static Platform,
    base: usize,
    host: rsf_game_host_services,
    capture: usize,
    leaves: Leaves,
}

// The process-lived Runtime session lends host and native tables. Only the renderer owns
// the command context and resources used during an admitted callback.
unsafe impl Send for Binding {}

static HOOK: RenderHook<Binding> = RenderHook::new();
static CALLS: AtomicU32 = AtomicU32::new(0);
static WINDOWS: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy)]
struct Scene {
    chain: usize,
    pass: rsf_game_render_pass,
}

struct Window {
    binding: Binding,
    pass: rsf_game_render_pass,
    context: *mut c_void,
}

impl Drop for Window {
    fn drop(&mut self) {
        unsafe { (self.binding.leaves.release)(self.pass.swapchain, self.context) };
    }
}

#[derive(Default)]
enum WindowState {
    #[default]
    Idle,
    Opening {
        finish_requested: bool,
    },
    Open,
    Recovery,
    Closing,
}

#[derive(Default)]
struct Frame {
    scene: Option<Scene>,
    window: Option<Window>,
    window_state: WindowState,
}

impl Frame {
    fn reserve_window(&mut self) -> bool {
        if !matches!(self.window_state, WindowState::Idle) {
            return false;
        }
        self.window_state = WindowState::Opening {
            finish_requested: false,
        };
        WINDOWS.fetch_add(1, Ordering::AcqRel);
        true
    }

    fn publish_window(&mut self, window: Window) -> Option<Window> {
        if matches!(
            self.window_state,
            WindowState::Opening {
                finish_requested: true
            }
        ) {
            self.window_state = WindowState::Closing;
            Some(window)
        } else {
            self.window_state = WindowState::Open;
            self.window = Some(window);
            None
        }
    }

    fn finish(&mut self) -> Option<Window> {
        match &mut self.window_state {
            WindowState::Idle => self.scene = None,
            WindowState::Opening { finish_requested } => *finish_requested = true,
            WindowState::Open | WindowState::Recovery => {
                self.window_state = WindowState::Closing;
                return self.window.take();
            }
            WindowState::Closing => {}
        }
        None
    }
}

thread_local! {
    static FRAME: RefCell<Frame> = RefCell::new(Frame::default());
}

fn binding() -> Option<Binding> {
    HOOK.binding()
}

pub(crate) fn empty_pass() -> rsf_game_render_pass {
    // The GameSDK record contains scalars, arrays and raw resource pointers.
    unsafe { std::mem::zeroed() }
}

pub(crate) fn same_source(a: &rsf_game_render_pass, b: &rsf_game_render_pass) -> bool {
    a.source_frame_id == b.source_frame_id
        && a.submission_id == b.submission_id
        && a.scope_id == b.scope_id
}

impl Binding {
    fn native_frame(self) -> Option<u64> {
        let mut frame = [0; 4];
        self.platform
            .read(self.base + NATIVE_FRAME, &mut frame)
            .then_some(u64::from(u32::from_le_bytes(frame)))
    }

    fn identity(self, pass: &mut rsf_game_render_pass) -> bool {
        unsafe { (self.leaves.identity)(pass) }
    }

    fn render(self, context: *mut c_void, pass: &rsf_game_render_pass, begin: bool) -> bool {
        unsafe { (self.leaves.render)(&self.host, context, pass, u32::from(begin)) }
    }
}

fn scalar_scene(pass: &rsf_game_render_pass) -> Scene {
    let mut scene = empty_pass();
    scene.struct_size = size_of::<rsf_game_render_pass>() as u32;
    scene.role = pass.role;
    scene.session_id = pass.session_id;
    scene.source_frame_id = pass.source_frame_id;
    scene.submission_id = pass.submission_id;
    scene.scope_id = pass.scope_id;
    scene.family_key = pass.family_key;
    scene.view_key = pass.view_key;
    scene.history_key = pass.history_key;
    scene.native_frame = pass.native_frame;
    scene.pass_key = pass.pass_key;
    scene.resource_generation = pass.resource_generation;
    scene.viewport_key = pass.viewport_key;
    scene.window_key = pass.window_key;
    scene.rhi_viewport_key = pass.rhi_viewport_key;
    scene.flags = pass.flags;
    if pass.role != RSF_GAME_RENDER_FG_INPUTS {
        scene.flags |= RSF_GAME_RENDER_NO_FG;
    }
    scene.screen = pass.screen;
    scene.camera = pass.camera;
    scene.camera_valid = pass.camera_valid;
    scene.motion_camera_included = pass.motion_camera_included;
    scene.motion_to_uv = pass.motion_to_uv;
    scene.previous_clip_to_clip = pass.previous_clip_to_clip;
    scene.render_rect = pass.render_rect;
    scene.output_rect = pass.output_rect;
    Scene {
        chain: pass.swapchain.addr(),
        pass: scene,
    }
}

pub(crate) fn begin_inputs(services: &rsf_game_host_services) -> bool {
    if !FRAME.with_borrow_mut(|frame| {
        if !matches!(frame.window_state, WindowState::Idle) {
            return false;
        }
        frame.scene = None;
        true
    }) {
        return false;
    }
    services.render_pass.is_some()
        && HOOK.live.load(Ordering::Acquire)
        && binding().is_some_and(|binding| {
            services.session_id == binding.host.session_id && services.user == binding.host.user
        })
}

pub(crate) fn identify(pass: &rsf_game_render_pass) {
    let Some(binding) = binding() else { return };
    if binding.host.session_id != pass.session_id
        || binding.native_frame() != Some(pass.native_frame)
    {
        return;
    }
    if pass.source_frame_id != 0 {
        let mut current = empty_pass();
        if !binding.identity(&mut current) || !same_source(&current, pass) {
            return;
        }
    }
    FRAME.with_borrow_mut(|frame| {
        if let Some(previous) = frame.scene
            && previous.pass.session_id != 0
            && previous.pass.session_id == pass.session_id
            && previous.pass.native_frame == pass.native_frame
            && (!same_source(&previous.pass, pass)
                || (previous.pass.role == RSF_GAME_RENDER_FG_INPUTS
                    && previous.pass.view_key == pass.view_key))
        {
            return;
        }
        frame.scene = Some(scalar_scene(pass));
    });
}

struct Snapshot {
    binding: Binding,
    scene: Scene,
    after: bool,
}

fn snapshot(binding: Binding, after: bool) -> Option<()> {
    if !HOOK.live.load(Ordering::Acquire) {
        return None;
    }
    let scene = FRAME.with_borrow(|frame| frame.scene)?;
    if scene.pass.session_id == 0
        || scene.pass.session_id != binding.host.session_id
        || binding.native_frame() != Some(scene.pass.native_frame)
    {
        return None;
    }
    let window = binding.platform.read_usize(binding.base + ACTIVE_WINDOW)?;
    if window == 0 {
        return None;
    }
    let chain = binding.platform.read_usize(window.checked_add(0x18)?)?;
    let context = binding
        .platform
        .read_usize(binding.base + IMMEDIATE_CONTEXT)?;
    if chain == 0 || chain != scene.chain || context == 0 {
        return None;
    }
    let snapshot = Snapshot {
        binding,
        scene,
        after,
    };
    unsafe {
        (binding.leaves.buffer)(
            ptr::with_exposed_provenance_mut(chain),
            ptr::with_exposed_provenance_mut(context),
            ptr::from_ref(&snapshot).cast(),
            use_buffer,
        )
    };
    Some(())
}

unsafe extern "C" fn use_buffer(snapshot: *const c_void, buffer: &Buffer) {
    let &Snapshot {
        binding,
        scene,
        after,
    } = unsafe { &*snapshot.cast::<Snapshot>() };
    if buffer.width != scene.pass.camera.output_width
        || buffer.height != scene.pass.camera.output_height
        || buffer.samples != 1
    {
        return;
    }
    let mut current = empty_pass();
    let same_scope = binding.identity(&mut current) && same_source(&current, &scene.pass);
    if !after
        && same_scope
        && binding.host.render_pass.is_some()
        && scene.pass.source_frame_id != 0
        && FRAME.with_borrow_mut(Frame::reserve_window)
    {
        let mut pass = scene.pass;
        if current.screen != pass.screen || current.flags & RSF_GAME_RENDER_NO_FG != 0 {
            pass.flags |= RSF_GAME_RENDER_NO_FG;
            pass.screen = current.screen;
        }
        pass.role = RSF_GAME_RENDER_FINAL_SCENE;
        pass.scene_surface = buffer.texture;
        pass.swapchain = buffer.chain;
        let began = binding.render(buffer.context, &pass, true);
        let ended = binding.render(buffer.context, &pass, false);
        if !began || !ended {
            clear_window();
            return;
        }
        pass.role = RSF_GAME_RENDER_WINDOW;
        pass.scene_surface = ptr::null_mut();
        unsafe { (binding.leaves.retain)(buffer.chain, buffer.context) };
        let window = Window {
            binding,
            pass,
            context: buffer.context,
        };
        unsafe { (binding.leaves.diagnostic)(c"WINDOW".as_ptr(), &pass) };
        let began = binding.render(buffer.context, &pass, true);
        if !began {
            // The host callback can throw after opening its scope. Recover immediately,
            // using the same pass and leases, and retry at renderer-end if recovery throws.
            FRAME.with_borrow_mut(|frame| frame.window_state = WindowState::Closing);
            binding.platform.log(
                &binding.host,
                c"Skyrim window begin threw; attempting scope recovery.",
            );
            retire_window(window);
            return;
        }
        if let Some(window) = FRAME.with_borrow_mut(|frame| frame.publish_window(window)) {
            retire_window(window);
        }
    }
    if binding.capture != 0 {
        let (name, description) = if after {
            (
                c"native_after_interface",
                c"Native backbuffer after interface drawing; later native output work may remain",
            )
        } else {
            (c"native_before_interface", c"Native backbuffer before interface drawing; candidate scene boundary, not yet FG eligible")
        };
        let request = Capture {
            struct_size: size_of::<Capture>() as u32,
            abi_version: binding.platform.d3d11_host_abi,
            session_id: scene.pass.session_id,
            native_frame: scene.pass.native_frame,
            view_key: scene.pass.view_key,
            name: name.as_ptr(),
            description: description.as_ptr(),
            texture: buffer.texture,
            view: 0,
            scale: 1.0,
        };
        unsafe {
            (binding.leaves.capture)(binding.capture, binding.host.user, buffer.context, &request)
        };
    }
}

fn clear_window() {
    FRAME.with_borrow_mut(|frame| {
        frame.scene = None;
        frame.window_state = WindowState::Idle;
    });
    WINDOWS.fetch_sub(1, Ordering::AcqRel);
}

fn retire_window(window: Window) {
    if window.binding.render(window.context, &window.pass, false) {
        drop(window);
        clear_window();
    } else {
        FRAME.with_borrow_mut(|frame| {
            frame.scene = None;
            frame.window_state = WindowState::Recovery;
            frame.window = Some(window);
        });
    }
}

pub(crate) fn finish() {
    if let Some(window) = FRAME.with_borrow_mut(Frame::finish) {
        retire_window(window);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_presentation_guard(platform: *const Platform) -> bool {
    let Some(platform) = (unsafe { platform::inspect(platform) }) else {
        return false;
    };
    let base = platform.main_module();
    base != 0 && platform.matches(base + DRAW_INTERFACE, &INTERFACE_BYTES)
}

/// Lifecycle calls are serialized by the adapter session.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_presentation_start(
    host: &rsf_game_host_services,
    platform: *const Platform,
    leaves: &Leaves,
    detour: *const c_void,
) -> bool {
    if rsf_skyrim_presentation_busy() {
        return false;
    }
    let Some(platform) = (unsafe { platform::inspect(platform) }) else {
        return false;
    };
    let base = platform.main_module();
    if base == 0
        || (!HOOK.live.load(Ordering::Acquire)
            && !platform.matches(base + DRAW_INTERFACE, &INTERFACE_BYTES))
    {
        return false;
    }
    let target = base + DRAW_INTERFACE;
    let runtime = platform.module_handle("ReScaleFrame.Runtime.dll");
    let capture = platform.export_address(runtime, c"rsf_d3d11_host_capture_texture");
    let binding = Binding {
        platform,
        base,
        host: *host,
        capture,
        leaves: *leaves,
    };
    HOOK.enable(
        binding,
        || platform.hook_create(target, detour),
        || platform.hook_enable(target),
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_presentation_quiesce() {
    HOOK.live.store(false, Ordering::Release);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_presentation_busy() -> bool {
    CALLS.load(Ordering::Acquire) != 0 || WINDOWS.load(Ordering::Acquire) != 0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_presentation_stop(platform: *const Platform) -> bool {
    rsf_skyrim_presentation_quiesce();
    if HOOK.original.load(Ordering::Acquire).is_null() {
        return !rsf_skyrim_presentation_busy();
    }
    let Some(platform) = (unsafe { platform::inspect(platform) }) else {
        return false;
    };
    let base = platform.main_module();
    base != 0 && platform.hook_disable(base + DRAW_INTERFACE) && !rsf_skyrim_presentation_busy()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_presentation_enter(ui: usize, primary: &mut bool) -> *mut c_void {
    CALLS.fetch_add(1, Ordering::AcqRel);
    if let Some(binding) = binding()
        && binding.platform.read_usize(binding.base + PRIMARY_UI) == Some(ui)
    {
        *primary = true;
        let _ = snapshot(binding, false);
    }
    HOOK.original.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_presentation_leave(primary: bool) {
    if primary && let Some(binding) = binding() {
        let _ = snapshot(binding, true);
    }
    CALLS.fetch_sub(1, Ordering::AcqRel);
}
