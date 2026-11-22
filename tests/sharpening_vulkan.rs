// SPDX-License-Identifier: MIT
use super::{TestResult, check};
use crate::dxgi_format::half_to_f32;
use crate::sharpening_policy::defaults;
use crate::sys::rsf_sharpening_options as Options;
use crate::sys::rsf_sharpenvk_host as Host;
use crate::sys::{rsf_srvk_frame as Frame, rsf_srvk_image as Image, rsf_srvk_setup as Setup};
use crate::vulkan_sharpening::{
    rsf_sharpenvk_host_destroy, rsf_sharpenvk_host_dispatches, rsf_sharpenvk_host_retire,
    rsf_sharpenvk_host_run,
};
use ash::vk::{self, Handle};
use rsf_game_sys::rsf_frame_record;
use std::ffi::{CStr, c_void};
use std::ptr;
use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};

const W: usize = 33;
const H: usize = 17;
const COUNT: usize = W * H;
type Pixels = [u16; COUNT * 4];
const BYTES: usize = size_of::<Pixels>();
const EXTENT: vk::Extent3D = vk::Extent3D {
    width: W as u32,
    height: H as u32,
    depth: 1,
};
const GENERAL: vk::ImageLayout = vk::ImageLayout::GENERAL;
const READ: vk::ImageLayout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
const UNDEFINED: vk::ImageLayout = vk::ImageLayout::UNDEFINED;
const SRC: vk::ImageLayout = vk::ImageLayout::TRANSFER_SRC_OPTIMAL;
const DST: vk::ImageLayout = vk::ImageLayout::TRANSFER_DST_OPTIMAL;
const NO_LOADER: &str = "Vulkan loader unavailable";
const NO_DEVICE: &str = "enumerate GPUs";

struct Library(HMODULE);
impl Drop for Library {
    fn drop(&mut self) {
        unsafe { FreeLibrary(self.0) };
    }
}

