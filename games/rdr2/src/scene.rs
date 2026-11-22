// SPDX-License-Identifier: MIT
//! Completed-scene producer and native frame events. The postfx output scope opened by the three
//! measured callers marks the blit that writes the completed scene into the swapchain buffer; the
//! native marker function reports frame events. This module reports facts and leases resources; it
//! never chooses latency or frame-generation behaviour.
use crate::host::{Host, HostCell};
use crate::layout::{
    BLIT, BLIT_RETURN, FRAME_COUNTER, FRAME_COUNTER_POINTER, MARKER, MODE, NATIVE_TEXTURE,
    NATIVE_TEXTURE_RVA, OUTPUT, OUTPUT_CALLS, OUTPUT_RETURNS, SLEEP, TLS_INDEX, block, call_target,
    draw,
};
use crate::platform::{
    self, ResourceFacts, Retained, Scope, lock, read_bytes, read_pointer, read_value, trampoline,
};
use rsf_game_sys::{
    RSF_GAME_CPU_EVENT_VERSION, RSF_GAME_CPU_INPUT_EVENT, RSF_GAME_CPU_INPUT_KEYED,
    RSF_GAME_CPU_INPUT_UNFRAMED, RSF_GAME_CPU_WINDOW_MESSAGE, RSF_GAME_INPUT_CONTROLLER,
    RSF_GAME_LATENCY_MODE, RSF_GAME_LATENCY_SLEEP, RSF_GAME_RENDER_FINAL_SCENE,
    RSF_GAME_RENDER_WINDOW, rsf_game_cpu_event, rsf_game_host_services, rsf_game_render_pass,
};
use std::cell::Cell;
use std::ffi::c_void;
use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};

type BlitFn = unsafe extern "C" fn(
    *mut c_void,
    u32,
    u32,
    *const c_void,
    *const c_void,
    *const c_void,
) -> usize;
type MarkerFn = unsafe extern "C" fn(u32, u64) -> bool;
type OutputFn = unsafe extern "C" fn(*mut c_void, *mut c_void, u32) -> usize;
type SleepFn = unsafe extern "C" fn() -> bool;
type ModeFn = unsafe extern "C" fn(*mut c_void, *const c_void) -> i32;

/// Native marker kinds 0..=6 share the SDK latency marker numbering.
const LAST_MARKER: u32 = 6;
const INPUT_SAMPLE: u32 = 1;
const PRESENT_END: u32 = 4;
/// Simulation start/end, input sample and render submit start must all be observed for a frame
/// to carry a source identity.
const REQUIRED_MARKERS: u32 = (1 << 0) | (1 << 1) | (1 << 2) | (1 << 6);
/// Each primary scene may be submitted to at most this many swapchain blits.
const MAX_SCENE_BLITS: u32 = 16;
const MAX_BUFFERS: u32 = 8;

static PREPARED: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static READERS: AtomicU32 = AtomicU32::new(0);
static IMAGE: AtomicUsize = AtomicUsize::new(0);
static HOST: HostCell = HostCell::new();
static INPUT_SOURCE: AtomicU64 = AtomicU64::new(0);
static ORIGINAL_BLIT: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_MARKER: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_OUTPUT: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_SLEEP: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL_MODE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

thread_local! {
    static OUTPUT_SCOPE: Cell<bool> = const { Cell::new(false) };
    static PRESENT_RECEIPT: Cell<Option<PresentReceipt>> = const { Cell::new(None) };
}

#[derive(Clone, Copy)]
struct Markers {
    frame: u64,
    mask: u32,
}

#[derive(Clone, Copy)]
struct Scene {
    pass: rsf_game_render_pass,
    commands: usize,
    surface: usize,
    ordinal: u32,
}

impl Scene {
    const EMPTY: Self = Self {
        // Null resources and zero identity.
        pass: unsafe { std::mem::zeroed() },
        commands: 0,
        surface: 0,
        ordinal: 0,
    };
}

struct State {
    swapchain: usize,
    generation: u32,
    markers: [Markers; 128],
    scenes: [Scene; 64],
}

