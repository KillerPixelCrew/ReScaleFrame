// SPDX-License-Identifier: MIT
//! XeSS Vulkan super resolution. Rust owns the SDK session, context lifetime,
//! admission, sizing, the private image cache, history, capture order and
//! teardown. The native table resolves typed SDK entry points and records Vulkan work.
use super::*;
use crate::vulkan_fg_resources::{ensure_image, rsf_core_vkfg_release_image};
use crate::vulkan_fg_resources_abi::{rsf_vkfg_image as OwnedImage, rsf_vkfg_platform as ImageOps};
use crate::vulkan_sdk::{Loader, SdkModule};
use std::ffi::CString;
use std::sync::{Arc, PoisonError};

const LOAD_FAILED: i32 = -4;
const NOT_READY: i32 = -8;
const FEATURE_FAILED: i32 = -9;

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
const XESS_QUALITY: [u32; 6] = [106, 103, 102, 101, 100, 104];
const REQUIREMENTS_ABI_VERSION: u32 = 1;

const INPUT_USAGE: u32 = SAMPLED_USAGE | TRANSFER_DST_USAGE;
const OUTPUT_USAGE: u32 = STORAGE_USAGE | SAMPLED_USAGE | TRANSFER_DST_USAGE | TRANSFER_SRC_USAGE;

#[repr(C)]
pub(super) struct XessSetup {
    struct_size: u32,
    abi_version: u32,
    runtime_directory_utf8: *const c_char,
}

type Names = *mut *const *const c_char;

// Private platform ABI with native_xess_vulkan.cpp.
#[repr(C)]
pub(super) struct XessOps {
    loader: Loader,
    resolve: unsafe extern "C" fn(Handle, &mut Handle) -> i32,
    pin: unsafe extern "C" fn(Handle) -> u32,
    drop_table: unsafe extern "C" fn(Handle),
    instance_requirements: unsafe extern "C" fn(Handle, *mut u32, Names, *mut u32) -> i32,
    device_extensions: unsafe extern "C" fn(Handle, Handle, Handle, *mut u32, Names) -> i32,
    device_features: unsafe extern "C" fn(Handle, Handle, Handle, *mut Handle) -> i32,
    version: unsafe extern "C" fn(Handle, &mut [u16; 4]) -> i32,
    open: unsafe extern "C" fn(Handle, &mut Handle) -> i32,
    close: unsafe extern "C" fn(Handle),
    create_context: unsafe extern "C" fn(Handle, &Setup) -> i32,
    initialize: unsafe extern "C" fn(Handle, &XessInit) -> i32,
    resolution: unsafe extern "C" fn(Handle, &XessInit, &mut [u32; 6]) -> i32,
    destroy_context: unsafe extern "C" fn(Handle) -> i32,
    bind_device: unsafe extern "C" fn(Handle, &Setup) -> i32,
    image_user: unsafe extern "C" fn(Handle) -> Handle,
    images: &'static ImageOps,
    velocity: unsafe extern "C" fn(Handle, f32, f32) -> i32,
    stage: unsafe extern "C" fn(Handle, Handle, &Image, u64, u32),
    prepare_output: unsafe extern "C" fn(Handle, Handle, u64, u32),
    execute: unsafe extern "C" fn(Handle, Handle, &XessExecute) -> i32,
    copy_output: unsafe extern "C" fn(Handle, Handle, &Image, &Image),
    sharpen: unsafe extern "C" fn(Handle, &Setup, Handle, &Frame, u64) -> i32,
    clear_sharpen: unsafe extern "C" fn(Handle),
    log: unsafe extern "C" fn(&Frame, *const c_char),
    capture_provider: unsafe extern "C" fn(Handle, u64, u32),
    capture_record: Capture,
    capture_parameters: unsafe extern "C" fn(&Frame, u32, u32),
    retire_commands: unsafe extern "C" fn(Handle, Handle),
    flush_capture: unsafe extern "C" fn(Handle),
}

#[repr(C)]
pub(super) struct XessInit {
    width: u32,
    height: u32,
    quality: u32,
    flags: u32,
}

#[repr(C)]
pub(super) struct XessTexture {
    image: u64,
    view: u64,
    width: u32,
    height: u32,
    format: u32,
}

#[repr(C)]
pub(super) struct XessExecute {
    color: XessTexture,
    depth: XessTexture,
    motion: XessTexture,
    output: XessTexture,
    jitter: [f32; 2],
    exposure_scale: f32,
    input_width: u32,
    input_height: u32,
    reset: u32,
}

