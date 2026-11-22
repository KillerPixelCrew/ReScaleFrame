// SPDX-License-Identifier: MIT
//! The render hook lifecycle and ordinary World and outer Present admission for Fallout 4 AE
//! 1.11.240.
//!
//! Rust owns the eight hook records (RVA, expected entry bytes and created state), the call-site
//! and instruction guards, Runtime export negotiation, the live flag and the verify, prepare,
//! start, quiesce, stop and busy decisions, the reduced-render recovery transaction
//! (recovery.rs), SR and HUDless publication (publication.rs), the FRAME, SUBMISSION and WINDOW
//! scope (scope.rs), the imagespace, Effects and TAA seams (imagespace.rs) and the viewport, DR
//! and prepass seams (producer.rs). render_hooks.cpp keeps each typed detour and its typed
//! MinHook trampoline, which it calls on its own C++ frame, the callback scope counters, the
//! nested prepass counter and the recovery, publication, scope, imagespace and producer leaves
//! (render_hooks_platform.h). Every value belongs to the exact recognized build; see
//! docs/research/rust-fallout4-render-hooks.md.

mod imagespace;
mod producer;
mod publication;
mod recovery;
mod scope;

use crate::build_layout::{
    RENDERER_CHAIN, RENDERER_CONTEXT, RENDERER_DEVICE, RENDERER_OBJECT, RENDERER_POINTER,
    RENDERER_WINDOW,
};
use crate::cpu_abi::rsf_fallout4_cpu_identity as CpuIdentity;
use crate::cpu_events::{rsf_fallout4_cpu_begin_render, rsf_fallout4_cpu_render_source_is_current};
use crate::creation::{ExactBuild, accept_window};
use crate::depth_domains::rsf_fallout4_depth_verify;
use crate::render_inputs::verify_for_build;
use crate::resolution::{is_busy as resolution_busy, rsf_fallout4_resolution_verify};
use crate::screen::rsf_fallout4_verify_screen_queries;
use crate::{MH_OK, Ops, image, read};
use rsf_game_sys::{rsf_game_host_services, rsf_game_render_pass};
use std::ffi::{CStr, c_void};
use std::mem::offset_of;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::core::{s, w};

// A fresh snapshot of the calling thread's World. Pass pointers name that World's fields.
#[repr(C)]
struct World {
    cpu: CpuIdentity,
    pass: *mut rsf_game_render_pass,
    frame_pass: *mut rsf_game_render_pass,
    submission_pass: *mut rsf_game_render_pass,
    screen_basis: *mut rsf_game_render_pass,
    context: *mut c_void,
    chain: *mut c_void,
    upscale: bool,
    recovery_exported: bool,
    in_imagespace: bool,
    inputs: bool,
    published: bool,
    window_open: bool,
    frame_open: bool,
    submission_open: bool,
    refused: bool,
    raster_refused: bool,
    auxiliary_open: bool,
    depth_valid: bool,
    raster_valid: bool,
    depth_interrupted: bool,
    full_output_resolved: bool,
}

const _: () = {
    assert!(size_of::<World>() == 104);
    assert!(offset_of!(World, pass) == 40);
    assert!(offset_of!(World, context) == 72);
    assert!(offset_of!(World, upscale) == 88);
    assert!(offset_of!(World, full_output_resolved) == 102);
};

impl World {
    fn read(snapshot: unsafe extern "C" fn(&mut Self)) -> Self {
        // SAFETY: the record contains integers, raw pointers and bools, all valid at zero.
        let mut world = unsafe { std::mem::zeroed() };
        unsafe { snapshot(&mut world) };
        world
    }

    fn state(&self) -> *const rsf_game_render_pass {
        if self.inputs {
            self.pass
        } else {
            self.frame_pass
        }
    }
}

