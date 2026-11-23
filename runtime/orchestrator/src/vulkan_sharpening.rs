// SPDX-License-Identifier: MIT
//! Owner of the shared Vulkan sharpening pass used by the Vulkan DLSS, FFX,
//! XeSS and FSR4 super-resolution services.
//!
//! Rust decides admission, the effective filter, slot reuse, device and
//! pipeline lifetime and retirement. Typed Rust Vulkan calls use the original
//! SPIR-V and GPU constants supplied by the licensed payload.
mod native;
use crate::sharpening_policy::{FILTERS, effective_mode};
use crate::sys::rsf_sharpening_options as Options;

use crate::sys::{rsf_srvk_frame as Frame, rsf_srvk_image as Image, rsf_srvk_setup as Setup};
use crate::sys::{rsf_vkfg_image as OwnedImage, rsf_vkfg_platform as ImageOps};
use crate::vulkan_fg_resources::{ensure_image, rsf_core_vkfg_release_image};
use crate::vulkan_sr::{R32_FLOAT, RGBA16_FLOAT, SHADER_READ, STORAGE_USAGE, TRANSFER_SRC_USAGE};
use std::ffi::{c_char, c_void};
use std::ptr;

const R16_FLOAT: u32 = 76;
// Recordings in flight. A host that never retires its command buffers
// exhausts the slots and keeps the vendor output.
const MAX_SLOTS: usize = 64;
const SCRATCH_USAGE: u32 = STORAGE_USAGE | TRANSFER_SRC_USAGE;

/// Per-recording uniform block and descriptor set. `mapped` is host coherent.
#[repr(C)]
pub(crate) struct Uniform {
    pub(crate) pool: u64,
    pub(crate) set: u64,
    pub(crate) buffer: u64,
    pub(crate) memory: u64,
    pub(crate) mapped: *mut c_void,
}

impl Default for Uniform {
    fn default() -> Self {
        Self {
            pool: 0,
            set: 0,
            buffer: 0,
            memory: 0,
            mapped: ptr::null_mut(),
        }
    }
}

#[repr(C)]
#[derive(Default)]
pub(crate) struct Layouts {
    pub(crate) descriptor_set: u64,
    pub(crate) pipeline: u64,
}

/// Facts for the shader constant block. Sizes are pixels.
#[repr(C)]
pub(crate) struct Facts {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) motion_width: u32,
    pub(crate) motion_height: u32,
    pub(crate) depth_width: u32,
    pub(crate) depth_height: u32,
    pub(crate) depth_inverted: u32,
    pub(crate) exposure_texture: u32,
    pub(crate) effective_mode: u32,
    pub(crate) near_plane: f32,
    pub(crate) far_plane: f32,
    pub(crate) exposure: f32,
    pub(crate) motion_x: f32,
    pub(crate) motion_y: f32,
}

/// One filter pass. Sampled views fill bindings 1 to 4: output colour, motion, depth and
/// exposure. The output is GENERAL before and after.
#[repr(C)]
pub(crate) struct Pass {
    pub(crate) set: u64,
    pub(crate) buffer: u64,
    pub(crate) pipeline: u64,
    pub(crate) pipeline_layout: u64,
    pub(crate) sampled: [u64; 4],
    pub(crate) scratch: u64,
    pub(crate) scratch_view: u64,
    pub(crate) output: u64,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) groups_x: u32,
    pub(crate) groups_y: u32,
    pub(crate) scratch_initialized: u32,
    pub(crate) reserved: u32,
}

type Leaf = *mut c_void;

/// The native Vulkan leaves of the owner: `native::PLATFORM` in production, a CPU fake in tests.
/// Scratch images use the shared Vulkan image leaves.
#[repr(C)]
pub(crate) struct Platform {
    pub(crate) images: *const ImageOps,
    pub(crate) open: Option<unsafe extern "C" fn(*const Setup, *mut Leaf) -> i32>,
    pub(crate) close: Option<unsafe extern "C" fn(Leaf)>,
    pub(crate) image_user: Option<unsafe extern "C" fn(Leaf) -> *mut c_void>,
    pub(crate) create_layouts: Option<unsafe extern "C" fn(Leaf, *mut Layouts) -> i32>,
    pub(crate) release_layouts: Option<unsafe extern "C" fn(Leaf, *mut Layouts)>,
    /// Mode 1 to 3 selects the RCAS, depth RCAS or depth DAS SPIR-V.
    pub(crate) create_pipeline: Option<unsafe extern "C" fn(Leaf, u64, u32, *mut u64) -> i32>,
    pub(crate) destroy_pipeline: Option<unsafe extern "C" fn(Leaf, u64)>,
    pub(crate) create_uniform: Option<unsafe extern "C" fn(Leaf, u64, *mut Uniform) -> i32>,
    pub(crate) release_uniform: Option<unsafe extern "C" fn(Leaf, *mut Uniform)>,
    pub(crate) create_view: Option<unsafe extern "C" fn(Leaf, u64, u32, *mut u64) -> i32>,
    /// Sanitizes the request, or the defaults when it is null. Returns nonzero when the options
    /// require a filter dispatch.
    pub(crate) options: Option<unsafe extern "C" fn(*const Options, *mut Options) -> u32>,
    pub(crate) write_constants:
        Option<unsafe extern "C" fn(*mut c_void, *const Options, *const Facts)>,
    pub(crate) record: Option<unsafe extern "C" fn(Leaf, *mut c_void, *const Pass)>,
}

