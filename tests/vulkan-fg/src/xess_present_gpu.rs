// SPDX-License-Identifier: MIT
use crate::vulkan_sr_abi::rsf_srvk_image;
use ash::{
    Device,
    vk::{self, Handle as _},
};
use std::{ffi::c_void, slice::from_ref};

pub const OUTPUT: vk::Extent2D = vk::Extent2D {
    width: 1280,
    height: 720,
};
pub const GUIDE: vk::Extent2D = vk::Extent2D {
    width: 640,
    height: 360,
};
pub const COLOR: vk::Format = vk::Format::R8G8B8A8_UNORM;
pub const COLOR_RANGE: vk::ImageSubresourceRange = vk::ImageSubresourceRange {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    base_mip_level: 0,
    level_count: 1,
    base_array_layer: 0,
    layer_count: 1,
};
const READ: vk::ImageLayout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
const TRANSFER: vk::ImageLayout = vk::ImageLayout::TRANSFER_DST_OPTIMAL;
const ATTACHMENT: vk::ImageLayout = vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL;
const ALL: vk::PipelineStageFlags = vk::PipelineStageFlags::ALL_COMMANDS;
const COPY: vk::PipelineStageFlags = vk::PipelineStageFlags::TRANSFER;
const DRAW: vk::PipelineStageFlags = vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT;

pub struct Queue {
    pub handle: vk::Queue,
    pub submit: vk::PFN_vkQueueSubmit,
}

pub unsafe extern "C" fn submit(
    user: *mut c_void,
    count: u32,
    records: *const c_void,
    fence: u64,
) -> i32 {
    // The caller serializes this fixture's single Vulkan queue.
    let queue = unsafe { &*user.cast::<Queue>() };
    unsafe {
        (queue.submit)(
            queue.handle,
            count,
            records.cast(),
            vk::Fence::from_raw(fence),
        )
    }
    .as_raw()
}

pub unsafe extern "C" fn lock(_: *mut c_void) {}

#[derive(Default)]
struct Guide {
    image: vk::Image,
    memory: vk::DeviceMemory,
}

#[derive(Default)]
pub struct Resources {
    pool: vk::CommandPool,
    pub commands: vk::CommandBuffer,
    pub acquired: vk::Semaphore,
    pub finished: vk::Semaphore,
    guides: [Guide; 2],
    pub images: [vk::Image; 3],
    render_pass: vk::RenderPass,
    views: [vk::ImageView; 3],
    framebuffers: [vk::Framebuffer; 3],
}

const GUIDE_FORMATS: [vk::Format; 2] = [vk::Format::R32_SFLOAT, vk::Format::R16G16_SFLOAT];

