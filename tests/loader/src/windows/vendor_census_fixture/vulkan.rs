// SPDX-License-Identifier: MIT
use super::{ConstPtr, Ptr, lock};
use std::ffi::{CStr, c_char};
use std::ptr::null_mut;
use std::sync::Mutex;

struct VkSeen {
    queue: usize,
    submits: u32,
    presents: u32,
    present_info: usize,
}
static VK: Mutex<VkSeen> = Mutex::new(VkSeen {
    queue: 0,
    submits: 0,
    presents: 0,
    present_info: 0,
});

unsafe extern "C" fn create_instance(_: ConstPtr, _: ConstPtr, out: *mut Ptr) -> i32 {
    unsafe { out.write(0x1001 as Ptr) };
    0
}

unsafe extern "C" fn create_device(_: Ptr, _: ConstPtr, _: ConstPtr, out: *mut Ptr) -> i32 {
    unsafe { out.write(0x2002 as Ptr) };
    0
}

unsafe extern "C" fn get_device_queue(_: Ptr, family: u32, index: u32, out: *mut Ptr) {
    let handle = 0x3000u32
        .wrapping_add(family.wrapping_mul(16))
        .wrapping_add(index);
    unsafe { out.write(handle as Ptr) };
}

extern "C" fn queue_submit(queue: Ptr, count: u32, _: ConstPtr, _: u64) -> i32 {
    let mut seen = lock(&VK);
    seen.queue = queue as usize;
    seen.submits += count;
    0x10
}

extern "C" fn queue_submit2(queue: Ptr, count: u32, _: ConstPtr, _: u64) -> i32 {
    let mut seen = lock(&VK);
    seen.queue = queue as usize;
    seen.submits += count;
    0x11
}

extern "C" fn queue_present(queue: Ptr, info: ConstPtr) -> i32 {
    let mut seen = lock(&VK);
    seen.queue = queue as usize;
    seen.present_info = info as usize;
    seen.presents += 1;
    0
}

unsafe extern "C" fn create_swapchain(_: Ptr, _: ConstPtr, _: ConstPtr, out: *mut u64) -> i32 {
    unsafe { out.write(0x4004) };
    0
}

unsafe fn lookup(name: *const c_char) -> Ptr {
    match unsafe { CStr::from_ptr(name) }.to_bytes() {
        b"vkCreateInstance" => create_instance as Ptr,
        b"vkCreateDevice" => create_device as Ptr,
        b"vkGetDeviceQueue" => get_device_queue as Ptr,
        b"vkQueueSubmit" => queue_submit as Ptr,
        b"vkQueueSubmit2" => queue_submit2 as Ptr,
        b"vkQueuePresentKHR" => queue_present as Ptr,
        b"vkCreateSwapchainKHR" => create_swapchain as Ptr,
        _ => null_mut(),
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn vkGetInstanceProcAddr(_: Ptr, name: *const c_char) -> Ptr {
    // Distinct export addresses are part of the census hook fixture.
    std::hint::black_box(1);
    unsafe { lookup(name) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn vkGetDeviceProcAddr(_: Ptr, name: *const c_char) -> Ptr {
    std::hint::black_box(2);
    unsafe { lookup(name) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn fixture_vk_raw(name: *const c_char) -> Ptr {
    unsafe { lookup(name) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn fixture_vk_seen(
    queue: *mut Ptr,
    submits: *mut u32,
    presents: *mut u32,
    present_info: *mut ConstPtr,
) {
    let seen = lock(&VK);
    unsafe {
        queue.write(seen.queue as Ptr);
        submits.write(seen.submits);
        presents.write(seen.presents);
        present_info.write(seen.present_info as ConstPtr);
    }
}
