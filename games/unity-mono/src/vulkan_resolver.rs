// SPDX-License-Identifier: MIT
//! Unity's Vulkan resolver routes and the Win32 device extension request for shared resources.
//! Native leaves retain the typed Vulkan create and loader calls.
use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::slice;
use std::sync::{Mutex, MutexGuard, PoisonError};

const FORWARD: u32 = 0;
const INSTANCE_PROC: u32 = 1;
const DEVICE_PROC: u32 = 2;
const CREATE_DEVICE: u32 = 3;
const CREATE_SWAPCHAIN: u32 = 4;
const DESTROY_SWAPCHAIN: u32 = 5;
const PRESENT: u32 = 6;

const SUCCESS: i32 = 0;
const OUT_OF_HOST_MEMORY: i32 = -1;
const INITIALIZATION_FAILED: i32 = -3;

const INTEROP: [&CStr; 2] = [
    c"VK_KHR_external_memory_win32",
    c"VK_KHR_external_semaphore_win32",
];

type Function = unsafe extern "system" fn();
type Available = unsafe extern "C" fn(*mut c_void, *const *const c_char, u32) -> bool;
type Create =
    unsafe extern "C" fn(*mut c_void, Function, *const *const c_char, u32, *mut *mut c_void) -> i32;

struct Resolver {
    originals: [Option<Function>; PRESENT as usize + 1],
    /// The device created with both interop extensions in its request.
    interop_device: usize,
}

static RESOLVER: Mutex<Resolver> = Mutex::new(Resolver {
    originals: [None; PRESENT as usize + 1],
    interop_device: 0,
});

fn resolver() -> MutexGuard<'static, Resolver> {
    RESOLVER.lock().unwrap_or_else(PoisonError::into_inner)
}

fn route(name: &CStr, from_instance: bool) -> u32 {
    match name.to_bytes() {
        b"vkGetDeviceProcAddr" if from_instance => DEVICE_PROC,
        b"vkCreateDevice" => CREATE_DEVICE,
        b"vkCreateSwapchainKHR" => CREATE_SWAPCHAIN,
        b"vkDestroySwapchainKHR" => DESTROY_SWAPCHAIN,
        b"vkQueuePresentKHR" => PRESENT,
        _ => FORWARD,
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vk_bind_resolver(original: Option<Function>) -> bool {
    if let Some(original) = original {
        resolver().originals[INSTANCE_PROC as usize] = Some(original);
        true
    } else {
        false
    }
}

/// Unity lends a NUL-terminated loader name, or null, for the call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vk_resolver_route(
    name: *const c_char,
    original: Option<Function>,
    from_instance: bool,
) -> u32 {
    let Some(original) = original else {
        return FORWARD;
    };
    if name.is_null() {
        return FORWARD;
    }
    let slot = route(unsafe { CStr::from_ptr(name) }, from_instance);
    if slot != FORWARD {
        resolver().originals[slot as usize] = Some(original);
    }
    slot
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vk_resolver_original(slot: u32) -> Option<Function> {
    resolver().originals.get(slot as usize).copied().flatten()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vk_interop_enabled(device: *mut c_void) -> bool {
    !device.is_null() && resolver().interop_device == device as usize
}

/// The native caller lends the engine request and its extension names until creation returns.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vk_resolver_create_device(
    names: *const *const c_char,
    count: u32,
    context: *mut c_void,
    available: Available,
    create: Create,
) -> i32 {
    let Some(original) = rsf_unity_vk_resolver_original(CREATE_DEVICE) else {
        return INITIALIZATION_FAILED;
    };
    let name_count = count as usize;
    let supplied = if name_count == 0 {
        &[]
    } else if names.is_null() {
        return INITIALIZATION_FAILED;
    } else {
        unsafe { slice::from_raw_parts(names, name_count) }
    };
    let required = INTEROP.map(CStr::as_ptr);
    let interop = unsafe { available(context, required.as_ptr(), required.len() as u32) };
    let mut device = ptr::null_mut();
    let result = if interop {
        let mut extensions = Vec::new();
        if count > u32::MAX - required.len() as u32
            || extensions
                .try_reserve_exact(name_count + required.len())
                .is_err()
        {
            return OUT_OF_HOST_MEMORY;
        }
        extensions.extend_from_slice(supplied);
        for name in INTEROP {
            let has_name = extensions.iter().any(|&candidate| {
                !candidate.is_null() && unsafe { CStr::from_ptr(candidate) } == name
            });
            if !has_name {
                extensions.push(name.as_ptr());
            }
        }
        unsafe {
            create(
                context,
                original,
                extensions.as_ptr(),
                extensions.len() as u32,
                &mut device,
            )
        }
    } else {
        unsafe { create(context, original, ptr::null(), 0, &mut device) }
    };
    if result == SUCCESS && !device.is_null() {
        let mut state = resolver();
        if interop {
            state.interop_device = device as usize;
        } else if state.interop_device == device as usize {
            state.interop_device = 0;
        }
    }
    result
}
