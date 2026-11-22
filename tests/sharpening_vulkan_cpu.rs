// SPDX-License-Identifier: MIT
//! Production Vulkan dispatch contract exercised with typed fake entry points.
use super::{TestResult, check};
use crate::sharpening_policy::defaults;
use crate::sys::rsf_sharpenvk_host as Host;
use crate::sys::{
    rsf_srvk_frame as Frame, rsf_srvk_image as Image, rsf_srvk_setup as Setup, rsf_vk_function,
};
use crate::vulkan_sharpening::*;
use ash::vk::{self, Handle};
use rsf_game_sys::rsf_frame_record;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::{CStr, c_char, c_void};
use std::{ptr, slice};

const OUTPUT: u64 = 0x51;
const MOTION: u64 = 0x52;
const DEPTH: u64 = 0x53;
const EXPOSURE: u64 = 0x54;
const W: u32 = 33;
const H: u32 = 17;
const GENERAL: vk::ImageLayout = vk::ImageLayout::GENERAL;
const READ: vk::ImageLayout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
const UNDEFINED: vk::ImageLayout = vk::ImageLayout::UNDEFINED;
const RGBA: vk::Format = vk::Format::R16G16B16A16_SFLOAT;
const FMT_DEPTH: vk::Format = vk::Format::D32_SFLOAT;
const FMT_MOTION: vk::Format = vk::Format::R16G16_SFLOAT;
const FMT_EXPOSURE: vk::Format = vk::Format::R16_SFLOAT;
const LOG: &str = "Vulkan sharpening unavailable; retaining vendor output";
const TYPES: [vk::DescriptorType; 6] = [
    vk::DescriptorType::UNIFORM_BUFFER,
    vk::DescriptorType::SAMPLED_IMAGE,
    vk::DescriptorType::SAMPLED_IMAGE,
    vk::DescriptorType::SAMPLED_IMAGE,
    vk::DescriptorType::SAMPLED_IMAGE,
    vk::DescriptorType::STORAGE_IMAGE,
];

unsafe extern "C" {
    fn rsf_sharpening_spirv(mode: u32, words: &mut usize) -> *const u32;
}

type Write = (
    u32,
    vk::DescriptorType,
    vk::ImageView,
    vk::ImageLayout,
    vk::Buffer,
);
type Dispatch = (u64, vk::Pipeline, [u32; 3]);
type Barrier = (vk::Image, vk::ImageLayout, vk::ImageLayout);
type ImageSpec = (
    vk::Format,
    vk::Extent3D,
    vk::ImageUsageFlags,
    vk::ImageTiling,
    u32,
    u32,
);

#[derive(Default)]
struct Trace {
    next: u64,
    live: BTreeMap<u64, &'static str>,
    sets: BTreeMap<u64, u64>,
    made: Vec<&'static str>,
    shaders: Vec<Vec<u32>>,
    entries: Vec<Vec<u8>>,
    pipelines: Vec<vk::Pipeline>,
    images: Vec<ImageSpec>,
    views: BTreeMap<u64, (vk::Format, vk::ImageAspectFlags)>,
    memory_types: Vec<u32>,
    pools: usize,
    writes: Vec<Write>,
    barriers: Vec<Barrier>,
    dispatches: Vec<Dispatch>,
    copies: Vec<vk::Extent3D>,
    logs: Vec<String>,
    violations: Vec<&'static str>,
    fail: &'static str,
    bound: vk::Pipeline,
    last_mapped: u64,
    memory: BTreeMap<u64, Box<[u32; 64]>>,
}

thread_local! { static TRACE: RefCell<Trace> = RefCell::new(Trace::default()); }

fn with<T>(f: impl FnOnce(&mut Trace) -> T) -> T {
    TRACE.with_borrow_mut(f)
}

impl Trace {
    fn check(&mut self, valid: bool, label: &'static str) {
        if !valid {
            self.violations.push(label);
        }
    }

    fn make<T: Handle>(
        &mut self,
        kind: &'static str,
        entry: &'static str,
        out: *mut T,
    ) -> vk::Result {
        self.made.push(entry);
        if self.fail == entry {
            return vk::Result::ERROR_OUT_OF_DEVICE_MEMORY;
        }
        self.next += 1;
        let handle = self.next + 0x10000;
        self.live.insert(handle, kind);
        unsafe { out.write(T::from_raw(handle)) };
        vk::Result::SUCCESS
    }

