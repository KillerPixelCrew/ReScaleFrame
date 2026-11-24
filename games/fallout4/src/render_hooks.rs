// SPDX-License-Identifier: MIT
//! The render hooks of Fallout 4 AE: the eight hook records at their discovered sites with their
//! call-site and instruction guards, the hook lifecycle, and the calling thread's World that the
//! detours drive: World and Present admission, the FRAME, SUBMISSION and WINDOW scope, the
//! reduced-render recovery transaction, SR and HUDless publication, and the producer, imagespace,
//! Effects and TAA seams. render_hooks.c keeps each typed detour with its MinHook trampoline,
//! which it calls on its own C frame, the return addresses, the last-error saves and the guarded
//! Runtime calls. Sites are discovered (discovery.rs); see
//! docs/research/rust-fallout4-render-hooks.md and docs/research/fallout4-site-discovery.md.

use crate::build_layout::{
    RENDERER_CHAIN, RENDERER_CONTEXT, RENDERER_DEVICE, RENDERER_WINDOW, renderer_object,
    renderer_pointer,
};
use crate::cpu_events::{
    begin_present_phase, begin_render as cpu_begin_render, current_identity, end_present_phase,
    render_source_is_current,
};
use crate::creation::accept_window;
use crate::d3d11;
use crate::depth_domains::{self, EMPTY as NO_SNAPSHOT, Snapshot};
use crate::discovery::{self, CALL, Site, address, rva};
use crate::render_inputs::{self, Inputs};
use crate::resolution::{
    self, EFFECTS_RETURNED, EVALUATED, HUDLESS, OUTPUT_READY, OutputOwner, PRE_SR, SOURCE_RETIRED,
    UI_RESTORED, UI_RETURN, is_busy as resolution_busy,
};
use crate::screen::{self, NativeScreen as Screen, WORLD_LEASE_INVALID};
use crate::sys::rsf_fallout4_cpu_identity as CpuIdentity;
use crate::sys::{
    NativePrepassCall as PrepassCall, native_input_depth as DEPTH, native_input_motion as MOTION,
    native_input_pipboy as PIPBOY, native_input_refraction_normals as REFRACTION,
    native_input_vats as VATS,
};
use crate::{MH_OK, image, message, read};
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_game_sys::win32::typed_export;
use rsf_game_sys::{
    RSF_D3D11_AUXILIARY_COLOUR as COLOUR, RSF_D3D11_AUXILIARY_DEPTH_STENCIL as DEPTH_STENCIL,
    RSF_D3D11_AUXILIARY_RESTORE_CONTENTS as RESTORE_CONTENTS, RSF_D3D11_HOST_ABI_VERSION,
    RSF_D3D11_REDUCED_CANCEL as CANCEL, RSF_D3D11_REDUCED_FINISH as FINISH,
    RSF_D3D11_REDUCED_FRAME_ABI_VERSION, RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED as FULL_OUTPUT,
    RSF_D3D11_REDUCED_NEXT_REDUCTION_ALLOWED as NEXT_REDUCTION_ALLOWED,
    RSF_D3D11_REDUCED_RESERVE as RESERVE, RSF_D3D11_REDUCED_RESERVED as RESERVED,
    RSF_D3D11_REDUCED_RETIRE as RETIRE, RSF_D3D11_REDUCED_SEAL as SEAL,
    RSF_D3D11_REDUCED_TEMPORAL_ATTEMPTED as TEMPORAL_ATTEMPTED,
    RSF_D3D11_REDUCED_TEMPORAL_OUTPUT as TEMPORAL_OUTPUT, RSF_ERROR_NATIVE_REFUSED,
    RSF_ERROR_NOT_READY, RSF_GAME_RENDER_AFTER_SIMULATION, RSF_GAME_RENDER_FG_INPUTS,
    RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_FRAME, RSF_GAME_RENDER_NO_FG,
    RSF_GAME_RENDER_PRIMARY, RSF_GAME_RENDER_RECONFIGURE, RSF_GAME_RENDER_SR,
    RSF_GAME_RENDER_SUBMISSION, RSF_GAME_RENDER_WINDOW, RSF_OK, RSF_SCREEN_GAMEPLAY,
    RSF_SCREEN_UNKNOWN, rsf_d3d11_auxiliary_pass, rsf_d3d11_auxiliary_scope,
    rsf_d3d11_auxiliary_surface as Surface, rsf_d3d11_host_capture, rsf_d3d11_reduced_frame,
    rsf_game_host_services, rsf_game_render_config, rsf_game_render_pass, rsf_result,
};
use std::cell::RefCell;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::ptr::null_mut;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use windows::Win32::Graphics::Direct3D11::{ID3D11DeviceContext, ID3D11Texture2D};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;
use windows::core::Interface;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::core::w;

use Ordering::{AcqRel, Acquire, Relaxed, Release};

// ---------------------------------------------------------------------------------------------
// Hook records and native guards.

const HOOK_COUNT: usize = 8;

// NativeRenderHook order, which is also the creation and enable order. Each entry is verified at
// its discovered site before creation and before every start; the TAA query must still be slot 8
// of ImageSpaceEffectTemporalAA's vtable.
const HOOKS: [Site; HOOK_COUNT] = [
    Site::DrWriter,
    Site::Prepass,
    Site::OuterPresent,
    Site::WorldBegin,
    Site::ViewportStore,
    Site::Imagespace,
    Site::Effects,
    Site::TaaQuery,
];

const IMAGESPACE_CALLS: [Site; 3] = [
    Site::ImagespaceCall1,
    Site::ImagespaceCall2,
    Site::ImagespaceCall3,
];

/// Every hook entry, the resolution, screen and depth-domain guards, then the call sites: the
/// outer Swap's native Present with its renderer argument (the Present pattern covers both
/// window-cell loads, the chain at +18 and the IDXGISwapChain::Present slot +40), the selected
/// prepass and DR calls with the camera update, its three camera reads, the builder call and
/// the native camera-setter caller (fallout4-depth-domains and fallout4-reduced-consumers
/// notes); then the ordinary MainDraw WorldBegin call, WorldBegin's viewport selection (in its
/// pattern) and temporal update calls, the Effects and HUDless calls inside imagespace, and the
/// three selected imagespace calls.
pub(crate) fn verify() -> bool {
    discovery::verify_all(&HOOKS)
        && resolution::verify()
        && screen::verify_queries()
        && depth_domains::verify()
        && discovery::verify_all(&[
            Site::PresentCall,
            Site::PrepassCall,
            Site::DrCall,
            Site::CameraReads,
            Site::BuilderCall,
            Site::SetterCall,
            Site::WorldCall,
            Site::WorldRanges,
            Site::EffectsCall,
            Site::HudlessCall,
        ])
        && discovery::verify_all(&IMAGESPACE_CALLS)
}

/// The render hook guards, then the render-input reader guards.
pub(crate) fn verify_with_inputs() -> bool {
    verify() && render_inputs::verify()
}

// ---------------------------------------------------------------------------------------------
// Native leaves, the bound Runtime and the lifecycle.

type UpscaleAuxiliaries =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *const rsf_d3d11_auxiliary_pass) -> i32;
type RestoreAuxiliaries =
    unsafe extern "C" fn(*mut c_void, *const rsf_d3d11_auxiliary_scope) -> i32;
type ReducedFrame =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *mut rsf_d3d11_reduced_frame) -> i32;
type CaptureTexture =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *const rsf_d3d11_host_capture) -> i32;
type CaptureRequested = unsafe extern "C" fn() -> c_int;
type Notify = unsafe extern "C" fn(*mut c_void, *mut c_void, *const rsf_game_render_pass, u32);
type Upscale = unsafe extern "C" fn(*mut c_void, *mut c_void, *const rsf_game_render_pass) -> i32;

