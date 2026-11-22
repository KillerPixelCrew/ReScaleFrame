// SPDX-License-Identifier: MIT
// D3D11 to D3D12 SR bridge: the game's D3D11 device, a D3D12 device on the same adapter, shared
// surfaces and one shared fence. This owner decides lifetime, cross-API ordering, input leases,
// GPU retirement and failure policy. `native` (platform.rs; fakes under test) supplies the
// native operations.
use crate::sys::RSF_SR_NONE;
use rsf_game_sys::backend_result::{
    ABI_MISMATCH, FEATURE_FAILED, INIT_FAILED, INVALID_ARGUMENT, NOT_READY, NOT_SUPPORTED, OK,
    WRONG_API,
};
#[cfg(all(feature = "test-fixtures", not(test)))]
mod fixture;
#[cfg(not(test))]
mod platform;
#[cfg(not(test))]
use platform as native;
#[cfg(test)]
use tests::native;

use crate::com_ref::ComRef;
use crate::owned_handle::OwnedHandle as Owned;
use crate::sr_session::has_capacity;
use crate::sys::*;
use rsf_game_sys::{
    RSF_FRAME_FLAG_RESET, RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record,
};
use std::ffi::{CString, c_void};
use std::ptr::{self, NonNull};
use std::time::Instant;

const API_D3D11: u32 = 1;

// DXGI formats and D3D12 states, checked against the Windows bindings in platform.rs.
const COLOR_FORMAT: u32 = 10; // R16G16B16A16_FLOAT
const MOTION_FORMAT: u32 = 34; // R16G16_FLOAT; R32G32_FLOAT is not shareable
const SCALAR_FORMAT: u32 = 41; // R32_FLOAT
const STATE_COMMON: u32 = 0;
const STATE_UNORDERED_ACCESS: u32 = 0x8;
const STATE_SHADER_RESOURCE: u32 = 0x40;

const FENCE_TIMEOUT_MS: u32 = 10_000;
// Trivial GPU work: a longer wait is a hung device, not a compile.
const WARM_UP_TIMEOUT_MS: u32 = 30_000;
const COMMAND_SLOTS: usize = 3;

// Colour, depth, motion, exposure, output, then the reactive and transparency masks.
#[allow(dead_code)] // Slot 0 is addressed as the first entry.
const COLOR: usize = 0;
const DEPTH: usize = 1;
const MOTION: usize = 2;
const EXPOSURE: usize = 3;
const OUTPUT: usize = 4;
const MASKS: usize = 5;
const SLOTS: usize = 7;

type Handle = *mut c_void;
type Log = rsf_backend_log_fn;

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub(crate) struct TextureDesc {
    width: u32,
    height: u32,
    format: u32,
    samples: u32,
    array_size: u32,
}

fn owned_com(object: Handle) -> Option<ComRef> {
    NonNull::new(object).map(ComRef)
}

fn com(object: &ComRef) -> Handle {
    object.0.as_ptr()
}

struct SharedFence {
    // Borrowed from the shared fence.
    d3d11: Handle,
    d3d12: Handle,
    _owner: Owned,
}

// Shared surfaces for one render shape. The pointers are borrowed from `owners`.
struct Surfaces {
    owners: Vec<Owned>,
    d3d11: [Handle; SLOTS],
    d3d12: [Handle; SLOTS],
}

#[derive(Clone, Copy, PartialEq)]
struct Shape {
    width: u32,
    height: u32,
    output_width: u32,
    output_height: u32,
}

struct Prepared {
    shape: Shape,
    surfaces: Surfaces,
    output: ComRef,
}

struct Commands {
    allocator: ComRef,
    list: ComRef,
    complete: u64,
}

// The texture last validated in a slot. Holding it keeps its address from being reused by another
// texture while the description is cached.
#[derive(Default)]
struct Validated {
    texture: Option<ComRef>,
    desc: TextureDesc,
}