    fn drop_handle(&mut self, id: impl Handle) {
        let was_live = self.live.remove(&id.as_raw()).is_some();
        self.check(was_live, "released handle was live");
    }

    fn count(&self, kind: &str) -> usize {
        self.live.values().filter(|&&name| name == kind).count()
    }

    fn clean(&self) -> TestResult {
        check(
            self.live.is_empty() && self.memory.is_empty(),
            "all handles retired",
        )?;
        check(
            self.violations.is_empty(),
            &format!("fake Vulkan violations: {:?}", self.violations),
        )
    }
}

macro_rules! destroy {
    ($name:ident, $type:ident) => {
        unsafe extern "system" fn $name(
            _: vk::Device,
            handle: vk::$type,
            _: *const vk::AllocationCallbacks<'_>,
        ) {
            with(|trace| trace.drop_handle(handle));
        }
    };
}
destroy!(destroy_shader, ShaderModule);
destroy!(destroy_set_layout, DescriptorSetLayout);
destroy!(destroy_pipeline_layout, PipelineLayout);
destroy!(destroy_pipeline, Pipeline);
destroy!(destroy_buffer, Buffer);
destroy!(destroy_image, Image);
destroy!(destroy_view, ImageView);

unsafe extern "system" fn create_shader(
    _: vk::Device,
    info: *const vk::ShaderModuleCreateInfo<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::ShaderModule,
) -> vk::Result {
    let info = unsafe { &*info };
    let words = unsafe { slice::from_raw_parts(info.p_code, info.code_size / 4) };
    with(|s| {
        s.shaders.push(words.to_vec());
        s.make("shader", "vkCreateShaderModule", out)
    })
}

unsafe extern "system" fn create_set_layout(
    _: vk::Device,
    info: *const vk::DescriptorSetLayoutCreateInfo<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::DescriptorSetLayout,
) -> vk::Result {
    let info = unsafe { &*info };
    let bindings = unsafe { slice::from_raw_parts(info.p_bindings, info.binding_count as usize) };
    with(|s| {
        s.check(
            bindings.len() == 6
                && bindings
                    .iter()
                    .zip(TYPES)
                    .enumerate()
                    .all(|(i, (item, ty))| {
                        item.binding == i as u32
                            && item.descriptor_type == ty
                            && item.descriptor_count == 1
                            && item.stage_flags == vk::ShaderStageFlags::COMPUTE
                    }),
            "sampled, storage and constant layout",
        );
        s.make("layout", "vkCreateDescriptorSetLayout", out)
    })
}

unsafe extern "system" fn create_pipeline_layout(
    _: vk::Device,
    _: *const vk::PipelineLayoutCreateInfo<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::PipelineLayout,
) -> vk::Result {
    with(|s| s.make("layout", "vkCreatePipelineLayout", out))
}

unsafe extern "system" fn create_pipeline(
    _: vk::Device,
    _: vk::PipelineCache,
    count: u32,
    info: *const vk::ComputePipelineCreateInfo<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::Pipeline,
) -> vk::Result {
    let info = unsafe { &*info };
    with(|s| {
        s.check(
            count == 1 && s.live.contains_key(&info.stage.module.as_raw()),
            "pipeline consumes one live shader module",
        );
        let result = s.make("pipeline", "vkCreateComputePipelines", out);
        if result == vk::Result::SUCCESS {
            s.entries.push(
                unsafe { CStr::from_ptr(info.stage.p_name) }
                    .to_bytes()
                    .to_vec(),
            );
            s.pipelines.push(unsafe { *out });
        }
        result
    })
}

unsafe extern "system" fn create_pool(
    _: vk::Device,
    info: *const vk::DescriptorPoolCreateInfo<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::DescriptorPool,
) -> vk::Result {
    let info = unsafe { &*info };
    with(|s| {
        s.check(
            info.max_sets == 1 && info.pool_size_count == 3,
            "one set per slot",
        );
        let result = s.make("pool", "vkCreateDescriptorPool", out);
        if result == vk::Result::SUCCESS {
            s.pools += 1;
        }
        result
    })
}