unsafe extern "C" {
    fn rsf_fallout4_render_create_hook(hook: u32, target: *mut c_void) -> bool;
    fn rsf_fallout4_render_remove_hook(hook: u32, target: *mut c_void) -> bool;
    fn rsf_fallout4_render_entered() -> bool;
    fn rsf_fallout4_guard_notify(
        notify: Notify,
        user: *mut c_void,
        context: *mut c_void,
        pass: &rsf_game_render_pass,
        begin: u32,
    ) -> bool;
    fn rsf_fallout4_guard_upscale(
        upscale: Upscale,
        user: *mut c_void,
        context: *mut c_void,
        pass: &rsf_game_render_pass,
        result: &mut i32,
    ) -> bool;
    fn rsf_fallout4_guard_auxiliaries(
        upscale: UpscaleAuxiliaries,
        user: *mut c_void,
        context: *mut c_void,
        request: &rsf_d3d11_auxiliary_pass,
        result: &mut i32,
    ) -> bool;
    fn rsf_fallout4_guard_restore(
        restore: RestoreAuxiliaries,
        user: *mut c_void,
        scope: &rsf_d3d11_auxiliary_scope,
        result: &mut i32,
    ) -> bool;
    fn rsf_fallout4_guard_reduced(
        reduce: ReducedFrame,
        user: *mut c_void,
        context: *mut c_void,
        request: &mut rsf_d3d11_reduced_frame,
        result: &mut i32,
    ) -> bool;
    fn rsf_fallout4_guard_capture(
        capture: CaptureTexture,
        user: *mut c_void,
        context: *mut c_void,
        request: &rsf_d3d11_host_capture,
    ) -> bool;
    fn rsf_fallout4_guard_requested(query: CaptureRequested, requested: &mut bool) -> bool;
}

/// The planning and evaluation call of the host's upscaler; false when it threw.
pub(crate) fn guarded_upscale(
    services: &rsf_game_host_services,
    context: *mut c_void,
    pass: &rsf_game_render_pass,
    result: &mut i32,
) -> bool {
    services.process_upscale.is_some_and(|upscale| unsafe {
        rsf_fallout4_guard_upscale(upscale, services.user, context, pass, result)
    })
}

#[derive(Clone, Copy)]
struct Exports {
    upscale_auxiliaries: Option<UpscaleAuxiliaries>,
    restore_auxiliaries: Option<RestoreAuxiliaries>,
    reduced_frame: Option<ReducedFrame>,
    capture_texture: Option<CaptureTexture>,
    capture_requested: Option<CaptureRequested>,
}

/// The Runtime services and exports the callbacks use, written by prepare before any hook is
/// enabled.
#[derive(Clone, Copy)]
struct Bound {
    services: rsf_game_host_services,
    exports: Exports,
}
// SAFETY: process-lived Runtime services and exports.
unsafe impl Send for Bound {}

static BOUND: Mutex<Bound> = Mutex::new(Bound {
    // SAFETY: integers, a raw user pointer and optional callbacks, all valid at zero.
    services: unsafe { std::mem::zeroed() },
    exports: Exports {
        upscale_auxiliaries: None,
        restore_auxiliaries: None,
        reduced_frame: None,
        capture_texture: None,
        capture_requested: None,
    },
});
// Each record's created state: set when MinHook created it, cleared only by a successful removal.
static CREATED: Mutex<[bool; HOOK_COUNT]> = Mutex::new([false; HOOK_COUNT]);
static LIVE: AtomicBool = AtomicBool::new(false);
// Open WINDOW and FRAME scopes, open auxiliary scopes and owned recovery tokens, on any thread.
static WINDOWS: AtomicU32 = AtomicU32::new(0);
static FRAMES: AtomicU32 = AtomicU32::new(0);
static AUXILIARIES: AtomicU32 = AtomicU32::new(0);
static RECOVERIES: AtomicU32 = AtomicU32::new(0);

fn bound() -> Bound {
    *lock_or_recover(&BOUND)
}

fn target(hook: &Site) -> *mut c_void {
    address(*hook) as *mut c_void
}

fn report(text: &CStr) {
    let _ = crate::log_host(&bound().services, text);
}

/// The receipt sink handed to the input, depth and resolution readers.
unsafe extern "C" fn report_sink(text: *const c_char) {
    if !text.is_null() {
        report(unsafe { CStr::from_ptr(text) });
    }
}

const REPORT: Option<unsafe extern "C" fn(*const c_char)> = Some(report_sink);

/// The render hooks' shared budget of sixteen diagnostics.
fn diagnostic(text: &CStr) {
    static COUNT: AtomicU32 = AtomicU32::new(0);
    if COUNT.fetch_add(1, Relaxed) < 16 {
        report(text);
    }
}

fn live() -> bool {
    LIVE.load(Acquire)
}

/// A detour in flight, an open WINDOW, FRAME or auxiliary scope, an owned recovery token, or
/// pending resolution ownership.
pub(crate) fn busy() -> bool {
    (unsafe { rsf_fallout4_render_entered() })
        || WINDOWS.load(Acquire) != 0
        || FRAMES.load(Acquire) != 0
        || AUXILIARIES.load(Acquire) != 0
        || RECOVERIES.load(Acquire) != 0
        || resolution_busy()
}

/// Checks the synchronous services, negotiates the Runtime exports and creates every missing
/// detour disabled. A creation failure removes the hooks this call created, in reverse order; a
/// removal MinHook refuses keeps its record and trampoline.
pub(crate) fn prepare(services: &rsf_game_host_services) -> bool {
    if live() || busy() || services.process_upscale.is_none() || services.render_pass.is_none() {
        return false;
    }
    let runtime = unsafe { GetModuleHandleW(w!("ReScaleFrame.Runtime.dll")) };
    // SAFETY: the process-lived Runtime exports these signatures in d3d11_host.h and input_dump.h.
    let exports = unsafe {
        Exports {
            upscale_auxiliaries: typed_export(runtime, c"rsf_d3d11_host_upscale_auxiliaries"),
            restore_auxiliaries: typed_export(runtime, c"rsf_d3d11_host_restore_auxiliaries"),
            reduced_frame: typed_export(runtime, c"rsf_d3d11_host_reduced_frame"),
            capture_texture: typed_export(runtime, c"rsf_d3d11_host_capture_texture"),
            capture_requested: typed_export(runtime, c"rsf_input_dump_requested"),
        }
    };
    lock_or_recover(&BOUND).exports = exports;
    // Independent export negotiation. An older runtime retains native raster.
    if exports.upscale_auxiliaries.is_none()
        || exports.restore_auxiliaries.is_none()
        || services.render_config.is_none()
    {
        return false;
    }
    let mut created = lock_or_recover(&CREATED);
    let mut added = Vec::with_capacity(HOOK_COUNT);
    for (index, hook) in HOOKS.iter().enumerate() {
        if created[index] {
            continue;
        }
        if unsafe { rsf_fallout4_render_create_hook(index as u32, target(hook)) } {
            created[index] = true;
            added.push(index);
            continue;
        }
        while let Some(index) = added.pop() {
            if unsafe { rsf_fallout4_render_remove_hook(index as u32, target(&HOOKS[index])) } {
                created[index] = false;
            } else {
                report(
                    c"Fallout4 cold render-hook rollback refused; retained hook ownership, startup remains refused.",
                );
            }
        }
        return false;
    }
    lock_or_recover(&BOUND).services = *services;
    true
}

/// Rechecks the guards, queues every created detour, sets live and applies the queue. A missing
/// record or a refused queue or apply clears live and queues every created detour disabled.
pub(crate) fn start() -> bool {
    use rsf_minhook_sys::{MH_ApplyQueued, MH_QueueDisableHook, MH_QueueEnableHook};
    if busy() || !verify() {
        return false;
    }
    let created = lock_or_recover(&CREATED);
    let mut queued = true;
    for (index, hook) in HOOKS.iter().enumerate() {
        if !created[index] || unsafe { MH_QueueEnableHook(target(hook)) } != MH_OK {
            queued = false;
        }
    }
    if queued {
        LIVE.store(true, Release);
    }
    if !queued || unsafe { MH_ApplyQueued() } != MH_OK {
        LIVE.store(false, Release);
        for (index, hook) in HOOKS.iter().enumerate() {
            if created[index] {
                let _ = unsafe { MH_QueueDisableHook(target(hook)) };
            }
        }
        let _ = unsafe { MH_ApplyQueued() };
        return false;
    }
    drop(created);
    report(
        c"Fallout4 guarded native DR/SR/auxiliary output callbacks enabled; actual producer and screen admission required, FG/input capabilities remain gated.",
    );
    true
}

pub(crate) fn quiesce() {
    LIVE.store(false, Release);
}

/// Clears live and disables every created detour once no scope is open. Trampolines and records
/// stay for a later start.
pub(crate) fn stop() -> bool {
    quiesce();
    if busy() {
        return false;
    }
    let created = lock_or_recover(&CREATED);
    let mut disabled = true;
    for (index, hook) in HOOKS.iter().enumerate() {
        if created[index] {
            disabled = crate::disable_hook(target(hook) as usize) && disabled;
        }
    }
    drop(created);
    disabled && !busy()
}