// A warm-up dispatch whose GPU completion was not observed. The submitted list references these
// objects and the session's SDK context, so all of them stay alive until it is reaped.
struct PendingWarmUp {
    _resources: [Option<ComRef>; 4],
    _target: ComRef,
    _commands: Commands,
    fence: ComRef,
    event: Owned,
    // Whether a signal of 1 on `fence` is queued behind the list. Without one, the fence value
    // proves nothing about the list.
    signaled: bool,
}

#[derive(Clone, Copy)]
enum SignalSource {
    Context,
    Queue,
}

// Fields drop in declaration order: any pending warm-up and the SDK session first, then shared
// resources, the fence and event, and the devices last.
pub(crate) struct Bridge {
    warm_up: Option<PendingWarmUp>,
    session: Owned,
    prepared: Option<Prepared>,
    fence: SharedFence,
    event: Owned,
    validated: [Validated; SLOTS],
    commands: Vec<Commands>,
    next_commands: usize,
    queue: ComRef,
    device12: ComRef,
    context: ComRef,
    // The immediate context as GetImmediateContext returned it, which is the pointer callers pass,
    // so the per-frame identity check needs no QueryInterface.
    immediate: ComRef,
    device11: ComRef,
    log: Log,
    log_user: Handle,
    tick: u64,
    faulted: bool,
    // A refused signal must be retried on the stream that issued the work for this tick.
    pending_signal: Option<SignalSource>,
    // A D3D11 signal of `tick` that has not been flushed yet. CPU waits flush it first.
    signal_unflushed: bool,
    auto_exposure: u32,
    hdr: bool,
}

fn say(log: Log, user: Handle, message: &str) {
    if let Some(log) = log
        && let Ok(message) = CString::new(message)
    {
        unsafe { log(user, message.as_ptr()) };
    }
}

// The extent a slot must cover: output size, 1x1 exposure, otherwise the render size.
fn extent(slot: usize, record: &rsf_frame_record) -> (u32, u32) {
    match slot {
        OUTPUT => (record.output_width, record.output_height),
        EXPOSURE => (1, 1),
        _ => (record.render_width, record.render_height),
    }
}

fn input_accepted(slot: usize, desc: &TextureDesc, record: &rsf_frame_record) -> bool {
    let (width, height) = extent(slot, record);
    let format = match slot {
        DEPTH | EXPOSURE => SCALAR_FORMAT,
        MOTION => MOTION_FORMAT,
        _ => COLOR_FORMAT,
    };
    desc.width >= width
        && desc.height >= height
        && desc.samples == 1
        && desc.array_size == 1
        && (slot != OUTPUT || (desc.width == width && desc.height == height))
        && desc.format == format
}

// An unusable mask is dropped rather than refusing the reconstruction.
fn mask_accepted(desc: &TextureDesc, record: &rsf_frame_record) -> bool {
    desc.format == SCALAR_FORMAT
        && desc.width >= record.render_width
        && desc.height >= record.render_height
        && desc.samples == 1
        && desc.array_size == 1
}

