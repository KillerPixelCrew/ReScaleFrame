// SPDX-License-Identifier: MIT
use super::{Facts, Layouts, Options, Pass, Platform, Setup, Uniform};
use crate::d3d11_sharpening::{Frame, rsf_sharpen11_active, rsf_sharpen11_pack};
use crate::vulkan_fg_resources_abi::rsf_vkfg_platform as ImageOps;
use ash::vk::{self, Handle};
use std::ffi::{CStr, c_void};
use std::{ptr, slice};

const CONSTANT_BYTES: u64 = 128;
const REQUIRED: &[&CStr] = &[
    c"vkCreateImage",
    c"vkDestroyImage",
    c"vkGetImageMemoryRequirements",
    c"vkAllocateMemory",
    c"vkFreeMemory",
    c"vkBindImageMemory",
    c"vkCreateImageView",
    c"vkDestroyImageView",
    c"vkCreateShaderModule",
    c"vkDestroyShaderModule",
    c"vkCreateDescriptorSetLayout",
    c"vkDestroyDescriptorSetLayout",
    c"vkCreatePipelineLayout",
    c"vkDestroyPipelineLayout",
    c"vkCreateComputePipelines",
    c"vkDestroyPipeline",
    c"vkCreateDescriptorPool",
    c"vkDestroyDescriptorPool",
    c"vkAllocateDescriptorSets",
    c"vkUpdateDescriptorSets",
    c"vkCreateBuffer",
    c"vkDestroyBuffer",
    c"vkGetBufferMemoryRequirements",
    c"vkBindBufferMemory",
    c"vkMapMemory",
    c"vkUnmapMemory",
    c"vkCmdPipelineBarrier",
    c"vkCmdBindPipeline",
    c"vkCmdBindDescriptorSets",
    c"vkCmdDispatch",
    c"vkCmdCopyImage",
];

unsafe extern "C" {
    fn rsf_sharpening_spirv(mode: u32, words: &mut usize) -> *const u32;
}

struct Device {
    vk: ash::Device,
    types: vk::PhysicalDeviceMemoryProperties,
}

impl Device {
    fn memory_types(&self, flags: vk::MemoryPropertyFlags) -> u32 {
        self.types
            .memory_types
            .iter()
            .take(self.types.memory_type_count as usize)
            .enumerate()
            .filter(|(_, ty)| ty.property_flags.contains(flags))
            .fold(0, |mask, (i, _)| mask | (1 << i))
    }
}

unsafe fn device<'a>(user: *mut c_void) -> &'a Device {
    unsafe { &*user.cast() }
}

unsafe extern "C" fn open(setup: *const Setup, out: *mut *mut c_void) -> i32 {
    unsafe { out.write(ptr::null_mut()) };
    let setup = unsafe { &*setup };
    let (Some(device_proc), Some(instance_proc)) = (setup.device_proc, setup.instance_proc) else {
        return vk::Result::ERROR_INITIALIZATION_FAILED.as_raw();
    };
    if setup.device.is_null() {
        return vk::Result::ERROR_INITIALIZATION_FAILED.as_raw();
    }
    let Some(properties) = (unsafe {
        instance_proc(
            setup.instance,
            c"vkGetPhysicalDeviceMemoryProperties".as_ptr(),
        )
    }) else {
        return vk::Result::ERROR_INITIALIZATION_FAILED.as_raw();
    };
    let functions: [_; REQUIRED.len()] =
        std::array::from_fn(|i| unsafe { device_proc(setup.device, REQUIRED[i].as_ptr()) });
    if functions.iter().any(Option::is_none) {
        return vk::Result::ERROR_INITIALIZATION_FAILED.as_raw();
    }
    let raw = vk::Device::from_raw(setup.device as u64);
    let vk = unsafe {
        ash::Device::load_with(
            |name| {
                REQUIRED
                    .iter()
                    .position(|&required| required == name)
                    .and_then(|i| functions[i])
                    .map_or(ptr::null(), |function| function as *const c_void)
            },
            raw,
        )
    };
    let properties: vk::PFN_vkGetPhysicalDeviceMemoryProperties =
        unsafe { std::mem::transmute(properties) };
    let mut types = vk::PhysicalDeviceMemoryProperties::default();
    unsafe {
        properties(
            vk::PhysicalDevice::from_raw(setup.physical_device as u64),
            &mut types,
        )
    };
    let leaf = crate::allocate_handle(Device { vk, types });
    if leaf.is_null() {
        return vk::Result::ERROR_OUT_OF_HOST_MEMORY.as_raw();
    }
    unsafe { out.write(leaf.cast()) };
    vk::Result::SUCCESS.as_raw()
}