// ---------------------------------------------------------------------------------------------
// The calling thread's World.

/// The reduced-render recovery transaction of one source.
#[derive(Default)]
struct Recovery {
    token: u64,
    // The held raster lease the surfaces name.
    inputs: Option<Inputs>,
    // The full-output owner recorded at reservation.
    owner: OutputOwner,
    surfaces: [Surface; 5],
    committed: bool,
    sealed: bool,
    finished: bool,
}

#[derive(Default)]
struct World {
    cpu: CpuIdentity,
    pass: rsf_game_render_pass,
    frame_pass: rsf_game_render_pass,
    submission_pass: rsf_game_render_pass,
    screen_basis: rsf_game_render_pass,
    screen: Screen,
    depth_snapshot: Snapshot,
    raster_snapshot: Snapshot,
    recovery: Recovery,
    auxiliary_scope: rsf_d3d11_auxiliary_scope,
    context: Option<ID3D11DeviceContext>,
    chain: Option<IDXGISwapChain>,
    // Kept to the end of the source, with the published passes that name them.
    final_colour: Option<ID3D11Texture2D>,
    colour: Option<ID3D11Texture2D>,
    depth: Option<ID3D11Texture2D>,
    motion: Option<ID3D11Texture2D>,
    full_output_resolved: bool,
    in_imagespace: bool,
    inputs: bool,
    published: bool,
    window_open: bool,
    frame_open: bool,
    submission_open: bool,
    refused: bool,
    depth_interrupted: bool,
    auxiliary_open: bool,
    raster_refused: bool,
}

impl World {
    fn new() -> Self {
        Self {
            screen: Screen::EMPTY,
            ..Self::default()
        }
    }

    /// The pass the resolution receipts describe.
    fn state(&self) -> &rsf_game_render_pass {
        if self.inputs {
            &self.pass
        } else {
            &self.frame_pass
        }
    }

    fn context(&self) -> *mut c_void {
        raw(&self.context)
    }

    fn chain(&self) -> *mut c_void {
        raw(&self.chain)
    }

    fn same_source(&self) -> bool {
        self.cpu.session == bound().services.session_id && render_source_is_current(&self.cpu)
    }

    /// Reports the resolution stage on the World's current pass.
    fn report_state(&self, stage: u32, result: rsf_result) {
        resolution::report(
            stage,
            self.state(),
            self.context(),
            self.published,
            result,
            REPORT,
        );
    }
}

thread_local! {
    // Each detour borrows the World for its Rust step only; the originals run outside it.
    static WORLD: RefCell<World> = RefCell::new(World::new());
}

fn with_world<R>(step: impl FnOnce(&mut World) -> R) -> R {
    WORLD.with_borrow_mut(step)
}

fn raw<T: Interface>(object: &Option<T>) -> *mut c_void {
    object.as_ref().map_or(null_mut(), Interface::as_raw)
}

/// A new reference to the live interface at `address`.
///
/// # Safety
/// `address` is zero or a live interface of type `T`.
unsafe fn referenced<T: Interface>(address: usize) -> Option<T> {
    let raw = address as *mut c_void;
    unsafe { T::from_raw_borrowed(&raw) }.cloned()
}

fn notify(pass: &rsf_game_render_pass, context: *mut c_void, begin: u32) {
    let services = bound().services;
    if let Some(callback) = services.render_pass {
        unsafe { rsf_fallout4_guard_notify(callback, services.user, context, pass, begin) };
    }
}