impl Bridge {
    fn open(setup: &rsf_sr_session_setup) -> Result<Self, i32> {
        let (log, log_user) = (setup.open.log, setup.open.log_user);
        let device11 = unsafe { ComRef::retain(setup.open.device) };
        let immediate = owned_com(unsafe { native::immediate_context(com(&device11)) });
        let immediate = immediate.ok_or(NOT_SUPPORTED)?;
        let context = owned_com(unsafe { native::query_context4(com(&immediate)) });
        let context = context.ok_or(NOT_SUPPORTED)?;
        let device12 = owned_com(unsafe { native::create_device12(com(&device11)) });
        let device12 = device12.ok_or(NOT_SUPPORTED)?;
        let queue =
            owned_com(unsafe { native::create_queue(com(&device12)) }).ok_or(INIT_FAILED)?;
        let fence = unsafe { native::fence_create(com(&device11), com(&device12), log, log_user) };
        let fence = Owned::new(fence, native::fence_destroy).ok_or(INIT_FAILED)?;
        let fence = SharedFence {
            d3d11: unsafe { native::fence_d3d11(fence.get()) },
            d3d12: unsafe { native::fence_d3d12(fence.get()) },
            _owner: fence,
        };
        let event = Owned::new(unsafe { native::create_event() }, native::close_event)
            .ok_or(INIT_FAILED)?;
        let mut commands = Vec::with_capacity(COMMAND_SLOTS);
        for _ in 0..COMMAND_SLOTS {
            commands.push(create_commands(&device12).ok_or(INIT_FAILED)?);
        }
        let mut translated = *setup;
        translated.open.api = RSF_API_D3D12;
        translated.open.device = com(&device12);
        let mut session = ptr::null_mut();
        let result = unsafe { native::session_create(&translated, &mut session) };
        if result != OK {
            return Err(result);
        }
        let session = Owned::new(session, native::session_destroy).ok_or(INIT_FAILED)?;
        Ok(Self {
            warm_up: None,
            session,
            prepared: None,
            fence,
            event,
            validated: Default::default(),
            commands,
            next_commands: 0,
            queue,
            device12,
            context,
            immediate,
            device11,
            log,
            log_user,
            tick: 0,
            faulted: false,
            pending_signal: None,
            signal_unflushed: false,
            auto_exposure: setup.open.auto_exposure,
            hdr: setup.open.hdr != 0,
        })
    }

    fn say(&self, message: &str) {
        say(self.log, self.log_user, message);
    }

    fn gpu_failure(&mut self) -> i32 {
        self.faulted = true;
        FEATURE_FAILED
    }

    fn wait_value(&self, value: u64) -> bool {
        unsafe {
            native::wait_fence(self.fence.d3d12, value, self.event.get(), FENCE_TIMEOUT_MS) != 0
        }
    }

    fn arm_signal(&mut self, source: SignalSource) -> bool {
        let armed = unsafe {
            match source {
                SignalSource::Context => {
                    native::context_signal(com(&self.context), self.fence.d3d11, self.tick)
                }
                SignalSource::Queue => {
                    native::queue_signal(com(&self.queue), self.fence.d3d12, self.tick)
                }
            }
        } != 0;
        if armed {
            self.pending_signal = None;
            self.signal_unflushed = matches!(source, SignalSource::Context);
        }
        armed
    }

    fn signal(&mut self, source: SignalSource) -> bool {
        self.tick += 1;
        self.pending_signal = Some(source);
        self.arm_signal(source)
    }

    fn flush_signal(&mut self) {
        if self.signal_unflushed {
            unsafe { native::flush(com(&self.context)) };
            self.signal_unflushed = false;
        }
    }

    fn wait(&mut self) -> bool {
        // A signal refusal faults the bridge, so no later frame can advance the shared fence.
        // Append the signal behind the original work and require that exact value to complete.
        if let Some(source) = self.pending_signal
            && !self.arm_signal(source)
        {
            return false;
        }
        self.flush_signal();
        self.wait_value(self.tick)
    }

    fn effective_backend(&self) -> u32 {
        let mut status: rsf_sr_session_status = unsafe { std::mem::zeroed() };
        status.struct_size = size_of::<rsf_sr_session_status>() as u32;
        unsafe { native::session_get_status(self.session.get(), &mut status) };
        status.effective
    }

    // The description of a D3D11 input owned by the bridge's device, from the cache when the
    // texture is the one last seen in this slot. None for a texture from another device.
    fn describe(&mut self, slot: usize, resource: Handle) -> Option<TextureDesc> {
        let device = com(&self.device11);
        let cached = &mut self.validated[slot];
        if cached
            .texture
            .as_ref()
            .is_some_and(|texture| com(texture) == resource)
        {
            return Some(cached.desc);
        }
        cached.texture = None;
        if resource.is_null() {
            return None;
        }
        let mut desc = TextureDesc::default();
        if unsafe { native::describe(resource, device, &mut desc) } == 0 {
            return None;
        }
        cached.desc = desc;
        cached.texture = Some(unsafe { ComRef::retain(resource) });
        Some(desc)
    }