#[derive(Default)]
struct Slot {
    // The command buffer whose recording uses this slot, or 0 when free.
    commands: usize,
    uniform: Uniform,
    scratch: OwnedImage,
    // Output colour, motion, depth and exposure views of that recording.
    views: [u64; 4],
}

struct Device {
    key: usize,
    leaf: *mut c_void,
    layouts: Layouts,
    pipelines: [u64; 3],
    ready: bool,
}

pub(crate) struct Owner {
    platform: &'static Platform,
    device: Option<Device>,
    slots: Vec<Slot>,
    dispatches: u64,
}

fn admit_inputs(setup: &Setup, frame: &Frame) -> bool {
    let sampled = |image: &Image| {
        image.image == 0
            || (image.device == setup.device
                && image.layout == SHADER_READ
                && image.width != 0
                && image.height != 0)
    };
    let exposure = &frame.exposure;
    [&frame.motion, &frame.depth, exposure]
        .into_iter()
        .all(sampled)
        && (exposure.image == 0
            || (exposure.width == 1
                && exposure.height == 1
                && matches!(exposure.format, R32_FLOAT | R16_FLOAT)))
}

fn exposure_scale(pre_exposure: f32) -> f32 {
    // Same as 1/std::max(pre, 1e-6f): NaN stays NaN and the constant
    // leaf replaces it with 1.
    1.0 / if pre_exposure < 1e-6 {
        1e-6
    } else {
        pre_exposure
    }
}

fn image_ops(platform: &Platform) -> &ImageOps {
    // Owner construction requires the native table and its non-null image table
    // to stay alive for the lifetime of the owner.
    unsafe { &*platform.images }
}

fn release_views(platform: &Platform, leaf: *mut c_void, views: &mut [u64; 4]) {
    let user = unsafe { platform.image_user.unwrap()(leaf) };
    for view in views.iter_mut().filter(|view| **view != 0) {
        unsafe { (image_ops(platform).destroy_view.unwrap())(user, *view) };
        *view = 0;
    }
}

fn release_slot(platform: &Platform, leaf: *mut c_void, slot: &mut Slot) {
    release_views(platform, leaf, &mut slot.views);
    let user = unsafe { platform.image_user.unwrap()(leaf) };
    rsf_core_vkfg_release_image(image_ops(platform), user, &mut slot.scratch);
    unsafe { platform.release_uniform.unwrap()(leaf, &mut slot.uniform) };
    *slot = Slot::default();
}

impl Owner {
    fn initialize(&mut self, setup: &Setup) -> bool {
        if let Some(device) = &self.device {
            // A failed bind or another device stays refused until clear().
            return device.key == setup.device as usize && device.ready;
        }
        let platform = self.platform;
        let mut leaf = ptr::null_mut();
        let opened = unsafe { platform.open.unwrap()(setup, &mut leaf) } == 0 && !leaf.is_null();
        let device = self.device.insert(Device {
            key: setup.device as usize,
            leaf,
            layouts: Layouts::default(),
            pipelines: [0; 3],
            ready: false,
        });
        if !opened || unsafe { platform.create_layouts.unwrap()(leaf, &mut device.layouts) } != 0 {
            return false;
        }
        for (pipeline, mode) in device.pipelines.iter_mut().zip(FILTERS) {
            if unsafe {
                platform.create_pipeline.unwrap()(leaf, device.layouts.pipeline, mode, pipeline)
            } != 0
            {
                return false;
            }
        }
        device.ready = true;
        true
    }

    fn free_slot(&mut self) -> Option<usize> {
        if let Some(index) = self.slots.iter().position(|slot| slot.commands == 0) {
            return Some(index);
        }
        if self.slots.len() >= MAX_SLOTS || self.slots.try_reserve(1).is_err() {
            return None;
        }
        self.slots.push(Slot::default());
        Some(self.slots.len() - 1)
    }

