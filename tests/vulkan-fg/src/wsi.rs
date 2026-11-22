// SPDX-License-Identifier: MIT

// The corrected SDK header still owns its registration and forwards to these
// typed fake Vulkan entry points. All calls in this fixture are synchronous.
mod drivers {
    use super::Counts;
    use ash::vk::{self, Handle};
    use std::sync::Mutex;

    static COUNTS: Mutex<Counts> = Mutex::new(Counts {
        native_creates: 0,
        native_destroys: 0,
        host_creates: 0,
        host_destroys: 0,
        native_presents: 0,
        host_presents: 0,
        host_acquires: 0,
        host_images: 0,
    });

    pub(super) fn snapshot() -> Counts {
        *COUNTS.lock().unwrap()
    }

    pub(super) unsafe extern "system" fn rsf_fixture_host_create(
        device: vk::Device,
        _: *const vk::SwapchainCreateInfoKHR<'_>,
        _: *const vk::AllocationCallbacks<'_>,
        chain: *mut vk::SwapchainKHR,
    ) -> vk::Result {
        assert_eq!(device.as_raw(), 1, "host create device identity");
        COUNTS.lock().unwrap().host_creates += 1;
        unsafe { chain.write(vk::SwapchainKHR::from_raw(10)) };
        vk::Result::SUCCESS
    }

    pub(super) extern "system" fn rsf_fixture_host_destroy(
        device: vk::Device,
        _: vk::SwapchainKHR,
        _: *const vk::AllocationCallbacks<'_>,
    ) {
        assert_eq!(device.as_raw(), 1, "host destroy device identity");
        COUNTS.lock().unwrap().host_destroys += 1;
    }

    pub(super) extern "system" fn rsf_fixture_host_present(
        queue: vk::Queue,
        _: *const vk::PresentInfoKHR<'_>,
    ) -> vk::Result {
        assert_eq!(queue.as_raw(), 22);
        COUNTS.lock().unwrap().host_presents += 1;
        vk::Result::SUCCESS
    }

    pub(super) extern "system" fn rsf_fixture_host_acquire(
        _: vk::Device,
        _: vk::SwapchainKHR,
        _: u64,
        _: vk::Semaphore,
        _: vk::Fence,
        _: *mut u32,
    ) -> vk::Result {
        COUNTS.lock().unwrap().host_acquires += 1;
        vk::Result::SUCCESS
    }

    pub(super) extern "system" fn rsf_fixture_host_images(
        _: vk::Device,
        _: vk::SwapchainKHR,
        _: *mut u32,
        _: *mut vk::Image,
    ) -> vk::Result {
        COUNTS.lock().unwrap().host_images += 1;
        vk::Result::SUCCESS
    }

    #[unsafe(no_mangle)]
    unsafe extern "system" fn vkCreateSwapchainKHR(
        _: vk::Device,
        _: *const vk::SwapchainCreateInfoKHR<'_>,
        _: *const vk::AllocationCallbacks<'_>,
        chain: *mut vk::SwapchainKHR,
    ) -> vk::Result {
        COUNTS.lock().unwrap().native_creates += 1;
        unsafe { chain.write(vk::SwapchainKHR::from_raw(20)) };
        vk::Result::SUCCESS
    }

    #[unsafe(no_mangle)]
    extern "system" fn vkDestroySwapchainKHR(
        _: vk::Device,
        _: vk::SwapchainKHR,
        _: *const vk::AllocationCallbacks<'_>,
    ) {
        COUNTS.lock().unwrap().native_destroys += 1;
    }

    #[unsafe(no_mangle)]
    extern "system" fn vkQueuePresentKHR(
        _: vk::Queue,
        _: *const vk::PresentInfoKHR<'_>,
    ) -> vk::Result {
        COUNTS.lock().unwrap().native_presents += 1;
        vk::Result::SUBOPTIMAL_KHR
    }

    #[unsafe(no_mangle)]
    extern "system" fn vkAcquireNextImageKHR(
        _: vk::Device,
        _: vk::SwapchainKHR,
        _: u64,
        _: vk::Semaphore,
        _: vk::Fence,
        _: *mut u32,
    ) -> vk::Result {
        vk::Result::TIMEOUT
    }

    #[unsafe(no_mangle)]
    extern "system" fn vkGetSwapchainImagesKHR(
        _: vk::Device,
        _: vk::SwapchainKHR,
        _: *mut u32,
        _: *mut vk::Image,
    ) -> vk::Result {
        vk::Result::INCOMPLETE
    }
}

use crate::wsi_abi::*;
use ash::vk::{self, Handle};
use std::ptr;