    // Recreates the shared surfaces when the render shape changes, after the GPU has retired them.
    fn prepare(&mut self, record: &rsf_frame_record) -> Result<&Prepared, i32> {
        let shape = Shape {
            width: record.render_width,
            height: record.render_height,
            output_width: record.output_width,
            output_height: record.output_height,
        };
        if self
            .prepared
            .as_ref()
            .is_some_and(|prepared| prepared.shape == shape)
        {
            return Ok(self.prepared.as_ref().unwrap());
        }
        if !self.wait() {
            return Err(self.gpu_failure());
        }
        self.prepared = None;
        let (device11, device12) = (com(&self.device11), com(&self.device12));
        let mut surfaces = Surfaces {
            owners: Vec::with_capacity(SLOTS),
            d3d11: [ptr::null_mut(); SLOTS],
            d3d12: [ptr::null_mut(); SLOTS],
        };
        for slot in 0..SLOTS {
            let (width, height) = extent(slot, record);
            let format = match slot {
                DEPTH | EXPOSURE | MASKS.. => SCALAR_FORMAT,
                MOTION => MOTION_FORMAT,
                _ => COLOR_FORMAT,
            };
            let surface = unsafe {
                native::surface_create(
                    device11,
                    device12,
                    width,
                    height,
                    format,
                    self.log,
                    self.log_user,
                )
            };
            let Some(surface) = Owned::new(surface, native::surface_destroy) else {
                return Err(INIT_FAILED);
            };
            surfaces.d3d11[slot] = unsafe { native::surface_d3d11(surface.get()) };
            surfaces.d3d12[slot] = unsafe { native::surface_d3d12(surface.get()) };
            surfaces.owners.push(surface);
        }
        let output = unsafe {
            native::create_output(
                device12,
                record.output_width,
                record.output_height,
                COLOR_FORMAT,
            )
        };
        let Some(output) = owned_com(output) else {
            return Err(INIT_FAILED);
        };
        Ok(self.prepared.insert(Prepared {
            shape,
            surfaces,
            output,
        }))
    }

    // Releases a pending warm-up once its own fence, signalled behind its list on the bridge
    // queue, reaches 1, or once the device is removed. A failed first signal is retried here; the
    // queue runs nothing else while a warm-up is pending, so a retried signal is ordered after the
    // list. Returns whether no warm-up is pending.
    fn reap_warm_up(&mut self, timeout_ms: u32) -> bool {
        let Some(pending) = &mut self.warm_up else {
            return true;
        };
        let fence = com(&pending.fence);
        if !pending.signaled {
            pending.signaled = unsafe { native::queue_signal(com(&self.queue), fence, 1) } != 0;
        }
        let completed = pending.signaled
            && unsafe { native::wait_fence(fence, 1, pending.event.get(), timeout_ms) } != 0;
        if !completed && unsafe { native::device_ready(com(&self.device12)) } != 0 {
            return false;
        }
        self.warm_up = None;
        let state = if completed {
            "completed"
        } else {
            "abandoned with the removed device"
        };
        self.say(&format!(
            "SR warm-up GPU work {state}; releasing its resources"
        ));
        true
    }

    fn select(&mut self, backend: u32, quality: u32, version_id: u64) -> i32 {
        if self.faulted && backend != RSF_SR_NONE {
            return NOT_READY;
        }
        // Any selection, including none, may close the SDK context the warm-up still uses.
        if !self.reap_warm_up(FENCE_TIMEOUT_MS) {
            return NOT_READY;
        }
        if !self.wait() {
            return self.gpu_failure();
        }
        unsafe { native::session_select(self.session.get(), backend, quality, version_id) }
    }

    fn set_auto_exposure(&mut self, enabled: u32) -> i32 {
        if self.faulted {
            return NOT_READY;
        }
        if self.auto_exposure == enabled {
            return OK;
        }
        // Changing exposure recreates the SDK context.
        if !self.reap_warm_up(FENCE_TIMEOUT_MS) {
            return NOT_READY;
        }
        if !self.wait() {
            return self.gpu_failure();
        }
        let result = unsafe { native::session_set_auto_exposure(self.session.get(), enabled) };
        if result == OK {
            self.auto_exposure = enabled;
        }
        result
    }