// Scene passes keep identities only; their resource pointers are cleared before storage.
unsafe impl Send for State {}

static STATE: Mutex<State> = Mutex::new(State {
    swapchain: 0,
    generation: 0,
    markers: [Markers { frame: 0, mask: 0 }; 128],
    scenes: [Scene::EMPTY; 64],
});

fn slot<T>(items: &mut [T], frame: u64) -> &mut T {
    let index = (frame % items.len() as u64) as usize;
    &mut items[index]
}

/// The per-thread render block's `field` and the native frame counter it belongs to.
pub(crate) fn render_block(image: usize, field: usize) -> Option<(usize, u64)> {
    let index = read_value::<u32>(image.wrapping_add(TLS_INDEX))?;
    if index >= block::TLS_LIMIT {
        return None;
    }
    let slots = unsafe { platform::worker_tls_slots() };
    let block = read_pointer(slots.wrapping_add(index as usize * size_of::<usize>()))?;
    let value = read_pointer(block.wrapping_add(field))?;
    let flags = read_value::<u32>(block.wrapping_add(block::FLAGS))?;
    let counter = if flags & block::INDIRECT_COUNTER != 0 {
        let pointer = read_pointer(image.wrapping_add(FRAME_COUNTER_POINTER))?;
        read_value::<u32>(pointer)?
    } else {
        read_value::<u32>(image.wrapping_add(FRAME_COUNTER))?
    };
    (value != 0 && counter != 0).then_some((value, u64::from(counter)))
}

fn latency(kind: u32, frame: u64) -> bool {
    if !ACTIVE.load(Ordering::SeqCst) {
        return false;
    }
    let Some(host) = HOST.get() else { return false };
    if host.0.process_latency.is_none() {
        return false;
    }
    host.latency(kind, frame, platform::qpc()).unwrap_or(false)
}

fn received_input(kind: u32, message: u32) {
    if !ACTIVE.load(Ordering::SeqCst) {
        return;
    }
    let Some(host) = HOST.get() else { return };
    if host.0.cpu_event.is_none() {
        return;
    }
    let source = INPUT_SOURCE.load(Ordering::SeqCst);
    let (now, frequency) = platform::qpc();
    let mut flags = RSF_GAME_CPU_INPUT_KEYED;
    if message != 0 {
        flags |= RSF_GAME_CPU_WINDOW_MESSAGE;
    }
    if source == 0 && kind != 0 {
        flags |= RSF_GAME_CPU_INPUT_UNFRAMED;
    }
    host.cpu_event(&rsf_game_cpu_event {
        struct_size: size_of::<rsf_game_cpu_event>() as u32,
        stage: RSF_GAME_CPU_INPUT_EVENT,
        session_id: host.session(),
        source_frame_id: source,
        timestamp_qpc: now,
        qpc_frequency: frequency,
        render_expected: 0,
        input_kind: kind,
        message_id: message,
        event_version: RSF_GAME_CPU_EVENT_VERSION,
        flags,
    });
}

/// A window message removed by the game's input thread, classified by the native leaf.
pub(crate) fn received_message(kind: u32, message: u32) {
    let _scope = Scope::enter(&READERS);
    received_input(kind, message);
}

/// A successful public XInput read.
pub(crate) fn controller_received() {
    let _scope = Scope::enter(&READERS);
    if INPUT_SOURCE.load(Ordering::SeqCst) != 0 {
        received_input(RSF_GAME_INPUT_CONTROLLER, 0);
    }
}

fn input_thread(frame: u64) {
    INPUT_SOURCE.store(frame, Ordering::SeqCst);
    unsafe { platform::input_install() };
}