unsafe extern "system" fn destroy_pool(
    _: vk::Device,
    pool: vk::DescriptorPool,
    _: *const vk::AllocationCallbacks<'_>,
) {
    with(|s| {
        s.sets.retain(|id, owner| {
            if *owner == pool.as_raw() {
                s.live.remove(id);
                false
            } else {
                true
            }
        });
        s.drop_handle(pool);
    });
}

unsafe extern "system" fn allocate_sets(
    _: vk::Device,
    info: *const vk::DescriptorSetAllocateInfo<'_>,
    out: *mut vk::DescriptorSet,
) -> vk::Result {
    with(|s| {
        let result = s.make("set", "vkAllocateDescriptorSets", out);
        if result == vk::Result::SUCCESS {
            s.sets.insert(
                unsafe { *out }.as_raw(),
                unsafe { (*info).descriptor_pool }.as_raw(),
            );
        }
        result
    })
}

unsafe extern "system" fn update_sets(
    _: vk::Device,
    count: u32,
    writes: *const vk::WriteDescriptorSet<'_>,
    copies: u32,
    _: *const vk::CopyDescriptorSet<'_>,
) {
    let entries = unsafe { slice::from_raw_parts(writes, count as usize) };
    with(|s| {
        s.check(count == 6 && copies == 0, "six descriptor writes");
        for entry in entries {
            s.check(
                entry.descriptor_count == 1 && s.live.contains_key(&entry.dst_set.as_raw()),
                "live descriptor set",
            );
            let image = unsafe { entry.p_image_info.as_ref() };
            let buffer = unsafe { entry.p_buffer_info.as_ref() };
            s.writes.push((
                entry.dst_binding,
                entry.descriptor_type,
                image.map_or(vk::ImageView::null(), |i| i.image_view),
                image.map_or(UNDEFINED, |i| i.image_layout),
                buffer.map_or(vk::Buffer::null(), |b| b.buffer),
            ));
        }
    });
}

unsafe extern "system" fn create_buffer(
    _: vk::Device,
    info: *const vk::BufferCreateInfo<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::Buffer,
) -> vk::Result {
    let info = unsafe { &*info };
    with(|s| {
        s.check(
            info.size == 128 && info.usage == vk::BufferUsageFlags::UNIFORM_BUFFER,
            "uniform block",
        );
        s.make("buffer", "vkCreateBuffer", out)
    })
}

unsafe extern "system" fn buffer_requirements(
    _: vk::Device,
    _: vk::Buffer,
    out: *mut vk::MemoryRequirements,
) {
    unsafe {
        out.write(vk::MemoryRequirements {
            size: 256,
            alignment: 16,
            memory_type_bits: 3,
        })
    };
}

unsafe extern "system" fn allocate(
    _: vk::Device,
    info: *const vk::MemoryAllocateInfo<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::DeviceMemory,
) -> vk::Result {
    with(|s| {
        let result = s.make("memory", "vkAllocateMemory", out);
        if result == vk::Result::SUCCESS {
            s.memory_types.push(unsafe { (*info).memory_type_index });
            s.memory.insert(unsafe { *out }.as_raw(), Box::new([0; 64]));
        }
        result
    })
}

unsafe extern "system" fn free_memory(
    _: vk::Device,
    memory: vk::DeviceMemory,
    _: *const vk::AllocationCallbacks<'_>,
) {
    with(|s| {
        s.memory.remove(&memory.as_raw());
        s.drop_handle(memory);
    });
}

unsafe extern "system" fn bind_buffer(
    _: vk::Device,
    _: vk::Buffer,
    _: vk::DeviceMemory,
    _: u64,
) -> vk::Result {
    vk::Result::SUCCESS
}
unsafe extern "system" fn bind_image(
    _: vk::Device,
    _: vk::Image,
    _: vk::DeviceMemory,
    _: u64,
) -> vk::Result {
    vk::Result::SUCCESS
}

