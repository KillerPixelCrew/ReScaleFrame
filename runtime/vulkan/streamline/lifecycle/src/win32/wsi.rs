// SPDX-License-Identifier: MIT
//! Vulkan dispatch and corrected Streamline WSI leases.
use super::LIFECYCLE;
use ash::vk::{self, Handle as _};
use std::ffi::{c_char, c_void};
use std::{mem, ptr};

const UNAVAILABLE: vk::Result = vk::Result::ERROR_INITIALIZATION_FAILED;

#[unsafe(export_name = "rsf_vk_sl_get_instance_proc_addr")]
unsafe extern "C" fn instance_proc(
    instance: vk::Instance,
    name: *const c_char,
) -> vk::PFN_vkVoidFunction {
    let address = LIFECYCLE.instance_proc();
    if address.is_null() {
        return None;
    }
    let resolve: vk::PFN_vkGetInstanceProcAddr = unsafe { mem::transmute(address) };
    unsafe { resolve(instance, name) }
}

#[unsafe(export_name = "rsf_vk_sl_get_device_proc_addr")]
unsafe extern "C" fn device_proc(
    device: vk::Device,
    name: *const c_char,
) -> vk::PFN_vkVoidFunction {
    let address = LIFECYCLE.device_proc();
    if address.is_null() {
        return None;
    }
    let resolve: vk::PFN_vkGetDeviceProcAddr = unsafe { mem::transmute(address) };
    unsafe { resolve(device, name) }
}

macro_rules! entry {
    ($resolve:ident, $object:expr, $name:literal, $pfn:ident) => {
        unsafe { $resolve($object, $name.as_ptr()) }.map(|address| unsafe {
            mem::transmute::<unsafe extern "system" fn(), vk::$pfn>(address)
        })
    };
}

#[unsafe(export_name = "rsf_vk_sl_create_instance")]
unsafe extern "C" fn create_instance(
    info: *const vk::InstanceCreateInfo<'_>,
    output: *mut vk::Instance,
) -> vk::Result {
    entry!(
        instance_proc,
        vk::Instance::null(),
        c"vkCreateInstance",
        PFN_vkCreateInstance
    )
    .map_or(UNAVAILABLE, |call| unsafe {
        call(info, ptr::null(), output)
    })
}

#[unsafe(export_name = "rsf_vk_sl_enumerate_physical_devices")]
unsafe extern "C" fn physical_devices(
    instance: vk::Instance,
    count: *mut u32,
    output: *mut vk::PhysicalDevice,
) -> vk::Result {
    entry!(
        instance_proc,
        instance,
        c"vkEnumeratePhysicalDevices",
        PFN_vkEnumeratePhysicalDevices
    )
    .map_or(UNAVAILABLE, |call| unsafe { call(instance, count, output) })
}

#[unsafe(export_name = "rsf_vk_sl_create_device")]
unsafe extern "C" fn create_device(
    instance: vk::Instance,
    physical: vk::PhysicalDevice,
    info: *const vk::DeviceCreateInfo<'_>,
    output: *mut vk::Device,
) -> vk::Result {
    entry!(
        instance_proc,
        instance,
        c"vkCreateDevice",
        PFN_vkCreateDevice
    )
    .map_or(UNAVAILABLE, |call| unsafe {
        call(physical, info, ptr::null(), output)
    })
}

#[unsafe(export_name = "rsf_vk_sl_create_win32_surface")]
unsafe extern "C" fn create_surface(
    instance: vk::Instance,
    info: *const vk::Win32SurfaceCreateInfoKHR<'_>,
    output: *mut vk::SurfaceKHR,
) -> vk::Result {
    entry!(
        instance_proc,
        instance,
        c"vkCreateWin32SurfaceKHR",
        PFN_vkCreateWin32SurfaceKHR
    )
    .map_or(UNAVAILABLE, |call| unsafe {
        call(instance, info, ptr::null(), output)
    })
}

#[unsafe(export_name = "rsf_vk_sl_destroy_surface")]
unsafe extern "C" fn destroy_surface(instance: vk::Instance, surface: vk::SurfaceKHR) {
    if let Some(call) = entry!(
        instance_proc,
        instance,
        c"vkDestroySurfaceKHR",
        PFN_vkDestroySurfaceKHR
    ) {
        unsafe { call(instance, surface, ptr::null()) };
    }
}

#[unsafe(export_name = "rsf_vk_sl_create_swapchain")]
unsafe extern "C" fn create_swapchain(
    device: vk::Device,
    info: *const vk::SwapchainCreateInfoKHR<'_>,
    output: *mut vk::SwapchainKHR,
) -> vk::Result {
    let Some(call) = entry!(
        device_proc,
        device,
        c"vkCreateSwapchainKHR",
        PFN_vkCreateSwapchainKHR
    ) else {
        return UNAVAILABLE;
    };
    let result = unsafe { call(device, info, ptr::null(), output) };
    if result == vk::Result::SUCCESS && !output.is_null() {
        LIFECYCLE
            .swapchain()
            .created(unsafe { output.read() }.as_raw());
    }
    result
}

#[unsafe(export_name = "rsf_vk_sl_destroy_swapchain")]
unsafe extern "C" fn destroy_swapchain(device: vk::Device, swapchain: vk::SwapchainKHR) {
    if let Some(call) = entry!(
        device_proc,
        device,
        c"vkDestroySwapchainKHR",
        PFN_vkDestroySwapchainKHR
    ) {
        unsafe { call(device, swapchain, ptr::null()) };
        LIFECYCLE.swapchain().destroyed(swapchain.as_raw());
    }
}

