// SPDX-License-Identifier: MIT
use crate::ffx_version::{self, MAX_VERSIONS, Version};
use crate::render_abi::rsf_sharpening_options;
use crate::sdk_module::{FIDELITYFX_DLL, Loader, SdkModule};
use crate::sr_session::has_capacity;
use crate::vulkan_fg_resources_abi::rsf_vkfg_platform as ImageOps;
use crate::vulkan_sr_abi::*;
use crate::vulkan_sr_split::normalize_frame;
use rsf_game_sys::{
    RSF_FRAME_FLAG_RESET, RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record,
};
use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_void};
use std::ptr::{self, NonNull};
use std::sync::{LazyLock, Mutex};

const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const NOT_COMPILED: i32 = -3;
const INIT_FAILED: i32 = -6;
const NOT_SUPPORTED: i32 = -7;
const WRONG_API: i32 = -12;

// Vulkan C ABI values, checked against the native headers at the graphics bridge.
const R8_UNORM: u32 = 9;
pub(super) const RG16_FLOAT: u32 = 83;
pub(super) const RGBA16_FLOAT: u32 = 97;
pub(super) const R32_FLOAT: u32 = 100;
const D32_FLOAT: u32 = 126;
pub(super) const SHADER_READ: u32 = 5;
pub(super) const TRANSFER_DST: u32 = 7;
pub(super) const GENERAL: u32 = 1;
pub(super) const TRANSFER_SRC_USAGE: u32 = 1;
pub(super) const TRANSFER_DST_USAGE: u32 = 2;
pub(super) const SAMPLED_USAGE: u32 = 4;
pub(super) const STORAGE_USAGE: u32 = 8;
const LINEAR_BLIT: u32 = 0x400 | 0x800 | 0x1000;

const FFX_HDR: u32 = 1;
const FFX_JITTER_CANCELLATION: u32 = 4;
const FFX_INVERTED_DEPTH: u32 = 8;
const FFX_INFINITE_DEPTH: u32 = 16;
const FFX_AUTO_EXPOSURE: u32 = 32;
pub(super) const SHARPENING_VENDOR: u32 = 4;

type Setup = rsf_srvk_setup;
type Frame = rsf_srvk_frame;
type Image = rsf_srvk_image;
type Device = *mut c_void;

unsafe fn setup_admit(setup: *const Setup) -> i32 {
    if !unsafe { has_capacity(setup) } {
        return INVALID;
    }
    let s = unsafe { &*setup };
    if s.abi_version != RSF_SRVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if s.instance.is_null()
        || s.physical_device.is_null()
        || s.device.is_null()
        || s.device_proc.is_none()
        || s.instance_proc.is_none()
        || s.runtime_directory_utf8.is_null()
        || s.output_width == 0
        || s.output_height == 0
        || (s.quality > 4 && (s.backend != RSF_SR_XESS || s.quality > 5))
        || s.depth_inverted > 1
        || s.depth_infinite > 1
        || s.motion_jittered > 1
    {
        return INVALID;
    }
    if matches!(
        s.backend,
        RSF_SR_DLSS | RSF_SR_XESS | RSF_SR_FSR2 | RSF_SR_FSR3
    ) {
        0
    } else {
        NOT_SUPPORTED
    }
}

pub(super) fn valid_image(
    image: &Image,
    device: Device,
    extent: [u32; 2],
    layout: u32,
    formats: &[u32],
) -> bool {
    image.device == device
        && image.image != 0
        && [image.width, image.height] == extent
        && image.layout == layout
        && formats.contains(&image.format)
}

unsafe fn fallback_admit(call: *const rsf_srvk_fallback) -> i32 {
    if !unsafe { has_capacity(call) } {
        return INVALID;
    }
    let call = unsafe { &*call };
    if call.abi_version != RSF_SRVK_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let source = &call.color;
    let output = &call.output;
    if call.instance.is_null()
        || call.physical_device.is_null()
        || call.device.is_null()
        || call.instance_proc.is_none()
        || call.device_proc.is_none()
        || call.command_buffer.is_null()
        || !valid_image(
            source,
            call.device,
            [source.width, source.height],
            SHADER_READ,
            &[RGBA16_FLOAT],
        )
        || !valid_image(
            output,
            call.device,
            [output.width, output.height],
            TRANSFER_DST,
            &[RGBA16_FLOAT],
        )
        || source.image == output.image
        || source.copy_source == 0
        || [source.width, source.height, output.width, output.height]
            .into_iter()
            .any(|dim| dim == 0 || dim > i32::MAX as u32)
    {
        return INVALID;
    }
    0
}