unsafe extern "system" fn map(
    _: vk::Device,
    memory: vk::DeviceMemory,
    _: u64,
    _: u64,
    _: vk::MemoryMapFlags,
    out: *mut *mut c_void,
) -> vk::Result {
    with(|s| {
        let Some(words) = s.memory.get_mut(&memory.as_raw()) else {
            return vk::Result::ERROR_MEMORY_MAP_FAILED;
        };
        s.last_mapped = memory.as_raw();
        unsafe { out.write(words.as_mut_ptr().cast()) };
        vk::Result::SUCCESS
    })
}

unsafe extern "system" fn unmap(_: vk::Device, memory: vk::DeviceMemory) {
    with(|s| s.check(s.live.contains_key(&memory.as_raw()), "unmap before free"));
}

unsafe extern "system" fn create_image(
    _: vk::Device,
    info: *const vk::ImageCreateInfo<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::Image,
) -> vk::Result {
    let info = unsafe { &*info };
    with(|s| {
        s.images.push((
            info.format,
            info.extent,
            info.usage,
            info.tiling,
            info.mip_levels,
            info.array_layers,
        ));
        s.make("image", "vkCreateImage", out)
    })
}

unsafe extern "system" fn image_requirements(
    _: vk::Device,
    _: vk::Image,
    out: *mut vk::MemoryRequirements,
) {
    unsafe {
        out.write(vk::MemoryRequirements {
            size: 4096,
            alignment: 256,
            memory_type_bits: 3,
        })
    };
}

unsafe extern "system" fn create_view(
    _: vk::Device,
    info: *const vk::ImageViewCreateInfo<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::ImageView,
) -> vk::Result {
    let info = unsafe { &*info };
    with(|s| {
        s.views.insert(
            info.image.as_raw(),
            (info.format, info.subresource_range.aspect_mask),
        );
        if s.fail == "depth view" && info.format == FMT_DEPTH {
            return vk::Result::ERROR_OUT_OF_HOST_MEMORY;
        }
        s.make("view", "vkCreateImageView", out)
    })
}

unsafe extern "system" fn barrier(
    _: vk::CommandBuffer,
    _: vk::PipelineStageFlags,
    _: vk::PipelineStageFlags,
    _: vk::DependencyFlags,
    _: u32,
    _: *const vk::MemoryBarrier<'_>,
    _: u32,
    _: *const vk::BufferMemoryBarrier<'_>,
    count: u32,
    images: *const vk::ImageMemoryBarrier<'_>,
) {
    let images = unsafe { slice::from_raw_parts(images, count as usize) };
    with(|s| {
        s.barriers.extend(
            images
                .iter()
                .map(|image| (image.image, image.old_layout, image.new_layout)),
        )
    });
}

unsafe extern "system" fn bind_pipeline(
    _: vk::CommandBuffer,
    point: vk::PipelineBindPoint,
    pipeline: vk::Pipeline,
) {
    with(|s| {
        s.check(point == vk::PipelineBindPoint::COMPUTE, "compute pipeline");
        s.bound = pipeline;
    });
}

unsafe extern "system" fn bind_sets(
    _: vk::CommandBuffer,
    _: vk::PipelineBindPoint,
    _: vk::PipelineLayout,
    _: u32,
    count: u32,
    _: *const vk::DescriptorSet,
    _: u32,
    _: *const u32,
) {
    with(|s| s.check(count == 1, "one descriptor set"));
}

unsafe extern "system" fn dispatch(commands: vk::CommandBuffer, x: u32, y: u32, z: u32) {
    with(|s| s.dispatches.push((commands.as_raw(), s.bound, [x, y, z])));
}

unsafe extern "system" fn copy(
    _: vk::CommandBuffer,
    _: vk::Image,
    _: vk::ImageLayout,
    _: vk::Image,
    _: vk::ImageLayout,
    count: u32,
    regions: *const vk::ImageCopy,
) {
    with(|s| {
        s.check(count == 1, "one copy");
        if count != 0 {
            s.copies.push(unsafe { (*regions).extent });
        }
    });
}

