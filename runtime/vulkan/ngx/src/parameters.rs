// SPDX-License-Identifier: MIT
use super::{Handle, INVALID, ResultCode, SUCCESS, caller_result};
use std::ffi::c_char;
use std::ptr;

// NGX's pinned abstract class has no virtual destructor. Linux uses declaration
// order, while the DLSS 310.9.1 MSVC import-library wrappers use reverse
// overload order within each group of eight.
const fn overload(index: usize) -> usize {
    if cfg!(windows) { 7 - index } else { index }
}

unsafe fn vtable_slot(parameters: Handle, slot: usize) -> Handle {
    // The caller passes an SDK-owned parameter object of the platform's x64 ABI.
    let table = unsafe { parameters.cast::<*const Handle>().read() };
    unsafe { table.add(slot).read() }
}

macro_rules! accessors {
    ($set_export:literal, $get_export:literal, $set:ident, $get:ident, $value:ty, $index:expr $(, $initial:expr)?) => {
        #[unsafe(export_name = $set_export)]
        unsafe extern "C" fn $set(parameters: Handle, name: *const c_char, value: $value) -> ResultCode {
            if parameters.is_null() || name.is_null() {
                return INVALID;
            }
            let call = unsafe {
                std::mem::transmute::<Handle, unsafe extern "C" fn(Handle, *const c_char, $value)>(
                    vtable_slot(parameters, overload($index)),
                )
            };
            unsafe { call(parameters, name, value) };
            SUCCESS
        }

        #[unsafe(export_name = $get_export)]
        unsafe extern "C" fn $get(parameters: Handle, name: *const c_char, output: *mut $value) -> ResultCode {
            if parameters.is_null() || name.is_null() || output.is_null() {
                return INVALID;
            }
            $(unsafe { output.write($initial) };)?
            let call = unsafe {
                std::mem::transmute::<
                    Handle,
                    unsafe extern "C" fn(Handle, *const c_char, *mut $value) -> ResultCode,
                >(vtable_slot(parameters, 8 + overload($index)))
            };
            caller_result(unsafe { call(parameters, name, output) })
        }
    };
}

accessors!(
    "VulkanStoryNgx_ParameterSetULongLong",
    "VulkanStoryNgx_ParameterGetULongLong",
    set_ull,
    get_ull,
    u64,
    0
);
accessors!(
    "VulkanStoryNgx_ParameterSetFloat",
    "VulkanStoryNgx_ParameterGetFloat",
    set_float,
    get_float,
    f32,
    1
);
accessors!(
    "VulkanStoryNgx_ParameterSetDouble",
    "VulkanStoryNgx_ParameterGetDouble",
    set_double,
    get_double,
    f64,
    2
);
accessors!(
    "VulkanStoryNgx_ParameterSetUInt",
    "VulkanStoryNgx_ParameterGetUInt",
    set_uint,
    get_uint,
    u32,
    3
);
accessors!(
    "VulkanStoryNgx_ParameterSetInt",
    "VulkanStoryNgx_ParameterGetInt",
    set_int,
    get_int,
    i32,
    4
);

// The public pointer getter clears the output before dispatching, unlike the
// scalar getters.
accessors!(
    "VulkanStoryNgx_ParameterSetVoidPointer",
    "VulkanStoryNgx_ParameterGetVoidPointer",
    set_pointer,
    get_pointer,
    Handle,
    7,
    ptr::null_mut()
);

unsafe fn get_uint_or(parameters: Handle, name: &std::ffi::CStr, fallback: u32) -> u32 {
    let mut value = 0;
    if unsafe { get_uint(parameters, name.as_ptr(), &mut value) } == SUCCESS {
        value
    } else {
        fallback
    }
}

#[unsafe(export_name = "VulkanStoryNgx_DlssGetOptimalSettings")]
unsafe extern "C" fn optimal_settings(
    parameters: Handle,
    width: u32,
    height: u32,
    quality: i32,
    optimal_width: *mut u32,
    optimal_height: *mut u32,
    min_width: *mut u32,
    min_height: *mut u32,
    max_width: *mut u32,
    max_height: *mut u32,
    sharpness: *mut f32,
) -> ResultCode {
    const OUT_OF_DATE: ResultCode = 0xBAD0_000C;
    if parameters.is_null() {
        return INVALID;
    }
    let mut callback = ptr::null_mut();
    let lookup = unsafe {
        get_pointer(
            parameters,
            c"DLSSOptimalSettingsCallback".as_ptr(),
            &mut callback,
        )
    };
    if callback.is_null() {
        return if lookup == SUCCESS {
            OUT_OF_DATE
        } else {
            lookup
        };
    }
    unsafe {
        set_uint(parameters, c"Width".as_ptr(), width);
        set_uint(parameters, c"Height".as_ptr(), height);
        set_int(parameters, c"PerfQualityValue".as_ptr(), quality);
        set_int(parameters, c"RTXValue".as_ptr(), 0);
    }
    let call = unsafe {
        std::mem::transmute::<Handle, unsafe extern "C" fn(Handle) -> ResultCode>(callback)
    };
    let result = caller_result(unsafe { call(parameters) });
    if result != SUCCESS {
        return result;
    }

    let width = unsafe { get_uint_or(parameters, c"OutWidth", 0) };
    let height = unsafe { get_uint_or(parameters, c"OutHeight", 0) };
    for (output, value) in [(optimal_width, width), (optimal_height, height)] {
        if !output.is_null() {
            unsafe { output.write(value) };
        }
    }
    for (output, name, fallback) in [
        (min_width, c"DLSS.Get.Dynamic.Min.Render.Width", width),
        (min_height, c"DLSS.Get.Dynamic.Min.Render.Height", height),
        (max_width, c"DLSS.Get.Dynamic.Max.Render.Width", width),
        (max_height, c"DLSS.Get.Dynamic.Max.Render.Height", height),
    ] {
        if !output.is_null() {
            unsafe { output.write(get_uint_or(parameters, name, fallback)) };
        }
    }
    if !sharpness.is_null() {
        let mut value = 0.0;
        if unsafe { get_float(parameters, c"Sharpness".as_ptr(), &mut value) } != SUCCESS {
            value = 0.0;
        }
        unsafe { sharpness.write(value) };
    }
    result
}