unsafe extern "C" fn sleep() -> bool {
    let _scope = Scope::enter(&READERS);
    let image = IMAGE.load(Ordering::SeqCst);
    // Same producer as the game's INPUT_SAMPLE caller at abcfea: direct counter + 1.
    if let Some(current) = read_value::<u32>(image.wrapping_add(FRAME_COUNTER)) {
        let frame = u64::from(current.wrapping_add(1));
        if ACTIVE.load(Ordering::SeqCst) {
            input_thread(frame);
        }
        if latency(RSF_GAME_LATENCY_SLEEP, frame) {
            return true;
        }
    }
    match unsafe { trampoline::<SleepFn>(&ORIGINAL_SLEEP) } {
        Some(native) => unsafe { native() },
        None => false,
    }
}

unsafe extern "C" fn mode(device: *mut c_void, parameters: *const c_void) -> i32 {
    let _scope = Scope::enter(&READERS);
    if latency(RSF_GAME_LATENCY_MODE, 0) {
        return 0;
    }
    match unsafe { trampoline::<ModeFn>(&ORIGINAL_MODE) } {
        Some(native) => unsafe { native(device, parameters) },
        None => 0,
    }
}

/// Entered from the native leaf with the call's return address.
pub(crate) unsafe extern "C" fn rsf_rdr2_output_detour(
    depth: *mut c_void,
    color: *mut c_void,
    technique: u32,
    caller: usize,
) -> usize {
    let _scope = Scope::enter(&READERS);
    let image = IMAGE.load(Ordering::SeqCst);
    let selected = OUTPUT_RETURNS
        .iter()
        .any(|&rva| caller == image.wrapping_add(rva));
    let scoped = ACTIVE.load(Ordering::SeqCst) && selected;
    let previous = OUTPUT_SCOPE.with(|scope| scope.replace(scoped));
    let result = match unsafe { trampoline::<OutputFn>(&ORIGINAL_OUTPUT) } {
        Some(native) => unsafe { native(depth, color, technique) },
        None => 0,
    };
    OUTPUT_SCOPE.with(|scope| scope.set(previous));
    result
}

/// The completed-scene colour target recorded by a selected blit.
struct BlitTarget {
    commands: usize,
    resource: usize,
    frame: u64,
}

fn blit_target(image: usize) -> Option<BlitTarget> {
    let (draw_context, frame) = render_block(image, block::DRAW)?;
    let commands = read_pointer(draw_context.wrapping_add(draw::COMMANDS))?;
    let count = read_value::<u32>(draw_context.wrapping_add(draw::COLOR_COUNT))?;
    if count != 1 {
        return None;
    }
    let view = read_pointer(draw_context.wrapping_add(draw::COLOR_VIEWS))?;
    let slice = read_value::<u32>(view.wrapping_add(draw::VIEW_SLICE))?;
    if (slice >> 4) & 15 != 0 {
        return None;
    }
    let texture = read_pointer(view.wrapping_add(draw::VIEW_TEXTURE))?;
    let resource = read_pointer(texture.wrapping_add(draw::TEXTURE_RESOURCE))?;
    Some(BlitTarget {
        commands,
        resource,
        frame,
    })
}

/// Entered from the native leaf with the call's return address.
pub(crate) unsafe extern "C" fn rsf_rdr2_blit_detour(
    shader: *mut c_void,
    technique: u32,
    pass_index: u32,
    source_rect: *const c_void,
    destination_rect: *const c_void,
    clip_rect: *const c_void,
    caller: usize,
) -> usize {
    let _scope = Scope::enter(&READERS);
    let image = IMAGE.load(Ordering::SeqCst);
    let selected = OUTPUT_SCOPE.with(Cell::get) && caller == image.wrapping_add(BLIT_RETURN);
    let result = match unsafe { trampoline::<BlitFn>(&ORIGINAL_BLIT) } {
        Some(native) => unsafe {
            native(
                shader,
                technique,
                pass_index,
                source_rect,
                destination_rect,
                clip_rect,
            )
        },
        None => 0,
    };
    if ACTIVE.load(Ordering::SeqCst) && selected {
        completed_scene(image);
    }
    result
}