unsafe extern "system" fn properties(
    _: vk::PhysicalDevice,
    out: *mut vk::PhysicalDeviceMemoryProperties,
) {
    let mut properties = vk::PhysicalDeviceMemoryProperties {
        memory_type_count: 2,
        ..Default::default()
    };
    properties.memory_types[0].property_flags = vk::MemoryPropertyFlags::DEVICE_LOCAL;
    properties.memory_types[1].property_flags =
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
    unsafe { out.write(properties) };
}

macro_rules! fptr {
    ($function:ident) => {
        Some(unsafe {
            std::mem::transmute::<*const (), unsafe extern "C" fn()>($function as *const ())
        })
    };
}

unsafe extern "C" fn device_proc(_: *mut c_void, name: *const c_char) -> rsf_vk_function {
    match unsafe { CStr::from_ptr(name) }.to_bytes() {
        b"vkCreateShaderModule" => fptr!(create_shader),
        b"vkDestroyShaderModule" => fptr!(destroy_shader),
        b"vkCreateDescriptorSetLayout" => fptr!(create_set_layout),
        b"vkDestroyDescriptorSetLayout" => fptr!(destroy_set_layout),
        b"vkCreatePipelineLayout" => fptr!(create_pipeline_layout),
        b"vkDestroyPipelineLayout" => fptr!(destroy_pipeline_layout),
        b"vkCreateComputePipelines" => fptr!(create_pipeline),
        b"vkDestroyPipeline" => fptr!(destroy_pipeline),
        b"vkCreateDescriptorPool" => fptr!(create_pool),
        b"vkDestroyDescriptorPool" => fptr!(destroy_pool),
        b"vkAllocateDescriptorSets" => fptr!(allocate_sets),
        b"vkUpdateDescriptorSets" => fptr!(update_sets),
        b"vkCreateBuffer" => fptr!(create_buffer),
        b"vkDestroyBuffer" => fptr!(destroy_buffer),
        b"vkGetBufferMemoryRequirements" => fptr!(buffer_requirements),
        b"vkAllocateMemory" => fptr!(allocate),
        b"vkFreeMemory" => fptr!(free_memory),
        b"vkBindBufferMemory" => fptr!(bind_buffer),
        b"vkMapMemory" => fptr!(map),
        b"vkUnmapMemory" => fptr!(unmap),
        b"vkCreateImage" => fptr!(create_image),
        b"vkDestroyImage" => fptr!(destroy_image),
        b"vkGetImageMemoryRequirements" => fptr!(image_requirements),
        b"vkBindImageMemory" => fptr!(bind_image),
        b"vkCreateImageView" => fptr!(create_view),
        b"vkDestroyImageView" => fptr!(destroy_view),
        b"vkCmdPipelineBarrier" => fptr!(barrier),
        b"vkCmdBindPipeline" => fptr!(bind_pipeline),
        b"vkCmdBindDescriptorSets" => fptr!(bind_sets),
        b"vkCmdDispatch" => fptr!(dispatch),
        b"vkCmdCopyImage" => fptr!(copy),
        _ => None,
    }
}

unsafe extern "C" fn instance_proc(_: *mut c_void, name: *const c_char) -> rsf_vk_function {
    match unsafe { CStr::from_ptr(name) }.to_bytes() {
        b"vkGetPhysicalDeviceMemoryProperties" => fptr!(properties),
        _ => None,
    }
}

unsafe extern "C" fn log(_: *mut c_void, text: *const c_char) {
    with(|s| {
        s.logs.push(
            unsafe { CStr::from_ptr(text) }
                .to_string_lossy()
                .into_owned(),
        )
    });
}

struct Fixture {
    host: Host,
    setup: Setup,
    record: Box<rsf_frame_record>,
}

impl Fixture {
    fn new() -> Self {
        with(|s| *s = Trace::default());
        let mut setup: Setup = unsafe { std::mem::zeroed() };
        setup.struct_size = size_of::<Setup>() as u32;
        setup.abi_version = 2;
        setup.instance = 0x11 as *mut c_void;
        setup.physical_device = 0x12 as *mut c_void;
        setup.device = 0x13 as *mut c_void;
        setup.instance_proc = Some(instance_proc);
        setup.device_proc = Some(device_proc);
        let mut record: Box<rsf_frame_record> = Box::new(unsafe { std::mem::zeroed() });
        record.output_width = W;
        record.output_height = H;
        record.camera.near_plane = 0.1;
        record.camera.far_plane = 1000.0;
        record.camera.depth_inverted = 1;
        Self {
            host: Host::default(),
            setup,
            record,
        }
    }

