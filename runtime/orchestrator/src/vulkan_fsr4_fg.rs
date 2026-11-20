// SPDX-License-Identifier: MIT
use crate::allocate_handle;
use crate::sr_session::has_capacity;
use crate::vulkan_fg_abi::{
    RSF_FGVK_ABI_VERSION, RSF_FGVK_FRAME_SLOTS, rsf_fgvk_frame as Frame,
    rsf_fgvk_frame_slot as Recording, rsf_fgvk_setup as Setup, rsf_fgvk_status as Status,
    rsf_fgvk_submit_plan as SubmitPlan, rsf_fgvk_timeline as RecordingTimeline,
};
use crate::vulkan_fg_frames::*;
use crate::vulkan_sr_abi::{
    RSF_SRVK_ABI_VERSION, rsf_srvk_image as Image, rsf_srvk_setup as Graphics,
};
use rsf_game_sys::{
    RSF_FRAME_FLAG_AMBIGUOUS_ID, RSF_FRAME_FLAG_NO_FG, RSF_GAME_CAMERA_ABI_VERSION,
    RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record as Record,
};
use std::ffi::{c_char, c_void};
use std::mem::offset_of;
use std::ptr::{self, addr_of};
use std::slice;

const INVALID: i32 = -13001;
const ABI_MISMATCH: i32 = -13002;
const UNSUPPORTED: i32 = -13003;
const NOT_READY: i32 = 1;
const INCOMPLETE: i32 = 5;
const OUT_OF_MEMORY: i32 = -1;
const INIT_FAILED: i32 = -3;
const FORMAT_UNSUPPORTED: i32 = -11;
const UNKNOWN: i32 = -13;
const INVALID_EXTERNAL_HANDLE: i32 = -1000072003;
const OUT_OF_DATE: i32 = -1000001004;
const RGBA8: u32 = 37;
const BGRA8: u32 = 44;
const RG16F: u32 = 83;
const R32F: u32 = 100;
const SHADER_READ: u32 = 5;
const SRGB_NONLINEAR: u32 = 0;
const EXCLUSIVE: u32 = 0;
const BRIDGE_ABI: u32 = 1;
const WAIT_NS: u64 = 10_000_000_000;
const BUFFER_COUNT: usize = 3;
const SLOT_COUNT: usize = RSF_FGVK_FRAME_SLOTS as usize;

type Handle = *mut c_void;
type Release = unsafe extern "C" fn(Handle) -> i32;

#[repr(C)]
struct Options {
    struct_size: u32,
    abi_version: u32,
    window: Handle,
    timeline_enabled: u32,
    generated_count: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Swapchain {
    width: u32,
    height: u32,
    format: u32,
    usage: u32,
    color_space: u32,
    layers: u32,
    sharing_mode: u32,
    min_images: u32,
    flags: u32,
    compatible_extensions: u32,
    vsync: u32,
    old_chain: u64,
}

impl Swapchain {
    fn supported(self) -> bool {
        self.old_chain == 0
            && self.layers == 1
            && self.sharing_mode == EXCLUSIVE
            && self.width != 0
            && self.height != 0
            && self.color_space == SRGB_NONLINEAR
            && matches!(self.format, RGBA8 | BGRA8)
            && self.compatible_extensions != 0
            && self.flags == 0
            && self.min_images <= BUFFER_COUNT as u32
    }
}

#[repr(C)]
struct PresentInfo {
    chain: u64,
    index: u32,
    wait_count: u32,
    waits: *const u64,
    result: *mut i32,
}

#[repr(C)]
struct BridgeCreate {
    struct_size: u32,
    abi_version: u32,
    adapter_luid: u64,
    runtime_directory: *const u16,
    width: u32,
    height: u32,
    depth_inverted: u32,
    depth_infinite: u32,
    motion_jittered: u32,
    generated_count: u32,
}

#[repr(C)]
struct BridgeInfo {
    struct_size: u32,
    abi_version: u32,
    adapter_luid: u64,
    provider_version: u64,
    vendor_id: u32,
    max_generated: u32,
}

#[repr(C)]
struct BridgeFrame {
    struct_size: u32,
    abi_version: u32,
    record: *const Record,
    color: Handle,
    depth: Handle,
    motion: Handle,
    hudless: Handle,
    motion_scale_x: f32,
    motion_scale_y: f32,
    ready_value: u64,
    done_value: u64,
    enabled: u32,
    vsync: u32,
}

#[repr(C)]
struct BridgeStatus {
    struct_size: u32,
    abi_version: u32,
    frames_presented: u32,
    fg_enabled: u32,
    result: i32,
    terminal: u32,
    generated_callbacks: u64,
}

// The importer owns the Vulkan allocation and the COM reference together.
// The Rust slot leases both until the importer's checked destruction.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SharedImage {
    image: u64,
    resource: Handle,
    format: u32,
    width: u32,
    height: u32,
}

