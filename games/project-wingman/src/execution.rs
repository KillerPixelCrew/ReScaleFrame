// SPDX-License-Identifier: MIT
//! Admission and retirement of native temporal graph packets and RHI callbacks.
use crate::camera::{read, read_camera, read_jitter};
use crate::observer::{self, CAMERA_CUT, Entry, HISTORY, Platform, install_trampoline};
use rsf_game_sys::{
    RSF_GAME_RENDER_RESET, RSF_GAME_RENDER_SR, rsf_game_host_services, rsf_game_render_pass,
};
use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};

// Install the tracked-allocation destructor before any hook can publish a packet.
const RVAS: [usize; 5] = [0x591_d000, 0x5c4_4910, 0x5c2_d4a0, 0x656_9660, 0x5c4_c9c0];
const EXPECTED: [[u8; 16]; 5] = [
    [
        0x40, 0x53, 0x48, 0x83, 0xec, 0x20, 0x48, 0x8b, 0xd9, 0xf6, 0xc2, 0x01, 0x74, 0x0a, 0xba,
        0x20,
    ],
    [
        0x48, 0x8b, 0xc4, 0x48, 0x89, 0x58, 0x08, 0x55, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41,
        0x56,
    ],
    [
        0x48, 0x89, 0x5c, 0x24, 0x08, 0x48, 0x89, 0x6c, 0x24, 0x10, 0x48, 0x89, 0x74, 0x24, 0x18,
        0x48,
    ],
    [
        0x48, 0x89, 0x54, 0x24, 0x10, 0x55, 0x53, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56,
        0x41,
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
const TRACKED_PASS_OFFSET: usize = 8;
const TRACKED_VTABLE: usize = 0x839_0f78;
const COMPUTE_VTABLE: usize = 0x83f_6838;

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

#[derive(Clone, Copy)]
struct Packet {
    pass: rsf_game_render_pass,
    rdg: [usize; 4],
    graph: usize,
}

#[derive(Clone, Copy)]
struct Construction {
    parameters: usize,
    packet: Packet,
}

struct State {
    sites: [usize; RVAS.len()],
    installed: [bool; RVAS.len()],
    // Keyed by the concrete pass inside the graph's tracked allocation.
    pending: HashMap<usize, Packet>,
}

impl State {
    fn enable_hooks(&mut self, enable: unsafe extern "C" fn(usize) -> bool) -> bool {
        for (site, installed) in self.sites.into_iter().zip(&mut self.installed) {
            if !*installed && !unsafe { enable(site) } {
                return false;
            }
            *installed = true;
        }
        true
    }

    fn disable_hooks(&mut self, disable: unsafe extern "C" fn(usize) -> bool) -> bool {
        self.pending.clear();
        for (site, installed) in self.sites.into_iter().zip(&mut self.installed).rev() {
            if *installed && !unsafe { disable(site) } {
                return false;
            }
            *installed = false;
        }
        true
    }
}

// Pending packets have no acquired resources. Their opaque fields are inspected only by the
// matching pass's invocation, before its graph releases the tracked allocation.
unsafe impl Send for State {}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    Mutex::new(State {
        sites: [0; RVAS.len()],
        installed: [false; RVAS.len()],
        pending: HashMap::new(),
    })
});
static EXECUTING: AtomicU32 = AtomicU32::new(0);
static QUEUED: AtomicU32 = AtomicU32::new(0);
static NEXT_LOG: AtomicU64 = AtomicU64::new(0);
static ORIGINAL: [AtomicUsize; RVAS.len()] = [const { AtomicUsize::new(0) }; RVAS.len()];

