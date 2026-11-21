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

use crate::cpu_abi::rsf_fallout4_cpu_identity as CpuIdentity;
use crate::cpu_events::{rsf_fallout4_cpu_begin_render, rsf_fallout4_cpu_render_source_is_current};
use crate::depth_domains::rsf_fallout4_depth_verify;
use crate::render_inputs::rsf_fallout4_inputs_verify;
use crate::resolution::{rsf_fallout4_resolution_busy, rsf_fallout4_resolution_verify};
use crate::screen::rsf_fallout4_verify_screen_queries;
use crate::{Ops, image, read};
use rsf_game_sys::rsf_game_host_services;
use std::ffi::{CStr, c_void};
use std::mem::offset_of;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Threading::GetCurrentProcess;
use windows_sys::core::{s, w};

// Return addresses of the guarded MainDraw and Swap calls in verify.
const ORDINARY_WORLD_RETURN: usize = 0xc3_82a5;
const OUTER_PRESENT_RETURN: usize = 0xc3_35ec;
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
    (0xc335e7, 0x1818550),
    (0x21f9f73, 0x21f38b0),
    (0x21f9f2f, 0x1839ed0),
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
    (0xc382a0, 0x21f0ac0),
    (0x21f0ae6, 0x1839ec0),
    (0x21f0e4c, 0x182d580),
    (0x21fa283, 0x2185420),
    (0x21fa2c5, 0x1839ec0),
    (0xc383c2, 0x21fa200),
    (0xc39d6a, 0x21fa200),
    (0xc39e0e, 0x21fa200),
];

/// `expected` matches the bytes at `address`, read whole. Entry bytes are at most 32 bytes.
fn matches(address: usize, expected: &[u8]) -> bool {
    let mut actual = [0u8; 32];
    let mut copied = 0;
    expected.len() <= actual.len()
        && unsafe {
            ReadProcessMemory(
                GetCurrentProcess(),
                address as *const c_void,
                actual.as_mut_ptr().cast(),
                expected.len(),
                &mut copied,
            )
        } != 0
        && copied == expected.len()
        && actual[..expected.len()] == *expected
}

/// An instruction guard of at most 16 bytes at `rva`.
fn guard(rva: usize, expected: &[u8]) -> bool {
    expected.len() <= 16 && matches(image() + rva, expected)
}

/// The rel32 call at `rva` targets `target`.
fn call(rva: usize, target: usize) -> bool {
    let Some(bytes) = read::<[u8; 5]>(image() + rva) else {
        return false;
    };
    let relative = i32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
    bytes[0] == 0xe8 && (rva as isize + 5 + relative as isize) as usize == target
}

/// Every hook entry, the resolution, screen and depth-domain guards, the TAA slot, then the call
/// sites and Present instructions, in the original order.
pub(crate) fn verify() -> bool {
    let base = image();
    HOOKS
        .iter()
        .all(|hook| matches(base + hook.rva, hook.entry))
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
pub(crate) fn verify_with_inputs() -> bool {
    verify() && rsf_fallout4_inputs_verify()
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

type CreateHook = unsafe extern "C" fn(u32, *mut c_void) -> bool;
type QueueHook = unsafe extern "C" fn(*mut c_void) -> bool;
type Query = unsafe extern "C" fn() -> bool;
type BindExports = unsafe extern "C" fn(&RenderExports);
type BindHost = unsafe extern "C" fn(&rsf_game_host_services);

/// NativeRenderHookLeaves.
#[repr(C)]
struct NativeLeaves {
    create_hook: Option<CreateHook>,
    remove_hook: Option<CreateHook>,
    queue_enable_hook: Option<QueueHook>,
    queue_disable_hook: Option<QueueHook>,
    apply_queued: Option<Query>,
    scopes_busy: Option<Query>,
    bind_exports: Option<BindExports>,
    bind_host: Option<BindHost>,
}

const _: () = {
    assert!(size_of::<RenderExports>() == 5 * size_of::<usize>());
    assert!(size_of::<NativeLeaves>() == 8 * size_of::<usize>());
};

/// The complete leaf table.
struct Leaves {
    create_hook: CreateHook,
    remove_hook: CreateHook,
    queue_enable_hook: QueueHook,
    queue_disable_hook: QueueHook,
    apply_queued: Query,
    scopes_busy: Query,
    bind_exports: BindExports,
    bind_host: BindHost,
}

static LEAVES: OnceLock<Leaves> = OnceLock::new();

fn leaves() -> Option<&'static Leaves> {
    LEAVES.get()
}

/// Binds the render hook leaves for the process.
///
/// # Safety
/// A nonnull `table` is a NativeRenderHookLeaves table whose leaves live for the process.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_render_publish(table: *const NativeLeaves) -> bool {
    let Some(table) = (unsafe { table.as_ref() }) else {
        return false;
    };
    let complete = || {
        Some(Leaves {
            create_hook: table.create_hook?,
            remove_hook: table.remove_hook?,
            queue_enable_hook: table.queue_enable_hook?,
            queue_disable_hook: table.queue_disable_hook?,
            apply_queued: table.apply_queued?,
            scopes_busy: table.scopes_busy?,
            bind_exports: table.bind_exports?,
            bind_host: table.bind_host?,
        })
    };
    match complete() {
        Some(leaves) => {
            let _ = LEAVES.get_or_init(|| leaves);
            true
        }
        None => false,
    }
}

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
    leaves().is_some_and(|leaves| unsafe { (leaves.scopes_busy)() })
        || recovery::owned()
        || rsf_fallout4_resolution_busy()
}