#[repr(C)]
struct Ops {
    adapter_luid: unsafe extern "C" fn(*const Graphics, *mut u64) -> i32,
    directory: unsafe extern "C" fn(*const c_char, *mut u16, i32) -> i32,
    probe: unsafe extern "C" fn(u64, *const u16, *mut BridgeInfo) -> i32,
    bridge_create: unsafe extern "C" fn(*const BridgeCreate, *mut Handle) -> i32,
    bridge_info: unsafe extern "C" fn(Handle, *mut BridgeInfo) -> i32,
    bridge_start: unsafe extern "C" fn(Handle, Handle) -> i32,
    bridge_present: unsafe extern "C" fn(Handle, *const BridgeFrame, *mut BridgeStatus) -> i32,
    bridge_drain: Release,
    bridge_prepare_destroy: Release,
    bridge_destroy: Release,
    interop_create: unsafe extern "C" fn(*const Graphics, Handle, u64, u32, *mut Handle) -> i32,
    interop_image: unsafe extern "C" fn(Handle, u32, u32, u32, *mut SharedImage) -> i32,
    interop_timeline: unsafe extern "C" fn(Handle) -> u64,
    interop_destroy: unsafe extern "C" fn(Handle, u32) -> i32,
    platform_create: unsafe extern "C" fn(*const Setup, *mut Handle) -> i32,
    platform_destroy: unsafe extern "C" fn(Handle),
    create_pool: unsafe extern "C" fn(Handle, *mut u64) -> i32,
    destroy_pool: unsafe extern "C" fn(Handle, u64),
    create_image: unsafe extern "C" fn(Handle, *const Swapchain, *mut u64, *mut u64) -> i32,
    destroy_image: unsafe extern "C" fn(Handle, u64, u64),
    allocate_commands: unsafe extern "C" fn(Handle, u64, *mut Handle) -> i32,
    begin: unsafe extern "C" fn(Handle, Handle) -> i32,
    end: unsafe extern "C" fn(Handle, Handle) -> i32,
    copy: unsafe extern "C" fn(Handle, Handle, *const Image, *const SharedImage, u32),
    clear: unsafe extern "C" fn(Handle, Handle, *const SharedImage),
    acquire_submit: unsafe extern "C" fn(Handle, u64, u64) -> i32,
    present_submit: unsafe extern "C" fn(Handle, Handle, *const PresentInfo, u64, u64, u64) -> i32,
    wait_timeline: unsafe extern "C" fn(Handle, u64, u64, u64) -> i32,
    create_fence: unsafe extern "C" fn(Handle, *mut u64) -> i32,
    mark_submit: unsafe extern "C" fn(Handle, u64) -> i32,
    wait_fence: unsafe extern "C" fn(Handle, u64, u64) -> i32,
    destroy_fence: unsafe extern "C" fn(Handle, u64),
    idle: Release,
    capture_provider: unsafe extern "C" fn(Handle, u64),
    capture_record:
        unsafe extern "C" fn(Handle, Handle, *const Image, *const Record, *const c_char, u64),
    capture_complete: unsafe extern "C" fn(Handle, u64),
    capture_discard: unsafe extern "C" fn(Handle, u64),
    capture_flush: Release,
}

const _: () = {
    assert!(size_of::<Swapchain>() == 56);
    assert!(size_of::<PresentInfo>() == 32);
    assert!(size_of::<Ops>() == 38 * size_of::<Handle>());
    assert!(size_of::<Options>() == 24);
    assert!(size_of::<BridgeCreate>() == 48);
    assert!(size_of::<BridgeInfo>() == 32);
    assert!(size_of::<BridgeFrame>() == 80);
    assert!(size_of::<BridgeStatus>() == 32);
    assert!(size_of::<SharedImage>() == 32);
};

fn succeeded(result: i32) -> Result<(), i32> {
    if result == 0 { Ok(()) } else { Err(result) }
}

#[derive(Default)]
struct Buffer {
    image: u64,
    memory: u64,
    commands: Handle,
    shared: SharedImage,
    done: u64,
    acquired: bool,
    initialized: bool,
}

#[derive(Default)]
struct SourceFence {
    handle: u64,
    stages: u32,
}

struct Slot {
    state: Recording,
    depth: SharedImage,
    motion: SharedImage,
    hudless: SharedImage,
    fences: [SourceFence; 2],
    done: u64,
    scales: [f32; 2],
    guides_initialized: bool,
    hudless_initialized: bool,
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            state: unsafe { std::mem::zeroed() },
            depth: SharedImage::default(),
            motion: SharedImage::default(),
            hudless: SharedImage::default(),
            fences: std::array::from_fn(|_| SourceFence::default()),
            done: 0,
            scales: [0.0; 2],
            guides_initialized: false,
            hudless_initialized: false,
        }
    }
}

const GUIDES: u32 = 1;
const HUDLESS: u32 = 2;
fn capture_group(slot: &Recording, stage: u32) -> u64 {
    (slot.recording_sequence << 1) | u64::from(stage == HUDLESS)
}

fn slot_index(frame: u64) -> usize {
    (frame % SLOT_COUNT as u64) as usize
}