#[derive(Clone, Copy)]
struct Counts {
    native_creates: u32,
    native_destroys: u32,
    host_creates: u32,
    host_destroys: u32,
    native_presents: u32,
    host_presents: u32,
    host_acquires: u32,
    host_images: u32,
}

fn verify() -> Result<(), &'static str> {
    let device = vk::Device::from_raw(1);
    let foreign = vk::Device::from_raw(2);
    let queue = vk::Queue::from_raw(22);
    let setup = rsf_ffx_wsi_dispatch {
        struct_size: size_of::<rsf_ffx_wsi_dispatch>() as u32,
        abi_version: 2,
        device,
        create: Some(drivers::rsf_fixture_host_create),
        destroy: Some(drivers::rsf_fixture_host_destroy),
        present: Some(drivers::rsf_fixture_host_present),
        acquire: Some(drivers::rsf_fixture_host_acquire),
        images: Some(drivers::rsf_fixture_host_images),
    };
    let short = rsf_ffx_wsi_dispatch {
        struct_size: setup.struct_size - 1,
        ..setup
    };
    let wrong_abi = rsf_ffx_wsi_dispatch {
        abi_version: 1,
        ..setup
    };
    let partial = rsf_ffx_wsi_dispatch {
        destroy: None,
        ..setup
    };
    let competing = rsf_ffx_wsi_dispatch {
        device: foreign,
        ..setup
    };
    let unregister = rsf_ffx_wsi_dispatch {
        create: None,
        destroy: None,
        present: None,
        acquire: None,
        images: None,
        ..setup
    };
    let mut chain = vk::SwapchainKHR::null();
    unsafe {
        if rsfFfxSetVulkanWsiDispatch(ptr::null()) != -1
            || rsfFfxSetVulkanWsiDispatch(&short) != -1
            || rsfFfxSetVulkanWsiDispatch(&wrong_abi) != -2
            || rsfFfxSetVulkanWsiDispatch(&partial) != -1
        {
            return Err("malformed dispatch records must be refused");
        }
        if rsfFfxSetVulkanWsiDispatch(&setup) != 0 || rsfFfxSetVulkanWsiDispatch(&competing) != -3 {
            return Err("a second device cannot replace the registered owner");
        }
        if rsf_fixture_sdk_wsi_create(device, &mut chain) != vk::Result::SUCCESS
            || chain.as_raw() != 10
        {
            return Err("host create must return its swapchain");
        }
        rsf_fixture_sdk_wsi_destroy(device, chain);
        if rsf_fixture_sdk_wsi_present(device, queue) != vk::Result::SUCCESS
            || rsf_fixture_sdk_wsi_acquire(device, chain) != vk::Result::SUCCESS
            || rsf_fixture_sdk_wsi_images(device, chain) != vk::Result::SUCCESS
        {
            return Err("present, acquire and image queries must forward their results");
        }
        if rsf_fixture_sdk_wsi_create(foreign, &mut chain) != vk::Result::SUCCESS
            || chain.as_raw() != 20
        {
            return Err("a foreign device must use the real SDK fallback");
        }
        rsf_fixture_sdk_wsi_destroy(foreign, chain);
        let first = drivers::snapshot();
        if first.native_creates != 1
            || first.native_destroys != 1
            || first.host_creates != 1
            || first.host_destroys != 1
            || first.native_presents != 0
            || first.host_presents != 1
            || first.host_acquires != 1
            || first.host_images != 1
        {
            return Err("the owner must route its own calls and leave the foreign device native");
        }
        if rsfFfxSetVulkanWsiDispatch(&unregister) != 0 {
            return Err("the owner must be able to unregister");
        }
        if rsf_fixture_sdk_wsi_create(device, &mut chain) != vk::Result::SUCCESS
            || chain.as_raw() != 20
        {
            return Err("native WSI discovery must resume after callback retirement");
        }
        rsf_fixture_sdk_wsi_destroy(device, chain);
        if rsf_fixture_sdk_wsi_present(device, queue) != vk::Result::SUBOPTIMAL_KHR
            || rsf_fixture_sdk_wsi_acquire(device, chain) != vk::Result::TIMEOUT
            || rsf_fixture_sdk_wsi_images(device, chain) != vk::Result::INCOMPLETE
        {
            return Err("native WSI must resume after callback retirement");
        }
        let retired = drivers::snapshot();
        if retired.native_creates != 2
            || retired.native_destroys != 2
            || retired.native_presents != 1
            || retired.host_creates != 1
            || retired.host_destroys != 1
            || retired.host_presents != 1
        {
            return Err("the retired callback table must never be used again");
        }
    }
    Ok(())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ffx_vulkan_wsi_dispatch_fixture() -> i32 {
    match verify() {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("{message}");
            1
        }
    }
}