thread_local! {
    static CONSTRUCTION: Cell<Option<Packet>> = const { Cell::new(None) };
    static COMPUTE: Cell<Option<Construction>> = const { Cell::new(None) };
}

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn prepare(platform: &Platform) -> bool {
    let mut state = state();
    let image = unsafe { (platform.image_base)() };
    if unsafe { read(platform.copy, image, TRACKED_VTABLE) } != Some(image + RVAS[0])
        || unsafe { read(platform.copy, image, COMPUTE_VTABLE + size_of::<usize>()) }
            != Some(image + RVAS[4])
    {
        return false;
    }
    for (&rva, expected) in RVAS.iter().zip(&EXPECTED) {
        let bytes: Option<[u8; 16]> = unsafe { read(platform.copy, image, rva) };
        if bytes.as_ref() != Some(expected) {
            return false;
        }
    }
    state.sites = RVAS.map(|rva| image + rva);
    let detours = [
        delete_tracked as *const (),
        add_taa as *const (),
        add_compute as *const (),
        setup_pass as *const (),
        execute_pass as *const (),
    ];
    state
        .sites
        .into_iter()
        .zip(detours)
        .zip(&ORIGINAL)
        .all(|((site, detour), original)| {
            install_trampoline(platform, site, detour as usize, original)
        })
}

pub(crate) fn start(platform: &Platform) -> bool {
    let mut state = state();
    if !state.enable_hooks(platform.enable_hook) {
        state.disable_hooks(platform.disable_hook);
        return false;
    }
    true
}

pub(crate) fn stop(platform: &Platform) -> bool {
    state().disable_hooks(platform.disable_hook)
        && EXECUTING.load(Ordering::Acquire) == 0
        && QUEUED.load(Ordering::Acquire) == 0
}

fn capture(graph: usize, view: usize, parameters: usize, history: usize) -> Option<Packet> {
    if !observer::accepting() {
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
    let jitter = read_jitter(view, platform.copy);
    let cut: u8 = unsafe { read(platform.copy, view, CAMERA_CUT) }.unwrap_or(0);
    let old_history: usize = unsafe { read(platform.copy, history, 0) }.unwrap_or(0);
    if cut != 0 || old_history == 0 {
        pass.flags = RSF_GAME_RENDER_RESET;
    }
    read_camera(view, &mut pass, platform.copy, jitter);
    Some(Packet {
        pass,
        rdg: [input.colour, input.depth, input.motion, 0],
        graph,
    })
}

fn compute(graph: usize, parameters: usize) -> Option<Construction> {
    if !observer::accepting() {
        return None;
    }
    let mut packet = CONSTRUCTION.get()?;
    if packet.graph != graph {
        return None;
    }
    let platform = observer::platform()?;
    let uav = unsafe { read(platform.copy, parameters, COMPUTE_OUTPUT_UAV) }?;
    packet.rdg[3] = unsafe { read(platform.copy, uav, UAV_RESOURCE) }?;
    Some(Construction { parameters, packet })
}

fn bind_pass(graph: usize, pass: usize) {
    let Some(candidate) = COMPUTE.get() else {
        return;
    };
    let Some(platform) = observer::platform() else {
        return;
    };
    let Some(owner) = pass.checked_sub(TRACKED_PASS_OFFSET) else {
        return;
    };
    let image = unsafe { (platform.image_base)() };
    if candidate.packet.graph != graph
        || unsafe { read(platform.copy, pass, PASS_PARAMETERS) } != Some(candidate.parameters)
        || unsafe { read(platform.copy, pass, 0) } != Some(image + COMPUTE_VTABLE)
        || unsafe { read(platform.copy, owner, 0) } != Some(image + TRACKED_VTABLE)
    {
        return;
    }
    let mut state = state();
    if observer::accepting()
        && state.pending.len() < CAPACITY as usize
        && state.pending.try_reserve(1).is_ok()
    {
        state.pending.insert(pass, candidate.packet);
    }
}

fn discard(pending: &mut HashMap<usize, Packet>, owner: usize) {
    if let Some(pass) = owner.checked_add(TRACKED_PASS_OFFSET) {
        pending.remove(&pass);
    }
}

fn reserve() -> bool {
    observer::accepting()
        && QUEUED
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |queued| {
                (queued < CAPACITY).then(|| queued + 1)
            })
            .is_ok()
}