impl Resources {
    pub fn allocate(
        &mut self,
        device: &Device,
        family: u32,
        memory: &vk::PhysicalDeviceMemoryProperties,
    ) -> VkResult {
        unsafe {
            self.pool = device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
                    .queue_family_index(family),
                None,
            )?;
            self.commands = device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(self.pool)
                    .command_buffer_count(1),
            )?[0];
            let semaphore = vk::SemaphoreCreateInfo::default();
            self.acquired = device.create_semaphore(&semaphore, None)?;
            self.finished = device.create_semaphore(&semaphore, None)?;
            for (guide, format) in self.guides.iter_mut().zip(GUIDE_FORMATS) {
                guide.image = device.create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(format)
                        .extent(vk::Extent3D {
                            width: GUIDE.width,
                            height: GUIDE.height,
                            depth: 1,
                        })
                        .mip_levels(1)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .usage(
                            vk::ImageUsageFlags::SAMPLED
                                | vk::ImageUsageFlags::TRANSFER_SRC
                                | vk::ImageUsageFlags::TRANSFER_DST,
                        ),
                    None,
                )?;
                let requirements = device.get_image_memory_requirements(guide.image);
                let index = memory.memory_types[..memory.memory_type_count as usize]
                    .iter()
                    .enumerate()
                    .find(|(index, entry)| {
                        requirements.memory_type_bits & (1u32 << *index) != 0
                            && entry
                                .property_flags
                                .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
                    })
                    .map(|(index, _)| index as u32)
                    .ok_or(vk::Result::ERROR_OUT_OF_DEVICE_MEMORY)?;
                guide.memory = device.allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(requirements.size)
                        .memory_type_index(index),
                    None,
                )?;
                device.bind_image_memory(guide.image, guide.memory, 0)?;
            }
        }
        Ok(())
    }

    pub fn attach_images(&mut self, device: &Device, images: [u64; 3]) -> VkResult {
        self.images = images.map(vk::Image::from_raw);
        let attachment = vk::AttachmentDescription::default()
            .format(COLOR)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::LOAD)
            .store_op(vk::AttachmentStoreOp::STORE)
            .initial_layout(ATTACHMENT)
            .final_layout(ATTACHMENT);
        let color = vk::AttachmentReference {
            attachment: 0,
            layout: ATTACHMENT,
        };
        let subpass = vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .color_attachments(from_ref(&color));
        unsafe {
            self.render_pass = device.create_render_pass(
                &vk::RenderPassCreateInfo::default()
                    .attachments(from_ref(&attachment))
                    .subpasses(from_ref(&subpass)),
                None,
            )?;
            for (index, image) in self.images.into_iter().enumerate() {
                self.views[index] = device.create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(COLOR)
                        .subresource_range(COLOR_RANGE),
                    None,
                )?;
                self.framebuffers[index] = device.create_framebuffer(
                    &vk::FramebufferCreateInfo::default()
                        .render_pass(self.render_pass)
                        .attachments(from_ref(&self.views[index]))
                        .width(OUTPUT.width)
                        .height(OUTPUT.height)
                        .layers(1),
                    None,
                )?;
            }
        }
        Ok(())
    }

    pub fn start_frame(
        &self,
        device: &Device,
        frame: u32,
        index: usize,
        initialized: bool,
    ) -> VkResult {
        let commands = self.commands;
        let image = self.images[index];
        unsafe {
            device.reset_command_buffer(commands, vk::CommandBufferResetFlags::empty())?;
            device.begin_command_buffer(
                commands,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
        }
        let mut barrier = vk::ImageMemoryBarrier::default()
            .image(image)
            .subresource_range(COLOR_RANGE)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .src_access_mask(if initialized {
                vk::AccessFlags::MEMORY_READ
            } else {
                vk::AccessFlags::empty()
            })
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .old_layout(if initialized {
                READ
            } else {
                vk::ImageLayout::UNDEFINED
            })
            .new_layout(TRANSFER);
        self.barrier(device, ALL, COPY, &barrier);
        self.clear(device, image, [0.1, 0.25, 0.5, 1.0]);
        barrier = barrier
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
            .old_layout(TRANSFER)
            .new_layout(READ);
        self.barrier(device, COPY, ALL, &barrier);
        barrier = barrier
            .src_access_mask(vk::AccessFlags::SHADER_READ)
            .dst_access_mask(
                vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
            )
            .old_layout(READ)
            .new_layout(ATTACHMENT);
        self.barrier(device, ALL, DRAW, &barrier);

        let area = vk::Rect2D {
            offset: vk::Offset2D::default(),
            extent: OUTPUT,
        };
        let draw = vk::RenderPassBeginInfo::default()
            .render_pass(self.render_pass)
            .framebuffer(self.framebuffers[index])
            .render_area(area);
        let rectangle = vk::ClearAttachment::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .clear_value(vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: [0.02, 0.9, 0.9, 1.0],
                },
            });
        let region = vk::ClearRect::default().layer_count(1).rect(vk::Rect2D {
            offset: vk::Offset2D {
                x: 300 + frame as i32 * 8,
                y: 200,
            },
            extent: vk::Extent2D {
                width: 80,
                height: 120,
            },
        });
        unsafe {
            device.cmd_begin_render_pass(commands, &draw, vk::SubpassContents::INLINE);
            device.cmd_clear_attachments(commands, from_ref(&rectangle), from_ref(&region));
            device.cmd_end_render_pass(commands);
        }
        barrier = barrier
            .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
            .old_layout(ATTACHMENT)
            .new_layout(READ);
        self.barrier(device, DRAW, ALL, &barrier);

        if frame == 0 {
            // The plane advances eight output pixels each frame. Motion is current-to-previous UV.
            for (guide, value) in self.guides.iter().zip([0.5, -8.0 / 1280.0]) {
                let transfer = barrier
                    .image(guide.image)
                    .src_access_mask(vk::AccessFlags::empty())
                    .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(TRANSFER);
                self.barrier(device, ALL, COPY, &transfer);
                self.clear(device, guide.image, [value, 0.0, 0.0, 0.0]);
                let readable = transfer
                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ)
                    .old_layout(TRANSFER)
                    .new_layout(READ);
                self.barrier(device, COPY, ALL, &readable);
            }
        }
        Ok(())
    }

    fn barrier(
        &self,
        device: &Device,
        from: vk::PipelineStageFlags,
        to: vk::PipelineStageFlags,
        barrier: &vk::ImageMemoryBarrier<'_>,
    ) {
        unsafe {
            device.cmd_pipeline_barrier(
                self.commands,
                from,
                to,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                from_ref(barrier),
            );
        }
    }

    fn clear(&self, device: &Device, image: vk::Image, color: [f32; 4]) {
        unsafe {
            device.cmd_clear_color_image(
                self.commands,
                image,
                TRANSFER,
                &vk::ClearColorValue { float32: color },
                from_ref(&COLOR_RANGE),
            );
        }
    }

    pub fn inputs(&self, device: &Device, index: usize) -> [rsf_srvk_image; 3] {
        let describe =
            |image: vk::Image, format: vk::Format, extent: vk::Extent2D| rsf_srvk_image {
                device: device.handle().as_raw() as *mut c_void,
                image: image.as_raw(),
                format: format.as_raw() as u32,
                width: extent.width,
                height: extent.height,
                layout: READ.as_raw() as u32,
                copy_source: 1,
            };
        [
            describe(self.guides[0].image, GUIDE_FORMATS[0], GUIDE),
            describe(self.guides[1].image, GUIDE_FORMATS[1], GUIDE),
            describe(self.images[index], COLOR, OUTPUT),
        ]
    }

    pub fn submit(&self, device: &Device, queue: vk::Queue) -> VkResult {
        let source = vk::SubmitInfo::default()
            .wait_semaphores(from_ref(&self.acquired))
            .wait_dst_stage_mask(from_ref(&COPY))
            .command_buffers(from_ref(&self.commands))
            .signal_semaphores(from_ref(&self.finished));
        unsafe { device.queue_submit(queue, from_ref(&source), vk::Fence::null()) }
    }

    pub fn release_views(&mut self, device: &Device) {
        unsafe {
            for framebuffer in self.framebuffers {
                device.destroy_framebuffer(framebuffer, None);
            }
            for view in self.views {
                device.destroy_image_view(view, None);
            }
            device.destroy_render_pass(self.render_pass, None);
        }
    }

    pub fn release(&self, device: &Device) {
        unsafe {
            device.destroy_semaphore(self.acquired, None);
            device.destroy_semaphore(self.finished, None);
            device.destroy_command_pool(self.pool, None);
            for guide in &self.guides {
                device.destroy_image(guide.image, None);
                device.free_memory(guide.memory, None);
            }
        }
    }
}

type VkResult = Result<(), vk::Result>;