// Calls are serialized by the Vulkan WSI contract. The shared primary owner
// in vulkan_fg.rs and the presentation borrow gate remain the entry point.
struct Owner {
    ops: &'static Ops,
    vk: Handle,
    device: Handle,
    depth_inverted: u32,
    bridge: Handle,
    interop: Handle,
    pool: u64,
    timeline: u64,
    chain: u64,
    swapchain: Swapchain,
    buffers: [Buffer; BUFFER_COUNT],
    next: usize,
    slots: [Slot; SLOT_COUNT],
    recording: RecordingTimeline,
    fallback_depth: SharedImage,
    fallback_motion: SharedImage,
    fallback_initialized: bool,
    last_done: u64,
    requested: bool,
    terminal: bool,
    retiring: bool,
    status: Status,
}

impl Owner {
    fn new(ops: &'static Ops, setup: &Setup, swapchain: Swapchain) -> Self {
        Self {
            ops,
            vk: ptr::null_mut(),
            device: setup.graphics.device,
            depth_inverted: setup.graphics.depth_inverted,
            bridge: ptr::null_mut(),
            interop: ptr::null_mut(),
            pool: 0,
            timeline: 0,
            chain: 0,
            swapchain,
            buffers: std::array::from_fn(|_| Buffer::default()),
            next: 0,
            slots: std::array::from_fn(|_| Slot::default()),
            recording: unsafe { std::mem::zeroed() },
            fallback_depth: SharedImage::default(),
            fallback_motion: SharedImage::default(),
            fallback_initialized: false,
            last_done: 0,
            requested: false,
            terminal: false,
            retiring: false,
            status: Status {
                struct_size: size_of::<Status>() as u32,
                abi_version: RSF_FGVK_ABI_VERSION,
                enabled: 0,
                available: 0,
                result: 0,
                prepared: 0,
                tagged: 0,
                generated_callbacks: 0,
                last_present_count: 0,
                source_frame: 0,
            },
        }
    }

    fn initialize(
        &mut self,
        setup: &Setup,
        options: &Options,
        directory: &[u16],
    ) -> Result<(), i32> {
        let ops = self.ops;
        let graphics = &setup.graphics;
        let mut luid = 0;
        succeeded(unsafe { (ops.adapter_luid)(graphics, &mut luid) })?;
        succeeded(unsafe { (ops.platform_create)(setup, &mut self.vk) })?;
        let create = BridgeCreate {
            struct_size: size_of::<BridgeCreate>() as u32,
            abi_version: BRIDGE_ABI,
            adapter_luid: luid,
            runtime_directory: directory.as_ptr(),
            width: self.swapchain.width,
            height: self.swapchain.height,
            depth_inverted: graphics.depth_inverted,
            depth_infinite: graphics.depth_infinite,
            motion_jittered: graphics.motion_jittered,
            generated_count: options.generated_count,
        };
        succeeded(unsafe { (ops.bridge_create)(&create, &mut self.bridge) })?;
        let mut provider = BridgeInfo {
            struct_size: size_of::<BridgeInfo>() as u32,
            abi_version: BRIDGE_ABI,
            adapter_luid: 0,
            provider_version: 0,
            vendor_id: 0,
            max_generated: 0,
        };
        succeeded(unsafe { (ops.bridge_info)(self.bridge, &mut provider) })?;
        if provider.adapter_luid != luid {
            return Err(INVALID_EXTERNAL_HANDLE);
        }
        if provider.provider_version == 0 || provider.max_generated != 1 {
            return Err(UNSUPPORTED);
        }
        succeeded(unsafe {
            (ops.interop_create)(
                graphics,
                self.bridge,
                luid,
                options.timeline_enabled,
                &mut self.interop,
            )
        })?;
        unsafe { (ops.capture_provider)(self.vk, provider.provider_version) };
        self.timeline = unsafe { (ops.interop_timeline)(self.interop) };
        if self.timeline == 0 {
            return Err(INVALID_EXTERNAL_HANDLE);
        }
        succeeded(unsafe { (ops.create_pool)(self.vk, &mut self.pool) })?;
        for buffer in &mut self.buffers {
            succeeded(unsafe {
                (ops.create_image)(
                    self.vk,
                    &self.swapchain,
                    &mut buffer.image,
                    &mut buffer.memory,
                )
            })?;
            succeeded(unsafe {
                (ops.interop_image)(
                    self.interop,
                    BGRA8,
                    self.swapchain.width,
                    self.swapchain.height,
                    &mut buffer.shared,
                )
            })?;
            succeeded(unsafe {
                (ops.allocate_commands)(self.vk, self.pool, &mut buffer.commands)
            })?;
        }
        succeeded(unsafe {
            (ops.interop_image)(self.interop, R32F, 1, 1, &mut self.fallback_depth)
        })?;
        succeeded(unsafe {
            (ops.interop_image)(self.interop, RG16F, 1, 1, &mut self.fallback_motion)
        })?;
        succeeded(unsafe { (ops.bridge_start)(self.bridge, options.window) })?;
        self.status.available = 1;
        Ok(())
    }

