// SPDX-License-Identifier: MIT
//! XeSS Vulkan super resolution. Rust owns SDK export resolution, the SDK session,
//! context lifetime, admission, sizing, the private image cache, history, capture
//! order and teardown. Rust plans transfers and the table in native.rs performs
//! the typed Vulkan and SDK calls.
use super::*;
use crate::frame_capture_vk::sr_capture_inputs;
use crate::sdk_module::{Loader, SdkModule};
use crate::sys::{rsf_vkfg_image as OwnedImage, rsf_vkfg_platform as ImageOps};
use crate::vulkan_fg_copy::{
    Barrier, BarrierFn, CopyFn, ImageCopy, TRANSFER_SRC, UNDEFINED, record_shader_read_copy,
};
use crate::vulkan_fg_resources::{ensure_image, rsf_core_vkfg_release_image};
use rsf_game_sys::backend_result::{FEATURE_FAILED, LOAD_FAILED, MISSING_ENTRY_POINT};
use std::ffi::CString;
use std::sync::{Arc, PoisonError};

mod graphics;
mod native;

#[path = "xess_sdk_abi.rs"]
mod sdk_abi;
use crate::sys::{
    RSF_XESSVK_REQUIREMENTS_ABI_VERSION as REQUIREMENTS_ABI_VERSION, rsf_xessvk_setup as XessSetup,
};
use sdk_abi::{
    VkImageSubresourceRange, xess_2d_t, xess_version_t, xess_vk_execute_params_t as XessExecute,
    xess_vk_image_view_info as XessTexture, xess_vk_init_params_t as XessInit,
};

// SDK C ABI values, checked against xess.h by the native leaf.
const XESS_SUCCESS: i32 = 0;
const XESS_UNSUPPORTED_DEVICE: i32 = -1;
const XESS_UNSUPPORTED_DRIVER: i32 = -2;
const XESS_NOT_IMPLEMENTED: i32 = -7;
const XESS_UNSUPPORTED: i32 = -10;
const XESS_INVERTED_DEPTH: u32 = 1 << 1;
const XESS_JITTERED_MV: u32 = 1 << 7;
const XESS_AUTOEXPOSURE: u32 = 1 << 8;
// RSF quality 0..5: native AA, quality, balanced, performance, ultra performance, ultra quality.
const XESS_QUALITY: [sdk_abi::xess_quality_settings_t; 6] = [
    sdk_abi::XESS_QUALITY_SETTING_AA,
    sdk_abi::XESS_QUALITY_SETTING_QUALITY,
    sdk_abi::XESS_QUALITY_SETTING_BALANCED,
    sdk_abi::XESS_QUALITY_SETTING_PERFORMANCE,
    sdk_abi::XESS_QUALITY_SETTING_ULTRA_PERFORMANCE,
    sdk_abi::XESS_QUALITY_SETTING_ULTRA_QUALITY,
];

const INPUT_USAGE: u32 = SAMPLED_USAGE | TRANSFER_DST_USAGE;
const OUTPUT_USAGE: u32 = STORAGE_USAGE | SAMPLED_USAGE | TRANSFER_DST_USAGE | TRANSFER_SRC_USAGE;

type Names = *mut *const *const c_char;

const REQUIRED_EXPORTS: [&CStr; 10] = [
    c"xessVKGetRequiredInstanceExtensions",
    c"xessVKGetRequiredDeviceExtensions",
    c"xessVKGetRequiredDeviceFeatures",
    c"xessVKCreateContext",
    c"xessVKInit",
    c"xessVKExecute",
    c"xessDestroyContext",
    c"xessGetOptimalInputResolution",
    c"xessSetVelocityScale",
    c"xessGetVersion",
];
type Entries = [Handle; REQUIRED_EXPORTS.len()];
type ExportLookup = unsafe extern "C" fn(Handle, *const c_char) -> Handle;