#[unsafe(export_name = "rsf_vk_sl_get_swapchain_images")]
unsafe extern "C" fn images(
    device: vk::Device,
    swapchain: vk::SwapchainKHR,
    count: *mut u32,
    output: *mut vk::Image,
) -> vk::Result {
    entry!(
        device_proc,
        device,
        c"vkGetSwapchainImagesKHR",
        PFN_vkGetSwapchainImagesKHR
    )
    .map_or(UNAVAILABLE, |call| unsafe {
        call(device, swapchain, count, output)
    })
}

#[unsafe(export_name = "rsf_vk_sl_acquire")]
unsafe extern "C" fn acquire(
    device: vk::Device,
    swapchain: vk::SwapchainKHR,
    timeout: u64,
    semaphore: vk::Semaphore,
    fence: vk::Fence,
    index: *mut u32,
) -> vk::Result {
    entry!(
        device_proc,
        device,
        c"vkAcquireNextImageKHR",
        PFN_vkAcquireNextImageKHR
    )
    .map_or(UNAVAILABLE, |call| unsafe {
        call(device, swapchain, timeout, semaphore, fence, index)
    })
}

#[unsafe(export_name = "rsf_vk_sl_present")]
unsafe extern "C" fn present(
    device: vk::Device,
    queue: vk::Queue,
    info: *const vk::PresentInfoKHR<'_>,
) -> vk::Result {
    entry!(
        device_proc,
        device,
        c"vkQueuePresentKHR",
        PFN_vkQueuePresentKHR
    )
    .map_or(UNAVAILABLE, |call| unsafe { call(queue, info) })
}

#[unsafe(export_name = "rsf_vk_sl_device_wait_idle")]
unsafe extern "C" fn wait_idle(device: vk::Device) -> vk::Result {
    entry!(
        device_proc,
        device,
        c"vkDeviceWaitIdle",
        PFN_vkDeviceWaitIdle
    )
    .map_or(UNAVAILABLE, |call| unsafe { call(device) })
}

#[unsafe(export_name = "rsf_vk_sl_queue_present_synchronized")]
unsafe extern "C" fn synchronized_present(
    address: usize,
    queue: vk::Queue,
    info: *const vk::PresentInfoKHR<'_>,
) -> vk::Result {
    type Lease = unsafe extern "C" fn(
        vk::PFN_vkQueuePresentKHR,
        vk::Queue,
        *const vk::PresentInfoKHR<'_>,
    ) -> vk::Result;
    if address == 0 || queue.is_null() || info.is_null() {
        return UNAVAILABLE;
    }
    let present: vk::PFN_vkQueuePresentKHR = unsafe { mem::transmute(address) };
    if !LIFECYCLE.initialized() {
        return unsafe { present(queue, info) };
    }
    let lease = unsafe { LIFECYCLE.common_symbol(c"rsfSlVulkanQueuePresent".as_ptr()) };
    if lease.is_null() {
        return vk::Result::ERROR_EXTENSION_NOT_PRESENT;
    }
    let lease: Lease = unsafe { mem::transmute(lease) };
    unsafe { lease(present, queue, info) }
}

type Callback = unsafe extern "C" fn(*mut c_void) -> i32;
type Lease = unsafe extern "C" fn(Callback, *mut c_void) -> i32;

#[unsafe(export_name = "rsf_vk_sl_vulkan_wsi_call")]
unsafe extern "C" fn synchronized_wsi(callback: Option<Callback>, user: *mut c_void) -> i32 {
    let Some(callback) = callback.filter(|_| !user.is_null()) else {
        return UNAVAILABLE.as_raw();
    };
    if !LIFECYCLE.initialized() {
        return unsafe { callback(user) };
    }
    let lease = unsafe { LIFECYCLE.common_symbol(c"rsfSlVulkanWsiCall".as_ptr()) };
    if lease.is_null() {
        return vk::Result::ERROR_EXTENSION_NOT_PRESENT.as_raw();
    }
    let lease: Lease = unsafe { mem::transmute(lease) };
    unsafe { lease(callback, user) }
}

#[unsafe(export_name = "rsf_vk_sl_without_dlss_g")]
unsafe extern "C" fn without_dlss_g(callback: Option<Callback>, user: *mut c_void) -> i32 {
    let Some(callback) = callback else {
        return UNAVAILABLE.as_raw();
    };
    if !LIFECYCLE.initialized() {
        return unsafe { callback(user) };
    }
    let lease = unsafe { LIFECYCLE.interposer_symbol(c"rsfSlVulkanWithoutDlssGCall".as_ptr()) };
    if lease.is_null() {
        return vk::Result::ERROR_EXTENSION_NOT_PRESENT.as_raw();
    }
    let lease: Lease = unsafe { mem::transmute(lease) };
    unsafe { lease(callback, user) }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" fn callback(user: *mut c_void) -> i32 {
        if user.is_null() { 7 } else { 8 }
    }

    #[test]
    fn unpublished_dispatch_does_not_touch_caller_outputs() {
        let mut count = 2;
        let result = unsafe {
            images(
                vk::Device::null(),
                vk::SwapchainKHR::null(),
                &mut count,
                ptr::null_mut(),
            )
        };
        assert_eq!(result.as_raw(), UNAVAILABLE.as_raw());
        assert_eq!(count, 2);
    }

    #[test]
    fn inactive_leases_call_directly_with_their_distinct_user_contracts() {
        let user = ptr::dangling_mut::<c_void>();
        assert_eq!(unsafe { synchronized_wsi(Some(callback), user) }, 8);
        assert_eq!(
            unsafe { synchronized_wsi(Some(callback), ptr::null_mut()) },
            UNAVAILABLE.as_raw()
        );
        assert_eq!(
            unsafe { without_dlss_g(Some(callback), ptr::null_mut()) },
            7
        );
        assert_eq!(unsafe { without_dlss_g(None, user) }, UNAVAILABLE.as_raw());
    }
}
