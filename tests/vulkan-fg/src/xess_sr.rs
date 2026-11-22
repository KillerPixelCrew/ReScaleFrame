// SPDX-License-Identifier: MIT
use crate::render_abi::rsf_sharpening_options;
use crate::vulkan_sr_abi::{
    RSF_SR_XESS, RSF_SRVK_ABI_VERSION, rsf_srvk_frame, rsf_srvk_image, rsf_srvk_setup,
};
use crate::xess_requirements_abi::{RSF_XESSVK_REQUIREMENTS_ABI_VERSION, rsf_xessvk_setup};
use rsf_game_sys::{
    RSF_FRAME_FLAG_RESET, RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record,
};
use std::{
    cell::Cell,
    ffi::{CStr, c_char, c_void},
    mem,
    ptr::{null, null_mut},
};

const OK: i32 = 0;
const INVALID_ARGUMENT: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const NOT_READY: i32 = -8;

type Handle = *mut c_void;
type Failure = &'static str;

#[repr(C)]
struct DeviceOps {
    open: unsafe extern "C" fn(*mut rsf_srvk_setup) -> Handle,
    release: unsafe extern "C" fn(Handle),
    allocate: unsafe extern "C" fn(Handle, u32, u32, *mut rsf_srvk_image) -> bool,
    begin: unsafe extern "C" fn(Handle) -> Handle,
    record: unsafe extern "C" fn(Handle, bool, *const f32),
    finish: unsafe extern "C" fn(Handle, *const f32) -> bool,
    drain: unsafe extern "C" fn(Handle) -> i32,
}
const _: () = assert!(size_of::<DeviceOps>() == 7 * size_of::<Handle>());

unsafe extern "C" {
    fn rsf_srvk_create(setup: *const rsf_srvk_setup, owner: *mut Handle) -> i32;
    fn rsf_srvk_plan(owner: Handle, width: *mut u32, height: *mut u32) -> i32;
    fn rsf_srvk_evaluate(owner: Handle, command: Handle, frame: *const rsf_srvk_frame) -> i32;
    fn rsf_srvk_retire_commands(owner: Handle, command: Handle);
    fn rsf_srvk_destroy(owner: Handle) -> i32;
    fn rsf_sharpening_core_defaults() -> rsf_sharpening_options;
    fn getenv(name: *const c_char) -> *const c_char;
    fn rsf_xessvk_initialize(setup: *const rsf_xessvk_setup) -> i32;
    fn rsf_xessvk_instance_requirements(
        count: *mut u32,
        names: *mut *const *const c_char,
        minimum_api: *mut u32,
    ) -> i32;
    fn rsf_xessvk_device_requirements(
        instance: *mut c_void,
        physical: *mut c_void,
        count: *mut u32,
        names: *mut *const *const c_char,
        features: *mut *mut c_void,
    ) -> i32;
    fn rsf_xessvk_shutdown() -> i32;
}

fn require(assertion: bool, message: &'static str) -> Result<(), &'static str> {
    if assertion { Ok(()) } else { Err(message) }
}

fn preflight(setup: &mut rsf_xessvk_setup) -> Result<(), &'static str> {
    unsafe {
        require(
            rsf_xessvk_initialize(null()) == INVALID_ARGUMENT
                && rsf_xessvk_initialize(setup) == INVALID_ARGUMENT,
            "XeSS requirements short/null setup",
        )?;
        setup.struct_size = size_of::<rsf_xessvk_setup>() as u32;
        setup.abi_version += 1;
        require(
            rsf_xessvk_initialize(setup) == ABI_MISMATCH,
            "XeSS requirements ABI mismatch",
        )?;
        setup.abi_version = RSF_XESSVK_REQUIREMENTS_ABI_VERSION;
        require(
            rsf_xessvk_initialize(setup) == INVALID_ARGUMENT,
            "XeSS requirements missing payload path",
        )?;
        require(
            rsf_xessvk_instance_requirements(null_mut(), null_mut(), null_mut())
                == INVALID_ARGUMENT
                && rsf_xessvk_device_requirements(
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                ) == INVALID_ARGUMENT,
            "XeSS requirements reject missing output pointers",
        )?;
        require(
            rsf_xessvk_shutdown() == OK,
            "XeSS requirements empty shutdown",
        )
    }
}