fn read_owner(base: usize) -> Option<scope::Owner> {
    let renderer =
        read::<usize>(base + RENDERER_POINTER).filter(|&value| value == base + RENDERER_OBJECT)?;
    let device = read::<usize>(renderer + RENDERER_DEVICE).filter(|&value| value != 0)?;
    let context = read::<usize>(renderer + RENDERER_CONTEXT).filter(|&value| value != 0)?;
    let window = read::<usize>(renderer + RENDERER_WINDOW).filter(|&value| value != 0)?;
    let chain = read::<usize>(renderer + RENDERER_CHAIN).filter(|&value| value != 0)?;
    let graph = read::<usize>(base + 0x32dd338).filter(|&value| value != 0)?;
    let view = read::<usize>(graph + 0x140).filter(|&value| value != 0)?;
    let frame = read::<u32>(base + 0x3d871a0 + 0x98)?;
    accept_window(window as *mut c_void).then_some(scope::Owner {
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

/// The supplied output points to the native caller's owner record.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_render_read_owner(output: &mut scope::Owner) -> bool {
    let Some(owner) = read_owner(image()) else {
        return false;
    };
    *output = owner;
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_read_manager(output: &mut usize) -> bool {
    let Some(manager) = read(image() + crate::resolution::MANAGER) else {
        return false;
    };
    *output = manager;
    true
}

fn read_outer_chain(base: usize, renderer: usize) -> Option<usize> {
    let window_cell =
        read::<usize>(base + 0x38e13b0).filter(|&value| value == renderer + RENDERER_WINDOW)?;
    read(window_cell + 0x18)
}

// Return addresses of the guarded MainDraw and Swap calls in verify.
const REL32_CALL_SIZE: usize = 5;
type CallSite = (usize, usize);
const fn return_rva(site: CallSite) -> usize {
    site.0 + REL32_CALL_SIZE
}

const OUTER_PRESENT_CALL: CallSite = (0xc335e7, 0x1818550);
const PREPASS_CALL: CallSite = (0x21f9f73, 0x21f38b0);
const DR_CALL: CallSite = (0x21f9f2f, 0x1839ed0);
const ORDINARY_WORLD_CALL: CallSite = (0xc382a0, 0x21f0ac0);
const EFFECTS_CALL: CallSite = (0x21fa283, 0x2185420);
const HUDLESS_CALL: CallSite = (0x21fa2c5, 0x1839ec0);
const IMAGESPACE_CALLS: [CallSite; 3] = [
    (0xc383c2, 0x21fa200),
    (0xc39d6a, 0x21fa200),
    (0xc39e0e, 0x21fa200),
];
const ORDINARY_WORLD_RETURN: usize = return_rva(ORDINARY_WORLD_CALL);
const OUTER_PRESENT_RETURN: usize = return_rva(OUTER_PRESENT_CALL);
const PRESENT_RENDERER: usize = 0x3a2_5d00;

// ---------------------------------------------------------------------------------------------
// Hook records and native guards.

struct Hook {
    rva: usize,
    // Expected entry bytes, checked before creation and before every start.
    entry: &'static [u8],
}

const HOOK_COUNT: usize = 8;

// NativeRenderHook order, which is also the creation and enable order.
const HOOKS: [Hook; HOOK_COUNT] = [
    // Dynamic resolution writer.
    Hook {
        rva: 0x1839ed0,
        entry: b"\x48\x83\xec\x18\x8b\x05\x5e\x48\x71\x01",
    },
    // Depth prepass.
    Hook {
        rva: 0x21f38b0,
        entry: b"\x48\x89\x5c\x24\x08\x48\x89\x6c\x24\x10",
    },
    // Native outer Present.
    Hook {
        rva: 0x1818550,
        entry: b"\x40\x53\x48\x83\xec\x20",
    },
    // WorldBegin.
    Hook {
        rva: 0x21f0ac0,
        entry: b"\x48\x8b\xc4\x53\x48\x81\xec\xb0\x00\x00\x00",
    },
    // Viewport selector store.
    Hook {
        rva: 0x1839ec0,
        entry: b"\x88\x91\xe4\x0f\x00\x00\xc3",
    },
    // Imagespace.
    Hook {
        rva: 0x21fa200,
        entry: b"\x48\x89\x5c\x24\x10\x56\x48\x83\xec\x40",
    },
    // Imagespace Effects range.
    Hook {
        rva: 0x2185420,
        entry: b"\x48\x89\x5c\x24\x18\x55\x56\x57\x41\x55\x41\x56\x48\x83\xec\x30",
    },
    // Native TAA active query.
    Hook {
        rva: 0x2214fc0,
        entry: b"\x83\x3d\x85\x22\xb7\x01\x01\x75\x12\x80\x79\x08\x00\x74\x0c\x83\x3d\x7a\x22\xb7\x01\x00\x75\x03\xb0\x01\xc3",
    },
];

// The TAA effect vtable whose slot 8 must name the hooked TAA active query.
const TAA_VTABLE: usize = 0x2915eb0;
const TAA_SLOT: usize = 8;
const TAA_HOOK: usize = 7;

// Exact rel32 calls (call site, callee), checked in this order after the TAA slot: the outer
// Swap's native Present (return 0xc335ec), the selected prepass (0x21f9f78) and DR (0x21f9f34)
// calls, the camera update 0x21f30b0, its three 0x182bf20 camera reads, the 0x182d750 builder
// call and the native camera-setter caller (fallout4-depth-domains and
// fallout4-reduced-consumers notes).
const PRODUCER_CALLS: [(usize, usize); 9] = [
    OUTER_PRESENT_CALL,
    PREPASS_CALL,
    DR_CALL,
    (0x21f9f69, 0x21f30b0),
    (0x21f32a5, 0x182bf20),
    (0x21f32bb, 0x182bf20),
    (0x21f32d1, 0x182bf20),
    (0x182c02d, 0x182d750),
    (0x182c597, 0x1817ef0),
];

// Outer Present instructions the owner reader depends on: the renderer argument (0x3a25d00),
// both loads of the renderer-window cell (0x38e13b0), the chain at +0x18 and the
// IDXGISwapChain::Present slot (+0x40) calls.
const PRESENT_GUARDS: [(usize, &[u8]); 7] = [
    (0xc335e0, b"\x48\x8d\x0d\x19\x27\xdf\x02"),
    (0x1818572, b"\x48\x8b\x05\x37\x8e\x0c\x02"),
    (0x181858a, b"\x48\x8b\x48\x18"),
    (0x181859b, b"\x41\xff\x51\x40"),
    (0x18185b6, b"\x48\x8b\x05\xf3\x8d\x0c\x02"),
    (0x18185c4, b"\x48\x8b\x48\x18"),
    (0x18185d5, b"\x41\xff\x51\x40"),
];

// The ordinary MainDraw WorldBegin call (return 0xc382a5), WorldBegin's viewport selection and
// temporal update calls, the Effects (return 0x21fa288) and HUDless viewport (0x21fa2ca) calls
// inside imagespace, and the three selected imagespace calls (0xc383c7, 0xc39d6f, 0xc39e13).
const WORLD_CALLS: [(usize, usize); 8] = [
    ORDINARY_WORLD_CALL,
    (0x21f0ae6, 0x1839ec0),
    (0x21f0e4c, 0x182d580),
    EFFECTS_CALL,
    HUDLESS_CALL,
    IMAGESPACE_CALLS[0],
    IMAGESPACE_CALLS[1],
    IMAGESPACE_CALLS[2],
];

/// An instruction guard of at most 16 bytes at `rva`.
fn guard(rva: usize, expected: &[u8]) -> bool {
    crate::matches_bytes::<16>(image() + rva, expected)
}

/// The rel32 call at `rva` targets `target`.
fn call(rva: usize, target: usize) -> bool {
    let Some(bytes) = read::<[u8; REL32_CALL_SIZE]>(image() + rva) else {
        return false;
    };
    let relative = i32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
    bytes[0] == 0xe8
        && (rva as isize + REL32_CALL_SIZE as isize + relative as isize) as usize == target
}

/// Every hook entry, the resolution, screen and depth-domain guards, the TAA slot, then the call
/// sites and Present instructions, in the original order.
pub(crate) fn verify() -> bool {
    let base = image();
    HOOKS
        .iter()
        .all(|hook| crate::matches_bytes::<32>(base + hook.rva, hook.entry))
        && rsf_fallout4_resolution_verify()
        && rsf_fallout4_verify_screen_queries()
        && rsf_fallout4_depth_verify()
        && read::<usize>(base + TAA_VTABLE + TAA_SLOT * size_of::<usize>())
            == Some(base + HOOKS[TAA_HOOK].rva)
        && PRODUCER_CALLS
            .iter()
            .all(|&(rva, target)| call(rva, target))
        && PRESENT_GUARDS
            .iter()
            .all(|&(rva, expected)| guard(rva, expected))
        && WORLD_CALLS.iter().all(|&(rva, target)| call(rva, target))
}

/// The render hook guards, then the render-input reader guards.
pub(crate) fn verify_with_inputs(build: &ExactBuild) -> bool {
    verify() && verify_for_build(build)
}

// ---------------------------------------------------------------------------------------------
// Native leaves and lifecycle state.

#[repr(C)]
struct RenderExports {
    upscale_auxiliaries: *mut c_void,
    restore_auxiliaries: *mut c_void,
    reduced_frame: *mut c_void,
    capture_texture: *mut c_void,
    capture_requested: *mut c_void,
}

unsafe extern "system" {
    fn MH_QueueEnableHook(target: *mut c_void) -> i32;
    fn MH_QueueDisableHook(target: *mut c_void) -> i32;
    fn MH_ApplyQueued() -> i32;
}

unsafe extern "C" {
    fn rsf_fallout4_render_create_hook(hook: u32, target: *mut c_void) -> bool;
    fn rsf_fallout4_render_remove_hook(hook: u32, target: *mut c_void) -> bool;
    fn rsf_fallout4_render_scopes_busy() -> bool;
    fn rsf_fallout4_render_bind_exports(exports: &RenderExports);
    fn rsf_fallout4_render_bind_host(services: &rsf_game_host_services);
}

const _: () = assert!(size_of::<RenderExports>() == 5 * size_of::<usize>());

#[derive(Clone, Copy)]
struct Host(rsf_game_host_services);
// SAFETY: process-lived Runtime services, written by prepare before any hook is enabled.
unsafe impl Send for Host {}

// The services last bound to the callbacks; the lifecycle receipts use the same binding.
static HOST: Mutex<Host> = Mutex::new(Host(unsafe { std::mem::zeroed() }));
// Each record's created state: set when MinHook created it, cleared only by a successful removal.
static CREATED: Mutex<[bool; HOOK_COUNT]> = Mutex::new([false; HOOK_COUNT]);
static LIVE: AtomicBool = AtomicBool::new(false);

fn created() -> MutexGuard<'static, [bool; HOOK_COUNT]> {
    CREATED.lock().unwrap_or_else(PoisonError::into_inner)
}

fn target(hook: &Hook) -> *mut c_void {
    (image() + hook.rva) as *mut c_void
}

fn report(ops: &Ops, message: &CStr) {
    let host = HOST.lock().unwrap_or_else(PoisonError::into_inner).0;
    let _ = ops.log_host(&host, message);
}

fn live() -> bool {
    LIVE.load(Ordering::Acquire)
}

/// The adapter's render callbacks are live.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_live() -> bool {
    live()
}

// ---------------------------------------------------------------------------------------------
// Lifecycle, called under the session lock.

/// A detour in flight, an open WINDOW, FRAME or auxiliary scope, an owned recovery token, or
/// pending resolution ownership.
pub(crate) fn busy() -> bool {
    (unsafe { rsf_fallout4_render_scopes_busy() }) || recovery::owned() || resolution_busy()
}

/// Checks the guards and the synchronous services, negotiates the Runtime exports and creates
/// every missing detour disabled. A creation failure removes the hooks this call created, in
/// reverse order; a removal MinHook refuses keeps its record and trampoline.
pub(crate) fn prepare(ops: &Ops, services: &rsf_game_host_services, build: &ExactBuild) -> bool {
    if live()
        || busy()
        || services.process_upscale.is_none()
        || services.render_pass.is_none()
        || !verify_with_inputs(build)
    {
        return false;
    }
    let runtime = unsafe { GetModuleHandleW(w!("ReScaleFrame.Runtime.dll")) };
    let export = |name| {
        unsafe { GetProcAddress(runtime, name) }
            .map_or(null_mut(), |address| address as *mut c_void)
    };
    let exports = RenderExports {
        upscale_auxiliaries: export(s!("rsf_d3d11_host_upscale_auxiliaries")),
        restore_auxiliaries: export(s!("rsf_d3d11_host_restore_auxiliaries")),
        reduced_frame: export(s!("rsf_d3d11_host_reduced_frame")),
        capture_texture: export(s!("rsf_d3d11_host_capture_texture")),
        capture_requested: export(s!("rsf_input_dump_requested")),
    };
    unsafe { rsf_fallout4_render_bind_exports(&exports) };
    // Independent export negotiation. An older runtime retains native raster.
    if exports.upscale_auxiliaries.is_null()
        || exports.restore_auxiliaries.is_null()
        || services.render_config.is_none()
    {
        return false;
    }
    let mut created = created();
    let mut added = [0usize; HOOK_COUNT];
    let mut count = 0;
    for (index, hook) in HOOKS.iter().enumerate() {
        if created[index] {
            continue;
        }
        if unsafe { rsf_fallout4_render_create_hook(index as u32, target(hook)) } {
            created[index] = true;
            added[count] = index;
            count += 1;
            continue;
        }
        while count != 0 {
            count -= 1;
            let index = added[count];
            if unsafe { rsf_fallout4_render_remove_hook(index as u32, target(&HOOKS[index])) } {
                created[index] = false;
            } else {
                report(
                    ops,
                    c"Fallout4 cold render-hook rollback refused; retained hook ownership, startup remains refused.",
                );
            }
        }
        return false;
    }
    HOST.lock().unwrap_or_else(PoisonError::into_inner).0 = *services;
    unsafe { rsf_fallout4_render_bind_host(services) };
    true
}

/// Rechecks the guards, queues every created detour, sets live and applies the queue. A missing
/// record or a refused queue or apply clears live and queues every created detour disabled.
pub(crate) fn start(ops: &Ops) -> bool {
    if busy() || !verify() {
        return false;
    }
    let created = created();
    let mut queued = true;
    for (index, hook) in HOOKS.iter().enumerate() {
        if !created[index] || unsafe { MH_QueueEnableHook(target(hook)) } != MH_OK {
            queued = false;
        }
    }
    if queued {
        LIVE.store(true, Ordering::Release);
    }
    if !queued || unsafe { MH_ApplyQueued() } != MH_OK {
        LIVE.store(false, Ordering::Release);
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
        ops,
        c"Fallout4 guarded native DR/SR/auxiliary output callbacks enabled; actual producer and screen admission required, FG/input capabilities remain gated.",
    );
    true
}

pub(crate) fn quiesce() {
    LIVE.store(false, Ordering::Release);
}

/// Clears live and disables every created detour once no scope is open. Trampolines and records
/// stay for a later start.
pub(crate) fn stop(ops: &Ops) -> bool {
    quiesce();
    if busy() {
        return false;
    }
    let created = created();
    let mut disabled = true;
    for (index, hook) in HOOKS.iter().enumerate() {
        if created[index] {
            let result = ops.disable_hook(target(hook) as usize);
            disabled = disabled && result;
        }
    }
    drop(created);
    disabled && !busy()
}

// ---------------------------------------------------------------------------------------------
// World and Present admission.

#[repr(C)]
struct WorldAdmission {
    caller: usize,
    session: u64,
    live: bool,
    ordinary_world: bool,
    frame_open: bool,
    auxiliary_open: bool,
    recovery_owned: bool,
}

#[repr(C)]
#[derive(Default, PartialEq, Eq)]
struct ScopeKey {
    frame: u64,
    view: u64,
    window: u64,
    context: usize,
    chain: usize,
}

#[repr(u32)]
#[derive(PartialEq, Eq)]
enum PresentMatch {
    Unrelated = 0,
    OwnerChanged = 1,
    Matched = 2,
}

#[repr(u32)]
enum PresentAdmission {
    SourceChanged = 0,
    Refused = 1,
    Complete = 2,
}

type RevalidatePresentScreen = unsafe extern "C" fn() -> bool;

const _: () = {
    assert!(size_of::<WorldAdmission>() == 24);
    assert!(offset_of!(WorldAdmission, live) == 16);
    assert!(offset_of!(WorldAdmission, recovery_owned) == 20);
    assert!(size_of::<ScopeKey>() == 40);
    assert!(offset_of!(ScopeKey, context) == 24);
    assert!(offset_of!(ScopeKey, chain) == 32);
};

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_admit_world(
    candidate: &WorldAdmission,
    cpu: &CpuIdentity,
) -> bool {
    candidate.live
        && candidate.caller == image() + ORDINARY_WORLD_RETURN
        && cpu.session == candidate.session
        && cpu.source != 0
        && cpu.submission == 0
        && !candidate.ordinary_world
        && !candidate.frame_open
        && !candidate.auxiliary_open
        && !candidate.recovery_owned
        && {
            crate::resolution::retry_retired(image());
            !resolution_busy()
        }
        && rsf_fallout4_cpu_begin_render()
}

// Capture is diagnostic and independent of temporal and FG admission. The native
// leaves contain exceptions from the two Runtime exports.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_capture_recovery(
    snapshot: *const crate::depth_domains::Snapshot,
    facts: unsafe extern "C" fn(&mut World),
    report: crate::render_inputs::Report,
    requested: unsafe extern "C" fn() -> bool,
    capture: unsafe extern "C" fn(
        *mut c_void,
        &rsf_game_sys::d3d11_host_abi::rsf_d3d11_host_capture,
    ),
) {
    use rsf_game_sys::d3d11_chain_abi::RSF_D3D11_HOST_ABI_VERSION;
    use rsf_game_sys::d3d11_host_abi::rsf_d3d11_host_capture;
    use windows::core::Interface;

    if !recovery::committed()
        || !unsafe { requested() }
        || !unsafe { snapshot.as_ref() }.is_some_and(crate::depth_domains::raster_current)
    {
        return;
    }
    let Some(frame) = crate::render_inputs::acquire_framebuffer(report) else {
        return;
    };
    let world = World::read(facts);
    // SAFETY: the callback reports the current thread's FRAME record.
    let basis = unsafe { *world.frame_pass };
    if frame.native_frame != basis.native_frame
        || frame.window_key != basis.window_key
        || frame.context.as_raw() != world.context
        || !unsafe {
            crate::d3d11::rsf_fallout4_render_same_object(frame.chain.as_raw(), world.chain)
        }
    {
        return;
    }
    // The identity comparison can call native COM. Read the current receipt facts
    // at the same point as the former native capture before invoking the export.
    let world = World::read(facts);
    let basis = unsafe { *world.frame_pass };
    let request = rsf_d3d11_host_capture {
        struct_size: size_of::<rsf_d3d11_host_capture>() as u32,
        abi_version: RSF_D3D11_HOST_ABI_VERSION,
        session_id: world.cpu.session,
        native_frame: basis.native_frame,
        view_key: basis.view_key,
        name: c"recovery_native_output".as_ptr(),
        description: c"Native framebuffer before Present after reduced rendering; no temporal or FG admission implied".as_ptr(),
        texture: frame.texture.as_raw(),
        view: 0,
        scale: 1.0,
    };
    unsafe { capture(frame.context.as_raw(), &request) };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_match_present(
    caller: usize,
    renderer: usize,
    submission_open: bool,
    leased: &ScopeKey,
) -> PresentMatch {
    let base = image();
    if caller != base + OUTER_PRESENT_RETURN
        || renderer != base + PRESENT_RENDERER
        || !submission_open
    {
        return PresentMatch::Unrelated;
    }
    let matches = read_owner(base).is_some_and(|current| {
        read_outer_chain(base, current.renderer) == Some(leased.chain)
            && ScopeKey {
                frame: u64::from(current.frame),
                view: current.view as u64,
                window: current.window as u64,
                context: current.context,
                chain: current.chain,
            } == *leased
    });
    if matches {
        PresentMatch::Matched
    } else {
        PresentMatch::OwnerChanged
    }
}

// Called after the optional recovery capture, which only requires a matched native owner.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_admit_present(
    matched: PresentMatch,
    session: u64,
    cpu: &CpuIdentity,
    revalidate_screen: RevalidatePresentScreen,
) -> PresentAdmission {
    if matched != PresentMatch::Matched
        || cpu.session != session
        || !unsafe { rsf_fallout4_cpu_render_source_is_current(cpu) }
    {
        return PresentAdmission::SourceChanged;
    }
    // Revalidate any leased screen and keep earlier refusals sticky for the source.
    let refused = unsafe { revalidate_screen() };
    if !unsafe { rsf_fallout4_cpu_render_source_is_current(cpu) } {
        PresentAdmission::SourceChanged
    } else if refused {
        PresentAdmission::Refused
    } else {
        PresentAdmission::Complete
    }
}