/// Pass identity from the World's CPU source. The admitted ordinary WorldBegin follows the
/// native update/redraw work; this constructor fact does not report a SIM_END or classify
/// gameplay.
fn identify(pass: &mut rsf_game_render_pass, cpu: &CpuIdentity) {
    pass.session_id = cpu.session;
    pass.source_frame_id = cpu.source;
    pass.submission_id = cpu.submission;
    pass.scope_id = cpu.submission;
    pass.flags |=
        RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_AFTER_SIMULATION | RSF_GAME_RENDER_NO_FG;
    pass.screen = RSF_SCREEN_UNKNOWN;
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Owner {
    renderer: usize,
    device: usize,
    context: usize,
    chain: usize,
    window: usize,
    graph: usize,
    view: usize,
    frame: u32,
}

fn read_owner(base: usize) -> Option<Owner> {
    let renderer =
        read::<usize>(renderer_pointer(base)).filter(|&value| value == renderer_object(base))?;
    let device = read::<usize>(renderer + RENDERER_DEVICE).filter(|&value| value != 0)?;
    let context = read::<usize>(renderer + RENDERER_CONTEXT).filter(|&value| value != 0)?;
    let window = read::<usize>(renderer + RENDERER_WINDOW).filter(|&value| value != 0)?;
    let chain = read::<usize>(renderer + RENDERER_CHAIN).filter(|&value| value != 0)?;
    let graph = read::<usize>(base + rva(Site::WorldGraph)).filter(|&value| value != 0)?;
    let view = read::<usize>(graph + 0x140).filter(|&value| value != 0)?;
    let frame = read::<u32>(base + rva(Site::State) + 0x98)?;
    accept_window(window as *mut c_void).then_some(Owner {
        renderer,
        device,
        context,
        chain,
        window,
        graph,
        view,
        frame,
    })
}

/// The native owner still names the FRAME's frame, view and window keys and the World Context
/// and chain.
fn leased(world: &World, owner: &Owner) -> bool {
    let frame = &world.frame_pass;
    u64::from(owner.frame) == frame.native_frame
        && owner.view as u64 == frame.view_key
        && owner.window as u64 == frame.window_key
        && owner.context == world.context() as usize
        && owner.chain == world.chain() as usize
}

// ---------------------------------------------------------------------------------------------
// FRAME, SUBMISSION and WINDOW scope.

static BEGIN_ANNOUNCED: AtomicBool = AtomicBool::new(false);
static PRESENT_ANNOUNCED: AtomicBool = AtomicBool::new(false);

/// FRAME then SUBMISSION, opened before the original ordinary WorldBegin once the owner read
/// before and after the Context and chain lease match and the source is current.
fn begin_scope(world: &mut World) -> bool {
    let Some(before) = read_owner(image()) else {
        return false;
    };
    // SAFETY: the owner read names the renderer's live Context and chain.
    let (Some(context), Some(chain)) = (
        unsafe { referenced::<ID3D11DeviceContext>(before.context) },
        unsafe { referenced::<IDXGISwapChain>(before.chain) },
    ) else {
        return false;
    };
    if !d3d11::owner_matches(&context, &chain, before.device, before.window)
        || read_owner(image()) != Some(before)
        || !world.same_source()
    {
        return false;
    }
    world.context = Some(context);
    world.chain = Some(chain);
    let frame = &mut world.frame_pass;
    frame.struct_size = size_of::<rsf_game_render_pass>() as u32;
    identify(frame, &world.cpu);
    frame.role = RSF_GAME_RENDER_FRAME;
    frame.native_frame = u64::from(before.frame);
    frame.family_key = before.view as u64;
    frame.view_key = before.view as u64;
    frame.history_key = before.view as u64;
    frame.viewport_key = before.window as u64;
    frame.window_key = before.window as u64;
    frame.rhi_viewport_key = before.window as u64;
    frame.pass_key = address(Site::WorldBegin) as u64;
    frame.swapchain = before.chain as *mut c_void;
    world.submission_pass = world.frame_pass;
    world.submission_pass.role = RSF_GAME_RENDER_SUBMISSION;
    world.frame_open = true;
    FRAMES.fetch_add(1, AcqRel);
    notify(&world.frame_pass, world.context(), 1);
    world.submission_open = true;
    notify(&world.submission_pass, world.context(), 1);
    if !BEGIN_ANNOUNCED.swap(true, Relaxed) {
        report(
            c"Fallout4 native FRAME/SUBMISSION begin before ordinary WorldBegin; screen/input eligibility not inferred.",
        );
    }
    true
}

/// Ends an open SUBMISSION once, with NO_FG when the source is refused.
fn close_submission(world: &mut World) {
    if !world.submission_open {
        return;
    }
    if world.refused {
        world.submission_pass.flags |= RSF_GAME_RENDER_NO_FG;
    }
    notify(&world.submission_pass, world.context(), 0);
    world.submission_open = false;
}

/// The guarded auxiliary restore; true when none is open or it drained.
fn drain_auxiliaries(world: &mut World) -> bool {
    if !world.auxiliary_open {
        return true;
    }
    let Bound { services, exports } = bound();
    let mut result = RSF_ERROR_NOT_READY;
    if let Some(restore) = exports.restore_auxiliaries {
        unsafe {
            rsf_fallout4_guard_restore(restore, services.user, &world.auxiliary_scope, &mut result)
        };
    }
    if result != RSF_OK {
        world.refused = true;
        return false;
    }
    world.auxiliary_open = false;
    AUXILIARIES.fetch_sub(1, AcqRel);
    true
}

/// The CPU owner's post-Swap scope end: balance a SUBMISSION the pre-Present guard withheld, end
/// WINDOW then FRAME, retire recovery, drain auxiliaries, finish the resolution source and reset
/// the World only when no obligation remains.
pub(crate) fn finish_scope() {
    with_world(|world| {
        if world.cpu.source == 0 {
            return;
        }
        if !world.same_source() {
            world.refused = true;
        }
        // Refused or unpresented sources drain here; only the guarded pre-Present path can
        // provide completion in time for that source's generation prepare.
        if world.submission_open {
            world.refused = true;
            diagnostic(
                c"Fallout4 refused/unpresented SUBMISSION balanced after Swap, before FRAME end.",
            );
            close_submission(world);
        }
        if world.refused {
            world.pass.flags |= RSF_GAME_RENDER_NO_FG;
        }
        if world.window_open {
            notify(&world.pass, world.context(), 0);
            world.window_open = false;
            WINDOWS.fetch_sub(1, AcqRel);
        }
        if world.frame_open {
            if world.refused {
                world.frame_pass.flags |= RSF_GAME_RENDER_NO_FG;
            }
            notify(&world.frame_pass, world.context(), 0);
            world.frame_open = false;
            FRAMES.fetch_sub(1, AcqRel);
        }
        let _ = retire(world);
        let _ = drain_auxiliaries(world);
        let finished = resolution::finish_current_source();
        if !finished {
            world.refused = true;
        }
        world.report_state(
            SOURCE_RETIRED,
            if finished {
                RSF_OK
            } else {
                RSF_ERROR_NOT_READY
            },
        );
        if !resolution_busy() && !world.auxiliary_open && world.recovery.token == 0 {
            *world = World::new();
        }
    })
}

// ---------------------------------------------------------------------------------------------
// The reduced-render recovery transaction.

// Refusal receipts by stage, each reported once per process.
const REFUSALS: [&CStr; 8] = [
    c"Fallout4 reduction preflight refused: recovery export/current source unavailable or previous token remains owned.",
    c"Fallout4 reduction preflight refused: required native raster resources or full-output owner unavailable; native ratios retained.",
    c"Fallout4 reduction reservation refused or native owner/resources changed before ratio commitment.",
    c"Fallout4 committed recovery refused: same-frame native raster/resource producer unavailable; token retained.",
    c"Fallout4 committed recovery refused: resources/native output owner changed since reservation; token retained.",
    c"Fallout4 committed recovery SEAL refused; no temporal publication, token retained.",
    c"Fallout4 committed recovery FINISH withheld: native producer/output owner unverified; foreign resources untouched, token retained.",
    c"Fallout4 committed recovery full-output native scope refused; token retained.",
];

/// Reports the stage once, refuses the World's temporal output while a reservation or its
/// commitment exists, and answers false.
fn refusal(world: &mut World, stage: usize) -> bool {
    static STAGES: AtomicU32 = AtomicU32::new(0);
    let bit = 1u32 << stage;
    if STAGES.fetch_or(bit, Relaxed) & bit == 0 {
        report(REFUSALS[stage]);
    }
    if world.recovery.committed || world.recovery.token != 0 {
        world.refused = true;
    }
    false
}

/// The recovery surfaces of a raster lease: depth, motion, Pip-Boy, VATS, refraction normals.
fn plan(inputs: &Inputs) -> [Surface; 5] {
    const NAMES: [&CStr; 5] = [
        c"native_depth_expanded",
        c"native_motion_expanded",
        c"native_pipboy_expanded",
        c"native_vats_expanded",
        c"native_refraction_expanded",
    ];
    let slots = [DEPTH, MOTION, PIPBOY, VATS, REFRACTION];
    std::array::from_fn(|index| Surface {
        struct_size: size_of::<Surface>() as u32,
        kind: if index == 0 { DEPTH_STENCIL } else { COLOUR },
        texture: inputs.texture(slots[index]),
        source_rect: inputs.pass.render_rect.map(|edge| edge as f32),
        output_rect: inputs.pass.output_rect,
        // Numeric nearest recovery keeps the native jittered integer lattice. These zero operator
        // offsets do not report zero native camera jitter.
        jitter_pixels: [0.0; 2],
        name: NAMES[index].as_ptr(),
        source_name: NAMES[index].as_ptr(),
        flags: if index == 3 { RESTORE_CONTENTS } else { 0 },
    })
}

/// One reduced-frame operation on the held lease. RESERVE passes token zero.
fn reduce(world: &World, operation: u32, token: u64) -> Option<(rsf_d3d11_reduced_frame, i32)> {
    let Bound { services, exports } = bound();
    let reduce = exports.reduced_frame?;
    let held = world.recovery.inputs.as_ref()?;
    let mut request = rsf_d3d11_reduced_frame {
        struct_size: size_of::<rsf_d3d11_reduced_frame>() as u32,
        abi_version: RSF_D3D11_REDUCED_FRAME_ABI_VERSION,
        operation,
        disposition: 0,
        token,
        owner: &held.pass,
        surface_count: 5,
        reserved: 0,
        surfaces: world.recovery.surfaces.as_ptr(),
        temporal_result: 0,
        result_flags: 0,
    };
    let mut result = RSF_ERROR_NOT_READY;
    unsafe {
        rsf_fallout4_guard_reduced(
            reduce,
            services.user,
            held.context.as_raw(),
            &mut request,
            &mut result,
        )
    };
    Some((request, result))
}

/// One operation on the owned token. FINISH sets the World's full-output and temporal verdict from
/// the host flags. A successful CANCEL or RETIRE that allows the next reduction drains the token
/// and releases the held lease; any other answer retains both.
fn recovery_call(world: &mut World, operation: u32) -> bool {
    if world.recovery.token == 0 {
        return false;
    }
    let Some((request, result)) = reduce(world, operation, world.recovery.token) else {
        return false;
    };
    if operation == FINISH {
        world.full_output_resolved = request.result_flags & FULL_OUTPUT != 0;
        world.published = world.full_output_resolved && request.result_flags & TEMPORAL_OUTPUT != 0;
        if !world.published {
            world.refused = true;
        }
        static ROWS: AtomicU32 = AtomicU32::new(0);
        if ROWS.fetch_add(1, Relaxed) < 32 {
            let native = world
                .recovery
                .inputs
                .as_ref()
                .map_or(0, |held| held.pass.native_frame);
            report(&message(format!(
                "Fallout4 reduced recovery source={} native={} result={} disposition={} flags=0x{:x} temporal_result={} temporal_attempted={}",
                world.cpu.source,
                native,
                result,
                request.disposition,
                request.result_flags,
                request.temporal_result,
                u32::from(request.result_flags & TEMPORAL_ATTEMPTED != 0),
            )));
        }
    }
    if result == RSF_OK
        && (operation == CANCEL || operation == RETIRE)
        && request.result_flags & NEXT_REDUCTION_ALLOWED != 0
    {
        world.recovery.token = 0;
        world.recovery.inputs = None;
        world.recovery.surfaces = Default::default();
        RECOVERIES.fetch_sub(1, AcqRel);
    }
    result == RSF_OK
}

/// Preflight, RESERVE, then confirm the same source, output owner and resources before the
/// caller's first ratio write. A failed confirmation cancels the unused reservation.
fn reserve(world: &mut World, planned: &rsf_game_render_pass) -> bool {
    if !live()
        || bound().exports.reduced_frame.is_none()
        || world.recovery.token != 0
        || !world.frame_open
        || !world.same_source()
    {
        return refusal(world, 0);
    }
    let Some(mut inputs) = render_inputs::read_raster(&world.frame_pass, None) else {
        return refusal(world, 1);
    };
    let Some(owner) = resolution::preflight(planned, world.context()) else {
        return refusal(world, 1);
    };
    inputs.pass.render_rect = planned.render_rect;
    world.recovery.owner = owner;
    world.recovery.surfaces = plan(&inputs);
    world.recovery.inputs = Some(inputs);
    let (request, result) = reduce(world, RESERVE, 0).unwrap_or((
        // SAFETY: integers and raw pointers, all valid at zero.
        unsafe { std::mem::zeroed() },
        RSF_ERROR_NOT_READY,
    ));
    if request.token != 0 {
        world.recovery.token = request.token;
        RECOVERIES.fetch_add(1, AcqRel);
    }
    let unchanged = result == RSF_OK
        && request.token != 0
        && request.disposition == RESERVED
        && live()
        && world.same_source()
        && resolution::preflight(planned, world.context()) == Some(world.recovery.owner)
        && render_inputs::read_raster(&world.frame_pass, None).is_some_and(|confirmed| {
            world
                .recovery
                .inputs
                .as_ref()
                .is_some_and(|held| confirmed.same_resources(held, false))
        });
    if !unchanged {
        if world.recovery.token != 0 {
            let _ = recovery_call(world, CANCEL);
        }
        return refusal(world, 2);
    }
    // Mark before the caller's first ratio write. Even a partial native-state write must retain
    // the reservation until explicit cancellation or retirement.
    world.recovery.committed = true;
    true
}

/// Adopts the same-frame produced resources, which must match the reservation and its output
/// owner, then SEALs them before any temporal reader.
fn seal(world: &mut World) -> bool {
    if world.recovery.token == 0 || world.recovery.sealed {
        return true;
    }
    let current = if world.raster_snapshot.raster_valid {
        render_inputs::read_raster(&world.frame_pass, Some(&world.raster_snapshot))
    } else {
        None
    };
    let Some(current) = current else {
        return refusal(world, 3);
    };
    let same = world
        .recovery
        .inputs
        .as_ref()
        .is_some_and(|held| current.same_resources(held, true));
    if !same
        || resolution::preflight(&current.pass, current.context.as_raw())
            != Some(world.recovery.owner)
    {
        return refusal(world, 4);
    }
    world.recovery.surfaces = plan(&current);
    world.recovery.inputs = Some(current);
    if !recovery_call(world, SEAL) {
        return refusal(world, 5);
    }
    world.recovery.sealed = true;
    true
}

/// FINISH a committed token once, inside the full-output scope. Answers whether full output was
/// resolved; a finished token answers the World's current full-output flag.
fn finish(world: &mut World) -> bool {
    let recovery = &world.recovery;
    if recovery.token == 0 || !recovery.committed {
        return true;
    }
    if recovery.finished {
        return world.full_output_resolved;
    }
    // Missing producer/owner proof is an unresolved committed frame, never an invitation to read
    // the new owner's resources or cancel the obligation.
    let Some(held) = recovery.inputs.as_ref() else {
        return refusal(world, 6);
    };
    if !recovery.sealed
        || !depth_domains::raster_current(&world.raster_snapshot)
        || resolution::preflight(&held.pass, held.context.as_raw()) != Some(recovery.owner)
    {
        return refusal(world, 6);
    }
    if !resolution::owns_full_output()
        && resolution::begin_output(&world.raster_snapshot, &held.pass).is_none()
    {
        return refusal(world, 7);
    }
    let finished = recovery_call(world, FINISH) && world.full_output_resolved;
    world.recovery.finished = finished;
    if !finished {
        world.refused = true;
    }
    finished
}

/// CANCEL an uncommitted token; FINISH then RETIRE a committed one. A finished token goes straight
/// to RETIRE, so a retry after the imagespace exit cleared `full_output_resolved` still reaches
/// the host and FINISH is never repeated. Success means no token is still owned.
fn retire(world: &mut World) -> bool {
    if world.recovery.token == 0 {
        return true;
    }
    if !world.recovery.committed {
        return recovery_call(world, CANCEL) && world.recovery.token == 0;
    }
    if !world.recovery.finished && !finish(world) {
        return false;
    }
    recovery_call(world, RETIRE) && world.recovery.token == 0
}

// ---------------------------------------------------------------------------------------------
// SR and HUDless publication.

/// Classifies the screen against the World's screen basis. The first call of a source records the
/// leased screen and its receipt; every call refuses the World unless the native screen is
/// unrefused gameplay, the source is current, the leased screen is unchanged and FRAME and
/// SUBMISSION are open. The pass carries the screen and NO_FG follows the World refusal.
fn screen_facts(world: &mut World, pass: &mut rsf_game_render_pass, first: bool) {
    let facts = screen::classify(&world.cpu, &world.screen_basis, true);
    if first {
        world.screen = facts;
        static ROWS: AtomicU32 = AtomicU32::new(0);
        if ROWS.fetch_add(1, Relaxed) < 8 {
            report(&message(format!(
                "Fallout4 screen facts source={} native={} screen={} facts=0x{:x} refusals=0x{:x} camera={}",
                world.cpu.source,
                pass.native_frame,
                facts.screen,
                facts.facts,
                facts.refusals,
                facts.camera_index,
            )));
        }
    }
    if facts.refusals != 0
        || facts.screen != RSF_SCREEN_GAMEPLAY
        || !world.same_source()
        || facts.screen != world.screen.screen
        || facts.facts != world.screen.facts
        || facts.camera_state != world.screen.camera_state
        || !world.frame_open
        || !world.submission_open
    {
        world.refused = true;
    }
    pass.screen = facts.screen;
    if world.refused {
        pass.flags |= RSF_GAME_RENDER_NO_FG;
    } else {
        pass.flags &= !RSF_GAME_RENDER_NO_FG;
    }
}

// Keep the depth mapping on the integer render rectangle. The colour surfaces share the producer's
// fractional viewport and the same camera jitter.
fn reconstruct(world: &mut World, inputs: &Inputs, pass: &rsf_game_render_pass) -> rsf_result {
    let Bound { services, exports } = bound();
    let context = inputs.context.as_raw();
    let reduced = pass.camera.render_width != pass.camera.output_width
        || pass.camera.render_height != pass.camera.output_height
        || pass.render_rect[0] != 0
        || pass.render_rect[1] != 0;
    if !reduced {
        let mut result = RSF_ERROR_NOT_READY;
        if !guarded_upscale(&services, context, pass, &mut result) {
            let _ = drain_auxiliaries(world);
            return RSF_ERROR_NOT_READY;
        }
        return result;
    }
    let [width, height] = [pass.camera.output_width, pass.camera.output_height];
    let auxiliary = |slot| {
        inputs.textures[slot as usize]
            .as_ref()
            .filter(|&texture| d3d11::auxiliary_colour(texture, &inputs.context, width, height))
    };
    if !inputs.textures[DEPTH as usize]
        .as_ref()
        .is_some_and(d3d11::auxiliary_depth)
        || auxiliary(MOTION).is_none()
    {
        return RSF_ERROR_NATIVE_REFUSED;
    }
    let [x, y, w, h] = [0, 1, 2, 3].map(|index| world.depth_snapshot.actual_raster[index]);
    let colours = [x, y, x + w, y + h];
    let names = [
        (c"native_depth_expanded", c"native_depth_partitioned"),
        (c"native_motion_expanded", c"native_motion_reduced"),
        (c"native_pipboy_expanded", c"native_pipboy_reduced"),
        (c"native_vats_expanded", c"native_vats_reduced"),
        (c"native_refraction_expanded", c"native_refraction_reduced"),
    ];
    let mut surfaces = [Surface::default(); 5];
    let mut count = 0;
    for (index, (slot, (name, source_name))) in [DEPTH, MOTION, PIPBOY, VATS, REFRACTION]
        .into_iter()
        .zip(names)
        .enumerate()
    {
        let texture = inputs.texture(slot);
        // SAFETY: the colour output and earlier surfaces name textures of the input lease.
        if index >= 2
            && (auxiliary(slot).is_none()
                || unsafe { d3d11::same_raw(texture, pass.color_output) }
                || surfaces[..count]
                    .iter()
                    .any(|surface| unsafe { d3d11::same_raw(texture, surface.texture) }))
        {
            continue;
        }
        let depth = index == 0;
        surfaces[count] = Surface {
            struct_size: size_of::<Surface>() as u32,
            kind: if depth { DEPTH_STENCIL } else { COLOUR },
            texture,
            source_rect: if depth {
                pass.render_rect.map(|edge| edge as f32)
            } else {
                colours
            },
            output_rect: pass.output_rect,
            jitter_pixels: pass.camera.jitter_pixels,
            name: name.as_ptr(),
            source_name: source_name.as_ptr(),
            flags: if index == 3 { RESTORE_CONTENTS } else { 0 },
        };
        count += 1;
    }
    if world.recovery.token != 0 && count != surfaces.len() {
        return RSF_ERROR_NATIVE_REFUSED;
    }
    let request = rsf_d3d11_auxiliary_pass {
        struct_size: size_of::<rsf_d3d11_auxiliary_pass>() as u32,
        abi_version: RSF_D3D11_HOST_ABI_VERSION,
        pass,
        surface_count: count as u32,
        surfaces: surfaces.as_ptr(),
    };
    if world.recovery.token == 0 {
        world.auxiliary_scope = rsf_d3d11_auxiliary_scope {
            struct_size: size_of::<rsf_d3d11_auxiliary_scope>() as u32,
            abi_version: RSF_D3D11_HOST_ABI_VERSION,
            session_id: pass.session_id,
            native_frame: pass.native_frame,
            view_key: pass.view_key,
        };
        world.auxiliary_open = true;
        AUXILIARIES.fetch_add(1, AcqRel);
    }
    // The host adopts the sealed recovery originals while retaining this actual fractional
    // viewport/jitter temporal mapping. One evaluation, one owner.
    let mut result = RSF_ERROR_NOT_READY;
    let returned = exports.upscale_auxiliaries.is_some_and(|upscale| unsafe {
        rsf_fallout4_guard_auxiliaries(upscale, services.user, context, &request, &mut result)
    });
    if !returned {
        let _ = drain_auxiliaries(world);
        return RSF_ERROR_NOT_READY;
    }
    if result != RSF_OK && world.recovery.token == 0 {
        let _ = drain_auxiliaries(world);
    }
    result
}

/// The SR seam before the original Effects range. The recovery token is SEALed before any
/// temporal reader, CPU attribution or screen gate, and FINISHed when the seam returns.
fn emit_sr(world: &mut World) {
    if seal(world) {
        publish_sr(world);
    } else {
        world.refused = true;
    }
    let _ = finish(world);
}

fn publish_sr(world: &mut World) {
    let services = bound().services;
    if !live()
        || !world.in_imagespace
        || !world.same_source()
        || world.inputs
        || world.raster_refused
        || world.auxiliary_open
        || services.process_upscale.is_none()
    {
        return;
    }
    let Some(inputs) = render_inputs::read_temporal(REPORT, Some(&world.depth_snapshot)) else {
        return;
    };
    if world.recovery.token != 0
        && !world
            .recovery
            .inputs
            .as_ref()
            .is_some_and(|held| inputs.same_resources(held, false))
    {
        return;
    }
    let mut pass = inputs.pass;
    identify(&mut pass, &world.cpu);
    pass.role = RSF_GAME_RENDER_SR;
    pass.pass_key = (address(Site::EffectsCall) - CALL) as u64;
    if !resolution::inputs_match(&pass) {
        diagnostic(
            c"Fallout4 SR refused: actual producer LTRB crop/camera/output extents disagree.",
        );
        return;
    }
    let context = inputs.context.as_raw();
    let frame = &world.frame_pass;
    if !world.frame_open
        || pass.native_frame != frame.native_frame
        || pass.view_key != frame.view_key
        || pass.window_key != frame.window_key
        || context != world.context()
        || inputs.chain.as_raw() != world.chain()
    {
        world.refused = true;
        diagnostic(
            c"Fallout4 SR refused: native lease differs from the pre-world FRAME/SUBMISSION owner.",
        );
        return;
    }
    let domains = if world.depth_snapshot.valid {
        depth_domains::make(&world.depth_snapshot, &world.cpu, &pass, REPORT)
    } else {
        None
    };
    let Some(domains) = domains else {
        world.refused = true;
        diagnostic(
            c"Fallout4 temporal publication refused: same-source native prepass depth mapping unavailable; raw mixed depth not substituted.",
        );
        return;
    };
    pass.depth_domains = domains;
    let mut planning = pass;
    planning.flags |= RSF_GAME_RENDER_RECONFIGURE;
    if !guarded_upscale(&services, context, &planning, &mut 0) {
        return;
    }
    let mut requested = rsf_game_render_config::request();
    let configured = resolution::query_config(&services, &mut requested);
    if configured < 0 {
        diagnostic(c"Fallout4 SR callback failed; native imagespace retained.");
        return;
    }
    let sr_off = configured > 0
        && requested.enabled == 0
        && requested.output_width == pass.camera.output_width
        && requested.output_height == pass.camera.output_height;
    world.colour = inputs.textures[crate::sys::native_input_colour as usize].clone();
    world.depth = inputs.textures[DEPTH as usize].clone();
    world.motion = inputs.textures[MOTION as usize].clone();
    world.screen_basis = pass;
    let mut fg = pass;
    screen_facts(world, &mut fg, true);
    if world.screen.refusals != 0
        || world.screen.screen != RSF_SCREEN_GAMEPLAY
        || !world.same_source()
    {
        return;
    }
    // SR and diagnostic consumers receive the same verified screen fact.
    pass.screen = fg.screen;
    fg.role = RSF_GAME_RENDER_FG_INPUTS;
    notify(&fg, context, 1);
    notify(&fg, context, 0);
    if !world.same_source() {
        world.refused = true;
        return;
    }
    // The host normalizes and retains these contents synchronously before SR. No borrowed
    // resource pointer survives in the scalar identity.
    world.pass = fg.without_resources();
    world.context = Some(inputs.context.clone());
    world.chain = Some(inputs.chain.clone());
    world.inputs = true;
    if sr_off {
        // Keep actual FG/native output facts, with no SR/output-state mutation.
        return;
    }
    if resolution::begin_output(&world.depth_snapshot, &pass).is_none() {
        resolution::report(
            OUTPUT_READY,
            &pass,
            context,
            false,
            RSF_ERROR_NOT_READY,
            REPORT,
        );
        world.refused = true;
        return;
    }
    resolution::report(OUTPUT_READY, &pass, context, false, RSF_OK, REPORT);
    let result = reconstruct(world, &inputs, &pass);
    resolution::report(EVALUATED, &pass, context, result == RSF_OK, result, REPORT);
    if result != RSF_OK {
        // A sealed ticket owns rollback/recovery; retain its output scope until FINISH so spatial
        // resolution is not followed by reduced selectors.
        if world.recovery.token == 0 && !resolution::end_current_output() {
            world.refused = true;
        }
        return;
    }
    world.published = true;
    // Full output is resolved here only without a token, whose FINISH decides it.
    if world.recovery.token == 0 {
        world.full_output_resolved = true;
    }
    diagnostic(
        c"Fallout4 producer-verified SR publication accepted before imagespace; scoped native output/auxiliary consumers active, FG remains unproven.",
    );
}

/// MSVC `%p`: sixteen uppercase hexadecimal digits.
fn pointer(value: usize) -> String {
    format!("{value:016X}")
}

/// The first refusal of each HUDless stage, naming the framebuffer and the retained identities.
fn hudless_refusal(
    world: &World,
    stage: u32,
    name: &str,
    frame: Option<&render_inputs::FramebufferLease>,
) {
    static STAGES: AtomicU32 = AtomicU32::new(0);
    let bit = 1u32 << stage;
    if STAGES.fetch_or(bit, Relaxed) & bit != 0 {
        return;
    }
    let (native, window, context, chain) = frame.map_or((0, 0, 0, 0), |frame| {
        (
            frame.native_frame,
            frame.window_key as usize,
            frame.context.as_raw() as usize,
            frame.chain.as_raw() as usize,
        )
    });
    report(&message(format!(
        "Fallout4 HUDless stage={} source={} native={}/{} frame_basis={} screen_basis={} window={}/{} context={}/{} outer_chain={}/{}",
        name,
        world.cpu.source,
        native,
        world.pass.native_frame,
        world.frame_pass.native_frame,
        world.screen_basis.native_frame,
        pointer(window),
        pointer(world.pass.window_key as usize),
        pointer(context),
        pointer(world.context() as usize),
        pointer(chain),
        pointer(world.chain() as usize),
    )));
}

/// The HUDless seam after the native HUDless viewport selection: FINAL_SCENE on the leased
/// framebuffer, then WINDOW, which retains it to the end of the source.
fn emit_hudless(world: &mut World) {
    if !world.in_imagespace || !world.inputs || world.window_open || !world.same_source() {
        return;
    }
    // A failed logical-0 read is not evidence that any identity comparison failed.
    let Some(frame) = render_inputs::acquire_framebuffer(REPORT) else {
        hudless_refusal(world, 0, "framebuffer-read", None);
        return;
    };
    let refused = if frame.native_frame != world.pass.native_frame {
        Some((1, "native-frame"))
    } else if frame.window_key != world.pass.window_key {
        Some((2, "window"))
    } else if frame.context.as_raw() != world.context() {
        Some((3, "context"))
    } else if frame.chain.as_raw() != world.chain() {
        Some((4, "outer-chain"))
    } else {
        let [width, height, samples] = d3d11::framebuffer_extent(&frame.texture);
        (width != world.pass.camera.output_width
            || height != world.pass.camera.output_height
            || samples != 1)
            .then_some((5, "output-extent"))
    };
    if let Some((stage, name)) = refused {
        hudless_refusal(world, stage, name, Some(&frame));
        return;
    }
    let mut last = world.pass;
    screen_facts(world, &mut last, false);
    last.role = RSF_GAME_RENDER_FINAL_SCENE;
    last.scene_surface = frame.texture.as_raw();
    last.swapchain = world.chain();
    notify(&last, world.context(), 1);
    notify(&last, world.context(), 0);
    world.final_colour = Some(frame.texture.clone());
    world.pass = last;
    world.pass.role = RSF_GAME_RENDER_WINDOW;
    world.pass.swapchain = world.chain();
    world.pass.scene_surface = null_mut();
    world.window_open = true;
    WINDOWS.fetch_add(1, AcqRel);
    notify(&world.pass, world.context(), 1);
}

// ---------------------------------------------------------------------------------------------
// The detour seams, called by render_hooks.c around each original.

/// The ordinary WorldBegin from the verified MainDraw call on the current, unrendered source,
/// with no FRAME, auxiliary scope, recovery token or resolution ownership left: reset the World on
/// the source and open FRAME then SUBMISSION. True makes the call the ordinary World.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_begin_world(caller: usize, ordinary_world: bool) -> bool {
    let cpu = current_identity();
    with_world(|world| {
        let admitted = live()
            && caller == address(Site::WorldCall)
            && cpu.session == bound().services.session_id
            && cpu.source != 0
            && cpu.submission == 0
            && !ordinary_world
            && !world.frame_open
            && !world.auxiliary_open
            && world.recovery.token == 0
            && {
                resolution::retry_retired(image());
                !resolution_busy()
            }
            && cpu_begin_render();
        if admitted {
            *world = World::new();
            world.cpu = current_identity();
            if !begin_scope(world) {
                world.refused = true;
                diagnostic(c"Fallout4 pre-world FRAME/SUBMISSION owner/view/frame lease refused.");
            }
        }
        admitted
    })
}