    // The slot is free, so every recording that used its resources has retired.
    fn prepare(platform: &Platform, device: &Device, slot: &mut Slot, extent: [u32; 2]) -> bool {
        if slot.uniform.pool == 0
            && unsafe {
                platform.create_uniform.unwrap()(
                    device.leaf,
                    device.layouts.descriptor_set,
                    &mut slot.uniform,
                )
            } != 0
        {
            return false;
        }
        let user = unsafe { platform.image_user.unwrap()(device.leaf) };
        ensure_image(
            image_ops(platform),
            user,
            &mut slot.scratch,
            extent,
            RGBA16_FLOAT,
            SCRATCH_USAGE,
            true,
        )
        .is_ok()
            && !slot.uniform.mapped.is_null()
            && slot.uniform.set != 0
            && slot.scratch.view != 0
    }

    fn create_views(
        platform: &Platform,
        device: &Device,
        slot: &mut Slot,
        frame: &Frame,
        output: u64,
    ) -> bool {
        let requests = [
            (output, RGBA16_FLOAT),
            (frame.motion.image, frame.motion.format),
            (frame.depth.image, frame.depth.format),
            (frame.exposure.image, frame.exposure.format),
        ];
        for (view, (image, format)) in slot.views.iter_mut().zip(requests) {
            if image != 0
                && unsafe { platform.create_view.unwrap()(device.leaf, image, format, view) } != 0
            {
                release_views(platform, device.leaf, &mut slot.views);
                return false;
            }
        }
        true
    }

    fn record(&mut self, setup: &Setup, commands: *mut c_void, frame: &Frame, output: u64) -> bool {
        let platform = self.platform;
        let mut options: Options = unsafe { std::mem::zeroed() };
        let request = if frame.sharpening.struct_size != 0 {
            &raw const frame.sharpening
        } else {
            ptr::null()
        };
        // Off, Vendor and zero strength leave the output untouched.
        if unsafe { platform.options.unwrap()(request, &mut options) } == 0 {
            return true;
        }
        let Some(record) = (unsafe { frame.record.as_ref() }) else {
            return false;
        };
        if commands.is_null() || output == 0 || !admit_inputs(setup, frame) {
            return false;
        }
        // Vulkan forbids a zero image extent.
        let extent = [record.output_width, record.output_height];
        let Some(effective) = effective_mode(options.mode, frame.depth.image != 0) else {
            return false;
        };
        if extent.contains(&0) || !self.initialize(setup) {
            return false;
        }
        let Some(index) = self.free_slot() else {
            return false;
        };
        let device = self.device.as_ref().expect("initialized device");
        let slot = &mut self.slots[index];
        if !Self::prepare(platform, device, slot, extent) {
            release_slot(platform, device.leaf, slot);
            return false;
        }
        if !Self::create_views(platform, device, slot, frame, output) {
            return false;
        }
        let facts = Facts {
            width: extent[0],
            height: extent[1],
            motion_width: frame.motion.width,
            motion_height: frame.motion.height,
            depth_width: frame.depth.width,
            depth_height: frame.depth.height,
            depth_inverted: record.camera.depth_inverted,
            exposure_texture: u32::from(frame.exposure.image != 0),
            effective_mode: effective,
            near_plane: record.camera.near_plane,
            far_plane: record.camera.far_plane,
            exposure: exposure_scale(frame.pre_exposure),
            motion_x: frame.motion_scale_x,
            motion_y: frame.motion_scale_y,
        };
        unsafe { platform.write_constants.unwrap()(slot.uniform.mapped, &options, &facts) };
        let color = slot.views[0];
        let pass = Pass {
            set: slot.uniform.set,
            buffer: slot.uniform.buffer,
            pipeline: device.pipelines[effective as usize - 1],
            pipeline_layout: device.layouts.pipeline,
            // Absent inputs bind the output view; the constants disable them.
            sampled: slot.views.map(|view| if view != 0 { view } else { color }),
            scratch: slot.scratch.image,
            scratch_view: slot.scratch.view,
            output,
            width: extent[0],
            height: extent[1],
            groups_x: extent[0].div_ceil(16),
            groups_y: extent[1].div_ceil(16),
            scratch_initialized: slot.scratch.initialized,
            reserved: 0,
        };
        unsafe { platform.record.unwrap()(device.leaf, commands, &pass) };
        slot.commands = commands as usize;
        slot.scratch.initialized = 1;
        self.dispatches += 1;
        true
    }

    fn retire(&mut self, commands: *mut c_void) {
        let Some(device) = &self.device else {
            return;
        };
        if commands.is_null() {
            return;
        }
        for slot in self
            .slots
            .iter_mut()
            .filter(|slot| slot.commands == commands as usize)
        {
            release_views(self.platform, device.leaf, &mut slot.views);
            slot.commands = 0;
        }
    }