    fn image(&self, value: u64, format: vk::Format, width: u32, height: u32) -> Image {
        Image {
            device: self.setup.device,
            image: value,
            format: format.as_raw() as u32,
            width,
            height,
            layout: READ.as_raw() as u32,
            copy_source: 0,
        }
    }

    fn frame(&self, mode: u32, depth: bool) -> Frame {
        let mut frame: Frame = unsafe { std::mem::zeroed() };
        frame.struct_size = size_of::<Frame>() as u32;
        frame.abi_version = 2;
        frame.record = &*self.record;
        frame.motion = self.image(MOTION, FMT_MOTION, W, H);
        frame.motion_scale_x = 1.0;
        frame.motion_scale_y = 1.0;
        if depth {
            frame.depth = self.image(DEPTH, FMT_DEPTH, W, H);
        }
        frame.pre_exposure = 2.0;
        frame.sharpening = defaults();
        frame.sharpening.mode = mode;
        frame.sharpening.sharpness = 0.5;
        frame.log = Some(log);
        frame
    }

    fn run(&mut self, command: usize, frame: Frame) -> bool {
        unsafe {
            rsf_sharpenvk_host_run(
                &mut self.host,
                &self.setup,
                command as *mut c_void,
                &frame,
                OUTPUT,
            ) != 0
        }
    }

    fn retire(&mut self, command: usize) {
        unsafe { rsf_sharpenvk_host_retire(&mut self.host, command as *mut c_void) };
    }

    fn count(&self) -> u64 {
        unsafe { rsf_sharpenvk_host_dispatches(&self.host) }
    }
    fn clear(&mut self) -> TestResult {
        unsafe { rsf_sharpenvk_host_clear(&mut self.host) };
        with(|s| s.clean())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe { rsf_sharpenvk_host_destroy(&mut self.host) };
    }
}

fn admission() -> TestResult {
    let mut f = Fixture::new();
    let zero: Frame = unsafe { std::mem::zeroed() };
    let setup: Setup = unsafe { std::mem::zeroed() };
    check(
        unsafe { rsf_sharpenvk_host_run(&mut f.host, &setup, ptr::null_mut(), &zero, 0) } != 0
            && f.count() == 0,
        "Off requires no resources",
    )?;
    for mode in [0, 4, 9] {
        check(
            f.run(1, f.frame(mode, true)),
            "inactive filter keeps vendor output",
        )?;
    }
    let mut frame = f.frame(1, true);
    frame.sharpening.sharpness = 0.0;
    check(f.run(1, frame), "zero strength is inactive")?;
    let invalid: [fn(&mut Frame); 6] = [
        |f| f.record = ptr::null(),
        |f| f.motion.layout = GENERAL.as_raw() as u32,
        |f| f.depth.device = ptr::null_mut(),
        |f| f.depth.width = 0,
        |f| {
            f.exposure = f.motion;
            f.exposure.width = 2;
            f.exposure.height = 1;
            f.exposure.format = FMT_EXPOSURE.as_raw() as u32;
        },
        |f| {
            f.exposure = f.motion;
            f.exposure.width = 1;
            f.exposure.height = 1;
            f.exposure.format = RGBA.as_raw() as u32;
        },
    ];
    for change in invalid {
        let mut frame = f.frame(1, true);
        change(&mut frame);
        check(!f.run(1, frame), "invalid frame is refused")?;
    }
    check(!f.run(0, f.frame(1, true)), "commands required")?;
    let normal = f.frame(1, true);
    check(
        unsafe {
            rsf_sharpenvk_host_run(
                &mut f.host,
                &f.setup,
                std::ptr::dangling_mut::<c_void>(),
                &normal,
                0,
            )
        } == 0,
        "output required",
    )?;
    f.record.output_height = 0;
    check(!f.run(1, f.frame(1, true)), "zero extent refused")?;
    check(
        with(|s| s.made.is_empty()) && f.count() == 0,
        "refusals precede Vulkan work",
    )?;
    f.clear()
}