fn completed_scene(image: usize) {
    let Some(host) = HOST.get() else {
        return;
    };
    if host.0.render_pass.is_none() {
        return;
    }
    let Some(target) = blit_target(image) else {
        return;
    };
    let (chain, mut pass) = {
        let state = lock(&STATE);
        let saved = &state.scenes[(target.frame % state.scenes.len() as u64) as usize];
        if state.swapchain == 0
            || saved.pass.native_frame != target.frame
            || saved.commands != target.commands
        {
            return;
        }
        (state.swapchain as *mut c_void, saved.pass)
    };
    // Match the actual colour resource rather than the swapchain's current CPU index on a recording
    // worker. References are local to this callback and cannot block a later resize.
    let (mut buffers, mut window) = (0u32, 0u64);
    if !unsafe { platform::swapchain_facts(chain, &mut buffers, &mut window) }
        || buffers > MAX_BUFFERS
    {
        return;
    }
    let mut surface = None;
    for index in 0..buffers {
        let candidate = Retained(unsafe { platform::swapchain_buffer(chain, index) });
        if !candidate.0.is_null() && candidate.0 as usize == target.resource {
            surface = Some(candidate);
            break;
        }
    }
    let Some(surface) = surface else { return };
    let mut facts = ResourceFacts::default();
    if !unsafe { platform::resource_facts(surface.0, &mut facts) }
        || facts.width != u64::from(pass.camera.output_width)
        || facts.height != pass.camera.output_height
        || facts.samples != 1
    {
        return;
    }
    {
        let mut state = lock(&STATE);
        if state.swapchain != chain as usize || state.generation != pass.resource_generation {
            return;
        }
        let saved = slot(&mut state.scenes, target.frame);
        if saved.pass.native_frame != target.frame
            || saved.commands != target.commands
            || saved.ordinal >= MAX_SCENE_BLITS
        {
            return;
        }
        saved.ordinal += 1;
        pass.pass_key = u64::from(saved.ordinal);
        saved.surface = target.resource;
        pass.source_frame_id = saved.pass.source_frame_id;
    }
    pass.role = RSF_GAME_RENDER_FINAL_SCENE;
    pass.color_input = surface.0;
    pass.scene_surface = surface.0;
    pass.swapchain = chain;
    pass.window_key = window;
    host.render_pass(target.commands as *mut c_void, &pass, 0);
}

/// Thread-local receipt from the same native Present call path as marker4. A stored identity
/// carries no COM reference across callbacks; marker4 reacquires the exact indexed buffer.
#[derive(Clone, Copy)]
pub(crate) struct PresentReceipt {
    chain: usize,
    index: u32,
    surface: usize,
    generation: u32,
}

pub(crate) fn before_present(
    chain: *mut c_void,
    flags: u32,
    generation: u32,
) -> Option<PresentReceipt> {
    PRESENT_RECEIPT.set(None);
    if flags & 1 != 0 {
        return None; // DXGI_PRESENT_TEST does not present a buffer.
    }
    let index = unsafe { platform::back_buffer_index(chain) };
    let buffer = Retained(unsafe { platform::swapchain_buffer(chain, index) });
    (!buffer.0.is_null()).then_some(PresentReceipt {
        chain: chain as usize,
        index,
        surface: buffer.0 as usize,
        generation,
    })
}

pub(crate) fn after_present(receipt: Option<PresentReceipt>, result: i32) {
    PRESENT_RECEIPT.set(receipt.filter(|_| result == 0));
}

fn window_pass(
    scene: &mut Scene,
    frame: u64,
    active: usize,
    receipt: PresentReceipt,
) -> Option<rsf_game_render_pass> {
    if receipt.chain != active
        || scene.pass.native_frame != frame
        || receipt.generation != scene.pass.resource_generation
        || scene.surface == 0
        || scene.surface != receipt.surface
    {
        return None;
    }
    let mut pass = scene.pass;
    pass.role = RSF_GAME_RENDER_WINDOW;
    scene.surface = 0;
    Some(pass)
}