    fn open(&self) -> bool {
        self.status.available != 0 && !self.terminal && !self.retiring
    }

    fn wait_timeline(&self, value: u64, timeout: u64) -> Result<(), i32> {
        if value == 0 {
            return Ok(());
        }
        succeeded(unsafe { (self.ops.wait_timeline)(self.vk, self.timeline, value, timeout) })
    }

    fn complete_source(ops: &Ops, vk: Handle, record: &Recording, fence: &mut SourceFence) {
        for stage in [GUIDES, HUDLESS] {
            if fence.stages & stage != 0 {
                unsafe { (ops.capture_complete)(vk, capture_group(record, stage)) }
            }
        }
        fence.stages = 0;
    }

    fn wait_sources(ops: &Ops, vk: Handle, slot: &mut Slot) -> Result<(), i32> {
        for fence in &mut slot.fences {
            if fence.stages != 0 {
                succeeded(unsafe { (ops.wait_fence)(vk, fence.handle, WAIT_NS) })?;
                Self::complete_source(ops, vk, &slot.state, fence);
            }
        }
        Ok(())
    }

    fn frame_valid(&self, frame: &Frame, record: &Record) -> bool {
        let camera = &record.camera;
        record.abi_version == RSF_GAME_FRAME_ABI_VERSION
            && record.frame_id != 0
            && record.session_id != 0
            && record.render_width != 0
            && record.render_height != 0
            && record.output_width == self.swapchain.width
            && record.output_height == self.swapchain.height
            && camera.struct_size as usize >= size_of_val(camera)
            && camera.abi_version == RSF_GAME_CAMERA_ABI_VERSION
            && camera.motion_jittered == 0
            && camera.depth_inverted == self.depth_inverted
            && [frame.motion_scale_x, frame.motion_scale_y]
                .into_iter()
                .all(|scale| scale.is_finite() && scale != 0.0)
            && camera
                .world_to_view
                .iter()
                .chain(&camera.view_to_clip)
                .chain(&camera.jitter_pixels)
                .all(|value| value.is_finite())
    }

    fn image_valid(&self, image: &Image, format: u32, width: u32, height: u32) -> bool {
        image.device == self.device
            && image.image != 0
            && image.copy_source != 0
            && image.format == format
            && image.layout == SHADER_READ
            && image.width == width
            && image.height == height
    }

    fn acquire(&mut self, timeout: u64, semaphore: u64, fence: u64) -> Result<u32, i32> {
        if !self.open() || (semaphore == 0 && fence == 0) {
            return Err(INIT_FAILED);
        }
        let index = self.next;
        let buffer = &self.buffers[index];
        if buffer.acquired {
            return Err(NOT_READY);
        }
        self.wait_timeline(buffer.done, timeout)?;
        succeeded(unsafe { (self.ops.acquire_submit)(self.vk, semaphore, fence) })?;
        self.buffers[index].acquired = true;
        self.next = (index + 1) % BUFFER_COUNT;
        Ok(index as u32)
    }

    fn prepare(&mut self, commands: Handle, frame: &Frame, record: &Record) -> Result<(), i32> {
        let (width, height) = (record.render_width, record.render_height);
        if !self.image_valid(&frame.depth, R32F, width, height)
            || !self.image_valid(&frame.motion, RG16F, width, height)
        {
            return Err(INVALID);
        }
        let index = slot_index(record.frame_id);
        let slot = &self.slots[index];
        if rsf_core_fgvk_can_prepare(&slot.state) == 0 {
            return Err(NOT_READY);
        }
        self.wait_timeline(slot.done, WAIT_NS)?;
        let slot = &mut self.slots[index];
        Self::wait_sources(self.ops, self.vk, slot)?;
        if slot.depth.image != 0 && (slot.depth.width != width || slot.depth.height != height) {
            return Err(OUT_OF_DATE);
        }
        let ops = self.ops;
        for (target, format, width, height) in [
            (&mut slot.depth, R32F, width, height),
            (&mut slot.motion, RG16F, width, height),
            (
                &mut slot.hudless,
                BGRA8,
                self.swapchain.width,
                self.swapchain.height,
            ),
        ] {
            if target.image == 0 {
                succeeded(unsafe {
                    (ops.interop_image)(self.interop, format, width, height, target)
                })?;
            }
        }
        for fence in &mut slot.fences {
            if fence.handle == 0 {
                succeeded(unsafe { (ops.create_fence)(self.vk, &mut fence.handle) })?;
            }
        }
        slot.done = 0;
        slot.scales = [frame.motion_scale_x, frame.motion_scale_y];
        rsf_core_fgvk_prepare_begin(&mut self.recording, &mut slot.state, record, commands);
        for (source, target, role) in [
            (&frame.depth, &slot.depth, c"fg-depth"),
            (&frame.motion, &slot.motion, c"fg-motion-uv"),
        ] {
            unsafe {
                (ops.copy)(
                    self.vk,
                    commands,
                    source,
                    target,
                    u32::from(slot.guides_initialized),
                );
                (ops.capture_record)(
                    self.vk,
                    commands,
                    source,
                    &slot.state.record,
                    role.as_ptr(),
                    capture_group(&slot.state, GUIDES),
                );
            }
        }
        slot.state.recorded_prepare = 1;
        slot.state.prepared = 1;
        rsf_core_fgvk_prepare_accepted(&mut self.recording, &mut slot.state);
        self.status.prepared += 1;
        Ok(())
    }