// Private platform ABI. native.rs supplies the production table and fixtures copy it.
#[repr(C)]
pub(super) struct XessOps {
    loader: Loader,
    allocate_table: unsafe extern "C" fn(&mut Handle) -> i32,
    pin: unsafe extern "C" fn(Handle) -> u32,
    drop_table: unsafe extern "C" fn(Handle),
    instance_requirements: unsafe extern "C" fn(Handle, *mut u32, Names, *mut u32) -> i32,
    device_extensions: unsafe extern "C" fn(Handle, Handle, Handle, *mut u32, Names) -> i32,
    device_features: unsafe extern "C" fn(Handle, Handle, Handle, *mut Handle) -> i32,
    version: unsafe extern "C" fn(Handle, &mut xess_version_t) -> i32,
    open: unsafe extern "C" fn(Handle, &mut Handle) -> i32,
    close: unsafe extern "C" fn(Handle),
    create_context: unsafe extern "C" fn(Handle, &Setup) -> i32,
    initialize: unsafe extern "C" fn(Handle, &XessInit) -> i32,
    resolution: unsafe extern "C" fn(Handle, &XessInit, *mut xess_2d_t) -> i32,
    destroy_context: unsafe extern "C" fn(Handle) -> i32,
    bind_device: unsafe extern "C" fn(Handle, &Setup) -> i32,
    image_user: unsafe extern "C" fn(Handle) -> Handle,
    images: &'static ImageOps,
    velocity: unsafe extern "C" fn(Handle, f32, f32) -> i32,
    barrier: BarrierFn,
    copy: CopyFn,
    execute: unsafe extern "C" fn(Handle, Handle, &XessExecute) -> i32,
    sharpen: unsafe extern "C" fn(Handle, &Setup, Handle, &Frame, u64) -> i32,
    clear_sharpen: unsafe extern "C" fn(Handle),
    capture_provider: unsafe extern "C" fn(Handle, u64, u32),
    capture_record: Capture,
    capture_parameters: unsafe extern "C" fn(&Frame, u32, u32),
    retire_commands: unsafe extern "C" fn(Handle, Handle),
    flush_capture: unsafe extern "C" fn(Handle),
    bind_entries: unsafe extern "C" fn(Handle, &Entries),
    // Fixtures supply inert addresses. Production uses the loaded SDK module.
    lookup: Option<ExportLookup>,
}

const _: () = {
    assert!(size_of::<XessOps>() == 31 * 8);
    assert!(std::mem::offset_of!(XessOps, barrier) == 19 * 8);
    assert!(std::mem::offset_of!(XessOps, copy) == 20 * 8);
    assert!(std::mem::offset_of!(XessOps, execute) == 21 * 8);
    assert!(std::mem::offset_of!(XessOps, sharpen) == 22 * 8);
    assert!(std::mem::offset_of!(XessOps, bind_entries) == 29 * 8);
    assert!(std::mem::offset_of!(XessOps, lookup) == 30 * 8);
    assert!(size_of::<Entries>() == 10 * 8);
    assert!(size_of::<XessSetup>() == 16);
};

fn sdk(result: i32, failure: i32) -> Result<(), i32> {
    match result {
        XESS_SUCCESS => Ok(()),
        XESS_UNSUPPORTED_DEVICE
        | XESS_UNSUPPORTED_DRIVER
        | XESS_UNSUPPORTED
        | XESS_NOT_IMPLEMENTED => Err(NOT_SUPPORTED),
        _ => Err(failure),
    }
}

fn status(result: Result<(), i32>) -> i32 {
    result.err().unwrap_or(0)
}

// The loaded SDK module. Contexts hold a reference, so shutdown can refuse
// while any context still needs the SDK.
struct Library {
    ops: &'static XessOps,
    handle: Handle,
    module: SdkModule,
}

// The native handle is immutable SDK entry-point storage.
unsafe impl Send for Library {}
unsafe impl Sync for Library {}