unsafe extern "C" fn marker(kind: u32, frame: u64) -> bool {
    let _scope = Scope::enter(&READERS);
    let handled = kind <= LAST_MARKER && latency(kind, frame);
    if kind == INPUT_SAMPLE {
        let _ = INPUT_SOURCE.compare_exchange(frame, 0, Ordering::SeqCst, Ordering::SeqCst);
    }
    if ACTIVE.load(Ordering::SeqCst) && frame != 0 && kind <= LAST_MARKER {
        let mut state = lock(&STATE);
        let markers = slot(&mut state.markers, frame);
        if markers.frame != frame {
            *markers = Markers { frame, mask: 0 };
        }
        markers.mask |= 1 << kind;
    }
    if kind == PRESENT_END
        && let Some(receipt) = PRESENT_RECEIPT.take()
        && ACTIVE.load(Ordering::SeqCst)
        && let Some(host) = HOST.get()
        && host.0.render_pass.is_some()
    {
        let pass = {
            let mut state = lock(&STATE);
            let active = state.swapchain;
            window_pass(slot(&mut state.scenes, frame), frame, active, receipt)
        };
        if let Some(mut pass) = pass {
            let chain = receipt.chain as *mut c_void;
            let presented = Retained(unsafe { platform::swapchain_buffer(chain, receipt.index) });
            if presented.0 as usize == receipt.surface {
                pass.swapchain = chain;
                pass.scene_surface = presented.0;
                host.render_pass(ptr::null_mut(), &pass, 1);
            }
        }
    }
    if handled {
        return true;
    }
    match unsafe { trampoline::<MarkerFn>(&ORIGINAL_MARKER) } {
        Some(native) => unsafe { native(kind, frame) },
        None => false,
    }
}

struct Hook {
    rva: usize,
    detour: *const c_void,
    original: &'static AtomicPtr<c_void>,
}

/// Creation order; rollback runs in reverse.
fn hooks() -> [Hook; 5] {
    [
        Hook {
            rva: OUTPUT.rva,
            detour: platform::output_detour as *const c_void,
            original: &ORIGINAL_OUTPUT,
        },
        Hook {
            rva: BLIT.rva,
            detour: platform::blit_detour as *const c_void,
            original: &ORIGINAL_BLIT,
        },
        Hook {
            rva: MARKER.rva,
            detour: marker as *const c_void,
            original: &ORIGINAL_MARKER,
        },
        Hook {
            rva: SLEEP.rva,
            detour: sleep as *const c_void,
            original: &ORIGINAL_SLEEP,
        },
        Hook {
            rva: MODE.rva,
            detour: mode as *const c_void,
            original: &ORIGINAL_MODE,
        },
    ]
}

fn bytes_match(image: usize) -> bool {
    [&BLIT, &MARKER, &OUTPUT, &SLEEP, &MODE]
        .iter()
        .all(|site| platform::matches(image.wrapping_add(site.rva), &site.expected))
        && platform::matches(image.wrapping_add(NATIVE_TEXTURE_RVA), &NATIVE_TEXTURE)
}

fn output_callers_match(image: usize) -> bool {
    OUTPUT_CALLS.iter().all(|&site| {
        let mut call = [0; 5];
        read_bytes(image.wrapping_add(site), &mut call)
            && call_target(site, &call) == Some(OUTPUT.rva)
    })
}

pub(crate) fn prepare(image: usize, services: &rsf_game_host_services) -> bool {
    if PREPARED.load(Ordering::SeqCst) || image == 0 || services.render_pass.is_none() {
        return false;
    }
    IMAGE.store(image, Ordering::SeqCst);
    let host = Host(*services);
    if !bytes_match(image) {
        host.log("RDR2 scene producer refused: native blit/marker/texture bytes differ.");
        return false;
    }
    if !output_callers_match(image) {
        host.log("RDR2 scene producer refused: postfx output callers differ.");
        return false;
    }
    let hooks = hooks();
    for (index, hook) in hooks.iter().enumerate() {
        if !platform::create(image.wrapping_add(hook.rva), hook.detour, hook.original) {
            for created in hooks[..index].iter().rev() {
                platform::remove(image.wrapping_add(created.rva));
            }
            return false;
        }
    }
    HOST.set(Some(*services));
    PREPARED.store(true, Ordering::SeqCst);
    host.log("RDR2 scene producer prepared at +5c1254; native frame markers observed at +26d4674.");
    true
}