    fn hudless(&mut self, commands: Handle, frame: &Frame, record: &Record) -> Result<(), i32> {
        if !self.image_valid(
            &frame.hudless,
            RGBA8,
            self.swapchain.width,
            self.swapchain.height,
        ) {
            return Err(INVALID);
        }
        let slot = &mut self.slots[slot_index(record.frame_id)];
        if slot.state.record.render_width != record.render_width
            || slot.state.record.render_height != record.render_height
            || rsf_core_fgvk_hudless_begin(&mut slot.state, record, commands) == 0
        {
            return Err(INVALID);
        }
        unsafe {
            (self.ops.copy)(
                self.vk,
                commands,
                &frame.hudless,
                &slot.hudless,
                u32::from(slot.hudless_initialized),
            );
            (self.ops.capture_record)(
                self.vk,
                commands,
                &frame.hudless,
                &slot.state.record,
                c"fg-hudless-color".as_ptr(),
                capture_group(&slot.state, HUDLESS),
            );
        }
        slot.state.recorded_hudless = 1;
        slot.state.hudless = 1;
        self.status.tagged += 1;
        Ok(())
    }

    fn submitted(&mut self, commands: &[*mut c_void]) {
        let slots: [*mut Recording; SLOT_COUNT] =
            std::array::from_fn(|i| &raw mut self.slots[i].state);
        let mut plan = SubmitPlan {
            fences: 0,
            discarded_prepare: 0,
            discarded_hudless: 0,
        };
        unsafe {
            rsf_core_fgvk_submitted(
                slots.as_ptr(),
                commands.as_ptr(),
                commands.len() as u32,
                &mut plan,
            )
        };
        for (index, slot) in self.slots.iter_mut().enumerate() {
            let bit = 1 << index;
            for (discarded, stage) in [
                (plan.discarded_prepare, GUIDES),
                (plan.discarded_hudless, HUDLESS),
            ] {
                if discarded & bit != 0 {
                    unsafe {
                        (self.ops.capture_discard)(self.vk, capture_group(&slot.state, stage))
                    };
                }
            }
            if plan.fences & bit == 0 {
                continue;
            }
            // A joint notification uses one marker. Split notifications use
            // separate fences so neither has to wait in the submission callback.
            let submitted =
                (slot.state.submitted_prepare * GUIDES) | (slot.state.submitted_hudless * HUDLESS);
            let pending = slot.fences[0].stages | slot.fences[1].stages;
            let fence = if slot.fences[0].stages == 0 {
                &mut slot.fences[0]
            } else {
                &mut slot.fences[1]
            };
            let result = unsafe { (self.ops.mark_submit)(self.vk, fence.handle) };
            if result != 0 {
                slot.state.completion_unknown = 1;
                self.terminal = true;
                self.status.result = result;
                return;
            }
            fence.stages = submitted & !pending;
            slot.guides_initialized |= slot.state.submitted_prepare != 0;
            slot.hudless_initialized |= slot.state.submitted_hudless != 0;
        }
    }