unsafe extern "C" fn close(user: *mut c_void) {
    drop(unsafe { Box::from_raw(user.cast::<Device>()) });
}

unsafe extern "C" fn image_user(user: *mut c_void) -> *mut c_void {
    user
}

fn published<T: Handle>(out: *mut u64, result: Result<T, vk::Result>) -> i32 {
    match result {
        Ok(handle) => {
            unsafe { out.write(handle.as_raw()) };
            0
        }
        Err(error) => error.as_raw(),
    }
}

unsafe extern "C" fn local_memory_types(user: *mut c_void) -> u32 {
    unsafe { device(user) }.memory_types(vk::MemoryPropertyFlags::DEVICE_LOCAL)
}

unsafe extern "C" fn create_image(
    user: *mut c_void,
    width: u32,
    height: u32,
    format: u32,
    usage: u32,
    out: *mut u64,
) -> i32 {
    let d = unsafe { device(user) };
    let info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .extent(vk::Extent3D {
            width,
            height,
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(1)
        .format(vk::Format::from_raw(format as i32))
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(vk::ImageUsageFlags::from_raw(usage));
    published(out, unsafe { d.vk.create_image(&info, None) })
}

unsafe extern "C" fn requirements(user: *mut c_void, image: u64, size: *mut u64, types: *mut u32) {
    let d = unsafe { device(user) };
    let req = unsafe {
        d.vk.get_image_memory_requirements(vk::Image::from_raw(image))
    };
    unsafe {
        size.write(req.size);
        types.write(req.memory_type_bits);
    }
}

unsafe extern "C" fn allocate(user: *mut c_void, size: u64, index: u32, out: *mut u64) -> i32 {
    let d = unsafe { device(user) };
    let info = vk::MemoryAllocateInfo::default()
        .allocation_size(size)
        .memory_type_index(index);
    published(out, unsafe { d.vk.allocate_memory(&info, None) })
}

unsafe extern "C" fn bind(user: *mut c_void, image: u64, memory: u64) -> i32 {
    let d = unsafe { device(user) };
    unsafe {
        d.vk.bind_image_memory(
            vk::Image::from_raw(image),
            vk::DeviceMemory::from_raw(memory),
            0,
        )
    }
    .map_or_else(vk::Result::as_raw, |()| 0)
}

fn range(aspect_mask: vk::ImageAspectFlags) -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
}

unsafe extern "C" fn create_view(user: *mut c_void, image: u64, format: u32, out: *mut u64) -> i32 {
    let d = unsafe { device(user) };
    let format = vk::Format::from_raw(format as i32);
    let aspect = if format == vk::Format::D32_SFLOAT {
        vk::ImageAspectFlags::DEPTH
    } else {
        vk::ImageAspectFlags::COLOR
    };
    let info = vk::ImageViewCreateInfo::default()
        .image(vk::Image::from_raw(image))
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(format)
        .subresource_range(range(aspect));
    published(out, unsafe { d.vk.create_image_view(&info, None) })
}

unsafe extern "C" fn destroy_view(user: *mut c_void, view: u64) {
    unsafe {
        device(user)
            .vk
            .destroy_image_view(vk::ImageView::from_raw(view), None)
    };
}

unsafe extern "C" fn destroy_image(user: *mut c_void, image: u64) {
    unsafe {
        device(user)
            .vk
            .destroy_image(vk::Image::from_raw(image), None)
    };
}

unsafe extern "C" fn free_memory(user: *mut c_void, memory: u64) {
    unsafe {
        device(user)
            .vk
            .free_memory(vk::DeviceMemory::from_raw(memory), None)
    };
}

const DESCRIPTORS: [vk::DescriptorType; 6] = [
    vk::DescriptorType::UNIFORM_BUFFER,
    vk::DescriptorType::SAMPLED_IMAGE,
    vk::DescriptorType::SAMPLED_IMAGE,
    vk::DescriptorType::SAMPLED_IMAGE,
    vk::DescriptorType::SAMPLED_IMAGE,
    vk::DescriptorType::STORAGE_IMAGE,
];

unsafe extern "C" fn create_layouts(user: *mut c_void, out: *mut Layouts) -> i32 {
    let d = unsafe { device(user) };
    let out = unsafe { &mut *out };
    let bindings: [_; DESCRIPTORS.len()] = std::array::from_fn(|i| {
        vk::DescriptorSetLayoutBinding::default()
            .binding(i as u32)
            .descriptor_count(1)
            .descriptor_type(DESCRIPTORS[i])
            .stage_flags(vk::ShaderStageFlags::COMPUTE)
    });
    let result = unsafe {
        d.vk.create_descriptor_set_layout(
            &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
            None,
        )
    };
    let layout = match result {
        Ok(layout) => layout,
        Err(error) => return error.as_raw(),
    };
    out.descriptor_set = layout.as_raw();
    let layouts = [layout];
    let info = vk::PipelineLayoutCreateInfo::default().set_layouts(&layouts);
    published(&mut out.pipeline, unsafe {
        d.vk.create_pipeline_layout(&info, None)
    })
}

unsafe extern "C" fn release_layouts(user: *mut c_void, layouts: *mut Layouts) {
    let d = unsafe { device(user) };
    let layouts = unsafe { &mut *layouts };
    if layouts.pipeline != 0 {
        unsafe {
            d.vk.destroy_pipeline_layout(vk::PipelineLayout::from_raw(layouts.pipeline), None)
        };
    }
    if layouts.descriptor_set != 0 {
        unsafe {
            d.vk.destroy_descriptor_set_layout(
                vk::DescriptorSetLayout::from_raw(layouts.descriptor_set),
                None,
            )
        };
    }
    *layouts = Layouts::default();
}

unsafe extern "C" fn create_pipeline(
    user: *mut c_void,
    layout: u64,
    mode: u32,
    out: *mut u64,
) -> i32 {
    let d = unsafe { device(user) };
    let mut words = 0;
    let code = unsafe { rsf_sharpening_spirv(mode, &mut words) };
    if code.is_null() || words == 0 {
        return vk::Result::ERROR_INITIALIZATION_FAILED.as_raw();
    }
    let info =
        vk::ShaderModuleCreateInfo::default().code(unsafe { slice::from_raw_parts(code, words) });
    let module = match unsafe { d.vk.create_shader_module(&info, None) } {
        Ok(module) => module,
        Err(error) => return error.as_raw(),
    };
    let stage = vk::PipelineShaderStageCreateInfo::default()
        .stage(vk::ShaderStageFlags::COMPUTE)
        .module(module)
        .name(c"CSMain");
    let info = vk::ComputePipelineCreateInfo::default()
        .layout(vk::PipelineLayout::from_raw(layout))
        .stage(stage);
    let mut pipeline = vk::Pipeline::null();
    // Preserve ownership of any returned pipeline even on a partial failure.
    let result = unsafe {
        (d.vk.fp_v1_0().create_compute_pipelines)(
            d.vk.handle(),
            vk::PipelineCache::null(),
            1,
            &info,
            ptr::null(),
            &mut pipeline,
        )
    };
    unsafe {
        d.vk.destroy_shader_module(module, None);
        out.write(pipeline.as_raw());
    }
    result.as_raw()
}

unsafe extern "C" fn destroy_pipeline(user: *mut c_void, pipeline: u64) {
    unsafe {
        device(user)
            .vk
            .destroy_pipeline(vk::Pipeline::from_raw(pipeline), None)
    };
}

fn uniform(
    d: &Device,
    layout: vk::DescriptorSetLayout,
    slot: &mut Uniform,
) -> Result<(), vk::Result> {
    let sizes = [
        vk::DescriptorPoolSize {
            ty: vk::DescriptorType::UNIFORM_BUFFER,
            descriptor_count: 1,
        },
        vk::DescriptorPoolSize {
            ty: vk::DescriptorType::SAMPLED_IMAGE,
            descriptor_count: 4,
        },
        vk::DescriptorPoolSize {
            ty: vk::DescriptorType::STORAGE_IMAGE,
            descriptor_count: 1,
        },
    ];
    let info = vk::DescriptorPoolCreateInfo::default()
        .max_sets(1)
        .pool_sizes(&sizes);
    let pool = unsafe { d.vk.create_descriptor_pool(&info, None) }?;
    slot.pool = pool.as_raw();
    let layouts = [layout];
    let info = vk::DescriptorSetAllocateInfo::default()
        .descriptor_pool(pool)
        .set_layouts(&layouts);
    let mut set = vk::DescriptorSet::null();
    unsafe { (d.vk.fp_v1_0().allocate_descriptor_sets)(d.vk.handle(), &info, &mut set) }
        .result()?;
    slot.set = set.as_raw();
    let info = vk::BufferCreateInfo::default()
        .size(CONSTANT_BYTES)
        .usage(vk::BufferUsageFlags::UNIFORM_BUFFER);
    let buffer = unsafe { d.vk.create_buffer(&info, None) }?;
    slot.buffer = buffer.as_raw();
    let required = unsafe { d.vk.get_buffer_memory_requirements(buffer) };
    let compatible = required.memory_type_bits
        & d.memory_types(
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        );
    if compatible == 0 {
        return Err(vk::Result::ERROR_OUT_OF_DEVICE_MEMORY);
    }
    let info = vk::MemoryAllocateInfo::default()
        .allocation_size(required.size)
        .memory_type_index(compatible.trailing_zeros());
    let memory = unsafe { d.vk.allocate_memory(&info, None) }?;
    slot.memory = memory.as_raw();
    unsafe { d.vk.bind_buffer_memory(buffer, memory, 0) }?;
    slot.mapped = unsafe {
        d.vk.map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
    }?;
    Ok(())
}

unsafe extern "C" fn create_uniform(user: *mut c_void, layout: u64, slot: *mut Uniform) -> i32 {
    uniform(
        unsafe { device(user) },
        vk::DescriptorSetLayout::from_raw(layout),
        unsafe { &mut *slot },
    )
    .map_or_else(vk::Result::as_raw, |()| 0)
}

unsafe extern "C" fn release_uniform(user: *mut c_void, slot: *mut Uniform) {
    let d = unsafe { device(user) };
    let slot = unsafe { &mut *slot };
    let memory = vk::DeviceMemory::from_raw(slot.memory);
    unsafe {
        if !slot.mapped.is_null() {
            d.vk.unmap_memory(memory);
        }
        if slot.buffer != 0 {
            d.vk.destroy_buffer(vk::Buffer::from_raw(slot.buffer), None);
        }
        if slot.memory != 0 {
            d.vk.free_memory(memory, None);
        }
        if slot.pool != 0 {
            d.vk.destroy_descriptor_pool(vk::DescriptorPool::from_raw(slot.pool), None);
        }
    }
    *slot = Uniform::default();
}

unsafe extern "C" fn options(request: *const Options, out: *mut Options) -> u32 {
    let value = unsafe { request.as_ref() }
        .copied()
        .unwrap_or_else(crate::sharpening_policy::defaults);
    // The licensed packer applies the same normalization when writing shader words.
    unsafe { out.write(value) };
    unsafe { rsf_sharpen11_active(&value) }
}

unsafe extern "C" fn constants(mapped: *mut c_void, request: *const Options, facts: *const Facts) {
    let f = unsafe { &*facts };
    let frame = Frame {
        depth_linear: 0,
        depth_inverted: f.depth_inverted,
        hdr: 1,
        near_plane: f.near_plane,
        far_plane: f.far_plane,
        exposure: f.exposure,
        motion_x: f.motion_x,
        motion_y: f.motion_y,
    };
    let extent = [
        f.width,
        f.height,
        f.motion_width,
        f.motion_height,
        f.depth_width,
        f.depth_height,
    ];
    unsafe {
        rsf_sharpen11_pack(
            &*request,
            &frame,
            extent.as_ptr(),
            f.exposure_texture,
            f.effective_mode,
            mapped.cast(),
        )
    };
}

fn barrier(
    d: &ash::Device,
    commands: vk::CommandBuffer,
    image: vk::Image,
    transition: (
        vk::ImageLayout,
        vk::ImageLayout,
        vk::AccessFlags,
        vk::AccessFlags,
    ),
) {
    let stages = vk::PipelineStageFlags::ALL_COMMANDS;
    let (before, after, from, to) = transition;
    let barrier = vk::ImageMemoryBarrier::default()
        .image(image)
        .old_layout(before)
        .new_layout(after)
        .src_access_mask(from)
        .dst_access_mask(to)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .subresource_range(range(vk::ImageAspectFlags::COLOR));
    unsafe {
        d.cmd_pipeline_barrier(
            commands,
            stages,
            stages,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[barrier],
        )
    };
}

unsafe extern "C" fn record(user: *mut c_void, commands: *mut c_void, pass: *const Pass) {
    use vk::AccessFlags as A;
    use vk::ImageLayout as L;
    let d = &unsafe { device(user) }.vk;
    let pass = unsafe { &*pass };
    let set = vk::DescriptorSet::from_raw(pass.set);
    let buffer = [vk::DescriptorBufferInfo {
        buffer: vk::Buffer::from_raw(pass.buffer),
        offset: 0,
        range: CONSTANT_BYTES,
    }];
    let images: [_; 5] = std::array::from_fn(|i| {
        let (view, layout) = if i == 4 {
            (pass.scratch_view, L::GENERAL)
        } else {
            (pass.sampled[i], L::SHADER_READ_ONLY_OPTIMAL)
        };
        [vk::DescriptorImageInfo::default()
            .image_view(vk::ImageView::from_raw(view))
            .image_layout(layout)]
    });
    let writes: [_; DESCRIPTORS.len()] = std::array::from_fn(|i| {
        let write = vk::WriteDescriptorSet::default()
            .dst_set(set)
            .dst_binding(i as u32)
            .descriptor_type(DESCRIPTORS[i]);
        if i == 0 {
            write.buffer_info(&buffer)
        } else {
            write.image_info(&images[i - 1])
        }
    });
    let commands = vk::CommandBuffer::from_raw(commands as u64);
    let output = vk::Image::from_raw(pass.output);
    let scratch = vk::Image::from_raw(pass.scratch);
    let prior = if pass.scratch_initialized != 0 {
        L::GENERAL
    } else {
        L::UNDEFINED
    };
    unsafe { d.update_descriptor_sets(&writes, &[]) };
    barrier(
        d,
        commands,
        output,
        (
            L::GENERAL,
            L::SHADER_READ_ONLY_OPTIMAL,
            A::MEMORY_WRITE,
            A::SHADER_READ,
        ),
    );
    barrier(
        d,
        commands,
        scratch,
        (prior, L::GENERAL, A::empty(), A::SHADER_WRITE),
    );
    unsafe {
        d.cmd_bind_pipeline(
            commands,
            vk::PipelineBindPoint::COMPUTE,
            vk::Pipeline::from_raw(pass.pipeline),
        );
        d.cmd_bind_descriptor_sets(
            commands,
            vk::PipelineBindPoint::COMPUTE,
            vk::PipelineLayout::from_raw(pass.pipeline_layout),
            0,
            &[set],
            &[],
        );
        d.cmd_dispatch(commands, pass.groups_x, pass.groups_y, 1);
    }
    barrier(
        d,
        commands,
        scratch,
        (
            L::GENERAL,
            L::TRANSFER_SRC_OPTIMAL,
            A::SHADER_WRITE,
            A::TRANSFER_READ,
        ),
    );
    barrier(
        d,
        commands,
        output,
        (
            L::SHADER_READ_ONLY_OPTIMAL,
            L::TRANSFER_DST_OPTIMAL,
            A::SHADER_READ,
            A::TRANSFER_WRITE,
        ),
    );
    let layers = vk::ImageSubresourceLayers {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        mip_level: 0,
        base_array_layer: 0,
        layer_count: 1,
    };
    let copy = vk::ImageCopy::default()
        .src_subresource(layers)
        .dst_subresource(layers)
        .extent(vk::Extent3D {
            width: pass.width,
            height: pass.height,
            depth: 1,
        });
    unsafe {
        d.cmd_copy_image(
            commands,
            scratch,
            L::TRANSFER_SRC_OPTIMAL,
            output,
            L::TRANSFER_DST_OPTIMAL,
            &[copy],
        )
    };
    barrier(
        d,
        commands,
        output,
        (
            L::TRANSFER_DST_OPTIMAL,
            L::GENERAL,
            A::TRANSFER_WRITE,
            A::MEMORY_READ | A::MEMORY_WRITE,
        ),
    );
    barrier(
        d,
        commands,
        scratch,
        (
            L::TRANSFER_SRC_OPTIMAL,
            L::GENERAL,
            A::TRANSFER_READ,
            A::empty(),
        ),
    );
}

static IMAGES: ImageOps = ImageOps {
    local_memory_types: Some(local_memory_types),
    create_image: Some(create_image),
    requirements: Some(requirements),
    allocate: Some(allocate),
    bind: Some(bind),
    create_view: Some(create_view),
    destroy_view: Some(destroy_view),
    destroy_image: Some(destroy_image),
    free_memory: Some(free_memory),
    create_fence: None,
    destroy_fence: None,
    wait_fence: None,
    reset_fence: None,
    submit_fence: None,
};

pub(super) const PLATFORM: &Platform = &Platform {
    images: &IMAGES,
    open: Some(open),
    close: Some(close),
    image_user: Some(image_user),
    create_layouts: Some(create_layouts),
    release_layouts: Some(release_layouts),
    create_pipeline: Some(create_pipeline),
    destroy_pipeline: Some(destroy_pipeline),
    create_uniform: Some(create_uniform),
    release_uniform: Some(release_uniform),
    create_view: Some(create_view),
    options: Some(options),
    write_constants: Some(constants),
    record: Some(record),
};
