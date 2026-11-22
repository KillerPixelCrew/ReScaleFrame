// SPDX-License-Identifier: MIT
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