fn entry() -> TestResult<(Library, ash::Entry)> {
    let path = crate::settings_store::wide("vulkan-1.dll");
    let module =
        unsafe { LoadLibraryExW(path.as_ptr(), ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
    if module.is_null() {
        return Err(NO_LOADER.into());
    }
    let library = Library(module);
    let proc = unsafe { GetProcAddress(module, c"vkGetInstanceProcAddr".as_ptr().cast()) }
        .ok_or("missing vkGetInstanceProcAddr")?;
    let get_instance_proc_addr = unsafe {
        std::mem::transmute::<
            unsafe extern "system" fn() -> isize,
            unsafe extern "system" fn(
                ash::vk::Instance,
                *const i8,
            )
                -> std::option::Option<unsafe extern "system" fn()>,
        >(proc)
    };
    let entry = unsafe {
        ash::Entry::from_static_fn(ash::StaticFn {
            get_instance_proc_addr,
        })
    };
    Ok((library, entry))
}

const RANGE: vk::ImageSubresourceRange = vk::ImageSubresourceRange {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    base_mip_level: 0,
    level_count: 1,
    base_array_layer: 0,
    layer_count: 1,
};
const LAYERS: vk::ImageSubresourceLayers = vk::ImageSubresourceLayers {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    mip_level: 0,
    base_array_layer: 0,
    layer_count: 1,
};

struct Gpu {
    _library: Library,
    entry: ash::Entry,
    instance: ash::Instance,
    physical: vk::PhysicalDevice,
    device: Option<ash::Device>,
    memory: vk::PhysicalDeviceMemoryProperties,
    queue: vk::Queue,
    pool: vk::CommandPool,
    fence: vk::Fence,
    buffer: vk::Buffer,
    buffer_memory: vk::DeviceMemory,
    images: Vec<(vk::Image, vk::DeviceMemory)>,
}

impl Gpu {
    fn open() -> TestResult<Self> {
        let (library, entry) = entry()?;
        let app = vk::ApplicationInfo::default()
            .application_name(c"ReScaleFrame Vulkan SR fixture")
            .api_version(vk::API_VERSION_1_2);
        let info = vk::InstanceCreateInfo::default().application_info(&app);
        let instance = unsafe { entry.create_instance(&info, None) }?;
        let mut gpu = Self {
            _library: library,
            entry,
            instance,
            physical: vk::PhysicalDevice::null(),
            device: None,
            memory: vk::PhysicalDeviceMemoryProperties::default(),
            queue: vk::Queue::null(),
            pool: vk::CommandPool::null(),
            fence: vk::Fence::null(),
            buffer: vk::Buffer::null(),
            buffer_memory: vk::DeviceMemory::null(),
            images: Vec::new(),
        };
        gpu.open_device()?;
        Ok(gpu)
    }

    fn open_device(&mut self) -> TestResult {
        let devices =
            unsafe { self.instance.enumerate_physical_devices() }.map_err(|_| NO_DEVICE)?;
        let first = *devices.first().ok_or(NO_DEVICE)?;
        let physical = devices
            .into_iter()
            .find(|&candidate| {
                unsafe { self.instance.get_physical_device_properties(candidate) }.device_type
                    == vk::PhysicalDeviceType::DISCRETE_GPU
            })
            .unwrap_or(first);
        self.physical = physical;
        let properties = unsafe { self.instance.get_physical_device_properties(physical) };
        let name = unsafe { CStr::from_ptr(properties.device_name.as_ptr()) };
        println!("Vulkan sharpening fixture GPU: {}", name.to_string_lossy());
        let flags = vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE;
        let families = unsafe {
            self.instance
                .get_physical_device_queue_family_properties(physical)
        };
        let family = families
            .iter()
            .position(|entry| entry.queue_flags.contains(flags))
            .ok_or("graphics and compute queue family")? as u32;
        let mut v12 = vk::PhysicalDeviceVulkan12Features::default();
        let mut v11 = vk::PhysicalDeviceVulkan11Features::default();
        let mut features = vk::PhysicalDeviceFeatures2::default()
            .push_next(&mut v12)
            .push_next(&mut v11);
        unsafe {
            self.instance
                .get_physical_device_features2(physical, &mut features)
        };
        let queues = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(family)
            .queue_priorities(&[1.0])];
        let info = vk::DeviceCreateInfo::default()
            .push_next(&mut features)
            .queue_create_infos(&queues);
        self.device = Some(unsafe { self.instance.create_device(physical, &info, None) }?);
        let device = self.device.as_ref().ok_or("null Vulkan device")?;
        self.queue = unsafe { device.get_device_queue(family, 0) };
        self.memory = unsafe {
            self.instance
                .get_physical_device_memory_properties(physical)
        };
        let pool = vk::CommandPoolCreateInfo::default().queue_family_index(family);
        self.pool = unsafe { device.create_command_pool(&pool, None) }?;
        self.fence = unsafe { device.create_fence(&vk::FenceCreateInfo::default(), None) }?;
        let info = vk::BufferCreateInfo::default()
            .size((BYTES * 3) as u64)
            .usage(vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::TRANSFER_DST);
        self.buffer = unsafe { device.create_buffer(&info, None) }?;
        let requirements = unsafe { device.get_buffer_memory_requirements(self.buffer) };
        self.buffer_memory = self.allocate(
            requirements,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;
        unsafe { device.bind_buffer_memory(self.buffer, self.buffer_memory, 0) }?;
        Ok(())
    }

    fn device(&self) -> &ash::Device {
        self.device.as_ref().expect("open Vulkan device")
    }

    fn allocate(
        &self,
        req: vk::MemoryRequirements,
        flags: vk::MemoryPropertyFlags,
    ) -> TestResult<vk::DeviceMemory> {
        let memory = self
            .memory
            .memory_types
            .iter()
            .take(self.memory.memory_type_count as usize)
            .enumerate()
            .position(|(i, ty)| {
                req.memory_type_bits & (1 << i) != 0 && ty.property_flags.contains(flags)
            })
            .ok_or("Vulkan memory type")? as u32;
        let info = vk::MemoryAllocateInfo::default()
            .allocation_size(req.size)
            .memory_type_index(memory);
        Ok(unsafe { self.device().allocate_memory(&info, None) }?)
    }

    fn image(&mut self, format: vk::Format, extra: vk::ImageUsageFlags) -> TestResult<Image> {
        let usage = vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST | extra;
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(EXTENT)
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .usage(usage);
        let image = unsafe { self.device().create_image(&info, None) }?;
        self.images.push((image, vk::DeviceMemory::null()));
        let req = unsafe { self.device().get_image_memory_requirements(image) };
        let memory = self.allocate(req, vk::MemoryPropertyFlags::DEVICE_LOCAL)?;
        self.images
            .last_mut()
            .ok_or("missing Vulkan image allocation")?
            .1 = memory;
        unsafe { self.device().bind_image_memory(image, memory, 0) }?;
        Ok(Image {
            device: self.device().handle().as_raw() as *mut c_void,
            image: image.as_raw(),
            format: format.as_raw() as u32,
            width: EXTENT.width,
            height: EXTENT.height,
            layout: READ.as_raw() as u32,
            copy_source: u32::from(usage.contains(vk::ImageUsageFlags::TRANSFER_SRC)),
        })
    }

    fn commands(&self, count: u32) -> TestResult<Vec<vk::CommandBuffer>> {
        let info = vk::CommandBufferAllocateInfo::default()
            .command_pool(self.pool)
            .command_buffer_count(count);
        Ok(unsafe { self.device().allocate_command_buffers(&info) }?)
    }

    fn reset(&self) -> TestResult {
        unsafe {
            self.device()
                .reset_command_pool(self.pool, vk::CommandPoolResetFlags::empty())
        }?;
        Ok(())
    }

    fn begin(&self, commands: vk::CommandBuffer) -> TestResult {
        let info = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        unsafe { self.device().begin_command_buffer(commands, &info) }?;
        Ok(())
    }

    fn submit(&self, commands: &[vk::CommandBuffer]) -> TestResult {
        let info = vk::SubmitInfo::default().command_buffers(commands);
        unsafe {
            self.device()
                .queue_submit(self.queue, &[info], self.fence)?;
            self.device()
                .wait_for_fences(&[self.fence], true, 15_000_000_000)?;
            self.device().reset_fences(&[self.fence])?;
        }
        Ok(())
    }

    fn finish(&self, commands: vk::CommandBuffer) -> TestResult {
        unsafe { self.device().end_command_buffer(commands) }?;
        self.submit(&[commands])
    }

    fn map(&self, use_pointer: impl FnOnce(*mut u8)) -> TestResult {
        let p = unsafe {
            self.device().map_memory(
                self.buffer_memory,
                0,
                vk::WHOLE_SIZE,
                vk::MemoryMapFlags::empty(),
            )
        }?;
        if !p.is_null() {
            use_pointer(p.cast());
        }
        unsafe { self.device().unmap_memory(self.buffer_memory) };
        check(!p.is_null(), "Vulkan mapping is null")
    }

    fn transition(
        &self,
        cmd: vk::CommandBuffer,
        image: Image,
        from: vk::ImageLayout,
        to: vk::ImageLayout,
    ) {
        let memory = vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE;
        let barrier = vk::ImageMemoryBarrier::default()
            .old_layout(from)
            .new_layout(to)
            .src_access_mask(if from == UNDEFINED {
                vk::AccessFlags::empty()
            } else {
                memory
            })
            .dst_access_mask(memory)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(vk::Image::from_raw(image.image))
            .subresource_range(RANGE);
        let stages = vk::PipelineStageFlags::ALL_COMMANDS;
        unsafe {
            self.device().cmd_pipeline_barrier(
                cmd,
                stages,
                stages,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            )
        };
    }

    fn upload(&self, cmd: vk::CommandBuffer, image: Image, offset: u64) {
        let region = vk::BufferImageCopy::default()
            .buffer_offset(offset)
            .image_subresource(LAYERS)
            .image_extent(EXTENT);
        unsafe {
            self.device().cmd_copy_buffer_to_image(
                cmd,
                self.buffer,
                vk::Image::from_raw(image.image),
                DST,
                &[region],
            )
        };
    }

    fn fill(&self, cmd: vk::CommandBuffer, image: Image, value: [f32; 4]) {
        let clear = vk::ClearColorValue { float32: value };
        unsafe {
            self.device().cmd_clear_color_image(
                cmd,
                vk::Image::from_raw(image.image),
                DST,
                &clear,
                &[RANGE],
            )
        };
    }

    fn readback(&self, cmd: vk::CommandBuffer, image: Image, offset: u64) {
        let barrier = vk::BufferMemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::TRANSFER_READ)
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .buffer(self.buffer)
            .offset(offset)
            .size(BYTES as u64);
        let stage = vk::PipelineStageFlags::TRANSFER;
        let region = vk::BufferImageCopy::default()
            .buffer_offset(offset)
            .image_subresource(LAYERS)
            .image_extent(EXTENT);
        unsafe {
            self.device().cmd_pipeline_barrier(
                cmd,
                stage,
                stage,
                vk::DependencyFlags::empty(),
                &[],
                &[barrier],
                &[],
            );
            self.device().cmd_copy_image_to_buffer(
                cmd,
                vk::Image::from_raw(image.image),
                SRC,
                self.buffer,
                &[region],
            );
        }
    }
}

impl Drop for Gpu {
    fn drop(&mut self) {
        if let Some(device) = &self.device {
            unsafe {
                let _ = device.device_wait_idle();
                for &(image, memory) in &self.images {
                    device.destroy_image(image, None);
                    device.free_memory(memory, None);
                }
                device.destroy_buffer(self.buffer, None);
                device.free_memory(self.buffer_memory, None);
                device.destroy_fence(self.fence, None);
                device.destroy_command_pool(self.pool, None);
                device.destroy_device(None);
            }
        }
        unsafe { self.instance.destroy_instance(None) };
    }
}

struct Fixture {
    gpu: Gpu,
    host: Host,
    setup: Setup,
    record: Box<rsf_frame_record>,
    commands: vk::CommandBuffer,
    output: Image,
    depth: Image,
    motion: Image,
    initialized: bool,
}

impl Fixture {
    fn open() -> TestResult<Self> {
        let mut gpu = Gpu::open()?;
        let output = gpu.image(
            vk::Format::R16G16B16A16_SFLOAT,
            vk::ImageUsageFlags::TRANSFER_SRC,
        )?;
        let depth = gpu.image(vk::Format::R32_SFLOAT, vk::ImageUsageFlags::empty())?;
        let motion = gpu.image(vk::Format::R16G16_SFLOAT, vk::ImageUsageFlags::empty())?;
        let mut setup: Setup = unsafe { std::mem::zeroed() };
        setup.struct_size = size_of::<Setup>() as u32;
        setup.abi_version = 2;
        setup.instance = gpu.instance.handle().as_raw() as *mut c_void;
        setup.physical_device = gpu.physical.as_raw() as *mut c_void;
        setup.device = gpu.device().handle().as_raw() as *mut c_void;
        // The private C proc signature transports the Vulkan proc address on Windows x64.
        setup.instance_proc = unsafe {
            std::mem::transmute::<
                unsafe extern "system" fn(
                    ash::vk::Instance,
                    *const i8,
                )
                    -> std::option::Option<unsafe extern "system" fn()>,
                std::option::Option<
                    unsafe extern "C" fn(
                        *mut std::ffi::c_void,
                        *const i8,
                    )
                        -> std::option::Option<unsafe extern "C" fn()>,
                >,
            >(gpu.entry.static_fn().get_instance_proc_addr)
        };
        setup.device_proc = unsafe {
            std::mem::transmute::<
                unsafe extern "system" fn(
                    ash::vk::Device,
                    *const i8,
                )
                    -> std::option::Option<unsafe extern "system" fn()>,
                std::option::Option<
                    unsafe extern "C" fn(
                        *mut std::ffi::c_void,
                        *const i8,
                    )
                        -> std::option::Option<unsafe extern "C" fn()>,
                >,
            >(gpu.instance.fp_v1_0().get_device_proc_addr)
        };
        let mut record: Box<rsf_frame_record> = Box::new(unsafe { std::mem::zeroed() });
        record.output_width = W as u32;
        record.output_height = H as u32;
        record.camera.depth_inverted = 1;
        let commands = gpu.commands(1)?[0];
        Ok(Self {
            gpu,
            host: Host::default(),
            setup,
            record,
            commands,
            output,
            depth,
            motion,
            initialized: false,
        })
    }

    fn record_pass(&mut self, cmd: vk::CommandBuffer, image: Image, frame: &Frame) -> TestResult {
        check(
            unsafe {
                rsf_sharpenvk_host_run(
                    &mut self.host,
                    &self.setup,
                    cmd.as_raw() as *mut c_void,
                    frame,
                    image.image,
                )
            } != 0,
            "Vulkan sharpening recording",
        )
    }

    fn frame(&self, options: Options, inputs: bool, depth: bool) -> Frame {
        let mut frame: Frame = unsafe { std::mem::zeroed() };
        frame.record = &*self.record;
        frame.pre_exposure = 1.0;
        frame.sharpening = options;
        if inputs {
            frame.motion = self.motion;
            frame.motion_scale_x = 1.0;
            frame.motion_scale_y = 1.0;
        }
        if depth {
            frame.depth = self.depth;
        }
        frame
    }

    fn run(
        &mut self,
        source: &Pixels,
        options: Options,
        depth: bool,
        velocity: f32,
        depth_step: bool,
    ) -> TestResult<Pixels> {
        let c = self.commands;
        self.gpu.map(|mapped| unsafe {
            ptr::copy_nonoverlapping(source.as_ptr(), mapped.cast(), source.len())
        })?;
        self.gpu.reset()?;
        unsafe { rsf_sharpenvk_host_retire(&mut self.host, c.as_raw() as *mut c_void) };
        self.gpu.begin(c)?;
        let previous = if self.initialized { GENERAL } else { UNDEFINED };
        self.gpu.transition(c, self.output, previous, DST);
        self.gpu.upload(c, self.output, 0);
        self.gpu.transition(c, self.output, DST, GENERAL);
        for image in [self.depth, self.motion] {
            self.gpu.transition(
                c,
                image,
                if self.initialized { READ } else { UNDEFINED },
                DST,
            );
        }
        self.gpu.fill(c, self.depth, [0.5; 4]);
        self.gpu.fill(c, self.motion, [velocity, 0.0, 0.0, 0.0]);
        if depth_step {
            self.gpu.finish(c)?;
            self.gpu.map(|mapped| {
                let mapped = mapped.cast::<f32>();
                for i in 0..COUNT {
                    unsafe { mapped.add(i).write(if i % W < W / 2 { 0.1 } else { 0.9 }) };
                }
            })?;
            self.gpu.reset()?;
            self.gpu.begin(c)?;
            self.gpu.upload(c, self.depth, 0);
        }
        for image in [self.depth, self.motion] {
            self.gpu.transition(c, image, DST, READ);
        }
        self.record_pass(c, self.output, &self.frame(options, true, depth))?;
        self.gpu.transition(c, self.output, GENERAL, SRC);
        self.gpu.readback(c, self.output, 0);
        self.gpu.transition(c, self.output, SRC, GENERAL);
        self.gpu.finish(c)?;
        self.initialized = true;
        let mut pixels = [0; COUNT * 4];
        self.gpu.map(|mapped| unsafe {
            ptr::copy_nonoverlapping(mapped.cast::<u16>(), pixels.as_mut_ptr(), pixels.len())
        })?;
        Ok(pixels)
    }

    fn dispatches(&self) -> u64 {
        unsafe { rsf_sharpenvk_host_dispatches(&self.host) }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe {
            let _ = self.gpu.device().device_wait_idle();
            rsf_sharpenvk_host_destroy(&mut self.host);
        }
    }
}

// Pattern components are zero or positive normal binary16 values in range.
pub(super) fn half(value: f32) -> u16 {
    if value == 0.0 {
        return 0;
    }
    let bits = value.to_bits();
    let rounded = bits + 0xfff + ((bits >> 13) & 1);
    ((rounded - (112 << 23)) >> 13) as u16
}

fn pattern(flat: bool) -> Pixels {
    std::array::from_fn(|index| {
        let x = (index / 4 % W) as f32;
        let value = if index % 4 == 3 {
            x / W as f32
        } else if flat {
            10.0
        } else if x < (W / 2) as f32 {
            10.0 + 0.12 * x * x
        } else {
            let distance = x - (W / 2) as f32;
            80.0 + 0.015 * distance * distance
        };
        half(value)
    })
}

fn delta(a: &Pixels, b: &Pixels) -> f64 {
    a.iter()
        .zip(b)
        .enumerate()
        .filter(|(i, _)| i % 4 != 3)
        .map(|(_, (&a, &b))| f64::from((half_to_f32(a) - half_to_f32(b)).abs()))
        .sum()
}

fn in_flight() -> TestResult {
    let mut f = Fixture::open()?;
    let pixels = pattern(false);
    let mut images = Vec::new();
    for _ in 0..3 {
        images.push(f.gpu.image(
            vk::Format::R16G16B16A16_SFLOAT,
            vk::ImageUsageFlags::TRANSFER_SRC,
        )?);
    }
    f.gpu.map(|mapped| {
        for i in 0..3 {
            unsafe {
                ptr::copy_nonoverlapping(
                    pixels.as_ptr(),
                    mapped.add(i * BYTES).cast(),
                    pixels.len(),
                )
            };
        }
    })?;
    let commands = f.gpu.commands(3)?;
    for (i, (&cmd, image)) in commands.iter().zip(images).enumerate() {
        let offset = (i * BYTES) as u64;
        f.gpu.begin(cmd)?;
        f.gpu.transition(cmd, image, UNDEFINED, DST);
        f.gpu.upload(cmd, image, offset);
        f.gpu.transition(cmd, image, DST, GENERAL);
        let options = Options {
            mode: 1,
            sharpness: 0.2 + 0.4 * i as f32,
            ..defaults()
        };
        f.record_pass(cmd, image, &f.frame(options, false, false))?;
        f.gpu.transition(cmd, image, GENERAL, SRC);
        f.gpu.readback(cmd, image, offset);
        unsafe { f.gpu.device().end_command_buffer(cmd) }?;
    }
    f.gpu.submit(&commands)?;
    let mut deltas = [0.0; 3];
    f.gpu.map(|mapped| {
        for (i, value) in deltas.iter_mut().enumerate() {
            let actual = unsafe { &*mapped.add(i * BYTES).cast::<Pixels>() };
            *value = delta(&pixels, actual);
        }
    })?;
    f.gpu.reset()?;
    for cmd in commands {
        unsafe { rsf_sharpenvk_host_retire(&mut f.host, cmd.as_raw() as *mut c_void) };
    }
    check(
        deltas[0] > 0.0 && deltas[1] > deltas[0] && deltas[2] > deltas[1],
        "in-flight constants cannot be overwritten by later recordings",
    )
}

fn verify() -> TestResult {
    let mut disabled = Host::default();
    let setup: Setup = unsafe { std::mem::zeroed() };
    let frame: Frame = unsafe { std::mem::zeroed() };
    let ok = unsafe { rsf_sharpenvk_host_run(&mut disabled, &setup, ptr::null_mut(), &frame, 0) };
    let count = unsafe { rsf_sharpenvk_host_dispatches(&disabled) };
    unsafe { rsf_sharpenvk_host_destroy(&mut disabled) };
    check(ok != 0 && count == 0, "Off needs no Vulkan device")?;
    in_flight()?;
    let mut f = Fixture::open()?;
    let source = pattern(false);
    let flat = pattern(true);
    for (mode, strength) in [(0, 0.4), (4, 0.4), (1, 0.0)] {
        check(
            f.run(
                &source,
                Options {
                    mode,
                    sharpness: strength,
                    ..defaults()
                },
                true,
                0.0,
                false,
            )? == source
                && f.dispatches() == 0,
            "inactive filter keeps output and dispatch count",
        )?;
    }
    for mode in [1, 2, 3] {
        let options = Options {
            mode,
            sharpness: 1.0,
            ..defaults()
        };
        check(
            f.run(&flat, options, true, 0.0, false)? == flat,
            "flat HDR byte identity and alpha",
        )?;
        let actual = f.run(&source, options, true, 0.0, false)?;
        check(delta(&source, &actual) > 0.0, "edge sharpens")?;
        for i in 0..COUNT {
            check(actual[i * 4 + 3] == source[i * 4 + 3], "alpha survives")?;
            let mut low = f32::INFINITY;
            let mut high = 0.0f32;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let x = ((i % W) as i32 + dx).clamp(0, W as i32 - 1) as usize;
                    let y = ((i / W) as i32 + dy).clamp(0, H as i32 - 1) as usize;
                    let value = half_to_f32(source[(y * W + x) * 4]);
                    low = low.min(value);
                    high = high.max(value);
                }
            }
            let value = half_to_f32(actual[i * 4]);
            check(
                value.is_finite() && (low - 0.07..=high + 0.07).contains(&value),
                "HDR local bounds",
            )?;
        }
        let fallback = f.run(&source, options, false, 0.0, false)?;
        check(
            fallback == f.run(&source, Options { mode: 1, ..options }, false, 0.0, false)?,
            "missing depth uses RCAS",
        )?;
        let mut deltas = [0.0; 3];
        for (delta_value, sharpness) in deltas.iter_mut().zip([0.2, 0.6, 1.0]) {
            let actual = f.run(
                &source,
                Options {
                    sharpness,
                    ..options
                },
                true,
                0.0,
                false,
            )?;
            *delta_value = delta(&source, &actual);
        }
        check(
            deltas[2] > 0.0 && deltas[2] >= deltas[1] && deltas[1] >= deltas[0],
            "monotonic sharpness",
        )?;
    }
    for mode in [2, 3] {
        let clipped = f.run(
            &flat,
            Options {
                mode,
                clamp_output: 1,
                ..defaults()
            },
            true,
            0.0,
            false,
        )?;
        check(
            clipped
                .iter()
                .step_by(4)
                .all(|&value| half_to_f32(value) == 1.0),
            "explicit clamp limits decoded HDR",
        )?;
    }
    let adaptive = Options {
        mode: 1,
        sharpness: 0.2,
        mas: 1,
        motion_threshold: 2.0,
        motion_scale_limit: 10.0,
        motion_sharpness: 0.6,
        ..defaults()
    };
    let still = f.run(&source, adaptive, true, 0.0, false)?;
    let below = f.run(&source, adaptive, true, 1.0, false)?;
    let moving = f.run(&source, adaptive, true, 10.0, false)?;
    check(
        still == below && delta(&source, &moving) > delta(&source, &still),
        "MAS threshold and ramp",
    )?;
    for mode in [2, 3] {
        let options = Options {
            mode,
            sharpness: 1.0,
            ..defaults()
        };
        let normal = f.run(&source, options, true, 0.0, false)?;
        let step = f.run(&source, options, true, 0.0, true)?;
        let edge_delta = |image: &Pixels| {
            (0..H)
                .flat_map(|y| [W / 2 - 1, W / 2].map(move |x| (y * W + x) * 4))
                .map(|pixel| (half_to_f32(image[pixel]) - half_to_f32(source[pixel])).abs())
                .sum::<f32>()
        };
        println!(
            "depth mode {mode} boundary deltas {:.6} {:.6}",
            edge_delta(&normal),
            edge_delta(&step)
        );
        check(
            edge_delta(&step) == 0.0,
            "depth edge rejects cross-depth sharpening",
        )?;
    }
    println!(
        "sharpening_vulkan: PASS, {} dispatches, explicit command buffer retirement",
        f.dispatches()
    );
    Ok(())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_sharpening_vulkan_fixture() -> i32 {
    match verify() {
        Ok(()) => 0,
        Err(error) if [NO_DEVICE, NO_LOADER].contains(&error.to_string().as_str()) => {
            println!("sharpening_vulkan skipped: {error}");
            77
        }
        Err(error) => {
            eprintln!("sharpening_vulkan: {error}");
            1
        }
    }
}