fn run(argc: i32, device_ops: Option<&DeviceOps>) -> Result<i32, &'static str> {
    let mut setup = rsf_xessvk_setup {
        struct_size: size_of::<rsf_xessvk_setup>() as u32 - 1,
        abi_version: RSF_XESSVK_REQUIREMENTS_ABI_VERSION,
        runtime_directory_utf8: null(),
    };
    preflight(&mut setup)?;
    if argc == 1 {
        println!("PASS: XeSS Vulkan requirements ABI guards");
        return Ok(0);
    }
    // Preserve the fixture's existing CRT environment and UTF-8 SDK path contract.
    setup.runtime_directory_utf8 = unsafe { getenv(c"RSF_VULKAN_XESS_RUNTIME".as_ptr()) };
    if setup.runtime_directory_utf8.is_null() {
        println!("SKIP: set RSF_VULKAN_XESS_RUNTIME for XeSS Vulkan device fixture");
        return Ok(77);
    }
    let Some(device_ops) = device_ops else {
        println!("SKIP: built without Vulkan SDK headers");
        return Ok(77);
    };

    require(
        unsafe { rsf_xessvk_initialize(&setup) } == OK,
        "load early XeSS Vulkan session",
    )?;
    struct Session;
    impl Drop for Session {
        fn drop(&mut self) {
            unsafe { rsf_xessvk_shutdown() };
        }
    }
    let _session = Session;
    require(
        unsafe { rsf_xessvk_initialize(&setup) } == NOT_READY,
        "refuse replacing an active XeSS SDK",
    )?;
    device_cases(device_ops, setup.runtime_directory_utf8)?;
    require(
        unsafe { rsf_xessvk_shutdown() } == OK,
        "retire XeSS session after device destruction",
    )?;
    Ok(0)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_xess_sr_main(argc: i32, device_ops: Option<&DeviceOps>) -> i32 {
    match run(argc, device_ops) {
        Ok(result) => result,
        Err(message) => {
            if !message.is_empty() {
                eprintln!("{message}");
            }
            1
        }
    }
}

struct Device<'a> {
    ops: &'a DeviceOps,
    native: Handle,
    retain: Cell<bool>,
}

impl Drop for Device<'_> {
    fn drop(&mut self) {
        if !self.retain.get() {
            unsafe { (self.ops.release)(self.native) };
        }
    }
}

struct Context<'a> {
    device: &'a Device<'a>,
    owner: Handle,
}

impl Context<'_> {
    fn retire(&mut self) -> bool {
        if self.owner.is_null() {
            return true;
        }
        if unsafe { (self.device.ops.drain)(self.device.native) } != OK
            || unsafe { rsf_srvk_destroy(self.owner) } != OK
        {
            return false;
        }
        self.owner = null_mut();
        true
    }
}

impl Drop for Context<'_> {
    fn drop(&mut self) {
        if !self.retire() {
            self.device.retain.set(true);
        }
    }
}

fn device_cases(ops: &DeviceOps, directory: *const c_char) -> Result<(), Failure> {
    // The native leaf fills the device handles and optional proc callbacks.
    let mut setup: rsf_srvk_setup = unsafe { mem::zeroed() };
    setup.struct_size = size_of::<rsf_srvk_setup>() as u32;
    setup.abi_version = RSF_SRVK_ABI_VERSION;
    let native = unsafe { (ops.open)(&mut setup) };
    require(!native.is_null(), "")?;
    let device = Device {
        ops,
        native,
        retain: Cell::new(false),
    };
    setup.backend = RSF_SR_XESS;
    setup.output_width = 320;
    setup.output_height = 180;
    setup.depth_inverted = 1;
    setup.runtime_directory_utf8 = directory;
    for quality in [1, 5] {
        setup.quality = quality;
        quality_case(&device, &setup)?;
    }
    Ok(())
}

fn record(quality: u32, width: u32, height: u32) -> rsf_frame_record {
    let mut record: rsf_frame_record = unsafe { mem::zeroed() };
    record.struct_size = size_of::<rsf_frame_record>() as u32;
    record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.session_id = u64::from(43 + quality);
    record.view_id = 1;
    record.resource_generation = 1;
    record.frame_time_ms = 16.667;
    record.render_width = width;
    record.render_height = height;
    record.output_width = 320;
    record.output_height = 180;
    let camera = &mut record.camera;
    camera.struct_size = size_of_val(camera) as u32;
    camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    camera.render_width = width;
    camera.render_height = height;
    camera.output_width = 320;
    camera.output_height = 180;
    camera.depth_inverted = 1;
    camera.near_plane = 0.1;
    camera.far_plane = 100.0;
    camera.vertical_fov_radians = 1.0;
    for matrix in [
        &mut camera.view_to_clip,
        &mut camera.clip_to_view,
        &mut camera.view_to_world,
        &mut camera.world_to_view,
        &mut camera.clip_to_previous_clip,
    ] {
        for i in [0, 5, 10, 15] {
            matrix[i] = 1.0;
        }
    }
    record
}

