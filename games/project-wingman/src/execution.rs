// SPDX-License-Identifier: MIT
//! Admission and retirement of native temporal graph packets and RHI callbacks.
use crate::camera::{read, read_camera};
use crate::discovery::{self, Site, rva};
use crate::observer::{self, Entry, install_trampoline};
use crate::windows::{
    disable_hook, enable_hook, image_base, rsf_wingman_acquire_texture,
    rsf_wingman_release_texture, rsf_wingman_report_execution, tick_count,
};
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_game_sys::ffi_util::try_box;
use rsf_game_sys::{
    RSF_GAME_RENDER_RESET, RSF_GAME_RENDER_SR, rsf_game_host_services, rsf_game_render_pass,
};
use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::CStr;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard};

// Hooks in `discovery::HOOKS` order: allocator retirement is installed before any hook can
// publish a packet.
const HOOKS: usize = discovery::HOOKS.len();
const CAPACITY: u32 = 256;
// Documented layout constants no discovered code reveals; bind_pass checks the pass and owner
// vtables before either is trusted.
const COMPUTE_OUTPUT_UAV: usize = 0x278;
const UAV_RESOURCE: usize = 0x28;
// Fixed by the execute entry's pattern (discovery.rs).
const PASS_PARAMETERS: usize = 0xf0;
// Fixed by the compute helper's patterns (discovery.rs): the pass at owner + 8 and the graph's
// allocator at graph + 0x10.
const TRACKED_PASS_OFFSET: usize = 8;
const GRAPH_MEM_STACK: usize = 0x10;

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
    sites: [usize; HOOKS],
    installed: [bool; HOOKS],
    // Keyed by the concrete pass inside the graph's tracked allocation.
    pending: HashMap<usize, Packet>,
}

impl State {
    fn enable_hooks(&mut self, enable: unsafe fn(usize) -> bool) -> bool {
        for (site, installed) in self.sites.into_iter().zip(&mut self.installed) {
            if !*installed && !unsafe { enable(site) } {
                return false;
            }
            *installed = true;
        }
        true
    }

    fn disable_hooks(&mut self, disable: unsafe fn(usize) -> bool) -> bool {
        self.pending.clear();
        HAS_PENDING.store(false, Ordering::Release);
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
        sites: [0; HOOKS],
        installed: [false; HOOKS],
        pending: HashMap::new(),
    })
});
static EXECUTING: AtomicU32 = AtomicU32::new(0);
static QUEUED: AtomicU32 = AtomicU32::new(0);
static NEXT_LOG: AtomicU64 = AtomicU64::new(0);
static HAS_PENDING: AtomicBool = AtomicBool::new(false);
static ORIGINAL: [AtomicUsize; HOOKS] = [const { AtomicUsize::new(0) }; HOOKS];

thread_local! {
    static CONSTRUCTION: Cell<Option<Packet>> = const { Cell::new(None) };
    static COMPUTE: Cell<Option<Construction>> = const { Cell::new(None) };
}

fn state() -> MutexGuard<'static, State> {
    lock_or_recover(&STATE)
}

/// Creates the disabled execution hooks at the discovered sites, after the live vtables still
/// name the discovered entries and every entry still matches its pattern.
pub(crate) fn prepare() -> Result<(), &'static CStr> {
    let mut state = state();
    let image = image_base();
    let slot = |vtable, index: usize| unsafe { read::<usize>(image + rva(vtable), index * 8) };
    if slot(Site::TrackedVtable, 0) != Some(image + rva(Site::Destructor))
        || slot(Site::ComputeVtable, 1) != Some(image + rva(Site::Execute))
        || !discovery::verify(image, Site::Destructor)
    {
        return Err(
            c"Native temporal execution refused: the pass vtables changed since discovery.",
        );
    }
    if !discovery::HOOKS
        .iter()
        .all(|&site| discovery::verify(image, site))
    {
        return Err(
            c"Native temporal execution refused: a queued-pass entry changed since discovery.",
        );
    }
    state.sites = discovery::HOOKS.map(|site| image + rva(site));
    let detours = [
        free_chunks as *const (),
        add_taa as *const (),
        add_compute as *const (),
        setup_pass as *const (),
        execute_pass as *const (),
    ];
    let created = state
        .sites
        .into_iter()
        .zip(detours)
        .zip(&ORIGINAL)
        .all(|((site, detour), original)| install_trampoline(site, detour as usize, original));
    created
        .then_some(())
        .ok_or(c"Native temporal execution refused: queued-pass hooks could not be created.")
}