fn constants() -> TestResult {
    let mut f = Fixture::new();
    let mut frame = f.frame(1, true);
    frame.sharpening.sharpness = f32::NAN;
    check(
        f.run(1, frame) && f.count() == 1,
        "non-finite strength uses active default",
    )?;
    check(
        with(|s| s.memory[&s.last_mapped][0] == 0.4f32.to_bits()),
        "default reaches GPU constants",
    )?;
    frame.sharpening.sharpness = 0.0;
    frame.sharpening.mas = 1;
    frame.sharpening.motion_sharpness = -0.2;
    check(
        f.run(2, frame) && f.count() == 1,
        "negative MAS alone is inactive",
    )?;
    frame.sharpening.motion_sharpness = 0.2;
    check(
        f.run(2, frame) && f.count() == 2,
        "positive MAS alone activates the pass",
    )?;
    f.clear()
}

fn exact_pass() -> TestResult {
    let mut f = Fixture::new();
    let mut frame = f.frame(2, true);
    frame.exposure = f.image(EXPOSURE, FMT_EXPOSURE, 1, 1);
    check(f.run(7, frame), "record depth RCAS")?;
    with(|s| -> TestResult {
        check(
            s.shaders.len() == 3
                && s.count("shader") == 0
                && s.entries.len() == 3
                && s.entries.iter().all(|entry| entry.as_slice() == b"CSMain"),
            "three shader modules and entry points",
        )?;
        for (i, payload) in s.shaders.iter().enumerate() {
            let mut words = 0;
            let expected = unsafe { rsf_sharpening_spirv(i as u32 + 1, &mut words) };
            check(
                !expected.is_null()
                    && payload.as_slice() == unsafe { slice::from_raw_parts(expected, words) },
                "SPIR-V bytes in mode order",
            )?;
        }
        let extent = vk::Extent3D {
            width: W,
            height: H,
            depth: 1,
        };
        check(
            s.memory_types == [1, 0]
                && s.images
                    == [(
                        RGBA,
                        extent,
                        vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::TRANSFER_SRC,
                        vk::ImageTiling::OPTIMAL,
                        1,
                        1,
                    )],
            "host-coherent uniforms, device-local RGBA16F scratch",
        )?;
        check(
            s.views.len() == 5
                && s.views.get(&OUTPUT) == Some(&(RGBA, vk::ImageAspectFlags::COLOR))
                && s.views.get(&DEPTH) == Some(&(FMT_DEPTH, vk::ImageAspectFlags::DEPTH))
                && s.views.get(&MOTION) == Some(&(FMT_MOTION, vk::ImageAspectFlags::COLOR))
                && s.views.get(&EXPOSURE) == Some(&(FMT_EXPOSURE, vk::ImageAspectFlags::COLOR)),
            "the scratch and four view bindings keep their formats and aspects",
        )?;
        check(s.writes.len() == 6, "one exact descriptor update")?;
        for (i, &(binding, ty, view, layout, buffer)) in s.writes.iter().enumerate() {
            check(
                binding == i as u32
                    && ty == TYPES[i]
                    && match i {
                        0 => buffer != vk::Buffer::null(),
                        5 => layout == GENERAL && view != vk::ImageView::null(),
                        _ => layout == READ && view != vk::ImageView::null(),
                    },
                "descriptor binding, type and layout",
            )?;
        }
        check(
            s.dispatches == [(7, s.pipelines[1], [3, 2, 1])] && s.copies == [extent],
            "dispatch shape, pipeline and copy extent",
        )?;
        let output = vk::Image::from_raw(OUTPUT);
        let scratch = s.barriers[1].0;
        let src = vk::ImageLayout::TRANSFER_SRC_OPTIMAL;
        let dst = vk::ImageLayout::TRANSFER_DST_OPTIMAL;
        check(
            s.barriers
                == [
                    (output, GENERAL, READ),
                    (scratch, UNDEFINED, GENERAL),
                    (scratch, GENERAL, src),
                    (output, READ, dst),
                    (output, dst, GENERAL),
                    (scratch, src, GENERAL),
                ],
            "six image barriers in order",
        )?;
        let words = &s.memory[&s.last_mapped];
        check(
            [
                words[0], words[14], words[15], words[17], words[29], words[30], words[31],
            ] == [1.0f32.to_bits(), W, H, 1, 1, 1, 0.5f32.to_bits()],
            "packed shader constants",
        )?;
        s.writes.clear();
        s.barriers.clear();
        Ok(())
    })?;
    check(f.run(8, f.frame(2, false)), "record RCAS fallback")?;
    with(|s| {
        check(
            s.dispatches.last().is_some_and(|d| d.1 == s.pipelines[0])
                && s.writes[3].2 == s.writes[1].2
                && s.writes[4].2 == s.writes[1].2
                && s.writes[2].2 != s.writes[1].2
                && s.pools == 2
                && s.memory[&s.last_mapped][0] == 0.5f32.to_bits(),
            "missing inputs bind output and fallback has RCAS strength",
        )
    })?;
    let views = with(|s| s.count("view"));
    f.retire(7);
    f.retire(0);
    f.retire(99);
    check(
        with(|s| s.count("view")) == views - 4,
        "retire releases only this recording's views",
    )?;
    with(|s| s.barriers.clear());
    check(f.run(9, f.frame(3, true)), "record depth DAS")?;
    with(|s| {
        check(
            s.pools == 2
                && s.dispatches.last().is_some_and(|d| d.1 == s.pipelines[2])
                && s.barriers[1].1 == GENERAL
                && s.logs.is_empty(),
            "retired slot reuses initialized scratch and UBO",
        )
    })?;
    check(f.count() == 3, "three dispatches")?;
    f.clear()
}