    fn evaluate(&mut self, context_pointer: Handle, frame: *const rsf_sr_frame) -> i32 {
        if self.faulted {
            return NOT_READY;
        }
        // A frame call only polls, so a hung warm-up does not stall the render thread.
        if !self.reap_warm_up(0) {
            return NOT_READY;
        }
        let result = unsafe { native::validate_frame(frame) };
        if result != OK {
            return result;
        }
        let frame = unsafe { &*frame };
        let record = unsafe { &*frame.record };
        let hdr = u32::from(self.hdr);
        if unsafe { native::dump_poll_requested() } != 0 {
            let backend = self.effective_backend();
            unsafe { native::dump_inputs(context_pointer, frame, backend, hdr) };
        }
        // The pointer the device handed out needs no QueryInterface; anything else must be the
        // same context reached through another interface.
        let context = com(&self.context);
        if context_pointer != com(&self.immediate)
            && unsafe { native::same_context(context_pointer, context) } == 0
        {
            return INVALID_ARGUMENT;
        }
        // Check all source owners, extents and formats before issuing any copy.
        let inputs = [
            &frame.color,
            &frame.depth,
            &frame.motion,
            &frame.exposure,
            &frame.output,
        ];
        for (slot, input) in inputs.iter().enumerate() {
            if slot == EXPOSURE && input.resource.is_null() {
                continue;
            }
            match self.describe(slot, input.resource) {
                Some(desc) if input_accepted(slot, &desc, record) => {}
                _ => return INVALID_ARGUMENT,
            }
        }
        let mut masks: [Handle; 2] = [ptr::null_mut(); 2];
        for (index, mask) in [&frame.reactive, &frame.transparency]
            .into_iter()
            .enumerate()
        {
            if mask.struct_size == 0 || mask.resource.is_null() {
                continue;
            }
            if self
                .describe(MASKS + index, mask.resource)
                .is_some_and(|desc| mask_accepted(&desc, record))
            {
                masks[index] = mask.resource;
            }
        }
        let prepared = match self.prepare(record) {
            Ok(prepared) => prepared,
            Err(code) => return code,
        };
        let (shared11, shared12, output) = (
            prepared.surfaces.d3d11,
            prepared.surfaces.d3d12,
            com(&prepared.output),
        );
        let (fence11, fence12) = (self.fence.d3d11, self.fence.d3d12);
        let queue = com(&self.queue);

        // CPU waits protect allocator reuse only. Cross-API resource ownership stays ordered on
        // the GPU: D3D11 inputs -> D3D12 SR -> D3D11 output copy -> the next frame's inputs.
        let slot = self.next_commands;
        if !self.wait_value(self.commands[slot].complete) {
            return self.gpu_failure();
        }
        let (allocator, list) = (
            com(&self.commands[slot].allocator),
            com(&self.commands[slot].list),
        );
        if unsafe { native::reset_commands(allocator, list) } == 0 {
            return self.gpu_failure();
        }
        for (index, input) in inputs[..=EXPOSURE].iter().enumerate() {
            if !input.resource.is_null() {
                let (width, height) = extent(index, record);
                unsafe {
                    native::copy_region(context, shared11[index], input.resource, width, height)
                };
            }
        }
        for (index, &mask) in masks.iter().enumerate() {
            if !mask.is_null() {
                let (width, height) = (record.render_width, record.render_height);
                unsafe {
                    native::copy_region(context, shared11[MASKS + index], mask, width, height)
                };
            }
        }
        if !self.signal(SignalSource::Context) {
            return self.gpu_failure();
        }
        // Required: the D3D12 queue waits on this signal, which has to reach the GPU first.
        self.flush_signal();
        if unsafe { native::queue_wait(queue, fence12, self.tick) } == 0 {
            return self.gpu_failure();
        }

        let mut translated = *frame;
        let targets = [
            &mut translated.color,
            &mut translated.depth,
            &mut translated.motion,
            &mut translated.exposure,
        ];
        for (index, target) in targets.into_iter().enumerate() {
            if !inputs[index].resource.is_null() {
                target.resource = shared12[index];
                target.state = STATE_SHADER_RESOURCE;
                unsafe {
                    native::transition(list, shared12[index], STATE_COMMON, STATE_SHADER_RESOURCE)
                };
            }
        }
        let mask_targets = [&mut translated.reactive, &mut translated.transparency];
        for (index, target) in mask_targets.into_iter().enumerate() {
            if masks[index].is_null() {
                *target = unsafe { std::mem::zeroed() };
                continue;
            }
            let shared = shared12[MASKS + index];
            target.resource = shared;
            target.state = STATE_SHADER_RESOURCE;
            unsafe { native::transition(list, shared, STATE_COMMON, STATE_SHADER_RESOURCE) };
        }
        translated.output.resource = output;
        translated.output.state = STATE_UNORDERED_ACCESS;
        let result = unsafe { native::session_evaluate(self.session.get(), list, &translated) };
        for (index, &mask) in masks.iter().enumerate() {
            if !mask.is_null() {
                let shared = shared12[MASKS + index];
                unsafe { native::transition(list, shared, STATE_SHADER_RESOURCE, STATE_COMMON) };
            }
        }
        for (index, input) in inputs[..=EXPOSURE].iter().enumerate() {
            if !input.resource.is_null() {
                let shared = shared12[index];
                unsafe { native::transition(list, shared, STATE_SHADER_RESOURCE, STATE_COMMON) };
            }
        }
        if result == OK {
            unsafe {
                native::copy_transitioned(
                    list,
                    shared12[OUTPUT],
                    STATE_COMMON,
                    STATE_COMMON,
                    output,
                    STATE_UNORDERED_ACCESS,
                    STATE_UNORDERED_ACCESS,
                )
            };
        }
        if unsafe { native::submit(queue, list) } == 0 {
            return self.gpu_failure();
        }
        if !self.signal(SignalSource::Queue) {
            return self.gpu_failure();
        }
        let complete = self.tick;
        self.commands[slot].complete = complete;
        self.next_commands = (slot + 1) % COMMAND_SLOTS;
        // Even a refused evaluation may have recorded commands that reference shared inputs.
        if unsafe { native::context_wait(context, fence11, complete) } == 0 {
            return self.gpu_failure();
        }
        if result == OK {
            unsafe { native::copy_resource(context, frame.output.resource, shared11[OUTPUT]) };
            if !self.signal(SignalSource::Context) {
                return self.gpu_failure();
            }
            // Not flushed here: the game's Present submits it, and only a CPU wait needs it
            // sooner, which wait() covers.
        }
        if unsafe { native::dump_wants(frame.record) } != 0 {
            let backend = self.effective_backend();
            unsafe { native::dump_result(context_pointer, frame, backend, result, hdr) };
        }
        result
    }