pub(crate) fn start() -> bool {
    let mut state = state();
    if !state.enable_hooks(enable_hook) {
        state.disable_hooks(disable_hook);
        return false;
    }
    true
}

pub(crate) fn stop() -> bool {
    state().disable_hooks(disable_hook)
        && EXECUTING.load(Ordering::Acquire) == 0
        && QUEUED.load(Ordering::Acquire) == 0
}

fn capture(graph: usize, view: usize, parameters: usize, history: usize) -> Option<Packet> {
    if !observer::accepting() {
        return None;
    }
    let input: TaaParameters = unsafe { read(parameters, 0) }?;
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
    let fields = discovery::view();
    pass.history_key = unsafe { read(view, fields.state) }.unwrap_or(0);
    let cut: u8 = unsafe { read(view, fields.camera_cut) }.unwrap_or(0);
    let old_history: usize = unsafe { read(history, 0) }.unwrap_or(0);
    if cut != 0 || old_history == 0 {
        pass.flags = RSF_GAME_RENDER_RESET;
    }
    read_camera(view, fields.jitter, &mut pass);
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
    let uav = unsafe { read(parameters, COMPUTE_OUTPUT_UAV) }?;
    packet.rdg[3] = unsafe { read(uav, UAV_RESOURCE) }?;
    Some(Construction { parameters, packet })
}

fn bind_pass(graph: usize, pass: usize) {
    let Some(candidate) = COMPUTE.get() else {
        return;
    };
    let Some(owner) = pass.checked_sub(TRACKED_PASS_OFFSET) else {
        return;
    };
    let image = image_base();
    if candidate.packet.graph != graph
        || unsafe { read(pass, PASS_PARAMETERS) } != Some(candidate.parameters)
        || unsafe { read(pass, 0) } != Some(image + rva(Site::ComputeVtable))
        || unsafe { read(owner, 0) } != Some(image + rva(Site::TrackedVtable))
    {
        return;
    }
    let mut state = state();
    if observer::accepting()
        && state.pending.len() < CAPACITY as usize
        && state.pending.try_reserve(1).is_ok()
    {
        state.pending.insert(pass, candidate.packet);
        HAS_PENDING.store(true, Ordering::Release);
    }
}

fn discard_graph(pending: &mut HashMap<usize, Packet>, stack: usize) {
    if let Some(graph) = stack.checked_sub(GRAPH_MEM_STACK) {
        pending.retain(|_, packet| packet.graph != graph);
    }
}