const _: () = {
    assert!(size_of::<XessOps>() == 31 * 8);
    assert!(size_of::<XessSetup>() == 16 && size_of::<XessInit>() == 16);
    assert!(size_of::<XessTexture>() == 32);
    assert!(size_of::<XessExecute>() == 152);
    assert!(std::mem::offset_of!(XessExecute, reset) == 148);
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
        return INVALID;
    }
    let setup = unsafe { &*setup };
    if setup.abi_version != REQUIREMENTS_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if setup.runtime_directory_utf8.is_null() || unsafe { *setup.runtime_directory_utf8 } == 0 {
        return INVALID;
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
    let mut handle = ptr::null_mut();
    let result = unsafe { (ops.resolve)(module.as_ptr(), &mut handle) };
    if result != 0 {
        return result;
    }
    if handle.is_null() {
        return LOAD_FAILED;
    }
    let library = Library {
        ops,
        handle,
        module,
    };
    // The host can destroy its Vulkan device after retiring the session. The
    // SDK owns storage in the device creation chain that must stay mapped.
    if unsafe { (ops.pin)(library.module.as_ptr()) } == 0 {
        return LOAD_FAILED;
    }
    *session = Some(Arc::new(library));
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xessvk_instance_requirements(
    ops: Option<&'static XessOps>,
    count: *mut u32,
    names: Names,
    minimum_api: *mut u32,
) -> i32 {
    if count.is_null() || names.is_null() || minimum_api.is_null() {
        return INVALID;
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

#[unsafe(no_mangle)]
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
        return INVALID;
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

// A private image and the native objects behind it.
struct Private {
    description: Image,
    allocation: OwnedImage,
    initialized: bool,
}

impl Private {
    fn new(device: Device, [width, height]: [u32; 2], format: u32) -> Self {
        Self {
            description: Image {
                device,
                image: 0,
                width,
                height,
                format,
                layout: GENERAL,
                copy_source: 1,
            },
            allocation: OwnedImage::default(),
            initialized: false,
        }
    }

    fn texture(&self, [width, height]: [u32; 2]) -> XessTexture {
        XessTexture {
            image: self.description.image,
            view: self.allocation.view,
            width,
            height,
            format: self.description.format,
        }
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
    sharpen_failed: bool,
    color: Private,
    depth: Private,
    motion: Private,
    output: Private,
    // Dropped after the native owner, which copied this SDK's entry points.
    _library: Arc<Library>,
}

impl XessOwner {
    pub(super) unsafe fn create(ops: &'static XessOps, setup: &Setup) -> Result<Self, i32> {
        let library = session().clone().ok_or(NOT_READY)?;
        let mut native = ptr::null_mut();
        check(unsafe { (ops.open)(library.handle, &mut native) })?;
        if native.is_null() {
            return Err(INIT_FAILED);
        }
        let device = setup.device;
        let output = [setup.output_width, setup.output_height];
        let mut setup = *setup;
        setup.runtime_directory_utf8 = ptr::null();
        let mut owner = Self {
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
            sharpen_failed: false,
            color: Private::new(device, [0; 2], RGBA16_FLOAT),
            depth: Private::new(device, [0; 2], R32_FLOAT),
            motion: Private::new(device, [0; 2], RG16_FLOAT),
            output: Private::new(device, output, RGBA16_FLOAT),
            _library: library,
        };
        // A refused creation can leave a partial context for the cleanup leaf.
        sdk(unsafe { (ops.create_context)(native, &setup) }, INIT_FAILED)?;
        let init = XessInit {
            width: output[0],
            height: output[1],
            quality: XESS_QUALITY[setup.quality as usize],
            flags: XESS_AUTOEXPOSURE
                | (setup.depth_inverted * XESS_INVERTED_DEPTH)
                | (setup.motion_jittered * XESS_JITTERED_MV),
        };
        sdk(unsafe { (ops.initialize)(native, &init) }, INIT_FAILED)?;
        let mut range = [0; 6];
        if unsafe { (ops.resolution)(native, &init, &mut range) } != XESS_SUCCESS {
            return Err(INIT_FAILED);
        }
        let [plan, minimum, maximum] = [
            [range[0], range[1]],
            [range[2], range[3]],
            [range[4], range[5]],
        ];
        if plan.contains(&0)
            || minimum.contains(&0)
            || (0..2).any(|axis| plan[axis] < minimum[axis] || plan[axis] > maximum[axis])
        {
            return Err(INIT_FAILED);
        }
        (owner.plan, owner.minimum, owner.maximum) = (plan, minimum, maximum);
        check(unsafe { (ops.bind_device)(native, &setup) })?;
        let mut version = [0; 4];
        // A missing version leaves an unidentified provider in capture metadata.
        let _ = unsafe { (ops.version)(owner._library.handle, &mut version) };
        let provider = version
            .iter()
            .fold(0u64, |value, &part| (value << 16) | u64::from(part));
        unsafe { (ops.capture_provider)(native, provider, RSF_SR_XESS) };
        // Inputs cover the SDK maximum; each frame names its active subrectangle.
        for image in [&mut owner.color, &mut owner.depth, &mut owner.motion] {
            [image.description.width, image.description.height] = maximum;
        }
        let image_user = unsafe { (ops.image_user)(native) };
        for (image, usage) in [
            (&mut owner.color, INPUT_USAGE),
            (&mut owner.depth, INPUT_USAGE),
            (&mut owner.motion, INPUT_USAGE),
            (&mut owner.output, OUTPUT_USAGE),
        ] {
            let desc = &mut image.description;
            ensure_image(
                ops.images,
                image_user,
                &mut image.allocation,
                [desc.width, desc.height],
                desc.format,
                usage,
                true,
            )
            .map_err(|_| INIT_FAILED)?;
            desc.image = image.allocation.image;
        }
        Ok(owner)
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
            return INVALID;
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
            let initialized = u32::from(target.initialized);
            unsafe {
                (ops.stage)(
                    native,
                    commands,
                    source,
                    target.description.image,
                    initialized,
                )
            };
            target.initialized = true;
        }
        let initialized = u32::from(self.output.initialized);
        unsafe {
            (ops.prepare_output)(native, commands, self.output.description.image, initialized)
        };
        self.output.initialized = true;

        let render = [record.render_width, record.render_height];
        let output = [record.output_width, record.output_height];
        let reset = u32::from(self.history.needs_reset(record) || self.active != render);
        let call = XessExecute {
            color: self.color.texture(self.maximum),
            depth: self.depth.texture(self.maximum),
            motion: self.motion.texture(self.maximum),
            output: self.output.texture(output),
            jitter: record.camera.jitter_pixels,
            exposure_scale: 1.0 / frame.pre_exposure,
            input_width: render[0],
            input_height: render[1],
            reset,
        };
        if unsafe { (ops.execute)(native, commands, &call) } != XESS_SUCCESS {
            self.history = History::default();
            return FEATURE_FAILED;
        }
        unsafe { (ops.capture_parameters)(frame, RSF_SR_XESS, reset) };
        for (image, role) in [
            (&frame.color, c"sr-source-jittered"),
            (&frame.color, c"sr-linear-color"),
            (&frame.depth, c"sr-depth"),
            (&frame.motion, c"sr-motion-uv"),
        ] {
            self.record_capture(commands, image, record, role, reset);
        }
        let private = Image {
            width: output[0],
            height: output[1],
            ..self.output.description
        };
        if unsafe { (ops.sharpen)(native, &self.setup, commands, frame, private.image) } == 0
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
        self.record_capture(commands, &private, record, c"sr-output-linear", reset);
        unsafe { (ops.copy_output)(native, commands, &private, &frame.output) };
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
        // Public destruction drops the owner only after the SDK accepted it.
        // The remaining best-effort SDK cleanup is for refused construction.
        let ops = self.ops;
        unsafe {
            (ops.clear_sharpen)(self.native);
            if self.context_live {
                (ops.destroy_context)(self.native);
            }
            let image_user = (ops.image_user)(self.native);
            for image in [
                &mut self.color,
                &mut self.depth,
                &mut self.motion,
                &mut self.output,
            ] {
                rsf_core_vkfg_release_image(ops.images, image_user, &mut image.allocation);
            }
            (ops.close)(self.native);
        }
    }
}

unsafe fn owner_admit(
    owner: &XessOwner,
    commands: Handle,
    frame: *const Frame,
) -> Result<Frame, i32> {
    if commands.is_null() {
        return Err(INVALID);
    }
    let frame = unsafe { normalize_frame(frame) }?;
    check(unsafe { frame_admit(&owner.setup, &frame, owner.plan) })?;
    Ok(frame)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xessvk_create(
    ops: Option<&'static XessOps>,
    setup: *const Setup,
    out: *mut *mut XessOwner,
) -> i32 {
    if out.is_null() {
        return INVALID;
    }
    unsafe { out.write(ptr::null_mut()) };
    let admission = unsafe { setup_admit(setup) };
    if admission != 0 {
        return admission;
    }
    if unsafe { (*setup).backend } != RSF_SR_XESS {
        return INVALID;
    }
    let Some(ops) = ops else {
        return NOT_COMPILED;
    };
    match unsafe { XessOwner::create(ops, &*setup) } {
        Ok(owner) => {
            let raw = crate::allocate_handle(owner);
            if raw.is_null() {
                return INIT_FAILED;
            }
            unsafe { out.write(raw) };
            0
        }
        Err(error) => error,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xessvk_plan(
    owner: *const XessOwner,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INVALID;
    };
    if width.is_null() || height.is_null() {
        return INVALID;
    }
    let [w, h] = owner.plan();
    unsafe {
        width.write(w);
        height.write(h);
    }
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xessvk_evaluate(
    owner: *mut XessOwner,
    commands: Handle,
    frame: *const Frame,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_mut() }) else {
        return INVALID;
    };
    match unsafe { owner_admit(owner, commands, frame) } {
        Ok(frame) => owner.evaluate(commands, &frame),
        Err(error) => error,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xessvk_retire_commands(owner: *mut XessOwner, commands: Handle) {
    if commands.is_null() {
        return;
    }
    if let Some(owner) = unsafe { owner.as_ref() } {
        owner.retire_commands(commands);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xessvk_destroy(owner: *mut XessOwner) -> i32 {
    let Some(raw) = NonNull::new(owner) else {
        return 0;
    };
    let result = unsafe { &mut *owner }.destroy();
    if result == 0 {
        drop(unsafe { Box::from_raw(raw.as_ptr()) });
    }
    result
}

#[cfg(test)]
pub(super) mod tests;