    // One throwaway dispatch on the bridge's D3D12 queue with blank resources of its own, so no
    // D3D11 context is touched.
    fn warm_up(&mut self, render: (u32, u32), output: (u32, u32), manual_exposure: bool) -> i32 {
        if self.faulted {
            return NOT_READY;
        }
        // One pending warm-up at most.
        if !self.reap_warm_up(FENCE_TIMEOUT_MS) {
            return NOT_READY;
        }
        let device = com(&self.device12);
        let inputs = [
            (render.0, render.1, COLOR_FORMAT),
            (render.0, render.1, SCALAR_FORMAT),
            (render.0, render.1, MOTION_FORMAT),
            (1, 1, SCALAR_FORMAT),
        ];
        let count = if manual_exposure { 4 } else { 3 };
        let mut resources: [Option<ComRef>; 4] = Default::default();
        for (resource, &(width, height, format)) in resources.iter_mut().zip(&inputs).take(count) {
            *resource = owned_com(unsafe { native::create_texture(device, width, height, format) });
            if resource.is_none() {
                return INIT_FAILED;
            }
        }
        let target = unsafe { native::create_output(device, output.0, output.1, COLOR_FORMAT) };
        let Some(target) = owned_com(target) else {
            return INIT_FAILED;
        };
        let Some(commands) = create_commands(&self.device12) else {
            return INIT_FAILED;
        };
        let list = com(&commands.list);
        if unsafe { native::reset_commands(com(&commands.allocator), list) } == 0 {
            return INIT_FAILED;
        }
        let Some(fence) = owned_com(unsafe { native::create_fence(device) }) else {
            return INIT_FAILED;
        };
        let Some(event) = Owned::new(unsafe { native::create_event() }, native::close_event) else {
            return INIT_FAILED;
        };

        let mut record: rsf_frame_record = unsafe { std::mem::zeroed() };
        record.struct_size = size_of::<rsf_frame_record>() as u32;
        record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
        record.frame_id = 1;
        record.flags = RSF_FRAME_FLAG_RESET;
        (record.render_width, record.render_height) = render;
        (record.output_width, record.output_height) = output;
        record.frame_time_ms = 16.6;
        record.camera.struct_size = size_of_val(&record.camera) as u32;
        record.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
        record.camera.near_plane = 0.1;
        record.camera.far_plane = 10000.0;
        record.camera.vertical_fov_radians = 1.0;
        record.camera.depth_inverted = 1;
        let mut frame: rsf_sr_frame = unsafe { std::mem::zeroed() };
        frame.struct_size = size_of::<rsf_sr_frame>() as u32;
        frame.record = &record;
        frame.motion_scale_x = 1.0;
        frame.motion_scale_y = 1.0;
        frame.pre_exposure = 1.0;
        frame.view_space_to_meters = 1.0;
        frame.reset = 1;
        let slots = [
            &mut frame.color,
            &mut frame.depth,
            &mut frame.motion,
            &mut frame.exposure,
        ];
        for ((slot, resource), &(width, height, _)) in
            slots.into_iter().zip(&resources).zip(&inputs)
        {
            let Some(resource) = resource else { continue };
            slot.struct_size = size_of::<rsf_backend_resource>() as u32;
            slot.resource = com(resource);
            slot.width = width;
            slot.height = height;
            slot.state = STATE_SHADER_RESOURCE;
            unsafe { native::transition(list, com(resource), STATE_COMMON, STATE_SHADER_RESOURCE) };
        }
        frame.output.struct_size = size_of::<rsf_backend_resource>() as u32;
        frame.output.resource = com(&target);
        (frame.output.width, frame.output.height) = output;
        frame.output.state = STATE_UNORDERED_ACCESS;

        let started = Instant::now();
        let mut result = unsafe { native::session_warm_up(self.session.get(), list, &frame) };
        let queue = com(&self.queue);
        let mut pending = None;
        if unsafe { native::submit(queue, list) } != 0 {
            let signaled = unsafe { native::queue_signal(queue, com(&fence), 1) } != 0;
            let completed = signaled
                && unsafe { native::wait_fence(com(&fence), 1, event.get(), WARM_UP_TIMEOUT_MS) }
                    != 0;
            if !completed && result == OK {
                result = FEATURE_FAILED;
            }
            if !completed && unsafe { native::device_ready(device) } != 0 {
                pending = Some(PendingWarmUp {
                    _resources: resources,
                    _target: target,
                    _commands: commands,
                    fence,
                    event,
                    signaled,
                });
            }
        } else if result == OK {
            result = FEATURE_FAILED;
        }
        let elapsed = started.elapsed().as_millis();
        let state = if result == OK { "done" } else { "failed" };
        self.say(&format!("SR warm-up dispatch {state} in {elapsed} ms"));
        if pending.is_some() {
            // The submitted list may still reference these resources and the SDK context, and
            // the fence may still signal the event. Operations that would change or use the
            // context are refused until reap_warm_up observes completion or device removal.
            self.say("SR warm-up GPU completion not observed; keeping its resources pending");
            self.warm_up = pending;
        }
        result
    }