    fn clear(&mut self) {
        let Some(mut device) = self.device.take() else {
            return;
        };
        let platform = self.platform;
        // Slots exist only after the device and its pipelines are ready.
        for slot in &mut self.slots {
            release_slot(platform, device.leaf, slot);
        }
        self.slots = Vec::new();
        if device.leaf.is_null() {
            return;
        }
        unsafe {
            for pipeline in device
                .pipelines
                .into_iter()
                .filter(|pipeline| *pipeline != 0)
            {
                platform.destroy_pipeline.unwrap()(device.leaf, pipeline);
            }
            platform.release_layouts.unwrap()(device.leaf, &mut device.layouts);
            platform.close.unwrap()(device.leaf);
        }
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        self.clear();
    }
}

unsafe fn rsf_core_sharpenvk_create(platform: *const Platform) -> *mut Owner {
    if platform.is_null() {
        return ptr::null_mut();
    }
    // The native facade passes its static leaf table.
    let platform: &'static Platform = unsafe { &*platform };
    if platform.images.is_null() {
        return ptr::null_mut();
    }
    crate::allocate_handle(Owner {
        platform,
        device: None,
        slots: Vec::new(),
        dispatches: 0,
    })
}

unsafe fn rsf_core_sharpenvk_destroy(owner: *mut Owner) {
    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner) });
    }
}

unsafe fn rsf_core_sharpenvk_record(
    owner: *mut Owner,
    setup: *const Setup,
    commands: *mut c_void,
    frame: *const Frame,
    output: u64,
) -> u32 {
    let (Some(owner), Some(setup), Some(frame)) =
        (unsafe { (owner.as_mut(), setup.as_ref(), frame.as_ref()) })
    else {
        return 0;
    };
    u32::from(owner.record(setup, commands, frame, output))
}

unsafe fn rsf_core_sharpenvk_retire(owner: *mut Owner, commands: *mut c_void) {
    if let Some(owner) = unsafe { owner.as_mut() } {
        owner.retire(commands);
    }
}

unsafe fn rsf_core_sharpenvk_clear(owner: *mut Owner) {
    if let Some(owner) = unsafe { owner.as_mut() } {
        owner.clear();
    }
}

#[cfg(test)]
mod tests;

unsafe extern "C" {
    fn rsf_call_native_log(
        log: crate::sys::rsf_backend_log_fn,
        user: *mut c_void,
        text: *const c_char,
    );
}

/// A recording host serializes calls, retires completed and reset command buffers, and drains
/// all users before clearing or destroying this state.
pub(crate) struct Host {
    owner: *mut c_void,
    failure_logged: bool,
}

impl Default for Host {
    fn default() -> Self {
        Self {
            owner: ptr::null_mut(),
            failure_logged: false,
        }
    }
}

pub(crate) unsafe fn rsf_sharpenvk_host_destroy(host: &mut Host) {
    unsafe { rsf_core_sharpenvk_destroy(host.owner.cast()) };
    *host = Host::default();
}

pub(crate) unsafe fn rsf_sharpenvk_host_clear(host: &mut Host) {
    unsafe { rsf_core_sharpenvk_clear(host.owner.cast()) };
}

pub(crate) unsafe fn rsf_sharpenvk_host_retire(host: &mut Host, commands: *mut c_void) {
    unsafe { rsf_core_sharpenvk_retire(host.owner.cast(), commands) };
}

// Read by the sharpening fixtures.
#[cfg(feature = "test-fixtures")]
pub(crate) unsafe fn rsf_sharpenvk_host_dispatches(host: &Host) -> u64 {
    unsafe { host.owner.cast::<Owner>().as_ref() }.map_or(0, |owner| owner.dispatches)
}

pub(crate) unsafe fn rsf_sharpenvk_host_run(
    host: &mut Host,
    setup: &Setup,
    commands: *mut c_void,
    frame: &Frame,
    output: u64,
) -> u32 {
    if host.owner.is_null() {
        host.owner = unsafe { rsf_core_sharpenvk_create(native::PLATFORM) }.cast();
    }
    let result =
        unsafe { rsf_core_sharpenvk_record(host.owner.cast(), setup, commands, frame, output) };
    if result == 0 && !host.failure_logged {
        host.failure_logged = true;
        let text = c"Vulkan sharpening unavailable; retaining vendor output";
        if frame.log.is_some() {
            unsafe { rsf_call_native_log(frame.log, frame.log_user, text.as_ptr()) };
        } else {
            unsafe {
                windows_sys::Win32::System::Diagnostics::Debug::OutputDebugStringA(
                    text.as_ptr().cast(),
                )
            };
        }
    }
    result
}
