// SPDX-License-Identifier: MIT
//! Vulkan-1.dll is borrowed from the caller's process for the legacy ABI.
use super::{FAILED, Handle, Request, Session, candidate, create};
use ash::vk::{self, Handle as _};
use core::ptr;
use windows::{
    Win32::{
        Foundation::FARPROC,
        System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
    },
    core::{PCSTR, w},
};

const _: () = {
    assert!(size_of::<vk::PhysicalDeviceIDProperties<'_>>() == 64);
    assert!(core::mem::offset_of!(vk::PhysicalDeviceIDProperties<'_>, device_luid) == 48);
    assert!(core::mem::offset_of!(vk::PhysicalDeviceIDProperties<'_>, device_luid_valid) == 60);
};

fn entry(name: &core::ffi::CStr) -> FARPROC {
    let module = unsafe { GetModuleHandleW(w!("vulkan-1.dll")) }.ok()?;
    unsafe { GetProcAddress(module, PCSTR(name.as_ptr().cast())) }
}

pub(super) unsafe fn probe_physical(physical: Handle) -> i32 {
    if physical.is_null() {
        return FAILED;
    }
    let Some(entry) = entry(c"vkGetPhysicalDeviceProperties") else {
        return -2;
    };
    // The borrowed Vulkan loader reported the exact core entry name.
    let properties = unsafe {
        core::mem::transmute::<
            unsafe extern "system" fn() -> isize,
            vk::PFN_vkGetPhysicalDeviceProperties,
        >(entry)
    };
    let mut info = vk::PhysicalDeviceProperties::default();
    unsafe { properties(vk::PhysicalDevice::from_raw(physical as u64), &mut info) };
    if candidate(info.vendor_id) { 0 } else { -3 }
}

pub(super) unsafe fn create_physical(
    physical: Handle,
    render_width: u32,
    render_height: u32,
    output_width: u32,
    output_height: u32,
    output: *mut *mut Session,
) -> i32 {
    if output.is_null() {
        return FAILED;
    }
    unsafe { output.write(ptr::null_mut()) };
    if physical.is_null() {
        return FAILED;
    }
    let Some(entry) = entry(c"vkGetPhysicalDeviceProperties2") else {
        return -3;
    };
    let properties = unsafe {
        core::mem::transmute::<
            unsafe extern "system" fn() -> isize,
            vk::PFN_vkGetPhysicalDeviceProperties2,
        >(entry)
    };
    let mut id = vk::PhysicalDeviceIDProperties::default();
    let mut info = vk::PhysicalDeviceProperties2::default().push_next(&mut id);
    unsafe { properties(vk::PhysicalDevice::from_raw(physical as u64), &mut info) };
    if id.device_luid_valid == vk::FALSE {
        return -4;
    }
    let request = Request {
        luid: u64::from_le_bytes(id.device_luid),
        directory: ptr::null(),
        render_width,
        render_height,
        output_width,
        output_height,
        flags: 0,
    };
    unsafe { create(&request, output) }
}