/// After the original viewport selector store: on the native targets the owned full-output
/// selector follows the native intent, refusing when it cannot; on the live HUDless selection the
/// `hudless` receipt, then the HUDless seam.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_after_viewport(caller: usize, manager: usize, enabled: bool) {
    let base = image();
    let targets = manager == base + rva(Site::Targets);
    with_world(|world| {
        if targets && !resolution::native_viewport(enabled) {
            world.refused = true;
            diagnostic(
                c"Fallout4 owned full-output selector write failed after native viewport intent; scope retained.",
            );
        }
        if live() && targets && caller == base + rva(Site::HudlessCall) {
            world.report_state(HUDLESS, RSF_OK);
            emit_hudless(world);
        }
    })
}

/// Before the original DR writer: an open FRAME drops its depth snapshot.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_before_dynamic_resolution() {
    with_world(|world| {
        if world.frame_open {
            world.depth_snapshot = NO_SNAPSHOT;
        }
    })
}

/// After the original DR writer. A call on the native targets while resolution ownership is
/// pending refuses the World raster and transfers the native write. The live selected call on an
/// open FRAME and SUBMISSION before inputs, on the current source, whose only screen refusal is the
/// World lease and whose native owner is the leased one, configures native resolution and refuses
/// the raster when that fails with ownership pending or a refused change. Any other call on an
/// open FRAME with pending ownership refuses the raster.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_after_dynamic_resolution(caller: usize, manager: usize) {
    let base = image();
    let targets = manager == base + rva(Site::Targets);
    with_world(|world| {
        let overlapping = targets && resolution_busy();
        if overlapping {
            world.raster_refused = true;
            world.refused = true;
            if !resolution::native_dr() {
                diagnostic(
                    c"Fallout4 native DR ownership transfer refused; pending scope retained.",
                );
            }
        }
        if live()
            && caller == base + rva(Site::DrCall)
            && targets
            && !overlapping
            && world.frame_open
            && world.submission_open
            && !world.inputs
            && world.same_source()
        {
            if screen::ordinary_refusals(&world.cpu) == WORLD_LEASE_INVALID
                && read_owner(base).is_some_and(|owner| leased(world, &owner))
            {
                let services = bound().services;
                let frame = world.frame_pass;
                let context = world.context();
                let configured = resolution::configure(&services, &frame, context, |planned| {
                    reserve(world, planned)
                });
                if configured.is_none() && (resolution_busy() || resolution::refused()) {
                    world.raster_refused = true;
                    world.refused = true;
                }
            }
        } else if world.frame_open && resolution_busy() {
            world.raster_refused = true;
            world.refused = true;
        }
    })
}

