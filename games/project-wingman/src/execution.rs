// SPDX-License-Identifier: MIT
//! Admission and retirement of native temporal graph packets and RHI callbacks.
use crate::camera::{JITTER, read, read_camera};
use crate::observer::{self, CAMERA_CUT, HISTORY, Platform};
use rsf_game_sys::{
    RSF_GAME_RENDER_RESET, RSF_GAME_RENDER_SR, rsf_game_host_services, rsf_game_render_pass,
};
use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};

const RVAS: [usize; 3] = [0x5c4_4910, 0x5c2_d4a0, 0x5c4_c9c0];
const EXPECTED: [[u8; 16]; 3] = [
    [
        0x48, 0x8b, 0xc4, 0x48, 0x89, 0x58, 0x08, 0x55, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41,
        0x56,
    ],
    [
        0x48, 0x89, 0x5c, 0x24, 0x08, 0x48, 0x89, 0x6c, 0x24, 0x10, 0x48, 0x89, 0x74, 0x24, 0x18,
        0x48,
    ],
    [
        0x48, 0x83, 0xec, 0x38, 0xf2, 0x0f, 0x10, 0x81, 0x08, 0x01, 0x00, 0x00, 0x4c, 0x8d, 0x4c,
        0x24,
    ],
];
const CAPACITY: u32 = 256;
const COMPUTE_OUTPUT_UAV: usize = 0x278;
const UAV_RESOURCE: usize = 0x28;
const PASS_PARAMETERS: usize = 0xf0;

#[repr(C)]
#[derive(Clone, Copy)]
struct TaaParameters {
    kind: u32,
    flags: [u8; 4],
    downsample_format: u32,
    input: [i32; 4],
    output: [i32; 4],
    divisor: i32,
    depth: usize,
    motion: usize,
    colour: usize,
    metadata: usize,
}

const _: () = {
    assert!(std::mem::offset_of!(TaaParameters, input) == 0xc);
    assert!(std::mem::offset_of!(TaaParameters, output) == 0x1c);
    assert!(std::mem::offset_of!(TaaParameters, colour) == 0x40);
    assert!(size_of::<TaaParameters>() == 0x50);
};

#[repr(C)]
#[derive(Clone, Copy)]
struct Packet {
    pass: rsf_game_render_pass,
    rdg: [usize; 4],
}

const _: () =
    assert!(size_of::<Packet>() == size_of::<rsf_game_render_pass>() + 4 * size_of::<usize>());

struct State {
    sites: [usize; 3],
    created: [bool; 3],
    installed: [bool; 3],
    host: Option<rsf_game_host_services>,
    pending: HashMap<usize, Packet>,
}

// The host is borrowed until the hooks, active calls and queued commands are retired. Pending
// packets contain opaque graph identities. The native RHI command acquires the COM leases.
unsafe impl Send for State {}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    Mutex::new(State {
        sites: [0; 3],
        created: [false; 3],
        installed: [false; 3],
        host: None,
        pending: HashMap::new(),
    })
});
static ACCEPTING: AtomicBool = AtomicBool::new(false);
static EXECUTING: AtomicU32 = AtomicU32::new(0);
static QUEUED: AtomicU32 = AtomicU32::new(0);
static NEXT_LOG: AtomicU64 = AtomicU64::new(0);

thread_local! {
    // The native TAA detour lends its stack packet during the original call, then restores the
    // previous pointer with a scope guard even if a nested call or native exception intervenes.
    static CONSTRUCTION: Cell<*const Packet> = const { Cell::new(ptr::null()) };
}

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn prepare(host: &rsf_game_host_services, platform: &Platform) -> bool {
    let mut state = state();
    let image = unsafe { (platform.image_base)() };
    for (&rva, expected) in RVAS.iter().zip(&EXPECTED) {
        let bytes: Option<[u8; 16]> = unsafe { read(platform.copy, image, rva) };
        if bytes.as_ref() != Some(expected) {
            return false;
        }
    }
    state.sites = RVAS.map(|rva| image + rva);
    for index in 0..RVAS.len() {
        if !state.created[index] {
            if !unsafe { (platform.create_execution_hook)(index as u32, state.sites[index]) } {
                return false;
            }
            state.created[index] = true;
        }
    }
    state.host = Some(*host);
    true
}

pub(crate) fn start(platform: &Platform) -> bool {
    let mut state = state();
    if state.host.is_none() {
        return false;
    }
    for index in 0..RVAS.len() {
        if !state.installed[index] && !unsafe { (platform.enable_hook)(state.sites[index]) } {
            drop(state);
            stop(platform);
            return false;
        }
        state.installed[index] = true;
    }
    ACCEPTING.store(true, Ordering::Release);
    true
}

pub(crate) fn quiesce() {
    ACCEPTING.store(false, Ordering::Release);
}

pub(crate) fn stop(platform: &Platform) -> bool {
    quiesce();
    let mut state = state();
    for index in 0..RVAS.len() {
        if state.installed[index] {
            if !unsafe { (platform.disable_hook)(state.sites[index]) } {
                return false;
            }
            state.installed[index] = false;
        }
    }
    if EXECUTING.load(Ordering::Acquire) != 0 || QUEUED.load(Ordering::Acquire) != 0 {
        return false;
    }
    state.pending.clear();
    state.host = None;
    true
}