pub(super) fn valid_frame_record(
    setup: &Setup,
    frame: &Frame,
    record: &rsf_frame_record,
    render: [u32; 2],
    output: [u32; 2],
) -> bool {
    let camera = &record.camera;
    !(render.contains(&0)
        || record.abi_version != RSF_GAME_FRAME_ABI_VERSION
        || camera.struct_size < size_of_val(camera) as u32
        || camera.abi_version != RSF_GAME_CAMERA_ABI_VERSION
        || record.session_id == 0
        || record.frame_id == 0
        || [record.render_width, record.render_height] != render
        || [record.output_width, record.output_height] != output
        || [camera.render_width, camera.render_height] != render
        || [camera.output_width, camera.output_height] != output
        || camera.depth_inverted != setup.depth_inverted
        || camera.motion_jittered != setup.motion_jittered
        || !record.frame_time_ms.is_finite()
        || record.frame_time_ms <= 0.0
        || !frame.motion_scale_x.is_finite()
        || !frame.motion_scale_y.is_finite()
        || frame.motion_scale_x == 0.0
        || frame.motion_scale_y == 0.0
        || !frame.pre_exposure.is_finite()
        || frame.pre_exposure <= 0.0
        || !camera.near_plane.is_finite()
        || camera.near_plane <= 0.0
        || !camera.far_plane.is_finite()
        || camera.far_plane < 0.0
        || (setup.depth_infinite == 0 && camera.far_plane <= camera.near_plane)
        || !(0.0..=std::f32::consts::PI).contains(&camera.vertical_fov_radians)
        || camera.vertical_fov_radians == 0.0
        || !camera.jitter_pixels.into_iter().all(f32::is_finite))
}

unsafe fn frame_admit(setup: &Setup, frame: &Frame, planned: [u32; 2]) -> i32 {
    if !unsafe { has_capacity(frame.record) } {
        return INVALID;
    }
    let record = unsafe { &*frame.record };
    // XeSS owns a queried inclusive range. All leased images still follow
    // the actual camera/record size, even when it differs from the plan.
    let render = if setup.backend == RSF_SR_XESS {
        [record.render_width, record.render_height]
    } else {
        planned
    };
    let output = [setup.output_width, setup.output_height];
    if !valid_frame_record(setup, frame, record, render, output) {
        return INVALID;
    }
    let device = setup.device;
    if !valid_image(&frame.color, device, render, SHADER_READ, &[RGBA16_FLOAT])
        || !valid_image(
            &frame.depth,
            device,
            render,
            SHADER_READ,
            &[D32_FLOAT, R32_FLOAT],
        )
        || !valid_image(&frame.motion, device, render, SHADER_READ, &[RG16_FLOAT])
        || !valid_image(&frame.output, device, output, TRANSFER_DST, &[RGBA16_FLOAT])
        || [
            &frame.reactive,
            &frame.composition,
            &frame.bias_current_color,
        ]
        .into_iter()
        .any(|mask| mask.image != 0 && !valid_image(mask, device, render, SHADER_READ, &[R8_UNORM]))
    {
        return INVALID;
    }
    0
}

type DeviceProc = unsafe extern "C" fn(Device, *const c_char) -> rsf_vk_function;
type Lookup = unsafe extern "C" fn(DeviceProc, Device, *const c_char, *mut rsf_vk_function) -> i32;

struct Dispatch {
    proc: DeviceProc,
    references: u32,
}

static DISPATCH: LazyLock<Mutex<HashMap<usize, Dispatch>>> = LazyLock::new(Mutex::default);

fn dispatch_register(device: Device, proc: rsf_vk_get_proc) -> i32 {
    let Some(proc) = proc else { return INVALID };
    let mut dispatch = DISPATCH.lock().unwrap();
    if let Some(entry) = dispatch.get_mut(&(device as usize)) {
        if !std::ptr::fn_addr_eq(entry.proc, proc) {
            return WRONG_API;
        }
        let Some(count) = entry.references.checked_add(1) else {
            return INIT_FAILED;
        };
        entry.references = count;
    } else {
        if dispatch.try_reserve(1).is_err() {
            return INIT_FAILED;
        }
        dispatch.insert(
            device as usize,
            Dispatch {
                proc,
                references: 1,
            },
        );
    }
    0
}