/// Before the original prepass, with the detour's nested depth: records the World source and
/// selects the live or recovery-owned outermost call from the verified site on an open FRAME and
/// SUBMISSION before inputs. Every call on an open FRAME, including foreign views, invalidates both
/// snapshots, a nested one interrupts depth and one after inputs refuses the World; the selected
/// call clears the interruption.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_before_prepass(
    caller: usize,
    depth: u32,
    call: &mut PrepassCall,
) {
    with_world(|world| {
        // FRAME admission records the ordinary parent beyond WorldBegin's return.
        let selected = (live() || world.recovery.token != 0)
            && depth == 1
            && caller == address(Site::PrepassCall)
            && world.frame_open
            && world.submission_open
            && !world.inputs;
        if world.frame_open {
            world.depth_snapshot = NO_SNAPSHOT;
            world.raster_snapshot = NO_SNAPSHOT;
            if depth != 1 {
                world.depth_interrupted = true;
            }
            if world.inputs {
                world.refused = true;
            }
        }
        if selected {
            world.depth_interrupted = false;
        }
        *call = PrepassCall {
            identity: world.cpu,
            selected,
        };
    })
}

/// After the original selected prepass, still live or recovery-owned on an open FRAME and
/// SUBMISSION before inputs and not interrupted: the raster capture, then a depth capture adopted
/// only when the recorded source is still the current render source and the World's source;
/// otherwise the World is refused.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_after_prepass(call: &PrepassCall) {
    if !call.selected {
        return;
    }
    with_world(|world| {
        if !(live() || world.recovery.token != 0)
            || !world.frame_open
            || !world.submission_open
            || world.inputs
            || world.depth_interrupted
        {
            return;
        }
        // The raster capture does not establish temporal admission.
        world.raster_snapshot =
            depth_domains::capture_raster(&call.identity, REPORT).unwrap_or(NO_SNAPSHOT);
        let snapshot = depth_domains::capture(&call.identity, REPORT)
            .filter(|_| render_source_is_current(&call.identity) && world.same_source());
        if let Some(snapshot) = snapshot {
            world.depth_snapshot = snapshot;
        } else {
            world.refused = true;
            diagnostic(c"Fallout4 native prepass returned without a valid same-source depth-domain snapshot.");
        }
    })
}

