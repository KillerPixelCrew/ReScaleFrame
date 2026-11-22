// SPDX-License-Identifier: MIT
use crate::dx12_vulkan_platform::lookup;
use crate::vulkan_fg_resources_abi::rsf_vkfg_image as Image;
use crate::vulkan_sr_abi::{rsf_srvk_image as Source, rsf_srvk_setup};
use ash::vk::{self, Handle as _};
use std::ffi::c_void;
use std::{mem, ptr};

const ALL: vk::PipelineStageFlags = vk::PipelineStageFlags::ALL_COMMANDS;
const MEMORY: vk::AccessFlags = vk::AccessFlags::from_raw(
    vk::AccessFlags::MEMORY_READ.as_raw() | vk::AccessFlags::MEMORY_WRITE.as_raw(),
);
const SUBRESOURCE: vk::ImageSubresourceLayers = vk::ImageSubresourceLayers {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    mip_level: 0,
    base_array_layer: 0,
    layer_count: 1,
};
const SHADER_READ: vk::ImageLayout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
const SOURCE: vk::ImageLayout = vk::ImageLayout::TRANSFER_SRC_OPTIMAL;
const DESTINATION: vk::ImageLayout = vk::ImageLayout::TRANSFER_DST_OPTIMAL;
const UNDEFINED: vk::ImageLayout = vk::ImageLayout::UNDEFINED;
const GENERAL: vk::ImageLayout = vk::ImageLayout::GENERAL;

pub(super) struct Dispatch {
    barrier: vk::PFN_vkCmdPipelineBarrier,
    copy: vk::PFN_vkCmdCopyImage,
    blit: vk::PFN_vkCmdBlitImage,
}

impl Dispatch {
    pub(super) fn load(setup: &rsf_srvk_setup) -> Option<Self> {
        let resolve = |name| lookup(setup.device_proc, setup.device, name);
        Some(Self {
            barrier: unsafe {
                mem::transmute::<unsafe extern "C" fn(), vk::PFN_vkCmdPipelineBarrier>(resolve(
                    c"vkCmdPipelineBarrier",
                )?)
            },
            copy: unsafe {
                mem::transmute::<unsafe extern "C" fn(), vk::PFN_vkCmdCopyImage>(resolve(
                    c"vkCmdCopyImage",
                )?)
            },
            blit: unsafe {
                mem::transmute::<unsafe extern "C" fn(), vk::PFN_vkCmdBlitImage>(resolve(
                    c"vkCmdBlitImage",
                )?)
            },
        })
    }

    fn transition(
        &self,
        commands: vk::CommandBuffer,
        image: vk::Image,
        before: vk::ImageLayout,
        after: vk::ImageLayout,
    ) {
        let source = if before == UNDEFINED {
            vk::AccessFlags::empty()
        } else {
            MEMORY
        };
        let barrier = vk::ImageMemoryBarrier::default()
            .image(image)
            .old_layout(before)
            .new_layout(after)
            .src_access_mask(source)
            .dst_access_mask(MEMORY)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            });
        unsafe {
            (self.barrier)(
                commands,
                ALL,
                ALL,
                vk::DependencyFlags::empty(),
                0,
                ptr::null(),
                0,
                ptr::null(),
                1,
                &barrier,
            )
        };
    }

    fn copy(
        &self,
        commands: vk::CommandBuffer,
        source: vk::Image,
        destination: vk::Image,
        width: u32,
        height: u32,
    ) {
        let region = vk::ImageCopy::default()
            .src_subresource(SUBRESOURCE)
            .dst_subresource(SUBRESOURCE)
            .extent(vk::Extent3D {
                width,
                height,
                depth: 1,
            });
        unsafe {
            (self.copy)(
                commands,
                source,
                SOURCE,
                destination,
                DESTINATION,
                1,
                &region,
            )
        };
    }

    pub(super) fn stage(
        &self,
        commands: *mut c_void,
        source: &Source,
        destination: &Image,
        bias: bool,
    ) {
        let commands = vk::CommandBuffer::from_raw(commands as u64);
        let input = vk::Image::from_raw(source.image);
        let output = vk::Image::from_raw(destination.image);
        let previous = if destination.initialized != 0 {
            SHADER_READ
        } else {
            UNDEFINED
        };
        self.transition(commands, input, SHADER_READ, SOURCE);
        self.transition(commands, output, previous, DESTINATION);
        if bias {
            // The pinned Streamline Vulkan format mapper omits R8_UNORM.
            let offsets = [
                vk::Offset3D::default(),
                vk::Offset3D {
                    x: source.width as i32,
                    y: source.height as i32,
                    z: 1,
                },
            ];
            let region = vk::ImageBlit::default()
                .src_subresource(SUBRESOURCE)
                .dst_subresource(SUBRESOURCE)
                .src_offsets(offsets)
                .dst_offsets(offsets);
            unsafe {
                (self.blit)(
                    commands,
                    input,
                    SOURCE,
                    output,
                    DESTINATION,
                    1,
                    &region,
                    vk::Filter::NEAREST,
                )
            };
        } else {
            self.copy(commands, input, output, source.width, source.height);
        }
        self.transition(commands, input, SOURCE, SHADER_READ);
        self.transition(commands, output, DESTINATION, SHADER_READ);
    }

    pub(super) fn initialize_output(&self, commands: *mut c_void, output: &Image) {
        self.transition(
            vk::CommandBuffer::from_raw(commands as u64),
            vk::Image::from_raw(output.image),
            UNDEFINED,
            GENERAL,
        );
    }

    pub(super) fn publish(
        &self,
        commands: *mut c_void,
        source: u64,
        destination: u64,
        width: u32,
        height: u32,
    ) {
        let commands = vk::CommandBuffer::from_raw(commands as u64);
        let source = vk::Image::from_raw(source);
        self.transition(commands, source, GENERAL, SOURCE);
        self.copy(
            commands,
            source,
            vk::Image::from_raw(destination),
            width,
            height,
        );
        self.transition(commands, source, SOURCE, GENERAL);
    }
}