unsafe extern "C" fn diagnostic(user: Handle, message: *const c_char) {
    let text = unsafe { CStr::from_ptr(message) };
    if !text.to_bytes().starts_with(b"XeSS input range ") {
        unsafe { *user.cast::<u32>() += 1 };
    }
    println!("SR diagnostic: {}", text.to_string_lossy());
}

fn refused_inputs(owner: Handle, command: Handle, frame: &rsf_srvk_frame) -> Result<(), Failure> {
    // These records contain copied scalar and borrowed-handle fields.
    let reject = |alter: fn(&mut rsf_srvk_frame), message| {
        let mut invalid = unsafe { std::ptr::read(frame) };
        alter(&mut invalid);
        require(
            unsafe { rsf_srvk_evaluate(owner, command, &invalid) } == INVALID_ARGUMENT,
            message,
        )
    };
    reject(
        |f| f.color.device = null_mut(),
        "foreign device image refusal",
    )?;
    reject(|f| f.motion.copy_source = 0, "motion copy usage required")?;
    reject(
        |f| f.depth.image = f.color.image,
        "color/depth alias refused",
    )?;
    reject(
        |f| f.motion.image = f.color.image,
        "color/motion alias refused",
    )?;
    reject(
        |f| f.motion.image = f.depth.image,
        "depth/motion alias refused",
    )?;
    reject(
        |f| f.pre_exposure = f32::from_bits(1),
        "nonfinite reciprocal exposure refused",
    )
}

fn quality_case(device: &Device<'_>, setup: &rsf_srvk_setup) -> Result<(), Failure> {
    let mut context = Context {
        device,
        owner: null_mut(),
    };
    let open = unsafe { rsf_srvk_create(setup, &mut context.owner) };
    println!("Vulkan SR backend {RSF_SR_XESS} create={open}");
    require(
        open == OK && !context.owner.is_null(),
        "create native Vulkan SR context",
    )?;
    let mut width = 0;
    let mut height = 0;
    require(
        unsafe { rsf_srvk_plan(context.owner, &mut width, &mut height) } == OK
            && width != 0
            && height != 0,
        "SR sizing plan",
    )?;
    require(
        unsafe { rsf_xessvk_shutdown() } == NOT_READY,
        "live XeSS context pins SDK module",
    )?;
    let ops = device.ops;
    let native = device.native;
    let mut images: [rsf_srvk_image; 4] = unsafe { mem::zeroed() };
    require(
        unsafe { (ops.allocate)(native, width, height, images.as_mut_ptr()) },
        "",
    )?;
    let mut record = record(setup.quality, width, height);
    let mut fallback_logs = 0u32;
    let mut frame: rsf_srvk_frame = unsafe { mem::zeroed() };
    frame.struct_size = size_of::<rsf_srvk_frame>() as u32;
    frame.abi_version = RSF_SRVK_ABI_VERSION;
    [frame.color, frame.depth, frame.motion, frame.output] = images;
    frame.motion_scale_x = 1.0;
    frame.motion_scale_y = 1.0;
    frame.pre_exposure = 1.0;
    frame.log_user = (&raw mut fallback_logs).cast();
    frame.log = Some(diagnostic);

    for index in 1..=12 {
        let color = if index < 7 {
            [0.18, 0.05, 0.7, 1.0]
        } else {
            [0.05, 0.4, 0.12, 1.0]
        };
        frame.sharpening = unsafe { rsf_sharpening_core_defaults() };
        frame.sharpening.mode = (index - 1) % 4;
        frame.sharpening.sharpness = 0.7;
        frame.exposure.image = u64::from(index == 6 || index == 10);
        let command = unsafe { (ops.begin)(native) };
        require(!command.is_null(), "")?;
        unsafe { rsf_srvk_retire_commands(context.owner, command) };
        println!("live sharpening mode={}", frame.sharpening.mode);
        unsafe { (ops.record)(native, index == 1, color.as_ptr()) };
        record.frame_id = u64::from(index);
        record.flags = if index == 1 || index == 7 {
            RSF_FRAME_FLAG_RESET
        } else {
            0
        };
        frame.record = &record;
        refused_inputs(context.owner, command, &frame)?;
        let result = unsafe { rsf_srvk_evaluate(context.owner, command, &frame) };
        println!(
            "backend={RSF_SR_XESS} frame={index} dispatch={result} reset={}",
            record.flags
        );
        require(result == OK, "Vulkan SR dispatch")?;
        require(unsafe { (ops.finish)(native, color.as_ptr()) }, "")?;
    }
    require(
        fallback_logs == 1,
        "optional failure logs once while frames remain valid",
    )?;
    require(context.retire(), "drain and retire Vulkan SR")?;
    println!("PASS: Vulkan SR backend {RSF_SR_XESS} GPU pixels, reset and image-owner guards");
    Ok(())
}
