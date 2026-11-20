// SPDX-License-Identifier: MIT
use crate::vulkan_fg_abi::rsf_fgvk_frame_slot as Slot;
use crate::vulkan_fg_resources_abi::{
    rsf_vkfg_fences as Fences, rsf_vkfg_image as Image, rsf_vkfg_platform as Platform,
};
use std::ffi::c_void;

const INIT_FAILED: i32 = -6;
const NOT_READY: i32 = -8;

struct Replacement<'a> {
    platform: &'a Platform,
    user: *mut c_void,
    image: Image,
}

impl Drop for Replacement<'_> {
    fn drop(&mut self) {
        rsf_core_vkfg_release_image(self.platform, self.user, &mut self.image);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_vkfg_release_image(
    platform: &Platform,
    user: *mut c_void,
    image: &mut Image,
) {
    unsafe {
        if image.view != 0 {
            (platform.destroy_view.unwrap())(user, image.view);
        }
        if image.image != 0 {
            (platform.destroy_image.unwrap())(user, image.image);
        }
        if image.memory != 0 {
            (platform.free_memory.unwrap())(user, image.memory);
        }
    }
    *image = Image::default();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_vkfg_ensure_image(
    platform: &Platform,
    user: *mut c_void,
    current: &mut Image,
    width: u32,
    height: u32,
    format: u32,
    with_view: u32,
) -> u32 {
    if current.image != 0
        && current.width == width
        && current.height == height
        && current.format == format
        && (with_view == 0 || current.view != 0)
    {
        return 1;
    }
    let mut replacement = Replacement {
        platform,
        user,
        image: Image::default(),
    };
    let image = &mut replacement.image;
    if unsafe { (platform.create_image.unwrap())(user, width, height, format, &mut image.image) }
        != 0
        || image.image == 0
    {
        return 0;
    }
    let (mut size, mut types) = (0, 0);
    unsafe { (platform.requirements.unwrap())(user, image.image, &mut size, &mut types) };
    types &= unsafe { (platform.local_memory_types.unwrap())(user) };
    if types == 0
        || unsafe {
            (platform.allocate.unwrap())(user, size, types.trailing_zeros(), &mut image.memory)
        } != 0
        || image.memory == 0
        || unsafe { (platform.bind.unwrap())(user, image.image, image.memory) } != 0
    {
        return 0;
    }
    if with_view != 0
        && (unsafe { (platform.create_view.unwrap())(user, image.image, format, &mut image.view) }
            != 0
            || image.view == 0)
    {
        return 0;
    }
    image.width = width;
    image.height = height;
    image.format = format;
    // The caller already retired the old resource. Publish the complete
    // replacement and retire the old image using the same cleanup path.
    std::mem::swap(current, image);
    1
}

fn wait_pending(platform: &Platform, user: *mut c_void, fence: u64, pending: u32) -> bool {
    pending == 0 || (fence != 0 && unsafe { (platform.wait_fence.unwrap())(user, fence) } == 0)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_vkfg_prepare_fences(
    platform: &Platform,
    user: *mut c_void,
    slot: &mut Slot,
    fences: &mut Fences,
) -> i32 {
    if !wait_pending(platform, user, fences.source, slot.source_pending)
        || !wait_pending(platform, user, fences.consumer, slot.consumer_pending)
    {
        return NOT_READY;
    }
    slot.source_pending = 0;
    slot.consumer_pending = 0;
    for fence in [&mut fences.source, &mut fences.consumer] {
        if *fence != 0 {
            continue;
        }
        let mut candidate = 0;
        let result = unsafe { (platform.create_fence.unwrap())(user, &mut candidate) };
        if result != 0 || candidate == 0 {
            if candidate != 0 {
                unsafe { (platform.destroy_fence.unwrap())(user, candidate) };
            }
            return INIT_FAILED;
        }
        *fence = candidate;
    }
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_vkfg_fence_submission(
    platform: &Platform,
    user: *mut c_void,
    slot: &mut Slot,
    fences: &Fences,
    consumer: u32,
) {
    let (fence, pending) = if consumer != 0 {
        (fences.consumer, &mut slot.consumer_pending)
    } else {
        (fences.source, &mut slot.source_pending)
    };
    if fence == 0
        || !wait_pending(platform, user, fence, *pending)
        || unsafe { (platform.reset_fence.unwrap())(user, fence) } != 0
        || unsafe { (platform.submit_fence.unwrap())(user, fence) } != 0
    {
        // The actual engine or provider submission already happened. A
        // failed completion point must keep the slot unavailable for reuse.
        slot.completion_unknown = 1;
    } else {
        *pending = 1;
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_vkfg_release_fences(
    platform: &Platform,
    user: *mut c_void,
    fences: &mut Fences,
) {
    for fence in [fences.source, fences.consumer] {
        if fence != 0 {
            unsafe { (platform.destroy_fence.unwrap())(user, fence) };
        }
    }
    *fences = Fences::default();
}