/// The caller is one of the three verified imagespace calls, the World has not entered imagespace,
/// FRAME is open, and the callbacks are live on the current source or a recovery token is owned.
/// True enters imagespace and asks the shell to run the exit after the original.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_enter_imagespace(caller: usize) -> bool {
    let base = image();
    with_world(|world| {
        let selected = !world.in_imagespace
            && world.frame_open
            && ((live() && world.same_source()) || world.recovery.token != 0)
            && IMAGESPACE_CALLS
                .iter()
                .any(|&site| caller == base + rva(site));
        if selected {
            world.in_imagespace = true;
        }
        selected
    })
}

/// After a selected imagespace returns or unwinds: RETIRE recovery, drain auxiliaries, end the
/// full-output scope between its two receipts, refuse while output state remains owned, then leave
/// imagespace.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_leave_imagespace() {
    with_world(|world| {
        if !retire(world) {
            diagnostic(c"Fallout4 reduced recovery remains owned after imagespace; next reduction blocked.");
        }
        if !drain_auxiliaries(world) {
            diagnostic(c"Fallout4 auxiliary retirement remains pending after native imagespace; owner retained.");
        }
        world.report_state(UI_RETURN, RSF_OK);
        let result = if resolution::end_current_output() {
            RSF_OK
        } else {
            world.refused = true;
            RSF_ERROR_NOT_READY
        };
        world.report_state(UI_RESTORED, result);
        if resolution::owns_full_output() {
            world.refused = true;
            diagnostic(c"Fallout4 native output-state cleanup remains owned; subsequent world admission blocked.");
        }
        world.in_imagespace = false;
        world.published = false;
        world.full_output_resolved = false;
    })
}