fn diagnostic_due(now: u64) -> bool {
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

type DeleteTracked = unsafe extern "C-unwind" fn(usize, u32) -> usize;
type AddTaa = unsafe extern "C-unwind" fn(usize, usize, usize, usize, usize, usize) -> usize;
type AddCompute = unsafe extern "C-unwind" fn(usize, usize, u8, usize, usize, usize);
type PassCall = unsafe extern "C-unwind" fn(usize, usize);

// The native originals can unwind through these detours. Keep the counters and nested
// construction scopes under Drop, and contain optional-consumer exceptions in the native leaf.
unsafe extern "C-unwind" fn delete_tracked(owner: usize, flags: u32) -> usize {
    let _entry = Entry::new(&EXECUTING);
    discard(&mut state().pending, owner);
    let original: DeleteTracked =
        unsafe { std::mem::transmute(ORIGINAL[0].load(Ordering::Acquire)) };
    unsafe { original(owner, flags) }
}

struct Restore<T: Copy + 'static>(&'static std::thread::LocalKey<Cell<Option<T>>>, Option<T>);

impl<T: Copy + 'static> Drop for Restore<T> {
    fn drop(&mut self) {
        self.0.set(self.1);
    }
}

unsafe extern "C-unwind" fn add_taa(
    result: usize,
    graph: usize,
    view: usize,
    parameters: usize,
    previous_history: usize,
    output_history: usize,
) -> usize {
    let _entry = Entry::new(&EXECUTING);
    let previous = CONSTRUCTION.replace(capture(graph, view, parameters, previous_history));
    let _restore = Restore(&CONSTRUCTION, previous);
    let original: AddTaa = unsafe { std::mem::transmute(ORIGINAL[1].load(Ordering::Acquire)) };
    unsafe {
        original(
            result,
            graph,
            view,
            parameters,
            previous_history,
            output_history,
        )
    }
}

unsafe extern "C-unwind" fn add_compute(
    graph: usize,
    name: usize,
    flags: u8,
    shader: usize,
    parameters: usize,
    groups: usize,
) {
    let _entry = Entry::new(&EXECUTING);
    let previous = COMPUTE.replace(compute(graph, parameters));
    let _restore = Restore(&COMPUTE, previous);
    let original: AddCompute = unsafe { std::mem::transmute(ORIGINAL[2].load(Ordering::Acquire)) };
    unsafe { original(graph, name, flags, shader, parameters, groups) };
}

unsafe extern "C-unwind" fn setup_pass(graph: usize, pass: usize) {
    let _entry = Entry::new(&EXECUTING);
    let original: PassCall = unsafe { std::mem::transmute(ORIGINAL[3].load(Ordering::Acquire)) };
    unsafe { original(graph, pass) };
    bind_pass(graph, pass);
}

unsafe extern "C-unwind" fn execute_pass(pass: usize, list: usize) {
    let _entry = Entry::new(&EXECUTING);
    let command = Command::acquire(pass);
    let original: PassCall = unsafe { std::mem::transmute(ORIGINAL[4].load(Ordering::Acquire)) };
    unsafe { original(pass, list) };
    if let Some(mut command) = command
        && reserve()
    {
        command.reserved = true;
        let command = Box::into_raw(command);
        if !unsafe { append(list, command) } {
            drop(unsafe { Box::from_raw(command) });
        }
    }
}

type CommandFn = unsafe extern "C" fn(*mut Command, usize, usize);
static COMMAND_VTABLE: [CommandFn; 1] = [run_command];

#[repr(C)]
struct Command {
    // Verified UE4.27 FRHICommandBase prefix.
    vtable: *const CommandFn,
    next: *mut Command,
    pass: rsf_game_render_pass,
    host: rsf_game_host_services,
    reserved: bool,
    platform: &'static Platform,
}

const _: () = assert!(std::mem::offset_of!(Command, next) == 8);