    fn retire(&mut self) -> bool {
        (self.wait() && self.reap_warm_up(FENCE_TIMEOUT_MS))
            || unsafe { native::device_ready(com(&self.device12)) } == 0
    }
}

fn create_commands(device12: &ComRef) -> Option<Commands> {
    let (mut allocator, mut list) = (ptr::null_mut(), ptr::null_mut());
    if unsafe { native::create_commands(com(device12), &mut allocator, &mut list) } == 0 {
        return None;
    }
    Some(Commands {
        allocator: owned_com(allocator)?,
        list: owned_com(list)?,
        complete: 0,
    })
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_sr_bridge_create(
    setup: *const rsf_sr_session_setup,
    out: *mut *mut Bridge,
) -> i32 {
    if out.is_null()
        || !unsafe { has_capacity(setup) && has_capacity(ptr::addr_of!((*setup).open)) }
    {
        return INVALID_ARGUMENT;
    }
    let setup = unsafe { &*setup };
    if setup.open.device.is_null() {
        return INVALID_ARGUMENT;
    }
    unsafe { out.write(ptr::null_mut()) };
    if setup.abi_version != RSF_SR_SESSION_ABI_VERSION
        || setup.open.abi_version != RSF_BACKEND_ABI_VERSION
    {
        return ABI_MISMATCH;
    }
    if setup.open.api != API_D3D11 {
        return WRONG_API;
    }
    let bridge = match Bridge::open(setup) {
        Ok(bridge) => bridge,
        Err(result) => return result,
    };
    let handle = crate::allocate_handle(bridge);
    if handle.is_null() {
        return INIT_FAILED;
    }
    unsafe { out.write(handle) };
    OK
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_sr_bridge_select(
    bridge: *mut Bridge,
    backend: u32,
    quality: u32,
    version_id: u64,
) -> i32 {
    match unsafe { bridge.as_mut() } {
        Some(bridge) => bridge.select(backend, quality, version_id),
        None => INVALID_ARGUMENT,
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_sr_bridge_set_auto_exposure(
    bridge: *mut Bridge,
    enabled: u32,
) -> i32 {
    match unsafe { bridge.as_mut() } {
        Some(bridge) if enabled <= 1 => bridge.set_auto_exposure(enabled),
        _ => INVALID_ARGUMENT,
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_sr_bridge_evaluate(
    bridge: *mut Bridge,
    context: *mut c_void,
    frame: *const rsf_sr_frame,
) -> i32 {
    match unsafe { bridge.as_mut() } {
        Some(bridge) if !context.is_null() => bridge.evaluate(context, frame),
        _ => INVALID_ARGUMENT,
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_sr_bridge_warm_up(
    bridge: *mut Bridge,
    render_width: u32,
    render_height: u32,
    output_width: u32,
    output_height: u32,
    manual_exposure: u32,
) -> i32 {
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return INVALID_ARGUMENT;
    };
    if render_width == 0
        || render_height == 0
        || render_width > output_width
        || render_height > output_height
    {
        return INVALID_ARGUMENT;
    }
    bridge.warm_up(
        (render_width, render_height),
        (output_width, output_height),
        manual_exposure != 0,
    )
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_sr_bridge_get_status(
    bridge: *const Bridge,
    status: *mut rsf_sr_session_status,
) -> i32 {
    match unsafe { bridge.as_ref() } {
        Some(bridge) => unsafe { native::session_get_status(bridge.session.get(), status) },
        None => INVALID_ARGUMENT,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_sr_bridge_close(bridge: *mut Bridge) -> i32 {
    let Some(owner) = (unsafe { bridge.as_mut() }) else {
        return INVALID_ARGUMENT;
    };
    if !owner.retire() {
        return NOT_READY;
    }
    drop(unsafe { Box::from_raw(bridge) });
    OK
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_sr_bridge_destroy(bridge: *mut Bridge) {
    if unsafe { rsf_sr_bridge_close(bridge) } == NOT_READY {
        // Legacy callers cannot observe retention. Preserve outstanding GPU resources.
        unsafe { &*bridge }.say("SR GPU completion failed; retaining the failed bridge resources");
    }
}

#[cfg(test)]
mod tests;