fn bounded_slots() -> TestResult {
    let mut f = Fixture::new();
    let frame = f.frame(1, true);
    for command in 1..=64 {
        check(f.run(command, frame), "64 in-flight recordings")?;
    }
    check(
        !f.run(65, frame) && !f.run(66, frame) && with(|s| s.logs == [LOG]),
        "one report after slot exhaustion",
    )?;
    f.retire(5);
    check(
        f.run(65, frame) && f.count() == 65 && with(|s| s.pools == 64),
        "reuse a retired slot",
    )?;
    f.clear()
}

fn failures() -> TestResult {
    {
        let mut f = Fixture::new();
        with(|s| s.fail = "vkCreateComputePipelines");
        check(!f.run(1, f.frame(1, true)), "pipeline failure refused")?;
        let modules = with(|s| {
            s.fail = "";
            s.shaders.len()
        });
        check(
            !f.run(1, f.frame(1, true)) && with(|s| s.shaders.len()) == modules,
            "failed binding latched",
        )?;
        f.clear()?;
        check(f.run(1, f.frame(1, true)), "clear permits rebinding")?;
        f.clear()?;
    }
    {
        let mut f = Fixture::new();
        with(|s| s.fail = "vkCreateImage");
        check(
            !f.run(1, f.frame(1, true))
                && with(|s| {
                    ["pool", "buffer", "memory"]
                        .iter()
                        .all(|name| s.count(name) == 0)
                }),
            "allocation failure releases the partial slot",
        )?;
        with(|s| s.fail = "depth view");
        check(!f.run(2, f.frame(2, true)), "view failure refused")?;
        with(|s| s.fail = "");
        f.retire(2);
        check(
            f.run(3, f.frame(2, true)) && f.count() == 1,
            "reuse after refusal",
        )?;
        f.clear()?;
    }
    let mut f = Fixture::new();
    f.setup.instance_proc = None;
    check(
        !f.run(1, f.frame(1, true)) && with(|s| s.made.is_empty() && s.logs == [LOG]),
        "missing dispatch is refused",
    )?;
    f.clear()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_sharpening_vulkan_cpu_fixture() -> i32 {
    let result = admission()
        .and_then(|()| constants())
        .and_then(|()| exact_pass())
        .and_then(|()| bounded_slots())
        .and_then(|()| failures());
    if let Err(error) = result {
        eprintln!("sharpening_vulkan_cpu: {error}");
        return 1;
    }
    println!("sharpening_vulkan_cpu: PASS");
    0
}