impl Library {
    fn resolve(ops: &'static XessOps, module: SdkModule) -> Result<Self, i32> {
        let mut handle = ptr::null_mut();
        check(unsafe { (ops.allocate_table)(&mut handle) })?;
        if handle.is_null() {
            return Err(LOAD_FAILED);
        }
        let library = Self {
            ops,
            handle,
            module,
        };
        let mut entries = [ptr::null_mut(); REQUIRED_EXPORTS.len()];
        for (entry, name) in entries.iter_mut().zip(REQUIRED_EXPORTS) {
            *entry = match ops.lookup {
                Some(lookup) => unsafe { lookup(library.module.as_ptr(), name.as_ptr()) },
                None => library
                    .module
                    .symbol(name)
                    .map_or(ptr::null_mut(), |function| function as *const () as Handle),
            };
            if entry.is_null() {
                return Err(MISSING_ENTRY_POINT);
            }
        }
        unsafe { (ops.bind_entries)(handle, &entries) };
        Ok(library)
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        unsafe { (self.ops.drop_table)(self.handle) };
    }
}

static SESSION: Mutex<Option<Arc<Library>>> = Mutex::new(None);

fn session() -> std::sync::MutexGuard<'static, Option<Arc<Library>>> {
    SESSION.lock().unwrap_or_else(PoisonError::into_inner)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xessvk_initialize(
    ops: Option<&'static XessOps>,
    setup: *const XessSetup,
) -> i32 {
    if !unsafe { has_capacity(setup) } {
        return INVALID_ARGUMENT;
    }
    let setup = unsafe { &*setup };
    if setup.abi_version != REQUIREMENTS_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if setup.runtime_directory_utf8.is_null() || unsafe { *setup.runtime_directory_utf8 } == 0 {
        return INVALID_ARGUMENT;
    }
    let Some(ops) = ops else {
        return NOT_COMPILED;
    };
    let mut session = session();
    if session.is_some() {
        return NOT_READY;
    }
    let module =
        match unsafe { SdkModule::open(ops.loader, setup.runtime_directory_utf8, "libxess.dll") } {
            Ok(module) => module,
            Err(result) => return result,
        };
    let library = match Library::resolve(ops, module) {
        Ok(library) => library,
        Err(result) => return result,
    };
    // The host can destroy its Vulkan device after retiring the session. The
    // SDK owns storage in the device creation chain that must stay mapped.
    if unsafe { (ops.pin)(library.module.as_ptr()) } == 0 {
        return LOAD_FAILED;
    }
    *session = Some(Arc::new(library));
    0
}

unsafe extern "C" fn rsf_core_xessvk_instance_requirements(
    ops: Option<&'static XessOps>,
    count: *mut u32,
    names: Names,
    minimum_api: *mut u32,
) -> i32 {
    if count.is_null() || names.is_null() || minimum_api.is_null() {
        return INVALID_ARGUMENT;
    }
    unsafe {
        count.write(0);
        names.write(ptr::null());
        minimum_api.write(0);
    }
    if ops.is_none() {
        return NOT_COMPILED;
    }
    let session = session();
    let Some(library) = session.as_ref() else {
        return NOT_READY;
    };
    let result =
        unsafe { (library.ops.instance_requirements)(library.handle, count, names, minimum_api) };
    status(sdk(result, INIT_FAILED))
}