/// Before the original Effects range: the verified call inside imagespace with its exact range
/// arguments on the native imagespace manager, while a recovery token or auxiliary scope is owned
/// or the callbacks are live on the current source: the pre-SR receipt, then the SR seam. The
/// admitted source travels to the matching return.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_before_effects(
    caller: usize,
    manager: usize,
    first: u32,
    last: u32,
    mode: u32,
    flags: u32,
    admitted: &mut CpuIdentity,
) -> bool {
    let base = image();
    if caller != base + rva(Site::EffectsCall) || (first, last, mode, flags) != (0, 13, 1, 1) {
        return false;
    }
    with_world(|world| {
        if !world.in_imagespace
            || !world.frame_open
            || world.cpu.source == 0
            || read::<usize>(base + rva(Site::ImagespaceManager)).filter(|&native| native != 0)
                != Some(manager)
        {
            return false;
        }
        // An already-owned transaction still needs the Effects cleanup boundary after quiesce or
        // source invalidation.
        if !(world.recovery.token != 0 || world.auxiliary_open || (live() && world.same_source())) {
            return false;
        }
        resolution::report(
            PRE_SR,
            &world.frame_pass,
            world.context(),
            world.published,
            RSF_ERROR_NOT_READY,
            REPORT,
        );
        emit_sr(world);
        *admitted = world.cpu;
        true
    })
}

/// After the admitted Effects range, on the same World source: the Effects receipt, RETIRE an
/// owned recovery token (its VATS original), then drain open auxiliaries.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_after_effects(admitted: &CpuIdentity) {
    with_world(|world| {
        if !world.in_imagespace || !world.frame_open || world.cpu != *admitted {
            return;
        }
        world.report_state(EFFECTS_RETURNED, RSF_OK);
        if world.recovery.token != 0 && !retire(world) {
            diagnostic(c"Fallout4 recovery VATS retirement refused at original Effects return; ownership retained.");
        }
        if world.auxiliary_open && !drain_auxiliaries(world) {
            diagnostic(c"Fallout4 VATS/auxiliary restoration refused at native Effects return; owner retained.");
        }
    })
}

/// The native TAA answer, excluded while the World's imagespace owns resolved full output on the
/// current raster snapshot (or, without one, the current source). The first sighting of each gate
/// combination is reported.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_gate_taa(original: bool) -> bool {
    static RECEIPTS: AtomicU32 = AtomicU32::new(0);
    with_world(|world| {
        let excluded = world.in_imagespace
            && world.full_output_resolved
            && if world.raster_snapshot.raster_valid {
                depth_domains::raster_current(&world.raster_snapshot)
            } else {
                world.same_source()
            };
        let active = original && !excluded;
        let key = u32::from(original)
            | (u32::from(world.in_imagespace) << 1)
            | (u32::from(world.full_output_resolved) << 2)
            | (u32::from(world.published) << 3)
            | (u32::from(excluded) << 4);
        let bit = 1u32 << key;
        if RECEIPTS.fetch_or(bit, Relaxed) & bit == 0 {
            report(&message(format!(
                "Fallout4 TAA gate source={} native={} original={} imagespace={} full_output={} temporal={} owner_current={} native_taa_runs={} recovery={}",
                world.cpu.source,
                world.frame_pass.native_frame,
                u32::from(original),
                u32::from(world.in_imagespace),
                u32::from(world.full_output_resolved),
                u32::from(world.published),
                u32::from(excluded),
                u32::from(active),
                world.recovery.token,
            )));
        }
        active
    })
}

/// Preserves the failed reduced-frame output for diagnostics independently of temporal and FG
/// admission. The Runtime rechecks the exact capture identity.
fn capture_recovery(world: &World) {
    let Bound { services, exports } = bound();
    let (Some(capture), Some(query)) = (exports.capture_texture, exports.capture_requested) else {
        return;
    };
    let mut requested = false;
    if !world.recovery.committed
        || !unsafe { rsf_fallout4_guard_requested(query, &mut requested) }
        || !requested
        || !depth_domains::raster_current(&world.raster_snapshot)
    {
        return;
    }
    let Some(frame) = render_inputs::acquire_framebuffer(REPORT) else {
        return;
    };
    if frame.native_frame != world.frame_pass.native_frame
        || frame.window_key != world.frame_pass.window_key
        || frame.context.as_raw() != world.context()
        || !world
            .chain
            .as_ref()
            .is_some_and(|chain| d3d11::same_object(&frame.chain, chain))
    {
        return;
    }
    let request = rsf_d3d11_host_capture {
        struct_size: size_of::<rsf_d3d11_host_capture>() as u32,
        abi_version: RSF_D3D11_HOST_ABI_VERSION,
        session_id: world.cpu.session,
        native_frame: world.frame_pass.native_frame,
        view_key: world.frame_pass.view_key,
        name: c"recovery_native_output".as_ptr(),
        description: c"Native framebuffer before Present after reduced rendering; no temporal or FG admission implied".as_ptr(),
        texture: frame.texture.as_raw(),
        view: 0,
        scale: 1.0,
    };
    unsafe { rsf_fallout4_guard_capture(capture, services.user, frame.context.as_raw(), &request) };
}

/// Before the original outer Present from the verified Swap call with SUBMISSION open: on the
/// leased native owner and current source, revalidate any leased screen and, unless the source is
/// refused, end SUBMISSION and open the CPU present phase, which `phase` carries to the return.
/// Earlier refusal stays sticky; no additional source is minted for retries.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_before_present(
    caller: usize,
    renderer: usize,
    phase: &mut CpuIdentity,
) -> bool {
    let base = image();
    if caller != base + rva(Site::PresentCall) || renderer != base + rva(Site::PresentRenderer) {
        return false;
    }
    with_world(|world| {
        if !world.submission_open {
            return false;
        }
        let matched = read_owner(base).is_some_and(|current| {
            let window = read::<usize>(base + rva(Site::RendererWindow))
                .filter(|&cell| cell == current.renderer + RENDERER_WINDOW);
            window.and_then(|cell| read::<usize>(cell + 0x18)) == Some(world.chain() as usize)
                && leased(world, &current)
        });
        if matched {
            capture_recovery(world);
        }
        if !matched || !world.same_source() {
            // Missing completion refuses prepare; the old scope drains after Swap.
            world.refused = true;
            diagnostic(c"Fallout4 pre-Present source/owner changed; submission completion withheld from FG.");
            return false;
        }
        if world.inputs {
            let mut pass = world.pass;
            screen_facts(world, &mut pass, false);
            world.pass = pass;
        }
        if world.refused {
            diagnostic(c"Fallout4 pre-Present screen/refusal changed; submission completion withheld from FG.");
            return false;
        }
        close_submission(world);
        *phase = world.cpu;
        let active = begin_present_phase(
            phase,
            world.frame_pass.native_frame,
            world.frame_pass.window_key,
        );
        if !PRESENT_ANNOUNCED.swap(true, Relaxed) {
            report(c"Fallout4 native SUBMISSION end before original outer Present; FRAME/WINDOW remain open.");
        }
        active
    })
}

/// After the original outer Present returns or unwinds: close the CPU present phase it opened.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_after_present(phase: &CpuIdentity) {
    end_present_phase(phase);
}

const _: () = assert!(size_of::<PrepassCall>() == 48 && size_of::<CpuIdentity>() == 40);
