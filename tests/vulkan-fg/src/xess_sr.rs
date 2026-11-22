// SPDX-License-Identifier: MIT
use crate::xess_requirements_abi::{RSF_XESSVK_REQUIREMENTS_ABI_VERSION, rsf_xessvk_setup};
use std::{
    ffi::{c_char, c_void},
    ptr::{null, null_mut},
};

const OK: i32 = 0;
const INVALID_ARGUMENT: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const NOT_READY: i32 = -8;

type DeviceRun = unsafe extern "C" fn(*const c_char, u32, u32) -> i32;

unsafe extern "C" {
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

fn run(argc: i32, device_run: Option<DeviceRun>) -> Result<i32, &'static str> {
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
    let Some(device_run) = device_run else {
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
    let result = unsafe { device_run(setup.runtime_directory_utf8, 1, 5) };
    if result != 0 {
        return Ok(result);
    }
    require(
        unsafe { rsf_xessvk_shutdown() } == OK,
        "retire XeSS session after device destruction",
    )?;
    Ok(0)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_xess_sr_main(argc: i32, device_run: Option<DeviceRun>) -> i32 {
    match run(argc, device_run) {
        Ok(result) => result,
        Err(message) => {
            eprintln!("{message}");
            1
        }
    }
}