unsafe extern "C" fn rsf_core_xessvk_device_requirements(
    ops: Option<&'static XessOps>,
    instance: Handle,
    physical: Handle,
    count: *mut u32,
    names: Names,
    features: *mut Handle,
) -> i32 {
    if instance.is_null()
        || physical.is_null()
        || count.is_null()
        || names.is_null()
        || features.is_null()
    {
        return INVALID_ARGUMENT;
    }
    unsafe {
        count.write(0);
        names.write(ptr::null());
    }
    if ops.is_none() {
        return NOT_COMPILED;
    }
    let session = session();
    let Some(library) = session.as_ref() else {
        return NOT_READY;
    };
    let ops = library.ops;
    let result =
        unsafe { (ops.device_extensions)(library.handle, instance, physical, count, names) };
    if result != XESS_SUCCESS {
        return status(sdk(result, INIT_FAILED));
    }
    let result = unsafe { (ops.device_features)(library.handle, instance, physical, features) };
    status(sdk(result, INIT_FAILED))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xessvk_shutdown(ops: Option<&'static XessOps>) -> i32 {
    if ops.is_none() {
        return 0;
    }
    let mut session = session();
    if session
        .as_ref()
        .is_some_and(|library| Arc::strong_count(library) != 1)
    {
        return NOT_READY;
    }
    drop(session.take());
    0
}

fn texture(image: &OwnedImage) -> XessTexture {
    XessTexture {
        image: image.image as Handle,
        imageView: image.view as Handle,
        subresourceRange: VkImageSubresourceRange {
            aspectMask: ash::vk::ImageAspectFlags::COLOR.as_raw(),
            baseMipLevel: 0,
            levelCount: 1,
            baseArrayLayer: 0,
            layerCount: 1,
        },
        width: image.width,
        height: image.height,
        format: image.format as i32,
    }
}

pub(super) struct XessOwner {
    ops: &'static XessOps,
    native: Handle,
    setup: Setup,
    context_live: bool,
    plan: [u32; 2],
    minimum: [u32; 2],
    maximum: [u32; 2],
    active: [u32; 2],
    history: History,
    range_logged: bool,
    color: OwnedImage,
    depth: OwnedImage,
    motion: OwnedImage,
    output: OwnedImage,
    // Dropped after the native owner, which copied this SDK's entry points.
    _library: Arc<Library>,
}

impl XessOwner {
    pub(super) unsafe fn create(
        ops: &'static XessOps,
        setup: &Setup,
    ) -> Result<Box<Self>, CreateError<Box<Self>>> {
        let library = session().clone().ok_or(NOT_READY)?;
        let mut native = ptr::null_mut();
        check(unsafe { (ops.open)(library.handle, &mut native) })?;
        if native.is_null() {
            return Err(INIT_FAILED.into());
        }
        let output = [setup.output_width, setup.output_height];
        let mut setup = *setup;
        setup.runtime_directory_utf8 = ptr::null();
        let mut owner = Box::new(Self {
            ops,
            native,
            setup,
            context_live: true,
            plan: [0; 2],
            minimum: [0; 2],
            maximum: [0; 2],
            active: [0; 2],
            history: History::default(),
            range_logged: false,
            color: OwnedImage::default(),
            depth: OwnedImage::default(),
            motion: OwnedImage::default(),
            output: OwnedImage::default(),
            _library: library,
        });
        match unsafe { owner.initialize(output) } {
            Ok(()) => Ok(owner),
            Err(status) => {
                let cleanup = (owner.destroy_context() != 0).then_some(owner);
                Err(CreateError { status, cleanup })
            }
        }
    }

    unsafe fn initialize(&mut self, output: [u32; 2]) -> Result<(), i32> {
        let (ops, native, setup) = (self.ops, self.native, self.setup);
        // A refused creation can leave a partial context for the cleanup leaf.
        sdk(unsafe { (ops.create_context)(native, &setup) }, INIT_FAILED)?;
        let init = XessInit {
            outputResolution: xess_2d_t {
                x: output[0],
                y: output[1],
            },
            qualitySetting: XESS_QUALITY[setup.quality as usize],
            initFlags: XESS_AUTOEXPOSURE
                | (setup.depth_inverted * XESS_INVERTED_DEPTH)
                | (setup.motion_jittered * XESS_JITTERED_MV),
            ..Default::default()
        };
        sdk(unsafe { (ops.initialize)(native, &init) }, INIT_FAILED)?;
        let mut range = [xess_2d_t::default(); 3];
        if unsafe { (ops.resolution)(native, &init, range.as_mut_ptr()) } != XESS_SUCCESS {
            return Err(INIT_FAILED);
        }
        let [plan, minimum, maximum] = range.map(|size| [size.x, size.y]);
        if plan.contains(&0)
            || minimum.contains(&0)
            || (0..2).any(|axis| plan[axis] < minimum[axis] || plan[axis] > maximum[axis])
        {
            return Err(INIT_FAILED);
        }
        (self.plan, self.minimum, self.maximum) = (plan, minimum, maximum);
        check(unsafe { (ops.bind_device)(native, &setup) })?;
        let mut version = xess_version_t::default();
        // A missing version leaves an unidentified provider in capture metadata.
        let _ = unsafe { (ops.version)(self._library.handle, &mut version) };
        let provider = [
            version.major,
            version.minor,
            version.patch,
            version.reserved,
        ]
        .into_iter()
        .fold(0u64, |value, part| (value << 16) | u64::from(part));
        unsafe { (ops.capture_provider)(native, provider, RSF_SR_XESS) };
        // Inputs cover the SDK maximum; each frame names its active subrectangle.
        let image_user = unsafe { (ops.image_user)(native) };
        for (image, extent, format, usage) in [
            (&mut self.color, maximum, RGBA16_FLOAT, INPUT_USAGE),
            (&mut self.depth, maximum, R32_FLOAT, INPUT_USAGE),
            (&mut self.motion, maximum, RG16_FLOAT, INPUT_USAGE),
            (&mut self.output, output, RGBA16_FLOAT, OUTPUT_USAGE),
        ] {
            ensure_image(ops.images, image_user, image, extent, format, usage, true)
                .map_err(|_| INIT_FAILED)?;
        }
        Ok(())
    }

    pub(super) fn plan(&self) -> [u32; 2] {
        self.plan
    }

    fn log_range(&mut self, frame: &Frame, record: &rsf_frame_record) {
        let Some(log) = frame.log else { return };
        if self.range_logged {
            return;
        }
        let message = format!(
            "XeSS input range recommended {}x{} minimum {}x{} maximum {}x{} actual {}x{}",
            self.plan[0],
            self.plan[1],
            self.minimum[0],
            self.minimum[1],
            self.maximum[0],
            self.maximum[1],
            record.render_width,
            record.render_height
        );
        if let Ok(message) = CString::new(message) {
            unsafe { log(frame.log_user, message.as_ptr()) };
        }
        self.range_logged = true;
    }

    fn admit(&self, frame: &Frame, record: &rsf_frame_record) -> bool {
        let render = [record.render_width, record.render_height];
        let jitter = record.camera.jitter_pixels;
        let guides = [&frame.color, &frame.depth, &frame.motion];
        (0..2).all(|axis| (self.minimum[axis]..=self.maximum[axis]).contains(&render[axis]))
            && (1.0 / frame.pre_exposure).is_finite()
            && jitter.iter().all(|offset| offset.abs() <= 0.5)
            && frame.depth.format == R32_FLOAT
            && guides.iter().all(|image| image.copy_source != 0)
            && guides.iter().enumerate().all(|(index, image)| {
                image.image != frame.output.image
                    && guides[index + 1..]
                        .iter()
                        .all(|other| other.image != image.image)
            })
    }

    pub(super) fn evaluate(&mut self, commands: Handle, frame: &Frame) -> i32 {
        let record = unsafe { &*frame.record };
        self.log_range(frame, record);
        if !self.admit(frame, record) {
            return INVALID_ARGUMENT;
        }
        let ops = self.ops;
        let native = self.native;
        if unsafe { (ops.velocity)(native, frame.motion_scale_x, frame.motion_scale_y) }
            != XESS_SUCCESS
        {
            return FEATURE_FAILED;
        }
        // Copy leased guides into the SDK-sized cache. The source returns to
        // shader read before the caller's later passes.
        for (source, target) in [
            (&frame.color, &mut self.color),
            (&frame.depth, &mut self.depth),
            (&frame.motion, &mut self.motion),
        ] {
            unsafe {
                record_shader_read_copy(
                    ops.barrier,
                    ops.copy,
                    native,
                    commands,
                    source.image,
                    target,
                    [source.width, source.height],
                )
            };
            target.initialized = 1;
        }
        let output_image = self.output.image;
        let output_before = if self.output.initialized == 0 {
            UNDEFINED
        } else {
            GENERAL
        };
        let barrier = |plan: Barrier| unsafe { (ops.barrier)(native, commands, &plan) };
        barrier(Barrier::same_queue(output_image, output_before, GENERAL));
        self.output.initialized = 1;

        let render = [record.render_width, record.render_height];
        let output = [record.output_width, record.output_height];
        let reset = u32::from(self.history.needs_reset(record) || self.active != render);
        let call = XessExecute {
            colorTexture: texture(&self.color),
            depthTexture: texture(&self.depth),
            velocityTexture: texture(&self.motion),
            outputTexture: texture(&self.output),
            jitterOffsetX: record.camera.jitter_pixels[0],
            jitterOffsetY: record.camera.jitter_pixels[1],
            exposureScale: 1.0 / frame.pre_exposure,
            inputWidth: render[0],
            inputHeight: render[1],
            resetHistory: reset,
            ..Default::default()
        };
        if unsafe { (ops.execute)(native, commands, &call) } != XESS_SUCCESS {
            self.history = History::default();
            return FEATURE_FAILED;
        }
        unsafe { (ops.capture_parameters)(frame, RSF_SR_XESS, reset) };
        for (image, role) in sr_capture_inputs(frame) {
            self.record_capture(commands, image, record, role, reset);
        }
        let private = image_descriptor(self.setup.device, &self.output, GENERAL);
        self.record_capture(commands, &private, record, c"sr-output-linear", reset);
        unsafe { (ops.sharpen)(native, &self.setup, commands, frame, private.image) };
        // The caller already supplied its output in TRANSFER_DST_OPTIMAL.
        barrier(Barrier::same_queue(output_image, GENERAL, TRANSFER_SRC));
        unsafe {
            (ops.copy)(
                native,
                commands,
                &ImageCopy::transfer(output_image, frame.output.image, output),
            )
        };
        barrier(Barrier::same_queue(output_image, TRANSFER_SRC, GENERAL));
        self.active = render;
        self.history.commit(record);
        0
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
            (self.ops.capture_record)(self.native, commands, image, record, role.as_ptr(), reset)
        };
    }

    pub(super) fn retire_commands(&self, commands: Handle) {
        // The host calls this after GPU completion and reset of the buffer.
        unsafe { (self.ops.retire_commands)(self.native, commands) };
    }

    pub(super) fn destroy(&mut self) -> i32 {
        // The ABI requires the caller to drain all work referencing this owner.
        unsafe { (self.ops.flush_capture)(self.native) };
        self.destroy_context()
    }

    fn destroy_context(&mut self) -> i32 {
        if self.context_live {
            if unsafe { (self.ops.destroy_context)(self.native) } != XESS_SUCCESS {
                return FEATURE_FAILED;
            }
            self.context_live = false;
        }
        0
    }
}