fn take_pass(pass: usize) -> Option<Packet> {
    let mut state = state();
    let packet = state.pending.remove(&pass);
    if state.pending.is_empty() {
        HAS_PENDING.store(false, Ordering::Release);
    }
    packet
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

type FreeChunks = unsafe extern "C-unwind" fn(usize, usize);
type AddTaa = unsafe extern "C-unwind" fn(usize, usize, usize, usize, usize, usize) -> usize;
type AddCompute = unsafe extern "C-unwind" fn(usize, usize, u8, usize, usize, usize);
type PassCall = unsafe extern "C-unwind" fn(usize, usize);

// The native originals can unwind through these detours. Keep the counters and nested
// construction scopes under Drop, and contain optional-consumer exceptions in the native leaf.
unsafe extern "C-unwind" fn free_chunks(stack: usize, new_top: usize) {
    let _entry = Entry::new(&EXECUTING);
    // Clear and the inlined builder destructor both reach FreeChunks(nullptr). Only a
    // matching graph allocator's full release can retire a packet. Other stacks and partial
    // releases keep their native behavior, even while several graphs are pending.
    if new_top == 0 && HAS_PENDING.load(Ordering::Acquire) {
        let mut state = state();
        discard_graph(&mut state.pending, stack);
        if state.pending.is_empty() {
            HAS_PENDING.store(false, Ordering::Release);
        }
    }
    let original: FreeChunks = unsafe { std::mem::transmute(ORIGINAL[0].load(Ordering::Acquire)) };
    unsafe { original(stack, new_top) };
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
}

const _: () = assert!(std::mem::offset_of!(Command, next) == 8);

impl Command {
    fn acquire(pass: usize) -> Option<Box<Self>> {
        let packet = take_pass(pass)?;
        let host = observer::active_host()?;
        let mut pass = packet.pass;
        pass.color_input = ptr::null_mut();
        pass.depth = ptr::null_mut();
        pass.motion = ptr::null_mut();
        pass.color_output = ptr::null_mut();
        pass.session_id = host.session_id;
        // A failed diagnostic allocation must leave the native pass playable.
        let mut command = try_box(Self {
            vtable: COMMAND_VTABLE.as_ptr(),
            next: ptr::null_mut(),
            pass,
            host,
            reserved: false,
        })?;
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
            *out = unsafe { rsf_wingman_acquire_texture(rdg) };
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
                unsafe { rsf_wingman_release_texture(texture) };
            }
        }
        if self.reserved {
            QUEUED.fetch_sub(1, Ordering::Release);
        }
    }
}

unsafe extern "C" fn run_command(command: *mut Command, list: usize, _: usize) {
    let command = unsafe { Box::from_raw(command) };
    let log = diagnostic_due(tick_count());
    unsafe { rsf_wingman_report_execution(&command.host, list, &command.pass, log) };
}

unsafe fn append(list: usize, command: *mut Command) -> bool {
    let Some(tail): Option<usize> = (unsafe { read(list, 8) }) else {
        return false;
    };
    let Some(count) = (unsafe { read::<u32>(list, 0x14) }).and_then(|n| n.checked_add(1)) else {
        return false;
    };
    if unsafe { read::<u8>(list, 0x10) } != Some(0) || unsafe { read::<usize>(tail, 0) } != Some(0)
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
        const GRAPH: usize = 0x1000;
        const OTHER_GRAPH: usize = 0x2000;
        const PASS: usize = 0x4000;
        let packet = Packet {
            pass: unsafe { std::mem::zeroed() },
            rdg: [100, 200, 300, 400],
            graph: GRAPH,
        };
        for index in 0..CAPACITY as usize - 1 {
            pending.insert(PASS + index * 0x120, packet);
        }
        pending.insert(
            0x1000,
            Packet {
                graph: OTHER_GRAPH,
                ..packet
            },
        );
        assert_eq!(pending.len(), CAPACITY as usize);
        // Construction finished but none of this graph's passes executed.
        discard_graph(&mut pending, GRAPH + GRAPH_MEM_STACK);
        // A new graph can reuse the address without seeing the old inputs.
        assert!(pending.remove(&PASS).is_none());
        pending.insert(
            PASS,
            Packet {
                graph: 0x3000,
                rdg: [1, 2, 3, 4],
                ..packet
            },
        );
        assert_eq!(pending.remove(&PASS).unwrap().rdg, [1, 2, 3, 4]);
        assert_eq!(pending.len(), 1);
        discard_graph(&mut pending, OTHER_GRAPH + GRAPH_MEM_STACK);
        assert!(pending.is_empty());
    }

    #[test]
    fn partial_enable_can_retry_after_rollback() {
        unsafe fn refuse_one(site: usize) -> bool {
            site != 3
        }
        unsafe fn succeed(_: usize) -> bool {
            true
        }
        let mut state = State {
            sites: [1, 2, 3, 4, 5],
            installed: [false; HOOKS],
            pending: HashMap::new(),
        };
        assert!(!state.enable_hooks(refuse_one));
        assert_eq!(state.installed, [true, true, false, false, false]);
        assert!(state.disable_hooks(succeed));
        assert!(state.enable_hooks(succeed));
        assert_eq!(state.installed, [true; HOOKS]);
    }
}
