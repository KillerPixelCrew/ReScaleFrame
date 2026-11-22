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

    #[unsafe(no_mangle)]
    unsafe extern "C" fn rsf_fixture_wsi_counts(out: *mut Counts) {
        unsafe { out.write(*COUNTS.lock().unwrap()) };
    }

    #[unsafe(no_mangle)]
    unsafe extern "system" fn rsf_fixture_host_create(
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

    #[unsafe(no_mangle)]
    extern "system" fn rsf_fixture_host_destroy(
        device: vk::Device,
        _: vk::SwapchainKHR,
        _: *const vk::AllocationCallbacks<'_>,
    ) {
        assert_eq!(device.as_raw(), 1, "host destroy device identity");
        COUNTS.lock().unwrap().host_destroys += 1;
    }

    #[unsafe(no_mangle)]
    extern "system" fn rsf_fixture_host_present(
        _: vk::Queue,
        _: *const vk::PresentInfoKHR<'_>,
    ) -> vk::Result {
        COUNTS.lock().unwrap().host_presents += 1;
        vk::Result::SUCCESS
    }

    #[unsafe(no_mangle)]
    extern "system" fn rsf_fixture_host_acquire(
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

    #[unsafe(no_mangle)]
    extern "system" fn rsf_fixture_host_images(
        _: vk::Device,
        _: vk::SwapchainKHR,
        _: *mut u32,
        _: *mut vk::Image,
    ) -> vk::Result {
        COUNTS.lock().unwrap().host_images += 1;
        vk::Result::SUCCESS
    }

    #[unsafe(no_mangle)]
    extern "system" fn vkCreateSwapchainKHR(
        _: vk::Device,
        _: *const vk::SwapchainCreateInfoKHR<'_>,
        _: *const vk::AllocationCallbacks<'_>,
        _: *mut vk::SwapchainKHR,
    ) -> vk::Result {
        COUNTS.lock().unwrap().native_creates += 1;
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
        vk::Result::SUCCESS
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
        vk::Result::SUCCESS
    }

    #[unsafe(no_mangle)]
    extern "system" fn vkGetSwapchainImagesKHR(
        _: vk::Device,
        _: vk::SwapchainKHR,
        _: *mut u32,
        _: *mut vk::Image,
    ) -> vk::Result {
        vk::Result::SUCCESS
    }
}
use crate::wsi_abi::{rsf_wsi_fixture_calls as Calls, rsf_wsi_fixture_counts as Counts};
use std::mem::MaybeUninit;

fn verify(calls: &Calls) -> Result<(), &'static str> {
    let registration = calls.registration.unwrap();
    let create = calls.create.unwrap();
    let destroy = calls.destroy.unwrap();
    let present = calls.present.unwrap();
    let acquire = calls.acquire.unwrap();
    let images = calls.images.unwrap();
    let counts = calls.counts.unwrap();
    let read_counts = || {
        let mut snapshot = MaybeUninit::uninit();
        unsafe {
            counts(snapshot.as_mut_ptr());
            snapshot.assume_init()
        }
    };

    const DEVICE: u32 = 1;
    const FOREIGN: u32 = 2;
    const ALL_HOOKS: u32 = 0x1f;
    let mut chain = 0;
    unsafe {
        if registration(0, 2, 0, 0) != -1
            || registration(DEVICE, 2, ALL_HOOKS, 1) != -1
            || registration(DEVICE, 1, ALL_HOOKS, 0) != -2
            || registration(DEVICE, 2, ALL_HOOKS & !2, 0) != -1
        {
            return Err("malformed dispatch records must be refused");
        }
        if registration(DEVICE, 2, ALL_HOOKS, 0) != 0
            || registration(FOREIGN, 2, ALL_HOOKS, 0) != -3
        {
            return Err("a second device cannot replace the registered owner");
        }
        if create(DEVICE, &mut chain) != 0 || chain != 10 {
            return Err("host create must return its swapchain");
        }
        destroy(DEVICE, chain);
        if present(DEVICE) != 0 || acquire(DEVICE, chain) != 0 || images(DEVICE, chain) != 0 {
            return Err("present, acquire and image queries must forward their results");
        }
        create(FOREIGN, &mut chain);
        destroy(FOREIGN, chain);
        let first: Counts = read_counts();
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
        if registration(DEVICE, 2, 0, 0) != 0 {
            return Err("the owner must be able to unregister");
        }
        create(DEVICE, &mut chain);
        destroy(DEVICE, chain);
        present(DEVICE);
        let retired = read_counts();
        if retired.native_creates != 2
            || retired.native_destroys != 2
            || retired.native_presents != 1
            || retired.host_creates != 1
            || retired.host_destroys != 1
            || retired.host_presents != 1
        {
            return Err("native WSI must resume after callback retirement");
        }
    }
    Ok(())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ffx_vulkan_wsi_dispatch_fixture(calls: &Calls) -> i32 {
    match verify(calls) {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("{message}");
            1
        }
    }
}