impl Command {
    fn acquire(pass: usize) -> Option<Box<Self>> {
        let packet = state().pending.remove(&pass)?;
        let host = observer::active_host()?;
        let platform = observer::platform()?;
        // A failed diagnostic allocation must leave the native pass playable.
        let raw = unsafe { std::alloc::alloc(std::alloc::Layout::new::<Self>()) }.cast::<Self>();
        if raw.is_null() {
            return None;
        }
        let mut pass = packet.pass;
        pass.color_input = ptr::null_mut();
        pass.depth = ptr::null_mut();
        pass.motion = ptr::null_mut();
        pass.color_output = ptr::null_mut();
        pass.session_id = host.session_id;
        unsafe {
            raw.write(Self {
                vtable: COMMAND_VTABLE.as_ptr(),
                next: ptr::null_mut(),
                pass,
                host,
                reserved: false,
                platform,
            });
        }
        let mut command = unsafe { Box::from_raw(raw) };
        let pass = &mut command.pass;
        for (out, rdg) in [
            &mut pass.color_input,
            &mut pass.depth,
            &mut pass.motion,
            &mut pass.color_output,
        ]
        .into_iter()
        .zip(packet.rdg)
        {
            *out = unsafe { (platform.acquire_texture)(rdg) };
            if out.is_null() {
                return None;
            }
        }
        Some(command)
    }
}

impl Drop for Command {
    fn drop(&mut self) {
        for texture in [
            self.pass.color_input,
            self.pass.depth,
            self.pass.motion,
            self.pass.color_output,
        ] {
            if !texture.is_null() {
                unsafe { (self.platform.release_texture)(texture) };
            }
        }
        if self.reserved {
            QUEUED.fetch_sub(1, Ordering::Release);
        }
    }
}

unsafe extern "C" fn run_command(command: *mut Command, list: usize, _: usize) {
    let command = unsafe { Box::from_raw(command) };
    let platform = command.platform;
    let log = diagnostic_due(unsafe { (platform.tick_count)() });
    unsafe {
        (platform.report_execution)(&command.host, list, &command.pass, log);
    }
}

unsafe fn append(list: usize, command: *mut Command) -> bool {
    let copy = unsafe { (*command).platform.copy };
    let Some(tail): Option<usize> = (unsafe { read(copy, list, 8) }) else {
        return false;
    };
    let Some(count) = (unsafe { read::<u32>(copy, list, 0x14) }).and_then(|n| n.checked_add(1))
    else {
        return false;
    };
    if unsafe { read::<u8>(copy, list, 0x10) } != Some(0)
        || unsafe { read::<usize>(copy, tail, 0) } != Some(0)
    {
        return false;
    }
    unsafe {
        (tail as *mut *mut Command).write_unaligned(command);
        ((list + 8) as *mut *mut *mut Command).write_unaligned(&raw mut (*command).next);
        ((list + 0x14) as *mut u32).write_unaligned(count);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skipped_passes_release_capacity_and_cannot_survive_address_reuse() {
        let mut pending = HashMap::new();
        for index in 0..CAPACITY as usize {
            let pass = 0x1000 + index * 0x120;
            let packet = Packet {
                pass: unsafe { std::mem::zeroed() },
                rdg: [100, 200, 300, 400],
                graph: 1,
            };
            pending.insert(pass, packet);
        }
        assert_eq!(pending.len(), CAPACITY as usize);
        for index in 0..CAPACITY as usize {
            let pass = 0x1000 + index * 0x120;
            // No execute callback occurred. The allocator still destroys every tracked pass.
            discard(&mut pending, pass - TRACKED_PASS_OFFSET);
            assert!(pending.remove(&pass).is_none());
        }
        assert!(pending.is_empty());
    }

    #[test]
    fn partial_enable_can_retry_after_rollback() {
        unsafe extern "C" fn refuse_one(site: usize) -> bool {
            site != RVAS[2]
        }
        unsafe extern "C" fn succeed(_: usize) -> bool {
            true
        }
        let mut state = State {
            sites: RVAS,
            installed: [false; RVAS.len()],
            pending: HashMap::new(),
        };
        assert!(!state.enable_hooks(refuse_one));
        assert_eq!(state.installed, [true, true, false, false, false]);
        assert!(state.disable_hooks(succeed));
        assert!(state.enable_hooks(succeed));
        assert_eq!(state.installed, [true; RVAS.len()]);
    }
}
