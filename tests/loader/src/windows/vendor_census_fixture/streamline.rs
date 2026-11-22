// SPDX-License-Identifier: MIT
use super::{ConstPtr, Ptr};
use std::ffi::{CStr, c_char};

extern "C" fn reflex_sleep(_: ConstPtr) -> i32 {
    0x31
}
extern "C" fn pcl_marker(_: u32, _: ConstPtr) -> i32 {
    0x32
}
extern "C" fn dlssg_options(_: ConstPtr, _: ConstPtr) -> i32 {
    0x33
}

fn lookup(name: &CStr) -> Option<Ptr> {
    match name.to_bytes() {
        b"slReflexSleep" => Some(reflex_sleep as Ptr),
        b"slPCLSetMarker" => Some(pcl_marker as Ptr),
        b"slDLSSGSetOptions" => Some(dlssg_options as Ptr),
        _ => None,
    }
}

#[unsafe(no_mangle)]
extern "C" fn slInit(_: ConstPtr, _: u64) -> i32 {
    0
}

#[unsafe(no_mangle)]
extern "C" fn slSetConstants(_: ConstPtr, _: ConstPtr, _: ConstPtr) -> i32 {
    0x34
}

#[unsafe(no_mangle)]
unsafe extern "C" fn slGetFeatureFunction(_: u32, name: *const c_char, out: *mut Ptr) -> i32 {
    let Some(function) = lookup(unsafe { CStr::from_ptr(name) }) else {
        return 7;
    };
    unsafe { out.write(function) };
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn fixture_sl_raw(name: *const c_char) -> Ptr {
    lookup(unsafe { CStr::from_ptr(name) }).unwrap_or(dlssg_options as Ptr)
}
