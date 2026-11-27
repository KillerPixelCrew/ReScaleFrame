// SPDX-License-Identifier: MIT
//! Guarded temporal graph observation and its GameSDK lifecycle.
use crate::INVALID_ARGUMENT;
use crate::camera::read;
use crate::discovery::{self, Site};
use crate::execution;
use crate::windows::{
    create_hook, disable_hook, enable_hook, image_base, is_game_process, tick_count,
};
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_game_sys::{
    host_log, rsf_game_control_args, rsf_game_host_services, rsf_game_prepare_args,
    rsf_game_renderer_status, rsf_game_start_args, rsf_result,
};
use std::ffi::{CStr, c_void};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use rsf_game_sys::call_header::validate;
use rsf_game_sys::{
    RSF_ERROR_BUSY as BUSY, RSF_ERROR_NATIVE_REFUSED as NATIVE_REFUSED,
    RSF_ERROR_NOT_READY as NOT_READY, RSF_OK as OK,
};

static ACCEPTING: AtomicBool = AtomicBool::new(false);
static CALLS: AtomicU32 = AtomicU32::new(0);
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static STATE: Mutex<State> = Mutex::new(State {
    host: None,
    target: 0,
    installed: false,
    reason: c"Native temporal pass observer has not been prepared.",
    samples: [Sample::EMPTY; 16],
    serial: 0,
});

pub(crate) fn accepting() -> bool {
    ACCEPTING.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_wingman_accepting() -> bool {
    accepting()
}

pub(crate) fn active_host() -> Option<rsf_game_host_services> {
    let state = state();
    state.host.filter(|_| accepting())
}

pub(crate) fn install_trampoline(target: usize, detour: usize, original: &AtomicUsize) -> bool {
    if original.load(Ordering::Acquire) != 0 {
        return true;
    }
    let mut address = 0;
    if !create_hook(target, detour, &mut address) || address == 0 {
        return false;
    }
    original.store(address, Ordering::Release);
    true
}

pub(crate) struct Entry<'a>(&'a AtomicU32);

impl<'a> Entry<'a> {
    pub(crate) fn new(calls: &'a AtomicU32) -> Self {
        calls.fetch_add(1, Ordering::AcqRel);
        Self(calls)
    }
}

impl Drop for Entry<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}

type Rect = [i32; 4];

#[derive(Clone, Copy)]
struct Sample {
    owner: usize,
    last: u64,
    input: Rect,
    output: Rect,
}

impl Sample {
    const EMPTY: Self = Self {
        owner: 0,
        last: 0,
        input: [0; 4],
        output: [0; 4],
    };
}

struct State {
    host: Option<rsf_game_host_services>,
    target: usize,
    installed: bool,
    reason: &'static CStr,
    samples: [Sample; 16],
    serial: u64,
}

// The copied services and user pointer are borrowed until stop verifies no observer call remains.
unsafe impl Send for State {}

fn state() -> MutexGuard<'static, State> {
    lock_or_recover(&STATE)
}

/// The discovered AddPasses entry, still matching its pattern in live memory, or why not.
fn target() -> Result<usize, &'static CStr> {
    if !is_game_process() {
        return Err(c"Native temporal observer refused: not the Project Wingman executable.");
    }
    discovery::admit()?;
    let image = image_base();
    if !discovery::verify(image, Site::AddPasses) {
        return Err(
            c"Native temporal observer refused: site 'default temporal upscaler AddPasses' changed since discovery.",
        );
    }
    Ok(image + discovery::rva(Site::AddPasses))
}