    fn present(&mut self, info: &PresentInfo, source: Source) -> Result<(), i32> {
        if !self.open()
            || info.chain != self.chain
            || info.wait_count > 16
            || (info.wait_count != 0 && info.waits.is_null())
        {
            return Err(INIT_FAILED);
        }
        let index = info.index as usize;
        if !self
            .buffers
            .get(index)
            .is_some_and(|buffer| buffer.acquired)
        {
            return Err(INIT_FAILED);
        }
        let source_frame = source.frame(self.recording.last_prepared.frame_id);
        let slot_index = slot_index(source_frame);
        let slot = &self.slots[slot_index];
        let identity = source.matches(&slot.state.record);
        let valid = slot.state.record.flags & (RSF_FRAME_FLAG_NO_FG | RSF_FRAME_FLAG_AMBIGUOUS_ID)
            == 0
            && rsf_core_fgvk_present_candidate(
                &self.recording,
                &slot.state,
                source_frame,
                u32::from(identity),
                u32::from(self.requested),
            ) != 0;

        let ops = self.ops;
        let buffer = &mut self.buffers[index];
        let source_image = Image {
            device: self.device,
            image: buffer.image,
            format: self.swapchain.format,
            width: self.swapchain.width,
            height: self.swapchain.height,
            layout: SHADER_READ,
            copy_source: 1,
        };
        succeeded(unsafe { (ops.begin)(self.vk, buffer.commands) })?;
        if !self.fallback_initialized {
            for image in [&self.fallback_depth, &self.fallback_motion] {
                unsafe { (ops.clear)(self.vk, buffer.commands, image) };
            }
        }
        unsafe {
            (ops.copy)(
                self.vk,
                buffer.commands,
                &source_image,
                &buffer.shared,
                u32::from(buffer.initialized),
            );
        }
        succeeded(unsafe { (ops.end)(self.vk, buffer.commands) })?;

        // The SDK queue signals done after restoring HUD-less ownership.
        // A later ready signal must never bypass the prior done, across slots.
        let done = self.last_done.checked_add(2).ok_or(UNKNOWN)?;
        let ready = done - 1;
        let result = unsafe {
            (ops.present_submit)(
                self.vk,
                buffer.commands,
                info,
                self.timeline,
                self.last_done,
                ready,
            )
        };
        if result != 0 {
            self.terminal = true;
            return Err(result);
        }
        self.fallback_initialized = true;
        buffer.initialized = true;
        buffer.done = done;
        self.last_done = done;

        let slot = &mut self.slots[slot_index];
        if valid {
            slot.done = done;
        }
        let (depth, motion, hudless, scales) = if valid {
            (
                slot.depth.resource,
                slot.motion.resource,
                slot.hudless.resource,
                slot.scales,
            )
        } else {
            // The SDK receives these placeholders only in its disabled path.
            (
                self.fallback_depth.resource,
                self.fallback_motion.resource,
                buffer.shared.resource,
                [1.0; 2],
            )
        };
        let frame = BridgeFrame {
            struct_size: size_of::<BridgeFrame>() as u32,
            abi_version: BRIDGE_ABI,
            record: if identity && source_frame != 0 && slot.state.record.frame_id == source_frame {
                &slot.state.record
            } else {
                ptr::null()
            },
            color: buffer.shared.resource,
            depth,
            motion,
            hudless,
            motion_scale_x: scales[0],
            motion_scale_y: scales[1],
            ready_value: ready,
            done_value: done,
            enabled: u32::from(valid),
            vsync: self.swapchain.vsync,
        };
        let mut status = BridgeStatus {
            struct_size: size_of::<BridgeStatus>() as u32,
            abi_version: BRIDGE_ABI,
            frames_presented: 0,
            fg_enabled: 0,
            result: 0,
            terminal: 0,
            generated_callbacks: 0,
        };
        let result = unsafe { (ops.bridge_present)(self.bridge, &frame, &mut status) };
        self.terminal = status.terminal != 0;
        buffer.acquired = false;
        rsf_core_fgvk_presented(
            &mut self.recording,
            &slot.state,
            source_frame,
            u32::from(identity),
        );
        self.status.source_frame = if matches!(source, Source::Implicit) || valid {
            source_frame
        } else {
            0
        };
        self.status.last_present_count = u64::from(status.frames_presented);
        self.status.enabled = status.fg_enabled;
        self.status.result = if result != 0 { result } else { status.result };
        self.status.generated_callbacks = status.generated_callbacks;
        let wsi_result = if result != 0 { UNKNOWN } else { 0 };
        if !info.result.is_null() {
            unsafe { info.result.write(wsi_result) };
        }
        succeeded(wsi_result)
    }

    fn drain(&mut self) -> Result<(), i32> {
        if self.retiring {
            return Ok(());
        }
        if self.terminal
            || self
                .slots
                .iter()
                .any(|slot| rsf_core_fgvk_unsubmitted(&slot.state) != 0)
        {
            return Err(NOT_READY);
        }
        if self.vk.is_null() {
            return Ok(());
        }
        self.wait_timeline(self.last_done, WAIT_NS)?;
        succeeded(unsafe { (self.ops.idle)(self.vk) })?;
        if !self.bridge.is_null() {
            succeeded(unsafe { (self.ops.bridge_drain)(self.bridge) })?;
        }
        for slot in &mut self.slots {
            for fence in &mut slot.fences {
                Self::complete_source(self.ops, self.vk, &slot.state, fence);
            }
        }
        succeeded(unsafe { (self.ops.capture_flush)(self.vk) })
    }

    fn retire(&mut self) -> Result<(), i32> {
        self.drain()?;
        self.retiring = true;
        let ops = self.ops;
        if !self.bridge.is_null() {
            succeeded(unsafe { (ops.bridge_prepare_destroy)(self.bridge) })?;
        }
        if self.pool != 0 {
            unsafe { (ops.destroy_pool)(self.vk, std::mem::take(&mut self.pool)) };
        }
        for buffer in &mut self.buffers {
            if buffer.image != 0 || buffer.memory != 0 {
                unsafe {
                    (ops.destroy_image)(
                        self.vk,
                        std::mem::take(&mut buffer.image),
                        std::mem::take(&mut buffer.memory),
                    )
                };
            }
        }
        for slot in &mut self.slots {
            for fence in &mut slot.fences {
                if fence.handle != 0 {
                    unsafe { (ops.destroy_fence)(self.vk, std::mem::take(&mut fence.handle)) };
                }
            }
        }
        if !self.interop.is_null() {
            succeeded(unsafe { (ops.interop_destroy)(self.interop, 1) })?;
            self.interop = ptr::null_mut();
        }
        if !self.bridge.is_null() {
            succeeded(unsafe { (ops.bridge_destroy)(self.bridge) })?;
            self.bridge = ptr::null_mut();
        }
        if !self.vk.is_null() {
            unsafe { (ops.platform_destroy)(std::mem::replace(&mut self.vk, ptr::null_mut())) };
        }
        Ok(())
    }
}