impl Drop for XessOwner {
    fn drop(&mut self) {
        debug_assert!(!self.context_live);
        let ops = self.ops;
        unsafe {
            (ops.clear_sharpen)(self.native);
            let image_user = (ops.image_user)(self.native);
            for image in [
                &mut self.color,
                &mut self.depth,
                &mut self.motion,
                &mut self.output,
            ] {
                rsf_core_vkfg_release_image(ops.images, image_user, image);
            }
            (ops.close)(self.native);
        }
    }
}

/// The production table when CMake found the XeSS Vulkan SDK headers (the xess-vk feature).
pub(super) const fn native_ops() -> Option<&'static XessOps> {
    if cfg!(feature = "xess-vk") {
        Some(&native::NATIVE_OPS)
    } else {
        None
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_xessvk_initialize(setup: *const XessSetup) -> i32 {
    unsafe { rsf_core_xessvk_initialize(native_ops(), setup) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_xessvk_instance_requirements(
    count: *mut u32,
    names: Names,
    minimum_api: *mut u32,
) -> i32 {
    unsafe { rsf_core_xessvk_instance_requirements(native_ops(), count, names, minimum_api) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_xessvk_device_requirements(
    instance: Handle,
    physical: Handle,
    count: *mut u32,
    names: Names,
    features: *mut Handle,
) -> i32 {
    unsafe {
        rsf_core_xessvk_device_requirements(native_ops(), instance, physical, count, names, features)
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_xessvk_shutdown() -> i32 {
    unsafe { rsf_core_xessvk_shutdown(native_ops()) }
}
#[cfg(test)]
pub(super) mod tests;
