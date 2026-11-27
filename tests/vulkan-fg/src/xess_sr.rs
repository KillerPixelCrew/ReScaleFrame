// SPDX-License-Identifier: MIT
use crate::sys::rsf_sharpening_options;
use crate::sys::{RSF_SR_XESS, RSF_SRVK_ABI_VERSION, rsf_srvk_frame, rsf_srvk_setup};
use crate::sys::{RSF_XESSVK_REQUIREMENTS_ABI_VERSION, rsf_xessvk_setup};
use rsf_game_sys::backend_result::{ABI_MISMATCH, INVALID_ARGUMENT, NOT_READY, OK};
use rsf_game_sys::{RSF_FRAME_FLAG_RESET, rsf_frame_record};
use rsf_test_support::{frame_record, sr_camera};
use std::{
    ffi::{CStr, c_char, c_void},
    mem,
    ptr::{null, null_mut},
};

type Handle = *mut c_void;
use crate::xess_sr_gpu::Gpu;

pub(super) type Failure = &'static str;

unsafe extern "C" {
    fn rsf_srvk_create(setup: *const rsf_srvk_setup, owner: *mut Handle) -> i32;
    fn rsf_srvk_plan(owner: Handle, width: *mut u32, height: *mut u32) -> i32;
    fn rsf_srvk_evaluate(owner: Handle, command: Handle, frame: *const rsf_srvk_frame) -> i32;
    fn rsf_srvk_retire_commands(owner: Handle, command: Handle);
    fn rsf_srvk_destroy(owner: Handle) -> i32;
    fn rsf_sharpening_core_defaults() -> rsf_sharpening_options;
    fn getenv(name: *const c_char) -> *const c_char;
    fn rsf_xessvk_initialize(setup: *const rsf_xessvk_setup) -> i32;
    pub(super) fn rsf_xessvk_instance_requirements(
        count: *mut u32,
        names: *mut *const *const c_char,
        minimum_api: *mut u32,
    ) -> i32;
    pub(super) fn rsf_xessvk_device_requirements(
        instance: *mut c_void,
        physical: *mut c_void,
        count: *mut u32,
        names: *mut *const *const c_char,
        features: *mut *mut c_void,
    ) -> i32;
    fn rsf_xessvk_shutdown() -> i32;
}

pub(super) fn require(assertion: bool, message: &'static str) -> Result<(), &'static str> {
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

fn run(argc: i32, device_available: bool) -> Result<i32, &'static str> {
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
    if !device_available {
        println!("SKIP: built without Vulkan SDK headers");
        return Ok(77);
    }

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
    device_cases(setup.runtime_directory_utf8)?;
    require(
        unsafe { rsf_xessvk_shutdown() } == OK,
        "retire XeSS session after device destruction",
    )?;
    Ok(0)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_xess_sr_main(argc: i32, device_available: bool) -> i32 {
    match run(argc, device_available) {
        Ok(result) => result,
        Err(message) => {
            if !message.is_empty() {
                eprintln!("{message}");
            }
            1
        }
    }
}

struct Context<'a> {
    device: &'a mut Option<Gpu>,
    owner: Handle,
}

impl Context<'_> {
    fn retire(&mut self) -> bool {
        if self.owner.is_null() {
            return true;
        }
        if self.device.as_ref().unwrap().drain().is_err()
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
            mem::forget(self.device.take());
        }
    }
}

fn device_cases(directory: *const c_char) -> Result<(), Failure> {
    // The Vulkan fixture fills the device handles and proc callbacks.
    let mut setup: rsf_srvk_setup = unsafe { mem::zeroed() };
    setup.struct_size = size_of::<rsf_srvk_setup>() as u32;
    setup.abi_version = RSF_SRVK_ABI_VERSION;
    let mut device = Some(Gpu::open(&mut setup)?);
    setup.backend = RSF_SR_XESS;
    setup.output_width = 320;
    setup.output_height = 180;
    setup.depth_inverted = 1;
    setup.runtime_directory_utf8 = directory;
    for quality in [1, 5] {
        setup.quality = quality;
        quality_case(&mut device, &setup)?;
    }
    Ok(())
}

fn record(quality: u32, width: u32, height: u32) -> rsf_frame_record {
    let mut record = rsf_frame_record {
        resource_generation: 1,
        frame_time_ms: 16.667,
        ..frame_record(u64::from(43 + quality), 0, 1, [width, height], [320, 180])
    };
    sr_camera(&mut record, 0.1, 100.0);
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

fn quality_case(device: &mut Option<Gpu>, setup: &rsf_srvk_setup) -> Result<(), Failure> {
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
    let gpu = context.device.as_mut().unwrap();
    let images = gpu.allocate_images(width, height)?;
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
        let command = gpu.begin()?;
        unsafe { rsf_srvk_retire_commands(context.owner, command) };
        println!("live sharpening mode={}", frame.sharpening.mode);
        gpu.record(index == 1, color);
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
        gpu.finish_and_verify(color)?;
    }
    require(
        fallback_logs == 1,
        "optional failure logs once while frames remain valid",
    )?;
    require(context.retire(), "drain and retire Vulkan SR")?;
    println!("PASS: Vulkan SR backend {RSF_SR_XESS} GPU pixels, reset and image-owner guards");
    Ok(())
}
