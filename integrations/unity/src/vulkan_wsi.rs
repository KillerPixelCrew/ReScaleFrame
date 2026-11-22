// SPDX-License-Identifier: MIT
//! Unity Vulkan surface identity and swapchain semaphore ownership.

use super::{
    Function, INITIALIZATION_FAILED, NativeHooks, OUT_OF_HOST_MEMORY, SUCCESS, instance_command,
    resolver,
};
use crate::vulkan_state::{
    forget_win32_surface, register_swapchain, register_win32_surface, retire_swapchain,
    supersede_swapchain,
};
use std::ffi::c_void;
use std::ptr;

const DEVICE_LOST: i32 = -4;
const OUT_OF_DEVICE_MEMORY: i32 = -2;
const TRANSFER_SRC: u32 = 1;
const SEMAPHORE_CREATE_INFO: i32 = 9;

// Vulkan SDK 1.4.357.0 Windows x64 sizes checked against bindgen 0.72.1.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct SwapchainCreateInfo {
    kind: i32,
    next: *const c_void,
    flags: u32,
    surface: u64,
    minimum_image_count: u32,
    format: i32,
    color_space: i32,
    width: u32,
    height: u32,
    layers: u32,
    usage: u32,
    sharing_mode: i32,
    family_count: u32,
    families: *const u32,
    pre_transform: u32,
    alpha: u32,
    mode: i32,
    clipped: u32,
    old_chain: u64,
}

#[repr(C)]
pub(super) struct SurfaceCreateInfo {
    kind: i32,
    next: *const c_void,
    flags: u32,
    application: *mut c_void,
    window: *mut c_void,
}

#[repr(C)]
pub(super) struct SemaphoreCreateInfo {
    kind: i32,
    next: *const c_void,
    flags: u32,
}

const _: () = assert!(
    size_of::<SwapchainCreateInfo>() == 104
        && std::mem::offset_of!(SwapchainCreateInfo, width) == 44
        && std::mem::offset_of!(SwapchainCreateInfo, usage) == 56
        && std::mem::offset_of!(SwapchainCreateInfo, old_chain) == 96
        && size_of::<SurfaceCreateInfo>() == 40
        && std::mem::offset_of!(SurfaceCreateInfo, window) == 32
        && size_of::<SemaphoreCreateInfo>() == 24
);

pub(super) type CreateSwapchain = unsafe extern "system" fn(
    *mut c_void,
    *const SwapchainCreateInfo,
    *const c_void,
    *mut u64,
) -> i32;
pub(super) type CreateSurface = unsafe extern "system" fn(
    *mut c_void,
    *const SurfaceCreateInfo,
    *const c_void,
    *mut u64,
) -> i32;
pub(super) type Destroy = unsafe extern "system" fn(*mut c_void, u64, *const c_void);
pub(super) type CreateSemaphore = unsafe extern "system" fn(
    *mut c_void,
    *const SemaphoreCreateInfo,
    *const c_void,
    *mut u64,
) -> i32;
pub(super) type WaitIdle = unsafe extern "system" fn(*mut c_void) -> i32;
type Images = unsafe extern "system" fn(*mut c_void, u64, *mut u32, *mut u64) -> i32;

struct PendingSwapchain {
    device: *mut c_void,
    allocator: *const c_void,
    destroy_chain: Destroy,
    destroy_semaphore: Destroy,
    chain: u64,
    semaphores: Vec<u64>,
}

impl Drop for PendingSwapchain {
    fn drop(&mut self) {
        if self.chain == 0 {
            return;
        }
        // These objects were never published or submitted.
        for &semaphore in &self.semaphores {
            unsafe { (self.destroy_semaphore)(self.device, semaphore, ptr::null()) };
        }
        unsafe { (self.destroy_chain)(self.device, self.chain, self.allocator) };
    }
}

unsafe fn populate(
    pending: &mut PendingSwapchain,
    native: NativeHooks,
    request: SwapchainCreateInfo,
    get_images: Images,
) -> Result<(), i32> {
    let mut count = 0;
    if unsafe { get_images(pending.device, pending.chain, &mut count, ptr::null_mut()) } != SUCCESS
        || count == 0
    {
        return Err(INITIALIZATION_FAILED);
    }
    let mut images = Vec::new();
    images
        .try_reserve_exact(count as usize)
        .map_err(|_| OUT_OF_HOST_MEMORY)?;
    images.resize(count as usize, 0);
    if unsafe {
        get_images(
            pending.device,
            pending.chain,
            &mut count,
            images.as_mut_ptr(),
        )
    } != SUCCESS
        || count == 0
        || count as usize > images.len()
    {
        return Err(INITIALIZATION_FAILED);
    }
    images.truncate(count as usize);
    pending
        .semaphores
        .try_reserve_exact(count as usize)
        .map_err(|_| OUT_OF_HOST_MEMORY)?;
    let info = SemaphoreCreateInfo {
        kind: SEMAPHORE_CREATE_INFO,
        next: ptr::null(),
        flags: 0,
    };
    for _ in 0..count {
        let mut semaphore = 0;
        if unsafe { (native.create_semaphore)(pending.device, &info, ptr::null(), &mut semaphore) }
            != SUCCESS
        {
            return Err(OUT_OF_DEVICE_MEMORY);
        }
        pending.semaphores.push(semaphore);
    }
    register_swapchain(
        pending.device,
        pending.chain,
        request.surface,
        (request.width, request.height),
        images,
        std::mem::take(&mut pending.semaphores),
    )
    .map_err(|semaphores| {
        pending.semaphores = semaphores;
        INITIALIZATION_FAILED
    })
}