struct Identity {
    frame: u64,
    session: u64,
    view: u32,
    generation: u32,
}

enum Source {
    Implicit,
    Explicit(Option<Identity>),
}

impl Source {
    unsafe fn read(record: *const Record, explicit: u32) -> Result<Self, i32> {
        if explicit == 0 {
            return Ok(Self::Implicit);
        }
        if record.is_null() {
            return Ok(Self::Explicit(None));
        }
        // The identity can be a short prefix. Do not borrow a full Record.
        if (unsafe { addr_of!((*record).struct_size).read() } as usize) < offset_of!(Record, phase)
            || unsafe { addr_of!((*record).abi_version).read() } != RSF_GAME_FRAME_ABI_VERSION
        {
            return Err(INIT_FAILED);
        }
        Ok(Self::Explicit(Some(Identity {
            frame: unsafe { addr_of!((*record).frame_id).read() },
            session: unsafe { addr_of!((*record).session_id).read() },
            view: unsafe { addr_of!((*record).view_id).read() },
            generation: unsafe { addr_of!((*record).resource_generation).read() },
        })))
    }

    fn frame(&self, latest: u64) -> u64 {
        match self {
            Self::Implicit => latest,
            Self::Explicit(identity) => identity.as_ref().map_or(0, |identity| identity.frame),
        }
    }