pub(crate) fn start() -> bool {
    if !PREPARED.load(Ordering::SeqCst) {
        return false;
    }
    let image = IMAGE.load(Ordering::SeqCst);
    ACTIVE.store(true, Ordering::SeqCst);
    // The blit is enabled last so a completed scene is never produced without its frame markers.
    let enabled = [OUTPUT.rva, MARKER.rva, SLEEP.rva, MODE.rva, BLIT.rva]
        .iter()
        .all(|&rva| platform::enable(image.wrapping_add(rva)));
    if enabled {
        return true;
    }
    ACTIVE.store(false, Ordering::SeqCst);
    for rva in [MARKER.rva, OUTPUT.rva, SLEEP.rva, MODE.rva] {
        platform::disable(image.wrapping_add(rva));
    }
    false
}

pub(crate) fn quiesce() -> bool {
    ACTIVE.store(false, Ordering::SeqCst);
    unsafe { platform::input_disable() };
    READERS.load(Ordering::SeqCst) == 0 && unsafe { platform::input_idle() }
}

pub(crate) fn stop() -> bool {
    if !PREPARED.load(Ordering::SeqCst) {
        return true;
    }
    if ACTIVE.load(Ordering::SeqCst) {
        return false;
    }
    let image = IMAGE.load(Ordering::SeqCst);
    INPUT_SOURCE.store(0, Ordering::SeqCst);
    unsafe { platform::input_disable() };
    let sites =
        [BLIT.rva, MARKER.rva, OUTPUT.rva, SLEEP.rva, MODE.rva].map(|rva| image.wrapping_add(rva));
    if !unsafe { platform::input_unhook() }
        || !sites.iter().all(|&site| platform::disable(site))
        || READERS.load(Ordering::SeqCst) != 0
        || !unsafe { platform::input_idle() && platform::input_remove() }
        || !sites.into_iter().all(platform::remove)
    {
        return false;
    }
    HOST.set(None);
    PREPARED.store(false, Ordering::SeqCst);
    true
}

/// Records the primary SR pass of a native frame. A second successful upscale view for the same
/// frame withdraws the scene identity instead of silently sharing it.
pub(crate) fn upscale(commands: usize, pass: &rsf_game_render_pass) {
    if !PREPARED.load(Ordering::SeqCst) || !ACTIVE.load(Ordering::SeqCst) {
        return;
    }
    let mut state = lock(&STATE);
    if state.generation == pass.resource_generation {
        record_upscale(&mut state, commands, pass);
    }
}

fn record_upscale(state: &mut State, commands: usize, pass: &rsf_game_render_pass) {
    let scene = slot(&mut state.scenes, pass.native_frame);
    if scene.pass.native_frame == pass.native_frame {
        scene.commands = 0;
        scene.pass.source_frame_id = 0;
        scene.pass.camera_valid = 0;
        return;
    }
    *scene = Scene::EMPTY;
    scene.pass = pass.without_resources();
    scene.commands = commands;
}

/// The swapchain whose Present observer the renderer attached; retained by the renderer.
pub(crate) fn set_swapchain(swapchain: usize, generation: u32) {
    let mut state = lock(&STATE);
    state.scenes.fill(Scene::EMPTY);
    state.swapchain = swapchain;
    state.generation = generation;
}

/// The native frame's source identity once its required markers were all observed.
pub(crate) fn source(frame: u64) -> u64 {
    if !ACTIVE.load(Ordering::SeqCst) || frame == 0 {
        return 0;
    }
    let mut state = lock(&STATE);
    source_of(slot(&mut state.markers, frame), frame)
}

fn source_of(markers: &Markers, frame: u64) -> u64 {
    if markers.frame == frame && markers.mask & REQUIRED_MARKERS == REQUIRED_MARKERS {
        frame
    } else {
        0
    }
}

#[cfg(test)]
mod tests;