fn dispatch_unregister(device: Device) {
    let mut dispatch = DISPATCH.lock().unwrap();
    if let Some(entry) = dispatch.get_mut(&(device as usize)) {
        entry.references -= 1;
        if entry.references == 0 {
            dispatch.remove(&(device as usize));
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_dispatch_lookup(
    device: Device,
    name: *const c_char,
    lookup: Lookup,
) -> rsf_vk_function {
    let proc = DISPATCH
        .lock()
        .unwrap()
        .get(&(device as usize))
        .map(|entry| entry.proc)?;
    let mut result = None;
    // The native caller holds the device lease. Invoke the supplied proc outside the lock.
    if unsafe { lookup(proc, device, name, &mut result) } == 0 {
        return None;
    }
    if result.is_some() || name.is_null() {
        return result;
    }
    // Older SDKs ask for KHR spellings even when Vulkan exposes their promoted core equivalents.
    let alias = match unsafe { CStr::from_ptr(name) }.to_bytes() {
        b"vkGetBufferMemoryRequirements2KHR" => c"vkGetBufferMemoryRequirements2",
        b"vkGetImageMemoryRequirements2KHR" => c"vkGetImageMemoryRequirements2",
        b"vkBindBufferMemory2KHR" => c"vkBindBufferMemory2",
        b"vkBindImageMemory2KHR" => c"vkBindImageMemory2",
        _ => return None,
    };
    if unsafe { lookup(proc, device, alias.as_ptr(), &mut result) } != 0 {
        result
    } else {
        None
    }
}

#[derive(Default)]
pub(super) struct History {
    frame: u64,
    session: u64,
    view: u32,
    generation: u32,
}

impl History {
    pub(super) fn needs_reset(&self, record: &rsf_frame_record) -> bool {
        record.flags & RSF_FRAME_FLAG_RESET != 0
            || self.frame.wrapping_add(1) != record.frame_id
            || self.session != record.session_id
            || self.view != record.view_id
            || self.generation != record.resource_generation
    }

    pub(super) fn commit(&mut self, record: &rsf_frame_record) {
        *self = Self {
            frame: record.frame_id,
            session: record.session_id,
            view: record.view_id,
            generation: record.resource_generation,
        };
    }
}

type Handle = *mut c_void;
type Capture = unsafe extern "C" fn(Handle, Handle, &Image, &rsf_frame_record, *const c_char, u32);

// Private platform ABI. Rust owns the public handle and sequences these leaves.
#[repr(C)]
struct Platform {
    dlss: Option<&'static crate::vulkan_dlss_sr_abi::rsf_vkdlss_platform>,
    xess: Option<&'static xess::XessOps>,
    ffx: FfxOps,
    fallback_probe: unsafe extern "C" fn(&rsf_srvk_fallback, &mut Blit) -> i32,
    fallback_record: unsafe extern "C" fn(&rsf_srvk_fallback, &Blit) -> i32,
    dump_begin: unsafe extern "C" fn(&rsf_frame_record, u32) -> i32,
    dump_wants: unsafe extern "C" fn(&rsf_frame_record) -> i32,
    dump_result: unsafe extern "C" fn(&rsf_frame_record, *const c_char, u32, i32),
}

#[repr(C)]
struct FfxOps {
    loader: Loader,
    open: unsafe extern "C" fn(&Setup, Handle, &mut Handle) -> i32,
    close: unsafe extern "C" fn(Handle),
    initialize_graphics: unsafe extern "C" fn(Handle, &Setup) -> i32,
    versions: unsafe extern "C" fn(Handle, &mut [Version; MAX_VERSIONS], &mut u64) -> i32,
    create_effect: unsafe extern "C" fn(Handle, &Setup, u64, u32) -> i32,
    effective_version: unsafe extern "C" fn(Handle, &mut u64) -> i32,
    destroy_effect: unsafe extern "C" fn(Handle) -> i32,
    plan: unsafe extern "C" fn(Handle, &Setup, &mut u32, &mut u32) -> i32,
    // Typed vkCreateImage/requirements/vkAllocateMemory/vkBindImageMemory leaves and the
    // device-local mask over the owner's device; Rust sequences them in StagingImage::allocate.
    image_user: unsafe extern "C" fn(Handle) -> Handle,
    images: &'static ImageOps,
    memory_type_count: unsafe extern "C" fn(Handle) -> u32,
    release: unsafe extern "C" fn(Handle, u64, u64),
    initialize_mask: unsafe extern "C" fn(Handle, Handle, u64),
    initialize_output: unsafe extern "C" fn(Handle, Handle, u64),
    dispatch: unsafe extern "C" fn(Handle, Handle, &FfxDispatch) -> i32,
    copy_output: unsafe extern "C" fn(Handle, Handle, &Image, &Image),
    sharpen: unsafe extern "C" fn(Handle, &Setup, Handle, &Frame, u64) -> i32,
    clear_sharpen: unsafe extern "C" fn(Handle),
    log: unsafe extern "C" fn(&Frame, *const c_char),
    capture_provider: unsafe extern "C" fn(Handle, u64, u32),
    capture_record: Capture,
    capture_wants: unsafe extern "C" fn(Handle, u64) -> i32,
    capture_parameters: unsafe extern "C" fn(&Frame, u32, u32),
    retire_commands: unsafe extern "C" fn(Handle, Handle),
    flush_capture: unsafe extern "C" fn(Handle),
}

#[repr(C)]
#[derive(Default)]
struct Blit {
    barrier: rsf_vk_function,
    blit: rsf_vk_function,
    features: u32,
}

#[repr(C)]
struct FfxDispatch<'a> {
    frame: &'a Frame,
    reactive: Image,
    composition: Image,
    output: Image,
    far_plane: f32,
    sharpness: f32,
    reset: u32,
}

const _: () = {
    assert!(size_of::<FfxOps>() == 27 * size_of::<usize>());
    assert!(size_of::<Platform>() == 272);
    assert!(size_of::<Blit>() == 24);
    assert!(size_of::<FfxDispatch>() == 144);
    assert!(std::mem::offset_of!(FfxDispatch, reset) == 136);
};

struct StagingImage {
    description: Image,
    memory: u64,
}

impl StagingImage {
    fn new(device: Device, [width, height]: [u32; 2], format: u32, layout: u32) -> Self {
        Self {
            description: Image {
                device,
                image: 0,
                width,
                height,
                format,
                layout,
                copy_source: 0,
            },
            memory: 0,
        }
    }

    /// Creates the 2D optimal-tiling image, then allocates and binds dedicated memory.
    /// Each handle is published as soon as it exists, so a failed later step leaves it
    /// for the owner's `release`. Every Vulkan failure reports `INIT_FAILED`.
    unsafe fn allocate(&mut self, ops: &FfxOps, handle: Handle, usage: u32) -> Result<(), i32> {
        let images = ops.images;
        let user = unsafe { (ops.image_user)(handle) };
        let description = &mut self.description;
        let mut image = 0;
        let result = unsafe {
            (images.create_image.unwrap())(
                user,
                description.width,
                description.height,
                description.format,
                usage,
                &mut image,
            )
        };
        if result != 0 {
            return Err(INIT_FAILED);
        }
        description.image = image;
        let (mut size, mut required) = (0, 0);
        unsafe { (images.requirements.unwrap())(user, image, &mut size, &mut required) };
        let kind = memory_type(
            required,
            unsafe { (ops.memory_type_count)(handle) },
            unsafe { (images.local_memory_types.unwrap())(user) },
        )
        .ok_or(INIT_FAILED)?;
        let mut memory = 0;
        if unsafe { (images.allocate.unwrap())(user, size, kind, &mut memory) } != 0 {
            return Err(INIT_FAILED);
        }
        self.memory = memory;
        if unsafe { (images.bind.unwrap())(user, image, memory) } != 0 {
            return Err(INIT_FAILED);
        }
        Ok(())
    }
}

/// The first compatible device-local memory type, else the last compatible type.
fn memory_type(required: u32, count: u32, device_local: u32) -> Option<u32> {
    let compatible = required & 1u32.checked_shl(count).map_or(u32::MAX, |bit| bit - 1);
    match compatible & device_local {
        0 => compatible.checked_ilog2(),
        preferred => Some(preferred.trailing_zeros()),
    }
}

struct DispatchLease(Device);

impl Drop for DispatchLease {
    fn drop(&mut self) {
        dispatch_unregister(self.0);
    }
}

struct FidelityFx {
    ops: &'static FfxOps,
    handle: Handle,
    dispatch_lease: Option<DispatchLease>,
    module: Option<SdkModule>,
    effect_live: bool,
    initialized: bool,
    output: StagingImage,
    zero_mask: StagingImage,
    history: History,
    sharpen_failed: bool,
}

fn check(result: i32) -> Result<(), i32> {
    if result == 0 { Ok(()) } else { Err(result) }
}

impl FidelityFx {
    unsafe fn create(ops: &'static FfxOps, setup: &Setup) -> Result<(Self, [u32; 2]), i32> {
        let output = [setup.output_width, setup.output_height];
        let mut handle = ptr::null_mut();
        let module =
            unsafe { SdkModule::open(ops.loader, setup.runtime_directory_utf8, FIDELITYFX_DLL) }?;
        check(unsafe { (ops.open)(setup, module.as_ptr(), &mut handle) })?;
        if handle.is_null() {
            return Err(INIT_FAILED);
        }
        let mut owner = Self {
            ops,
            handle,
            dispatch_lease: None,
            module: Some(module),
            effect_live: false,
            initialized: false,
            output: StagingImage::new(setup.device, output, RGBA16_FLOAT, GENERAL),
            zero_mask: StagingImage::new(setup.device, [0, 0], R8_UNORM, SHADER_READ),
            history: History::default(),
            sharpen_failed: false,
        };
        check(dispatch_register(setup.device, setup.device_proc))?;
        owner.dispatch_lease = Some(DispatchLease(setup.device));
        check(unsafe { (ops.initialize_graphics)(handle, setup) })?;
        let mut versions = [Version::default(); MAX_VERSIONS];
        let mut count = 0;
        check(unsafe { (ops.versions)(handle, &mut versions, &mut count) })?;
        let count = usize::try_from(count).map_err(|_| NOT_SUPPORTED)?;
        let version = ffx_version::select(
            versions.get(..count).ok_or(NOT_SUPPORTED)?,
            setup.backend,
            0,
        )
        .ok_or(NOT_SUPPORTED)?
        .id;
        let flags = FFX_HDR
            | FFX_AUTO_EXPOSURE
            | (setup.depth_inverted * FFX_INVERTED_DEPTH)
            | (setup.depth_infinite * FFX_INFINITE_DEPTH)
            | (setup.motion_jittered * FFX_JITTER_CANCELLATION);
        // Creation can leave a partial context for the cleanup leaf.
        owner.effect_live = true;
        check(unsafe { (ops.create_effect)(handle, setup, version, flags) })?;
        let mut effective = 0;
        check(unsafe { (ops.effective_version)(handle, &mut effective) })?;
        if effective != version {
            return Err(NOT_SUPPORTED);
        }
        unsafe { (ops.capture_provider)(handle, version, setup.backend) };

        let plan = if setup.backend == RSF_SR_FSR2 && setup.quality == 0 {
            output
        } else {
            let (mut width, mut height) = (0, 0);
            check(unsafe { (ops.plan)(handle, setup, &mut width, &mut height) })?;
            [width, height]
        };
        if plan.contains(&0) {
            return Err(INIT_FAILED);
        }
        [
            owner.zero_mask.description.width,
            owner.zero_mask.description.height,
        ] = plan;
        let mask_usage = TRANSFER_DST_USAGE | SAMPLED_USAGE;
        let output_usage = mask_usage | STORAGE_USAGE | TRANSFER_SRC_USAGE;
        for (image, usage) in [
            (&mut owner.output, output_usage),
            (&mut owner.zero_mask, mask_usage),
        ] {
            image.description.copy_source = u32::from(usage & TRANSFER_SRC_USAGE != 0);
            unsafe { image.allocate(ops, handle, usage) }?;
        }
        Ok((owner, plan))
    }

    fn record_capture(
        &self,
        commands: Handle,
        image: &Image,
        record: &rsf_frame_record,
        role: &CStr,
        reset: u32,
    ) {
        unsafe {
            (self.ops.capture_record)(self.handle, commands, image, record, role.as_ptr(), reset)
        };
    }

    fn evaluate(
        &mut self,
        platform: &Platform,
        setup: &Setup,
        commands: Handle,
        frame: &Frame,
        record: &rsf_frame_record,
    ) -> i32 {
        let ops = self.ops;
        let handle = self.handle;
        if unsafe { (platform.dump_wants)(record) } != 0 {
            self.record_capture(
                commands,
                &frame.color,
                record,
                c"sr-source-jittered",
                record.flags & RSF_FRAME_FLAG_RESET,
            );
            for (image, role) in [
                (&frame.bias_current_color, c"sr-bias-mask"),
                (&frame.reactive, c"sr-reactive-mask"),
                (&frame.composition, c"sr-transparency-mask"),
            ] {
                if image.image != 0 {
                    self.record_capture(commands, image, record, role, 0);
                }
            }
        }
        let output = self.output.description;
        let zero = self.zero_mask.description;
        if !self.initialized {
            // The ABI requires submission in recording order, including a
            // failed dispatch. First-use layouts survive a rejected SR result.
            unsafe {
                (ops.initialize_mask)(handle, commands, zero.image);
                (ops.initialize_output)(handle, commands, output.image);
            }
            self.initialized = true;
        }
        let reset = u32::from(self.history.needs_reset(record));
        let settings = &frame.sharpening;
        let dispatch = FfxDispatch {
            frame,
            reactive: if frame.reactive.image != 0 {
                frame.reactive
            } else {
                zero
            },
            composition: if frame.composition.image != 0 {
                frame.composition
            } else {
                zero
            },
            output,
            far_plane: if record.camera.far_plane > 0.0 {
                record.camera.far_plane
            } else {
                f32::MAX
            },
            sharpness: if settings.struct_size != 0 && settings.mode == SHARPENING_VENDOR {
                settings.sharpness
            } else {
                0.0
            },
            reset,
        };
        unsafe { (ops.capture_parameters)(frame, setup.backend, reset) };
        if unsafe { (ops.capture_wants)(handle, record.frame_id) } != 0 {
            self.record_capture(commands, &frame.color, record, c"sr-linear-color", reset);
            self.record_capture(commands, &frame.depth, record, c"sr-depth", reset);
            self.record_capture(commands, &frame.motion, record, c"sr-motion-uv", reset);
        }
        let result = unsafe { (ops.dispatch)(handle, commands, &dispatch) };
        if result != 0 {
            self.history = History::default();
            return result;
        }
        if unsafe { (ops.sharpen)(handle, setup, commands, frame, output.image) } == 0
            && !self.sharpen_failed
        {
            self.sharpen_failed = true;
            unsafe {
                (ops.log)(
                    frame,
                    c"Vulkan sharpening unavailable; retaining vendor output".as_ptr(),
                )
            };
        }
        self.record_capture(commands, &output, record, c"sr-output-linear", reset);
        unsafe { (ops.copy_output)(handle, commands, &output, &frame.output) };
        self.history.commit(record);
        0
    }

    fn retire_commands(&self, commands: Handle) {
        // The host calls this after GPU completion and reset of the specified buffer.
        unsafe { (self.ops.retire_commands)(self.handle, commands) };
    }

    fn destroy(&mut self) -> i32 {
        // The ABI requires the caller to drain all work referencing this owner.
        unsafe { (self.ops.flush_capture)(self.handle) };
        let result = unsafe { (self.ops.destroy_effect)(self.handle) };
        if result == 0 {
            self.effect_live = false;
        }
        result
    }
}

impl Drop for FidelityFx {
    fn drop(&mut self) {
        // Public destruction drops this owner after the SDK accepts it. A failed constructor
        // has no public handle to return, so if rollback refuses, keep the native leaf,
        // module and device dispatch alive instead of unloading live SDK code.
        unsafe {
            if self.effect_live && (self.ops.destroy_effect)(self.handle) != 0 {
                std::mem::forget(self.dispatch_lease.take());
                std::mem::forget(self.module.take());
                return;
            }
            (self.ops.clear_sharpen)(self.handle);
            for image in [&self.output, &self.zero_mask] {
                (self.ops.release)(self.handle, image.description.image, image.memory);
            }
            (self.ops.close)(self.handle);
        }
    }
}

enum Backend {
    Dlss(Box<crate::vulkan_dlss_sr::Owner>),
    Ffx(FidelityFx),
    Xess(Box<xess::XessOwner>),
}

struct Owner {
    setup: Setup,
    platform: &'static Platform,
    backend: Backend,
    render: [u32; 2],
}

impl Owner {
    unsafe fn create(platform: &'static Platform, mut setup: Setup) -> Result<Self, i32> {
        let (backend, render) = match setup.backend {
            RSF_SR_DLSS => {
                let ops = platform.dlss.ok_or(NOT_COMPILED)?;
                let owner = crate::vulkan_dlss_sr::Owner::create(ops, &setup)?;
                let plan = owner.plan();
                (Backend::Dlss(owner), plan)
            }
            RSF_SR_XESS => {
                let ops = platform.xess.ok_or(NOT_COMPILED)?;
                let owner = unsafe { xess::XessOwner::create(ops, &setup) }?;
                let plan = owner.plan();
                (Backend::Xess(Box::new(owner)), plan)
            }
            _ => {
                let (owner, render) = unsafe { FidelityFx::create(&platform.ffx, &setup) }?;
                (Backend::Ffx(owner), render)
            }
        };
        setup.runtime_directory_utf8 = ptr::null();
        Ok(Self {
            setup,
            platform,
            backend,
            render,
        })
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_create(
    platform: Option<&'static Platform>,
    setup: *const Setup,
    out: *mut Handle,
) -> i32 {
    if out.is_null() {
        return INVALID;
    }
    unsafe { out.write(ptr::null_mut()) };
    let admission = unsafe { setup_admit(setup) };
    if admission != 0 {
        return admission;
    }
    let Some(platform) = platform else {
        return NOT_COMPILED;
    };
    match unsafe { Owner::create(platform, *setup) } {
        Ok(owner) => {
            unsafe { out.write(Box::into_raw(Box::new(owner)).cast()) };
            0
        }
        Err(error) => error,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_plan(
    owner: *const Owner,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INVALID;
    };
    if width.is_null() || height.is_null() {
        return INVALID;
    }
    unsafe {
        width.write(owner.render[0]);
        height.write(owner.render[1]);
    }
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_evaluate(
    owner: *mut Owner,
    commands: Handle,
    frame: *const Frame,
    sharpening_valid: unsafe extern "C" fn(&rsf_sharpening_options) -> i32,
) -> i32 {
    if owner.is_null() || commands.is_null() {
        return INVALID;
    }
    let frame = match unsafe { normalize_frame(frame) } {
        Ok(frame) => frame,
        Err(error) => return error,
    };
    if frame.sharpening.struct_size != 0 && unsafe { sharpening_valid(&frame.sharpening) } == 0 {
        return INVALID;
    }
    let owner = unsafe { &mut *owner };
    let admission = unsafe { frame_admit(&owner.setup, &frame, owner.render) };
    if admission != 0 {
        return admission;
    }
    let record = unsafe { &*frame.record };
    unsafe { (owner.platform.dump_begin)(record, owner.setup.backend) };
    let result = match &mut owner.backend {
        Backend::Dlss(dlss) => dlss.evaluate(commands, &frame, record),
        Backend::Ffx(ffx) => ffx.evaluate(owner.platform, &owner.setup, commands, &frame, record),
        Backend::Xess(xess) => xess.evaluate(commands, &frame),
    };
    unsafe { (owner.platform.dump_result)(record, c"sr".as_ptr(), owner.setup.backend, result) };
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_retire_commands(owner: *mut Owner, commands: Handle) {
    if commands.is_null() {
        return;
    }
    if let Some(owner) = unsafe { owner.as_mut() } {
        match &mut owner.backend {
            Backend::Dlss(dlss) => dlss.retire_commands(commands),
            Backend::Ffx(ffx) => ffx.retire_commands(commands),
            Backend::Xess(xess) => xess.retire_commands(commands),
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_destroy(owner: *mut Owner) -> i32 {
    let Some(raw) = NonNull::new(owner) else {
        return 0;
    };
    let result = match &mut unsafe { &mut *owner }.backend {
        Backend::Dlss(dlss) => dlss.destroy(),
        Backend::Ffx(ffx) => ffx.destroy(),
        Backend::Xess(xess) => xess.destroy(),
    };
    if result == 0 {
        drop(unsafe { Box::from_raw(raw.as_ptr()) });
    }
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_dlss_preset(owner: *mut Owner, preset: u32) -> i32 {
    let Some(owner) = (unsafe { owner.as_mut() }) else {
        return INVALID;
    };
    if let Backend::Dlss(dlss) = &mut owner.backend {
        return dlss.set_preset(preset);
    }
    NOT_SUPPORTED
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_spatial_fallback(
    platform: Option<&Platform>,
    call: *const rsf_srvk_fallback,
) -> i32 {
    let admission = unsafe { fallback_admit(call) };
    if admission != 0 {
        return admission;
    }
    let Some(platform) = platform else {
        return NOT_SUPPORTED;
    };
    let call = unsafe { &*call };
    let mut blit = Blit::default();
    let result = unsafe { (platform.fallback_probe)(call, &mut blit) };
    if result != 0 {
        return result;
    }
    if blit.barrier.is_none() || blit.blit.is_none() || blit.features & LINEAR_BLIT != LINEAR_BLIT {
        return NOT_SUPPORTED;
    }
    unsafe { (platform.fallback_record)(call, &blit) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xess_actual_extent_may_differ_from_the_plan() {
        let device = std::ptr::dangling_mut::<u8>().cast();
        let mut setup = Setup {
            device,
            output_width: 128,
            output_height: 72,
            depth_infinite: 1,
            backend: RSF_SR_XESS,
            ..unsafe { std::mem::zeroed() }
        };
        let mut record: rsf_frame_record = unsafe { std::mem::zeroed() };
        record.struct_size = size_of::<rsf_frame_record>() as u32;
        record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
        record.frame_id = 9;
        record.session_id = 1;
        record.render_width = 64;
        record.render_height = 36;
        record.output_width = 128;
        record.output_height = 72;
        record.frame_time_ms = 16.0;
        record.camera.struct_size = size_of_val(&record.camera) as u32;
        record.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
        record.camera.render_width = 64;
        record.camera.render_height = 36;
        record.camera.output_width = 128;
        record.camera.output_height = 72;
        record.camera.near_plane = 0.1;
        record.camera.vertical_fov_radians = 1.0;
        let image = |id, format| Image {
            device,
            image: id,
            width: 64,
            height: 36,
            format,
            layout: SHADER_READ,
            copy_source: 1,
        };
        let mut frame = Frame {
            record: &record,
            color: image(1, RGBA16_FLOAT),
            depth: image(2, D32_FLOAT),
            motion: image(3, RG16_FLOAT),
            output: Image {
                width: 128,
                height: 72,
                layout: TRANSFER_DST,
                ..image(4, RGBA16_FLOAT)
            },
            motion_scale_x: -1.0,
            motion_scale_y: 1.0,
            pre_exposure: 1.0,
            ..unsafe { std::mem::zeroed() }
        };
        unsafe {
            assert_eq!(frame_admit(&setup, &frame, [80, 45]), 0);
            setup.backend = RSF_SR_FSR3;
            assert_eq!(frame_admit(&setup, &frame, [80, 45]), INVALID);
            setup.backend = RSF_SR_XESS;
            frame.depth.width = 80;
            assert_eq!(frame_admit(&setup, &frame, [80, 45]), INVALID);
        }
    }

    unsafe extern "C" fn promoted() {}
    unsafe extern "C" fn original() {}

    unsafe extern "C" fn adapter(_: Device, name: *const c_char) -> rsf_vk_function {
        match unsafe { CStr::from_ptr(name) }.to_bytes() {
            b"vkGetBufferMemoryRequirements2" => Some(promoted),
            b"vkOther" => Some(original),
            _ => None,
        }
    }

    unsafe extern "C" fn different(_: Device, _: *const c_char) -> rsf_vk_function {
        None
    }

    unsafe extern "C" fn call(
        proc: DeviceProc,
        device: Device,
        name: *const c_char,
        out: *mut rsf_vk_function,
    ) -> i32 {
        unsafe { out.write(proc(device, name)) };
        1
    }

    #[test]
    fn vulkan_dispatch_keeps_aliases_and_retires_only_after_last_registration() {
        let mut device_id = 0u8;
        let device = std::ptr::from_mut(&mut device_id).cast();
        assert_eq!(dispatch_register(device, Some(adapter)), 0);
        assert_eq!(dispatch_register(device, Some(adapter)), 0);
        assert_eq!(dispatch_register(device, Some(different)), WRONG_API);
        let alias = unsafe {
            rsf_core_srvk_dispatch_lookup(
                device,
                c"vkGetBufferMemoryRequirements2KHR".as_ptr(),
                call,
            )
        };
        assert!(alias.is_some_and(|p| std::ptr::fn_addr_eq(p, promoted as unsafe extern "C" fn())));
        dispatch_unregister(device);
        assert!(
            unsafe { rsf_core_srvk_dispatch_lookup(device, c"vkOther".as_ptr(), call) }.is_some()
        );
        dispatch_unregister(device);
        assert!(
            unsafe { rsf_core_srvk_dispatch_lookup(device, c"vkOther".as_ptr(), call) }.is_none()
        );
    }
}

#[cfg(test)]
mod owner_tests;

mod xess;