pub(super) unsafe extern "system" fn create_swapchain(
    device: *mut c_void,
    info: *const SwapchainCreateInfo,
    allocator: *const c_void,
    output: *mut u64,
) -> i32 {
    let (Some(native), Some(info)) = (resolver().native, unsafe { info.as_ref() }) else {
        return INITIALIZATION_FAILED;
    };
    if device.is_null() || output.is_null() {
        return INITIALIZATION_FAILED;
    }
    let lookup = |key: &std::ffi::CStr| unsafe { (native.device_proc)(device, key.as_ptr()) };
    let Some(create) = lookup(c"vkCreateSwapchainKHR") else {
        return INITIALIZATION_FAILED;
    };
    let create = unsafe { std::mem::transmute::<Function, CreateSwapchain>(create) };
    // oldSwapchain is retired by a valid create call even if creation fails.
    supersede_swapchain(device, info.old_chain);
    let (Some(destroy), Some(get_images)) = (
        lookup(c"vkDestroySwapchainKHR"),
        lookup(c"vkGetSwapchainImagesKHR"),
    ) else {
        return unsafe { create(device, info, allocator, output) };
    };
    let mut pending = PendingSwapchain {
        device,
        allocator,
        destroy_chain: unsafe { std::mem::transmute::<Function, Destroy>(destroy) },
        destroy_semaphore: native.destroy_semaphore,
        chain: 0,
        semaphores: Vec::new(),
    };
    let request = SwapchainCreateInfo {
        usage: info.usage | TRANSFER_SRC,
        ..*info
    };
    unsafe { output.write(0) };
    let mut created = 0;
    let result = unsafe { create(device, &request, allocator, &mut created) };
    if result != SUCCESS {
        return result;
    }
    pending.chain = created;
    let get_images = unsafe { std::mem::transmute::<Function, Images>(get_images) };
    if let Err(result) = unsafe { populate(&mut pending, native, request, get_images) } {
        return result;
    }
    unsafe { output.write(created) };
    pending.chain = 0;
    result
}

pub(super) unsafe extern "system" fn destroy_swapchain(
    device: *mut c_void,
    chain: u64,
    allocator: *const c_void,
) {
    let Some(native) = resolver().native else {
        return;
    };
    let Some(destroy) =
        (unsafe { (native.device_proc)(device, c"vkDestroySwapchainKHR".as_ptr()) })
    else {
        return;
    };
    let destroy = unsafe { std::mem::transmute::<Function, Destroy>(destroy) };
    let result = retire_swapchain(
        device,
        chain,
        || unsafe { (native.wait_idle)(device) },
        |semaphore| unsafe { (native.destroy_semaphore)(device, semaphore, ptr::null()) },
    );
    if !matches!(result, SUCCESS | DEVICE_LOST) {
        (native.retirement_fault)();
    }
    unsafe { destroy(device, chain, allocator) };
}

pub(super) unsafe extern "system" fn create_surface(
    instance: *mut c_void,
    info: *const SurfaceCreateInfo,
    allocator: *const c_void,
    output: *mut u64,
) -> i32 {
    let Some(create) = (unsafe { instance_command(instance, c"vkCreateWin32SurfaceKHR".as_ptr()) })
    else {
        return INITIALIZATION_FAILED;
    };
    let Some(info) = (unsafe { info.as_ref() }) else {
        return INITIALIZATION_FAILED;
    };
    if output.is_null() {
        return INITIALIZATION_FAILED;
    }
    let create = unsafe { std::mem::transmute::<Function, CreateSurface>(create) };
    let result = unsafe { create(instance, info, allocator, output) };
    if result == SUCCESS {
        register_win32_surface(instance as u64, unsafe { *output }, info.window);
    }
    result
}

pub(super) unsafe extern "system" fn destroy_surface(
    instance: *mut c_void,
    surface: u64,
    allocator: *const c_void,
) {
    if let Some(destroy) = unsafe { instance_command(instance, c"vkDestroySurfaceKHR".as_ptr()) } {
        forget_win32_surface(instance as u64, surface);
        let destroy = unsafe { std::mem::transmute::<Function, Destroy>(destroy) };
        unsafe { destroy(instance, surface, allocator) };
    }
}
