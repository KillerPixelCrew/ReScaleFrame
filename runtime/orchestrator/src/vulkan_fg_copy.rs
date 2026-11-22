// SPDX-License-Identifier: MIT
pub(super) use crate::vulkan_fg_resources_abi::rsf_vkfg_barrier as Barrier;
use crate::vulkan_fg_resources_abi::{rsf_vkfg_image as Image, rsf_vkfg_image_copy as ImageCopy};
use std::ffi::c_void;

pub(super) const UNDEFINED: u32 = 0;
pub(super) const SHADER_READ: u32 = 5;
pub(super) const TRANSFER_SRC: u32 = 6;
const TRANSFER_DST: u32 = 7;
pub(super) const FAMILY_IGNORED: u32 = !0;
pub(super) const FAMILY_EXTERNAL: u32 = !1;
// VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT.
const MEMORY_ACCESS: u32 = 0x8000 | 0x1_0000;
pub(super) const ALL_COMMANDS: u32 = 0x1_0000;

type Handle = *mut c_void;
pub(super) type BarrierFn = unsafe extern "C" fn(Handle, Handle, *const Barrier);
pub(super) type CopyFn = unsafe extern "C" fn(Handle, Handle, *const ImageCopy);

impl Barrier {
    pub(super) fn new(image: u64, old_layout: u32, new_layout: u32, from: u32, to: u32) -> Self {
        let source_access = if old_layout == UNDEFINED || from == FAMILY_EXTERNAL {
            0
        } else {
            MEMORY_ACCESS
        };
        let target_access = if to == FAMILY_EXTERNAL {
            0
        } else {
            MEMORY_ACCESS
        };
        let mut barrier =
            Self::with_access(image, old_layout, new_layout, source_access, target_access);
        barrier.source_family = from;
        barrier.target_family = to;
        barrier
    }

    fn same_queue(image: u64, old_layout: u32, new_layout: u32) -> Self {
        Self::new(
            image,
            old_layout,
            new_layout,
            FAMILY_IGNORED,
            FAMILY_IGNORED,
        )
    }

    pub(super) fn with_access(
        image: u64,
        old_layout: u32,
        new_layout: u32,
        source_access: u32,
        target_access: u32,
    ) -> Self {
        Self {
            image,
            old_layout,
            new_layout,
            source_family: FAMILY_IGNORED,
            target_family: FAMILY_IGNORED,
            source_access,
            target_access,
            source_stages: ALL_COMMANDS,
            target_stages: ALL_COMMANDS,
        }
    }
}

impl ImageCopy {
    pub(super) fn transfer(source: u64, target: u64, [width, height]: [u32; 2]) -> Self {
        Self {
            source,
            target,
            source_layout: TRANSFER_SRC,
            target_layout: TRANSFER_DST,
            width,
            height,
        }
    }
}

const _: () = {
    assert!(size_of::<Barrier>() == 40);
    assert!(size_of::<ImageCopy>() == 32);
};

// Both images stay on the graphics queue and return to shader-read layout.
// The caller supplies the active copy extent and tracks destination initialization.
pub(super) unsafe fn record_shader_read_copy(
    record_barrier: BarrierFn,
    record_copy: CopyFn,
    native: Handle,
    commands: Handle,
    source: u64,
    target: &Image,
    extent: [u32; 2],
) {
    let copy = ImageCopy::transfer(source, target.image, extent);
    let target_initial_layout = if target.initialized != 0 {
        SHADER_READ
    } else {
        UNDEFINED
    };
    let barrier = |plan: Barrier| unsafe { record_barrier(native, commands, &plan) };
    barrier(Barrier::same_queue(
        copy.source,
        SHADER_READ,
        copy.source_layout,
    ));
    barrier(Barrier::same_queue(
        copy.target,
        target_initial_layout,
        copy.target_layout,
    ));
    unsafe { record_copy(native, commands, &copy) };
    barrier(Barrier::same_queue(
        copy.source,
        copy.source_layout,
        SHADER_READ,
    ));
    barrier(Barrier::same_queue(
        copy.target,
        copy.target_layout,
        SHADER_READ,
    ));
}