fn capture(graph: usize, view: usize, parameters: usize, history: usize) -> Option<Packet> {
    if !ACCEPTING.load(Ordering::Acquire) {
        return None;
    }
    let platform = observer::platform()?;
    let input: TaaParameters = unsafe { read(platform.copy, parameters, 0) }?;
    if input.kind >= 3 || input.divisor != 1 {
        return None;
    }
    let mut pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
    pass.struct_size = size_of::<rsf_game_render_pass>() as u32;
    pass.role = RSF_GAME_RENDER_SR;
    pass.family_key = graph as u64;
    pass.view_key = view as u64;
    pass.render_rect = input.input;
    pass.output_rect = input.output;
    pass.history_key = unsafe { read(platform.copy, view, HISTORY) }.unwrap_or(0);
    pass.camera.jitter_pixels = unsafe { read(platform.copy, view, JITTER) }.unwrap_or_default();
    let cut: u8 = unsafe { read(platform.copy, view, CAMERA_CUT) }.unwrap_or(0);
    let old_history: usize = unsafe { read(platform.copy, history, 0) }.unwrap_or(0);
    if cut != 0 || old_history == 0 {
        pass.flags = RSF_GAME_RENDER_RESET;
    }
    read_camera(view, &mut pass, platform.copy);
    Some(Packet {
        pass,
        rdg: [input.colour, input.depth, input.motion, 0],
    })
}

#[unsafe(no_mangle)]
extern "C" fn rsf_wingman_exec_enter() {
    EXECUTING.fetch_add(1, Ordering::AcqRel);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_wingman_exec_leave() {
    EXECUTING.fetch_sub(1, Ordering::Release);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_wingman_exec_taa_begin(
    graph: usize,
    view: usize,
    parameters: usize,
    history: usize,
    storage: *mut Packet,
) -> *const Packet {
    let current = if let Some(packet) = capture(graph, view, parameters, history) {
        unsafe { storage.write(packet) };
        storage.cast_const()
    } else {
        ptr::null()
    };
    CONSTRUCTION.replace(current)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_wingman_exec_taa_end(parent: *const Packet) {
    CONSTRUCTION.set(parent);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_wingman_exec_record_compute(parameters: usize) {
    if !ACCEPTING.load(Ordering::Acquire) {
        return;
    }
    let Some(mut packet) = (unsafe { CONSTRUCTION.get().as_ref().copied() }) else {
        return;
    };
    let Some(platform) = observer::platform() else {
        return;
    };
    let Some(uav) = (unsafe { read(platform.copy, parameters, COMPUTE_OUTPUT_UAV) }) else {
        return;
    };
    let Some(output) = (unsafe { read(platform.copy, uav, UAV_RESOURCE) }) else {
        return;
    };
    packet.rdg[3] = output;
    let mut state = state();
    if !ACCEPTING.load(Ordering::Acquire) {
        return;
    }
    if let Some(existing) = state.pending.get_mut(&parameters) {
        *existing = packet;
    } else if state.pending.len() < CAPACITY as usize && state.pending.try_reserve(1).is_ok() {
        state.pending.insert(parameters, packet);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_wingman_exec_take(
    pass: usize,
    packet: *mut Packet,
    host: *mut rsf_game_host_services,
) -> bool {
    let Some(platform) = observer::platform() else {
        return false;
    };
    let Some(parameters) = (unsafe { read::<usize>(platform.copy, pass, PASS_PARAMETERS) }) else {
        return false;
    };
    let mut state = state();
    let Some(mut selected) = state.pending.remove(&parameters) else {
        return false;
    };
    let Some(services) = state.host.filter(|_| ACCEPTING.load(Ordering::Acquire)) else {
        return false;
    };
    selected.pass.session_id = services.session_id;
    unsafe {
        packet.write(selected);
        host.write(services);
    }
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_wingman_exec_reserve() -> bool {
    ACCEPTING.load(Ordering::Acquire)
        && QUEUED
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |queued| {
                (queued < CAPACITY).then(|| queued + 1)
            })
            .is_ok()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_wingman_exec_retire() {
    QUEUED.fetch_sub(1, Ordering::Release);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_wingman_exec_diagnostic_due(now: u64) -> bool {
    let previous = NEXT_LOG.load(Ordering::Relaxed);
    now >= previous
        && NEXT_LOG
            .compare_exchange(
                previous,
                now.saturating_add(1000),
                Ordering::AcqRel,
                Ordering::Relaxed,
            )
            .is_ok()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_wingman_exec_report(
    host: *const rsf_game_host_services,
    context: *mut c_void,
    pass: *const rsf_game_render_pass,
) {
    if ACCEPTING.load(Ordering::Acquire)
        && let Some(platform) = observer::platform()
    {
        unsafe { (platform.report_execution)(host, context, pass) };
    }
}