    fn matches(&self, record: &Record) -> bool {
        match self {
            Self::Implicit => true,
            Self::Explicit(identity) => identity.as_ref().is_some_and(|identity| {
                identity.frame != 0
                    && identity.session != 0
                    && identity.frame == record.frame_id
                    && identity.session == record.session_id
                    && identity.view == record.view_id
                    && identity.generation == record.resource_generation
                    && record.flags & (RSF_FRAME_FLAG_NO_FG | RSF_FRAME_FLAG_AMBIGUOUS_ID) == 0
            }),
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4gvk_probe(
    ops: &'static Ops,
    setup: *const Graphics,
    info: *mut BridgeInfo,
) -> i32 {
    if !unsafe { has_capacity(setup) } || info.is_null() {
        return INVALID;
    }
    let setup = unsafe { &*setup };
    if setup.instance.is_null()
        || setup.physical_device.is_null()
        || setup.instance_proc.is_none()
        || setup.runtime_directory_utf8.is_null()
    {
        return INVALID;
    }
    if setup.abi_version != RSF_SRVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let mut directory = [0u16; 32768];
    if unsafe { (ops.directory)(setup.runtime_directory_utf8, directory.as_mut_ptr(), 32768) } <= 1
    {
        return INVALID;
    }
    let mut luid = 0;
    let result = unsafe { (ops.adapter_luid)(setup, &mut luid) };
    if result != 0 {
        return result;
    }
    unsafe { (ops.probe)(luid, directory.as_ptr(), info) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4gvk_create(
    ops: &'static Ops,
    setup: *const Setup,
    options: *const Options,
    swapchain: *const Swapchain,
    output: *mut *mut Owner,
    chain: *mut u64,
) -> i32 {
    if output.is_null() || chain.is_null() {
        return INVALID;
    }
    unsafe {
        output.write(ptr::null_mut());
        chain.write(0);
    }
    if !unsafe { has_capacity(setup) && has_capacity(options) } {
        return INVALID;
    }
    let (setup, options) = unsafe { (&*setup, &*options) };
    if setup.abi_version != RSF_FGVK_ABI_VERSION || options.abi_version != BRIDGE_ABI {
        return ABI_MISMATCH;
    }
    let graphics = &setup.graphics;
    let Some(&swapchain) = (unsafe { swapchain.as_ref() }) else {
        return INVALID;
    };
    if graphics.instance_proc.is_none()
        || graphics.device_proc.is_none()
        || graphics.device.is_null()
        || graphics.physical_device.is_null()
        || setup.graphics_submit.is_none()
        || setup.lock_queues.is_none()
        || setup.unlock_queues.is_none()
        || options.window.is_null()
        || options.timeline_enabled != 1
        || options.generated_count != 1
        || graphics.runtime_directory_utf8.is_null()
        || graphics.motion_jittered != 0
    {
        return INVALID;
    }
    if !swapchain.supported() {
        return FORMAT_UNSUPPORTED;
    }
    let mut directory = [0u16; 32768];
    if unsafe {
        (ops.directory)(
            graphics.runtime_directory_utf8,
            directory.as_mut_ptr(),
            32768,
        )
    } <= 1
    {
        return INVALID;
    }
    let owner = allocate_handle(Owner::new(ops, setup, swapchain));
    if owner.is_null() {
        return OUT_OF_MEMORY;
    }
    let state = unsafe { &mut *owner };
    if let Err(result) = state.initialize(setup, options, &directory) {
        if unsafe { rsf_core_fsr4gvk_destroy(owner) } != 0 {
            unsafe { output.write(owner) };
        }
        return result;
    }
    let identity = owner.addr() as u64;
    state.chain = identity;
    unsafe {
        output.write(owner);
        chain.write(identity);
    }
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4gvk_images(
    owner: *const Owner,
    count: *mut u32,
    output: *mut u64,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INIT_FAILED;
    };
    if count.is_null() || owner.status.available == 0 || owner.retiring {
        return INIT_FAILED;
    }
    if output.is_null() {
        unsafe { count.write(BUFFER_COUNT as u32) };
        return 0;
    }
    let copied = (unsafe { count.read() } as usize).min(BUFFER_COUNT);
    for (i, buffer) in owner.buffers[..copied].iter().enumerate() {
        unsafe { output.add(i).write(buffer.image) };
    }
    unsafe { count.write(copied as u32) };
    if copied == BUFFER_COUNT {
        0
    } else {
        INCOMPLETE
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4gvk_acquire(
    owner: *mut Owner,
    timeout: u64,
    semaphore: u64,
    fence: u64,
    index: *mut u32,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_mut() }) else {
        return INIT_FAILED;
    };
    if index.is_null() {
        return INIT_FAILED;
    }
    match owner.acquire(timeout, semaphore, fence) {
        Ok(value) => {
            unsafe { index.write(value) };
            0
        }
        Err(result) => result,
    }
}

unsafe fn with_frame(
    owner: *mut Owner,
    commands: Handle,
    frame: *const Frame,
    record_stage: fn(&mut Owner, Handle, &Frame, &Record) -> Result<(), i32>,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_mut() }) else {
        return INVALID;
    };
    if !owner.open() || commands.is_null() || !unsafe { has_capacity(frame) } {
        return INVALID;
    }
    let frame = unsafe { &*frame };
    if frame.abi_version != RSF_FGVK_ABI_VERSION || !unsafe { has_capacity(frame.record) } {
        return INVALID;
    }
    let record = unsafe { &*frame.record };
    if !owner.frame_valid(frame, record) {
        return INVALID;
    }
    record_stage(owner, commands, frame, record)
        .err()
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4gvk_prepare(
    owner: *mut Owner,
    commands: Handle,
    frame: *const Frame,
) -> i32 {
    unsafe { with_frame(owner, commands, frame, Owner::prepare) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4gvk_hudless(
    owner: *mut Owner,
    commands: Handle,
    frame: *const Frame,
) -> i32 {
    unsafe { with_frame(owner, commands, frame, Owner::hudless) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4gvk_submitted(
    owner: *mut Owner,
    commands: *const Handle,
    count: u32,
) {
    if let Some(owner) = unsafe { owner.as_mut() }
        && owner.open()
        && !commands.is_null()
    {
        owner.submitted(unsafe { slice::from_raw_parts(commands, count as usize) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4gvk_enable_options(
    owner: *mut Owner,
    enabled: u32,
    count: u32,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_mut() }) else {
        return INVALID;
    };
    if !owner.open() || enabled > 1 {
        return INVALID;
    }
    if count != 1 {
        return UNSUPPORTED;
    }
    owner.requested = enabled == 1;
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_fsr4gvk_max_generated(owner: *const Owner) -> u32 {
    u32::from(!owner.is_null())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4gvk_present(
    owner: *mut Owner,
    info: *const PresentInfo,
    source: *const Record,
    explicit: u32,
) -> i32 {
    let (Some(owner), Some(info)) = (unsafe { (owner.as_mut(), info.as_ref()) }) else {
        return INIT_FAILED;
    };
    let source = match unsafe { Source::read(source, explicit) } {
        Ok(source) => source,
        Err(result) => return result,
    };
    owner.present(info, source).err().unwrap_or(0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4gvk_status(owner: *const Owner, status: *mut Status) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INVALID;
    };
    if !unsafe { has_capacity(status) } {
        return INVALID;
    }
    if unsafe { (*status).abi_version } != RSF_FGVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    unsafe { status.write(owner.status) };
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4gvk_drain(owner: *mut Owner) -> i32 {
    unsafe { owner.as_mut() }.map_or(0, |owner| owner.drain().err().unwrap_or(0))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr4gvk_destroy(owner: *mut Owner) -> i32 {
    let Some(state) = (unsafe { owner.as_mut() }) else {
        return 0;
    };
    if let Err(result) = state.retire() {
        return result;
    }
    drop(unsafe { Box::from_raw(owner) });
    0
}
