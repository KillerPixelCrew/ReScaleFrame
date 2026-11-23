// SPDX-License-Identifier: MIT
use crate::sys::rsf_vkfg_image as Image;
use ash::vk::{self, Handle as _};
use std::ffi::c_void;

/// One planned single-image barrier; layouts, accesses and stages are raw Vulkan values.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Barrier {
    pub(crate) image: u64,
    pub(crate) old_layout: u32,
    pub(crate) new_layout: u32,
    pub(crate) source_family: u32,
    pub(crate) target_family: u32,
    pub(crate) source_access: u32,
    pub(crate) target_access: u32,
    pub(crate) source_stages: u32,
    pub(crate) target_stages: u32,
}

/// One planned colour copy between two images of the same extent.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ImageCopy {
    pub(crate) source: u64,
    pub(crate) target: u64,
    pub(crate) source_layout: u32,
    pub(crate) target_layout: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

const _: () = assert!(size_of::<Barrier>() == 40 && size_of::<ImageCopy>() == 32);

pub(super) const COLOR_RANGE: vk::ImageSubresourceRange = vk::ImageSubresourceRange {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    base_mip_level: 0,
    level_count: 1,
    base_array_layer: 0,
    layer_count: 1,
};

pub(super) unsafe fn rsf_core_vkfg_record_barrier(
    barrier: vk::PFN_vkCmdPipelineBarrier,
    commands: vk::CommandBuffer,
    plan: &Barrier,
) {
    let data = vk::ImageMemoryBarrier::default()
        .old_layout(vk::ImageLayout::from_raw(plan.old_layout as i32))
        .new_layout(vk::ImageLayout::from_raw(plan.new_layout as i32))
        .src_access_mask(vk::AccessFlags::from_raw(plan.source_access))
        .dst_access_mask(vk::AccessFlags::from_raw(plan.target_access))
        .src_queue_family_index(plan.source_family)
        .dst_queue_family_index(plan.target_family)
        .image(vk::Image::from_raw(plan.image))
        .subresource_range(COLOR_RANGE);
    unsafe {
        barrier(
            commands,
            vk::PipelineStageFlags::from_raw(plan.source_stages),
            vk::PipelineStageFlags::from_raw(plan.target_stages),
            vk::DependencyFlags::empty(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            1,
            &data,
        );
    }
}

pub(super) unsafe fn rsf_core_vkfg_record_copy(
    copy: vk::PFN_vkCmdCopyImage,
    commands: vk::CommandBuffer,
    plan: &ImageCopy,
) {
    let layers = vk::ImageSubresourceLayers::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .layer_count(1);
    let region = vk::ImageCopy::default()
        .src_subresource(layers)
        .dst_subresource(layers)
        .extent(vk::Extent3D {
            width: plan.width,
            height: plan.height,
            depth: 1,
        });
    unsafe {
        copy(
            commands,
            vk::Image::from_raw(plan.source),
            vk::ImageLayout::from_raw(plan.source_layout as i32),
            vk::Image::from_raw(plan.target),
            vk::ImageLayout::from_raw(plan.target_layout as i32),
            1,
            &region,
        );
    }
}

pub(super) const UNDEFINED: u32 = vk::ImageLayout::UNDEFINED.as_raw() as u32;
pub(super) const SHADER_READ: u32 = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL.as_raw() as u32;
pub(super) const TRANSFER_SRC: u32 = vk::ImageLayout::TRANSFER_SRC_OPTIMAL.as_raw() as u32;
const TRANSFER_DST: u32 = vk::ImageLayout::TRANSFER_DST_OPTIMAL.as_raw() as u32;
pub(super) const FAMILY_IGNORED: u32 = vk::QUEUE_FAMILY_IGNORED;
pub(super) const FAMILY_EXTERNAL: u32 = vk::QUEUE_FAMILY_EXTERNAL;
// VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT.
pub(super) const MEMORY_ACCESS: u32 =
    vk::AccessFlags::MEMORY_READ.as_raw() | vk::AccessFlags::MEMORY_WRITE.as_raw();
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

    pub(super) fn same_queue(image: u64, old_layout: u32, new_layout: u32) -> Self {
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