/// Checks the guards and the synchronous services, negotiates the Runtime exports and creates
/// every missing detour disabled. A creation failure removes the hooks this call created, in
/// reverse order; a removal MinHook refuses keeps its record and trampoline.
pub(crate) fn prepare(ops: &Ops, services: &rsf_game_host_services) -> bool {
    let Some(leaves) = leaves() else {
        return false;
    };
    if live()
        || busy()
        || services.process_upscale.is_none()
        || services.render_pass.is_none()
        || !verify()
        || !rsf_fallout4_inputs_verify()
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
    unsafe { (leaves.bind_exports)(&exports) };
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
        if unsafe { (leaves.create_hook)(index as u32, target(hook)) } {
            created[index] = true;
            added[count] = index;
            count += 1;
            continue;
        }
        while count != 0 {
            count -= 1;
            let index = added[count];
            if unsafe { (leaves.remove_hook)(index as u32, target(&HOOKS[index])) } {
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
    unsafe { (leaves.bind_host)(services) };
    true
}

/// Rechecks the guards, queues every created detour, sets live and applies the queue. A missing
/// record or a refused queue or apply clears live and queues every created detour disabled.
pub(crate) fn start(ops: &Ops) -> bool {
    let Some(leaves) = leaves() else {
        return false;
    };
    if busy() || !verify() {
        return false;
    }
    let created = created();
    let mut queued = true;
    for (index, hook) in HOOKS.iter().enumerate() {
        if !created[index] || !unsafe { (leaves.queue_enable_hook)(target(hook)) } {
            queued = false;
        }
    }
    if queued {
        LIVE.store(true, Ordering::Release);
    }
    if !queued || !unsafe { (leaves.apply_queued)() } {
        LIVE.store(false, Ordering::Release);
        for (index, hook) in HOOKS.iter().enumerate() {
            if created[index] {
                let _ = unsafe { (leaves.queue_disable_hook)(target(hook)) };
            }
        }
        let _ = unsafe { (leaves.apply_queued)() };
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

type ReadPresentOwner = unsafe extern "C" fn(&mut ScopeKey, &mut usize) -> bool;
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
        && !rsf_fallout4_resolution_busy()
        && rsf_fallout4_cpu_begin_render()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_render_match_present(
    caller: usize,
    renderer: usize,
    submission_open: bool,
    leased: &ScopeKey,
    read_owner: ReadPresentOwner,
) -> PresentMatch {
    let base = image();
    if caller != base + OUTER_PRESENT_RETURN
        || renderer != base + PRESENT_RENDERER
        || !submission_open
    {
        return PresentMatch::Unrelated;
    }
    let mut current = ScopeKey::default();
    let mut outer_chain = 0;
    if unsafe { read_owner(&mut current, &mut outer_chain) }
        && outer_chain == leased.chain
        && current == *leased
    {
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
    if unsafe { revalidate_screen() } {
        PresentAdmission::Refused
    } else {
        PresentAdmission::Complete
    }
}
