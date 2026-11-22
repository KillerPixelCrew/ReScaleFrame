// SPDX-License-Identifier: MIT
use rsf_game_sys::backend_result::{INIT_FAILED, NOT_READY};
pub(super) mod device;

use crate::sys::rsf_fgvk_frame_slot as Slot;
use crate::sys::{
    rsf_vkfg_fences as Fences, rsf_vkfg_image as Image, rsf_vkfg_platform as Platform,
};
use std::ffi::c_void;

const FENCE_TIMEOUT_NS: u64 = 15_000_000_000;
pub(super) const SAMPLED_TRANSFER: u32 = 0x7;

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
pub(super) extern "C" fn rsf_core_vkfg_release_image(
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
pub(super) extern "C" fn rsf_core_vkfg_ensure_image(
    platform: &Platform,
    user: *mut c_void,
    current: &mut Image,
    width: u32,
    height: u32,
    format: u32,
    with_view: u32,
) -> u32 {
    rsf_core_vkfg_ensure_image_usage(
        platform,
        user,
        current,
        width,
        height,
        format,
        SAMPLED_TRANSFER,
        with_view,
    )
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_core_vkfg_ensure_image_usage(
    platform: &Platform,
    user: *mut c_void,
    current: &mut Image,
    width: u32,
    height: u32,
    format: u32,
    usage: u32,
    with_view: u32,
) -> u32 {
    u32::from(
        ensure_image(
            platform,
            user,
            current,
            [width, height],
            format,
            usage,
            with_view != 0,
        )
        .is_ok(),
    )
}

pub(super) fn ensure_image(
    platform: &Platform,
    user: *mut c_void,
    current: &mut Image,
    [width, height]: [u32; 2],
    format: u32,
    usage: u32,
    with_view: bool,
) -> Result<(), i32> {
    ensure_image_with_memory(
        platform,
        user,
        current,
        [width, height],
        format,
        usage,
        (with_view, MemoryChoice::DeviceLocal),
    )
}

pub(super) enum MemoryChoice {
    DeviceLocal,
    PreferDeviceLocal { type_count: u32 },
}

impl MemoryChoice {
    pub(super) fn select(self, required: u32, local: u32) -> Option<u32> {
        let compatible = match self {
            Self::DeviceLocal => required & local,
            Self::PreferDeviceLocal { type_count } => {
                required & 1u32.checked_shl(type_count).map_or(u32::MAX, |bit| bit - 1)
            }
        };
        match compatible & local {
            0 => compatible.checked_ilog2(),
            preferred => Some(preferred.trailing_zeros()),
        }
    }
}

pub(super) fn ensure_image_with_memory(
    platform: &Platform,
    user: *mut c_void,
    current: &mut Image,
    [width, height]: [u32; 2],
    format: u32,
    usage: u32,
    (with_view, memory): (bool, MemoryChoice),
) -> Result<(), i32> {
    const OUT_OF_DEVICE_MEMORY: i32 = -2;
    const INITIALIZATION_FAILED: i32 = -3;
    if current.image != 0
        && current.width == width
        && current.height == height
        && current.format == format
        && current.usage == usage
        && (!with_view || current.view != 0)
    {
        return Ok(());
    }
    let mut replacement = Replacement {
        platform,
        user,
        image: Image::default(),
    };
    let image = &mut replacement.image;
    let result = unsafe {
        (platform.create_image.unwrap())(user, width, height, format, usage, &mut image.image)
    };
    if result != 0 || image.image == 0 {
        return Err(if result != 0 {
            result
        } else {
            INITIALIZATION_FAILED
        });
    }
    let (mut size, mut types) = (0, 0);
    unsafe { (platform.requirements.unwrap())(user, image.image, &mut size, &mut types) };
    let local = unsafe { (platform.local_memory_types.unwrap())(user) };
    let kind = memory.select(types, local).ok_or(OUT_OF_DEVICE_MEMORY)?;
    let result = unsafe { (platform.allocate.unwrap())(user, size, kind, &mut image.memory) };
    if result != 0 || image.memory == 0 {
        return Err(if result != 0 {
            result
        } else {
            INITIALIZATION_FAILED
        });
    }
    let result = unsafe { (platform.bind.unwrap())(user, image.image, image.memory) };
    if result != 0 {
        return Err(result);
    }
    if with_view {
        let result =
            unsafe { (platform.create_view.unwrap())(user, image.image, format, &mut image.view) };
        if result != 0 || image.view == 0 {
            return Err(if result != 0 {
                result
            } else {
                INITIALIZATION_FAILED
            });
        }
    }
    image.width = width;
    image.height = height;
    image.format = format;
    image.usage = usage;
    // The caller already retired the old resource. Publish the complete
    // replacement and retire the old image using the same cleanup path.
    std::mem::swap(current, image);
    Ok(())
}

fn wait_pending(platform: &Platform, user: *mut c_void, fence: u64, pending: u32) -> bool {
    pending == 0
        || (fence != 0
            && unsafe { (platform.wait_fence.unwrap())(user, fence, FENCE_TIMEOUT_NS) } == 0)
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_core_vkfg_prepare_fences(
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
pub(super) extern "C" fn rsf_core_vkfg_fence_submission(
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
pub(super) extern "C" fn rsf_core_vkfg_release_fences(
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