pub(crate) unsafe extern "C" fn prepare(args: *const rsf_game_prepare_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != OK {
        return valid;
    }
    let host = unsafe { (*args).host };
    let valid = unsafe { validate(host) };
    if valid != OK {
        return valid;
    }
    let host = unsafe { &*host };
    if host.session_id == 0 {
        return INVALID_ARGUMENT;
    }
    let mut state = state();
    if state.host.is_some() || CALLS.load(Ordering::Acquire) != 0 {
        return BUSY;
    }
    let prepared = target().and_then(|target| {
        if !install_trampoline(target, add_passes as *const () as usize, &ORIGINAL) {
            return Err(
                c"Native temporal observer refused: the AddPasses hook could not be created.",
            );
        }
        execution::prepare().map(|()| target)
    });
    // Evidence only: the build is logged, the discovered sites decide.
    let evidence = discovery::evidence();
    if !evidence.is_empty() {
        let outcome =
            prepared.map_or_else(|reason| reason, |_| c"Native temporal sites discovered.");
        host_log(
            host,
            &format!("PW {evidence}. {}", outcome.to_string_lossy()),
        );
    }
    let target = match prepared {
        Ok(target) => target,
        Err(reason) => {
            state.reason = reason;
            return NATIVE_REFUSED;
        }
    };
    state.target = target;
    state.host = Some(*host);
    state.samples = [Sample::EMPTY; 16];
    state.serial = 0;
    state.reason = c"Native temporal observer prepared; no graphics processing implemented.";
    OK
}

pub(crate) unsafe extern "C" fn start(args: *const rsf_game_start_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != OK {
        return valid;
    }
    let mut state = state();
    if state.host.is_none() {
        return NOT_READY;
    }
    if !execution::start() {
        return NATIVE_REFUSED;
    }
    if !state.installed && !unsafe { enable_hook(state.target) } {
        execution::stop();
        return NATIVE_REFUSED;
    }
    state.installed = true;
    ACCEPTING.store(true, Ordering::Release);
    state.reason = c"Native temporal inputs leased on the RHI stream; SR, FG and latency execution remain unimplemented.";
    OK
}

pub(crate) unsafe extern "C" fn quiesce(args: *const rsf_game_control_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != OK {
        return valid;
    }
    ACCEPTING.store(false, Ordering::Release);
    OK
}

pub(crate) unsafe extern "C" fn stop(args: *const rsf_game_control_args) -> rsf_result {
    let valid = unsafe { validate(args) };
    if valid != OK {
        return valid;
    }
    let mut state = state();
    ACCEPTING.store(false, Ordering::Release);
    if !execution::stop() {
        return BUSY;
    }
    if state.installed && !unsafe { disable_hook(state.target) } {
        return BUSY;
    }
    state.installed = false;
    if CALLS.load(Ordering::Acquire) != 0 {
        return BUSY;
    }
    state.host = None;
    state.reason = c"Native temporal observer stopped; no host services retained.";
    OK
}

pub(crate) unsafe extern "C" fn status(out: *mut rsf_game_renderer_status) -> rsf_result {
    let valid = unsafe { validate(out) };
    if valid != OK {
        return valid;
    }
    let state = state();
    let out = unsafe { &mut *out };
    out.prepared = u32::from(state.host.is_some());
    out.active = u32::from(ACCEPTING.load(Ordering::Acquire));
    out.rendering_ready = 0;
    out.reason = state.reason.as_ptr();
    OK
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Inputs {
    allow_downsample: u32,
    downsample_format: u32,
    colour: usize,
    depth: usize,
    velocity: usize,
}

const _: () = assert!(size_of::<Inputs>() == 32 && std::mem::offset_of!(Inputs, colour) == 8);

#[derive(Default)]
struct Observation {
    inputs: Inputs,
    render: Rect,
    result: Rect,
    history: usize,
    jitter: [f32; 2],
    camera_cut: u8,
    output: usize,
    half_output: usize,
}

impl Observation {
    // Graph-construction pointers are opaque facts. The execution adapter owns GPU leases.
    fn capture(
        &mut self,
        view: usize,
        input: usize,
        output: usize,
        output_rect: usize,
        half: usize,
    ) -> Option<()> {
        let fields = discovery::view();
        self.inputs = unsafe { read(input, 0) }?;
        self.render = unsafe { read(view, fields.rect) }?;
        self.history = unsafe { read(view, fields.state) }?;
        self.jitter = unsafe { read(view, fields.jitter) }?;
        self.camera_cut = unsafe { read(view, fields.camera_cut) }?;
        self.output = unsafe { read(output, 0) }?;
        self.result = unsafe { read(output_rect, 0) }?;
        self.half_output = unsafe { read(half, 0) }?;
        Some(())
    }
}

fn sample(view: usize, data: &Observation) -> Option<(rsf_game_host_services, u64)> {
    let mut state = state();
    if !ACCEPTING.load(Ordering::Acquire) {
        return None;
    }
    let host = state.host?;
    let key = if data.history != 0 {
        data.history
    } else {
        view
    };
    let mut index = 0;
    for (candidate, sample) in state.samples.iter().enumerate() {
        if sample.owner == key {
            index = candidate;
            break;
        }
        if sample.last < state.samples[index].last {
            index = candidate;
        }
    }
    let last = &mut state.samples[index];
    let now = tick_count();
    if last.owner == key
        && last.input == data.render
        && last.output == data.result
        && data.camera_cut == 0
        && now.wrapping_sub(last.last) < 1000
    {
        return None;
    }
    *last = Sample {
        owner: key,
        last: now,
        input: data.render,
        output: data.result,
    };
    state.serial = state.serial.wrapping_add(1);
    Some((host, state.serial))
}

type AddPasses = unsafe extern "C-unwind" fn(
    usize,
    *const c_void,
    *const c_void,
    *const c_void,
    *const c_void,
    *const c_void,
    *const c_void,
    *const c_void,
);

unsafe extern "C-unwind" fn add_passes(
    this: usize,
    graph: *const c_void,
    view: *const c_void,
    input: *const c_void,
    output: *const c_void,
    output_rect: *const c_void,
    half: *const c_void,
    half_rect: *const c_void,
) {
    let _entry = Entry::new(&CALLS);
    let original: AddPasses = unsafe { std::mem::transmute(ORIGINAL.load(Ordering::Acquire)) };
    unsafe {
        original(
            this,
            graph,
            view,
            input,
            output,
            output_rect,
            half,
            half_rect,
        )
    };
    observe(graph, view, input, output, output_rect, half, half_rect);
}

fn observe(
    graph: *const c_void,
    view: *const c_void,
    input: *const c_void,
    output: *const c_void,
    output_rect: *const c_void,
    half: *const c_void,
    half_rect: *const c_void,
) {
    if !ACCEPTING.load(Ordering::Acquire) {
        return;
    }
    let mut data = Observation::default();
    let readable = data
        .capture(
            view as usize,
            input as usize,
            output as usize,
            output_rect as usize,
            half as usize,
        )
        .is_some();
    let half_rect: Rect = if data.half_output != 0 {
        unsafe { read(half_rect as usize, 0) }.unwrap_or_default()
    } else {
        [0; 4]
    };
    let Some((host, serial)) = sample(view as usize, &data) else {
        return;
    };
    if host.log.is_none() {
        return;
    }
    let graph = graph as usize;
    let view = view as usize;
    let Observation {
        inputs,
        render: [x0, y0, x1, y1],
        result: [ox0, oy0, ox1, oy1],
        history,
        jitter: [jx, jy],
        camera_cut,
        output,
        half_output,
    } = data;
    let [hx0, hy0, hx1, hy1] = half_rect;
    let Inputs {
        colour,
        depth,
        velocity,
        allow_downsample,
        downsample_format,
    } = inputs;
    let readable = u32::from(readable);
    let downsample = allow_downsample & 0xff;
    host_log(
        &host,
        &format!(
            concat!(
                "PW temporal serial={} graph={:016X} view={:016X} history={:016X} readable={} cut={} ",
                "render=({},{},{},{}) output=({},{},{},{}) jitter=({:.4},{:.4}) ",
                "rdg=({:016X},{:016X},{:016X})->{:016X} half={:016X} half_rect=({},{},{},{}) downsample={} format={}"
            ),
            serial,
            graph,
            view,
            history,
            readable,
            camera_cut,
            x0,
            y0,
            x1,
            y1,
            ox0,
            oy0,
            ox1,
            oy1,
            jx,
            jy,
            colour,
            depth,
            velocity,
            output,
            half_output,
            hx0,
            hy0,
            hx1,
            hy1,
            downsample,
            downsample_format,
        ),
    );
}
