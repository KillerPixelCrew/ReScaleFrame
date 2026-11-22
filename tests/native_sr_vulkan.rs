// SPDX-License-Identifier: MIT
use super::{TestResult, check};
use crate::dxgi_format::half_to_f32;
use crate::sharpening_policy::defaults;
use crate::sys::{
    RSF_FGVK_ABI_VERSION, rsf_fgvk as Fg, rsf_fgvk_frame as FgFrame, rsf_fgvk_setup as FgSetup,
};
use crate::sys::{RSF_SLVK_ABI_VERSION, RSF_SLVK_DLSS_SR, rsf_slvk_setup};
use crate::sys::{
    RSF_SR_DLSS, RSF_SR_FSR2, RSF_SR_FSR3, RSF_SRVK_ABI_VERSION, rsf_srvk_fallback as Fallback,
    rsf_srvk_frame as Frame, rsf_srvk_image as Image, rsf_srvk_setup as Setup, rsf_vk_function,
    rsf_vk_get_proc,
};
use crate::vulkan_fg_copy::COLOR_RANGE as RANGE;
use ash::vk::{self, Handle as _};
use rsf_game_sys::{
    RSF_FRAME_FLAG_RESET, RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_CPU_FRAME_BEGIN,
    RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record, rsf_game_cpu_event,
};
use std::{
    ffi::{CStr, CString, c_char, c_void},
    ptr,
};
use vulkan_gpu::{Library, NO_LOADER};

#[path = "native_sr_fsr4_vulkan.rs"]
mod fsr4;
#[path = "vulkan_gpu.rs"]
mod vulkan_gpu;

type Handle = *mut c_void;

const W: usize = 320;
const H: usize = 180;
const PIXELS: usize = W * H * 4;
const READ: vk::ImageLayout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
const UNDEFINED: vk::ImageLayout = vk::ImageLayout::UNDEFINED;
const SRC: vk::ImageLayout = vk::ImageLayout::TRANSFER_SRC_OPTIMAL;
const DST: vk::ImageLayout = vk::ImageLayout::TRANSFER_DST_OPTIMAL;
const ALL: vk::PipelineStageFlags = vk::PipelineStageFlags::ALL_COMMANDS;
const TRANSFER: vk::PipelineStageFlags = vk::PipelineStageFlags::TRANSFER;
const HOST: vk::PipelineStageFlags = vk::PipelineStageFlags::HOST;
const BOTH: vk::ImageUsageFlags = vk::ImageUsageFlags::from_raw(
    vk::ImageUsageFlags::TRANSFER_SRC.as_raw() | vk::ImageUsageFlags::TRANSFER_DST.as_raw(),
);
const RGBA: vk::Format = vk::Format::R16G16B16A16_SFLOAT;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const NOT_COMPILED: i32 = -3;

#[repr(C)]
struct Calls {
    create: unsafe extern "C" fn(*const Setup, *mut Handle) -> i32,
    plan: unsafe extern "C" fn(*const c_void, *mut u32, *mut u32) -> i32,
    evaluate: unsafe extern "C" fn(Handle, Handle, *const Frame) -> i32,
    retire: unsafe extern "C" fn(Handle, Handle),
    destroy: unsafe extern "C" fn(Handle) -> i32,
    preset: unsafe extern "C" fn(Handle, u32) -> i32,
    fallback: unsafe extern "C" fn(*const Fallback) -> i32,
    fg_create: unsafe extern "C" fn(*const FgSetup, *mut *mut Fg, *mut u64) -> i32,
    fg_destroy: unsafe extern "C" fn(*mut Fg) -> i32,
    fg_drain: unsafe extern "C" fn(*mut Fg) -> i32,
    fg_prepare: unsafe extern "C" fn(*mut Fg, Handle, *const FgFrame) -> i32,
    sl_initialize: unsafe extern "C" fn(*const rsf_slvk_setup) -> i32,
    sl_lookup: rsf_vk_get_proc,
    sl_options: unsafe extern "C" fn(u32, u32) -> i32,
    sl_cpu: unsafe extern "C" fn(*const rsf_game_cpu_event),
    sl_shutdown: unsafe extern "C" fn() -> i32,
}

const _: () = assert!(size_of::<Calls>() == 16 * size_of::<usize>());

struct Gpu {
    session: Option<Session>,
    _library: Library,
    entry: ash::Entry,
    instance: ash::Instance,
    physical: vk::PhysicalDevice,
    device: Option<ash::Device>,
    memory: vk::PhysicalDeviceMemoryProperties,
    queue: vk::Queue,
    pool: vk::CommandPool,
    fence: vk::Fence,
    commands: vk::CommandBuffer,
    buffer: vk::Buffer,
    buffer_memory: vk::DeviceMemory,
    images: Vec<(vk::Image, vk::DeviceMemory)>,
}

impl Gpu {
    fn open(streamline: rsf_vk_get_proc, session: Option<Session>) -> TestResult<Self> {
        let mut gpu = Self::open_instance(streamline, session, c"ReScaleFrame Vulkan SR fixture")?;
        gpu.open_device(streamline.is_some())?;
        Ok(gpu)
    }

    fn open_instance(
        streamline: rsf_vk_get_proc,
        session: Option<Session>,
        application: &CStr,
    ) -> TestResult<Self> {
        let (library, entry) = vulkan_gpu::entry(streamline)?;
        let app = vk::ApplicationInfo::default()
            .application_name(application)
            .api_version(vk::API_VERSION_1_2);
        let extensions = [ash::khr::surface::NAME.as_ptr()];
        let mut info = vk::InstanceCreateInfo::default().application_info(&app);
        if streamline.is_some() {
            info = info.enabled_extension_names(&extensions);
        }
        let instance = unsafe { entry.create_instance(&info, None) }?;
        Ok(Self {
            session,
            _library: library,
            entry,
            instance,
            physical: vk::PhysicalDevice::null(),
            device: None,
            memory: vk::PhysicalDeviceMemoryProperties::default(),
            queue: vk::Queue::null(),
            pool: vk::CommandPool::null(),
            fence: vk::Fence::null(),
            commands: vk::CommandBuffer::null(),
            buffer: vk::Buffer::null(),
            buffer_memory: vk::DeviceMemory::null(),
            images: Vec::new(),
        })
    }

    fn open_device(&mut self, streamline: bool) -> TestResult {
        let swapchain = [ash::khr::swapchain::NAME.as_ptr()];
        let (physical, device, family) = vulkan_gpu::create_device(
            &self.instance,
            "Vulkan SR fixture",
            if streamline { &swapchain } else { &[] },
        )?;
        self.physical = physical;
        self.device = Some(device);
        let device = self.device.as_ref().ok_or("null Vulkan device")?;
        self.queue = unsafe { device.get_device_queue(family, 0) };
        self.memory = unsafe {
            self.instance
                .get_physical_device_memory_properties(physical)
        };
        let pool = vk::CommandPoolCreateInfo::default().queue_family_index(family);
        self.pool = unsafe { device.create_command_pool(&pool, None) }?;
        let commands = vk::CommandBufferAllocateInfo::default()
            .command_pool(self.pool)
            .command_buffer_count(1);
        self.commands = unsafe { device.allocate_command_buffers(&commands) }?[0];
        self.fence = unsafe { device.create_fence(&vk::FenceCreateInfo::default(), None) }?;
        let info = vk::BufferCreateInfo::default()
            .size((PIXELS * size_of::<u16>()) as u64)
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
        vulkan_gpu::allocate(self.device(), &self.memory, req, flags)
    }

    fn image(
        &mut self,
        extent: vk::Extent3D,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
    ) -> TestResult<Image> {
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(extent)
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
            width: extent.width,
            height: extent.height,
            layout: READ.as_raw() as u32,
            copy_source: u32::from(usage.contains(vk::ImageUsageFlags::TRANSFER_SRC)),
        })
    }

    fn start(&self) -> TestResult {
        let info = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        unsafe {
            self.device()
                .reset_command_pool(self.pool, vk::CommandPoolResetFlags::empty())?;
            self.device().begin_command_buffer(self.commands, &info)?;
        }
        Ok(())
    }

    fn finish(&self) -> TestResult {
        let commands = [self.commands];
        let submit = [vk::SubmitInfo::default().command_buffers(&commands)];
        unsafe {
            let device = self.device();
            device.end_command_buffer(self.commands)?;
            device.queue_submit(self.queue, &submit, self.fence)?;
            device.wait_for_fences(&[self.fence], true, 15_000_000_000)?;
            device.reset_fences(&[self.fence])?;
        }
        Ok(())
    }

    fn transition(&self, image: Image, from: vk::ImageLayout, to: vk::ImageLayout) {
        let access = vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE;
        let barriers = [vk::ImageMemoryBarrier::default()
            .old_layout(from)
            .new_layout(to)
            .src_access_mask(if from == UNDEFINED {
                vk::AccessFlags::empty()
            } else {
                access
            })
            .dst_access_mask(access)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(vk::Image::from_raw(image.image))
            .subresource_range(RANGE)];
        unsafe {
            self.device().cmd_pipeline_barrier(
                self.commands,
                ALL,
                ALL,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &barriers,
            )
        };
    }

    fn fill(&self, image: Image, value: [f32; 4]) {
        unsafe {
            self.device().cmd_clear_color_image(
                self.commands,
                vk::Image::from_raw(image.image),
                DST,
                &vk::ClearColorValue { float32: value },
                &[RANGE],
            );
        }
    }

    fn buffer_barrier(
        &self,
        source: vk::PipelineStageFlags,
        target: vk::PipelineStageFlags,
        src_access: vk::AccessFlags,
        dst_access: vk::AccessFlags,
    ) {
        let barriers = [vk::BufferMemoryBarrier::default()
            .src_access_mask(src_access)
            .dst_access_mask(dst_access)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .buffer(self.buffer)
            .size(vk::WHOLE_SIZE)];
        unsafe {
            self.device().cmd_pipeline_barrier(
                self.commands,
                source,
                target,
                vk::DependencyFlags::empty(),
                &[],
                &barriers,
                &[],
            )
        };
    }

    fn read(&self, image: Image) {
        self.transition(image, DST, SRC);
        unsafe {
            self.device().cmd_copy_image_to_buffer(
                self.commands,
                vk::Image::from_raw(image.image),
                SRC,
                self.buffer,
                &[copy_region(extent(W, H))],
            );
        }
        self.transition(image, SRC, DST);
        self.buffer_barrier(
            TRANSFER,
            HOST,
            vk::AccessFlags::TRANSFER_WRITE,
            vk::AccessFlags::HOST_READ,
        );
    }

    fn pixels<R>(&mut self, with_pixels: impl FnOnce(&mut [u16]) -> R) -> TestResult<R> {
        let pointer = unsafe {
            self.device().map_memory(
                self.buffer_memory,
                0,
                vk::WHOLE_SIZE,
                vk::MemoryMapFlags::empty(),
            )?
        };
        let result =
            with_pixels(unsafe { std::slice::from_raw_parts_mut(pointer.cast::<u16>(), PIXELS) });
        unsafe { self.device().unmap_memory(self.buffer_memory) };
        Ok(result)
    }

    fn setup(&self, backend: u32, directory: &CStr) -> Setup {
        Setup {
            struct_size: size_of::<Setup>() as u32,
            abi_version: RSF_SRVK_ABI_VERSION,
            instance: raw(self.instance.handle()),
            physical_device: raw(self.physical),
            device: raw(self.device().handle()),
            // The x64 system and C ABIs use the same Vulkan calling convention.
            instance_proc: Some(unsafe {
                std::mem::transmute::<
                    vk::PFN_vkGetInstanceProcAddr,
                    unsafe extern "C" fn(
                        *mut std::ffi::c_void,
                        *const i8,
                    )
                        -> std::option::Option<unsafe extern "C" fn()>,
                >(self.entry.static_fn().get_instance_proc_addr)
            }),
            device_proc: Some(unsafe {
                std::mem::transmute::<
                    vk::PFN_vkGetDeviceProcAddr,
                    unsafe extern "C" fn(
                        *mut std::ffi::c_void,
                        *const i8,
                    )
                        -> std::option::Option<unsafe extern "C" fn()>,
                >(self.instance.fp_v1_0().get_device_proc_addr)
            }),
            backend,
            quality: 1,
            output_width: W as u32,
            output_height: H as u32,
            depth_inverted: 1,
            runtime_directory_utf8: directory.as_ptr(),
            ..unsafe { std::mem::zeroed() }
        }
    }
}
impl Drop for Gpu {
    fn drop(&mut self) {
        // Streamline must release the latency session while the device is still valid.
        drop(self.session.take());
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

fn raw(handle: impl vk::Handle) -> Handle {
    handle.as_raw() as usize as Handle
}

fn extent(width: usize, height: usize) -> vk::Extent3D {
    vk::Extent3D {
        width: width as u32,
        height: height as u32,
        depth: 1,
    }
}

fn copy_region(extent: vk::Extent3D) -> vk::BufferImageCopy {
    vk::BufferImageCopy::default()
        .image_extent(extent)
        .image_subresource(
            vk::ImageSubresourceLayers::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .layer_count(1),
        )
}

fn laid_out(image: Image, layout: vk::ImageLayout) -> Image {
    Image {
        layout: layout.as_raw() as u32,
        ..image
    }
}

fn fallback_case(gpu: &mut Gpu, api: &Calls) -> TestResult {
    let color = gpu.image(
        extent(W / 2, H / 2),
        RGBA,
        BOTH | vk::ImageUsageFlags::SAMPLED,
    )?;
    let output = gpu.image(extent(W, H), RGBA, BOTH)?;
    let setup = gpu.setup(RSF_SR_FSR2, c"");
    let call = Fallback {
        struct_size: size_of::<Fallback>() as u32,
        abi_version: RSF_SRVK_ABI_VERSION,
        instance: setup.instance,
        physical_device: setup.physical_device,
        device: setup.device,
        instance_proc: setup.instance_proc,
        device_proc: setup.device_proc,
        command_buffer: raw(gpu.commands),
        color: laid_out(color, READ),
        output: laid_out(output, DST),
    };
    for (i, blue) in [0.2, 0.7].into_iter().enumerate() {
        gpu.pixels(|pixels| {
            for (index, pixel) in pixels.chunks_exact_mut(4).take(W * H / 4).enumerate() {
                let x = index % (W / 2);
                let y = index / (W / 2);
                let value = [x as f32 / 159., y as f32 / 89., blue, 1.];
                pixel.copy_from_slice(&value.map(super::sharpening_vulkan::half));
            }
        })?;
        gpu.start()?;
        gpu.transition(color, if i == 0 { UNDEFINED } else { READ }, DST);
        if i == 0 {
            gpu.transition(output, UNDEFINED, DST);
        }
        gpu.buffer_barrier(
            HOST,
            TRANSFER,
            vk::AccessFlags::HOST_WRITE,
            vk::AccessFlags::TRANSFER_READ,
        );
        unsafe {
            gpu.device().cmd_copy_buffer_to_image(
                gpu.commands,
                gpu.buffer,
                vk::Image::from_raw(color.image),
                DST,
                &[copy_region(extent(W / 2, H / 2))],
            );
        }
        gpu.transition(color, DST, READ);
        gpu.fill(output, [0.9, 0.1, 0.9, 0.]);
        let mut invalid = call;
        invalid.color.device = ptr::null_mut();
        check(
            unsafe { (api.fallback)(&invalid) } == INVALID,
            "fallback rejects foreign image",
        )?;
        invalid = call;
        invalid.color.copy_source = 0;
        check(
            unsafe { (api.fallback)(&invalid) } == INVALID,
            "fallback requires transfer-source usage",
        )?;
        invalid = call;
        invalid.output.image = invalid.color.image;
        check(
            unsafe { (api.fallback)(&invalid) } == INVALID,
            "fallback rejects image alias",
        )?;
        check(
            unsafe { (api.fallback)(&call) } == 0,
            "record spatial fallback",
        )?;
        // The upload reads the same buffer that the output copy writes.
        gpu.buffer_barrier(
            TRANSFER,
            TRANSFER,
            vk::AccessFlags::TRANSFER_READ,
            vk::AccessFlags::TRANSFER_WRITE,
        );
        gpu.read(output);
        gpu.finish()?;
        let valid = gpu.pixels(|pixels| {
            pixels.chunks_exact(4).enumerate().all(|(index, pixel)| {
                let x = ((index % W) as f32 + 0.5) * 0.5 - 0.5;
                let y = ((index / W) as f32 + 0.5) * 0.5 - 0.5;
                let expected = [x.clamp(0., 159.) / 159., y.clamp(0., 89.) / 89., blue, 1.];
                pixel
                    .iter()
                    .zip(expected)
                    .all(|(&bits, target)| (half_to_f32(bits) - target).abs() < 0.0025)
            })
        })?;
        check(
            valid,
            "spatial fallback must scale the gradient and write every output pixel",
        )?;
    }
    println!(
        "PASS: Vulkan spatial fallback gradient, borders, alpha, layout reuse and ownership guards"
    );
    Ok(())
}

struct Context<'a> {
    api: &'a Calls,
    device: ash::Device,
    value: Handle,
}
impl Drop for Context<'_> {
    fn drop(&mut self) {
        if !self.value.is_null() {
            unsafe {
                let _ = self.device.device_wait_idle();
                (self.api.destroy)(self.value);
            }
        }
    }
}

unsafe extern "C" fn log(user: Handle, message: *const c_char) {
    unsafe { *user.cast::<u32>() += 1 };
    println!(
        "optional pass fallback: {}",
        unsafe { CStr::from_ptr(message) }.to_string_lossy()
    );
}

fn device_case(gpu: &mut Gpu, api: &Calls, directory: &CStr, backend: u32) -> TestResult {
    let setup = gpu.setup(backend, directory);
    let mut owner = Context {
        api,
        device: gpu.device().clone(),
        value: ptr::null_mut(),
    };
    let result = unsafe { (api.create)(&setup, &mut owner.value) };
    println!("Vulkan SR backend {backend} create={result}");
    check(
        result == 0 && !owner.value.is_null(),
        "create native Vulkan SR context",
    )?;
    let context = owner.value;
    let (mut width, mut height) = (0, 0);
    check(
        unsafe { (api.plan)(context, &mut width, &mut height) } == 0 && width != 0 && height != 0,
        "SR sizing plan",
    )?;
    let render = extent(width as usize, height as usize);
    let input_usage = BOTH | vk::ImageUsageFlags::SAMPLED;
    let color = gpu.image(render, RGBA, input_usage)?;
    let depth = gpu.image(render, vk::Format::R32_SFLOAT, input_usage)?;
    let motion = gpu.image(render, vk::Format::R16G16_SFLOAT, input_usage)?;
    let output = gpu.image(extent(W, H), RGBA, BOTH)?;
    let bias = gpu.image(render, vk::Format::R8_UNORM, input_usage)?;
    let mut record = rsf_frame_record {
        struct_size: size_of::<rsf_frame_record>() as u32,
        abi_version: RSF_GAME_FRAME_ABI_VERSION,
        session_id: 43,
        view_id: 1,
        resource_generation: 1,
        frame_time_ms: 16.667,
        render_width: width,
        render_height: height,
        output_width: W as u32,
        output_height: H as u32,
        ..unsafe { std::mem::zeroed() }
    };
    let camera = &mut record.camera;
    camera.struct_size = std::mem::size_of_val(camera) as u32;
    camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    camera.render_width = width;
    camera.render_height = height;
    camera.output_width = W as u32;
    camera.output_height = H as u32;
    camera.depth_inverted = 1;
    camera.near_plane = 0.1;
    camera.far_plane = 100.;
    camera.vertical_fov_radians = 1.;
    for matrix in [
        &mut camera.view_to_clip,
        &mut camera.clip_to_view,
        &mut camera.view_to_world,
        &mut camera.world_to_view,
        &mut camera.clip_to_previous_clip,
    ] {
        for i in 0..4 {
            matrix[i * 4 + i] = 1.;
        }
    }
    let mut fallback_logs = 0u32;
    let mut frame = Frame {
        struct_size: size_of::<Frame>() as u32,
        abi_version: RSF_SRVK_ABI_VERSION,
        color: laid_out(color, READ),
        depth: laid_out(depth, READ),
        motion: laid_out(motion, READ),
        output: laid_out(output, DST),
        motion_scale_x: 1.,
        motion_scale_y: 1.,
        pre_exposure: 1.,
        log_user: ptr::from_mut(&mut fallback_logs).cast(),
        log: Some(log),
        ..unsafe { std::mem::zeroed() }
    };
    let commands = raw(gpu.commands);
    for index in 1..=12 {
        if backend == RSF_SR_DLSS {
            check(
                unsafe { (api.preset)(context, 99) } == INVALID,
                "unknown DLSS preset refused",
            )?;
            check(
                unsafe { (api.preset)(context, if index < 7 { 0 } else { 11 }) } == 0,
                "DLSS default/K model request",
            )?;
            let event = rsf_game_cpu_event {
                struct_size: size_of::<rsf_game_cpu_event>() as u32,
                session_id: record.session_id,
                source_frame_id: u64::from(index),
                stage: RSF_GAME_CPU_FRAME_BEGIN,
                ..unsafe { std::mem::zeroed() }
            };
            unsafe { (api.sl_cpu)(&event) };
        }
        let value = if index < 7 {
            [0.18, 0.05, 0.7, 1.]
        } else {
            [0.05, 0.4, 0.12, 1.]
        };
        frame.sharpening = defaults();
        frame.sharpening.mode = if index == 12 && (2..=4).contains(&backend) {
            4
        } else {
            (index - 1) % 4
        };
        frame.sharpening.sharpness = 0.7;
        frame.exposure = unsafe { std::mem::zeroed() };
        if index == 6 || index == 10 {
            frame.exposure.image = 1;
        }
        gpu.start()?;
        unsafe { (api.retire)(context, commands) };
        let previous = if index == 1 { UNDEFINED } else { READ };
        for input in [color, depth, motion, bias] {
            gpu.transition(input, previous, DST);
        }
        if index == 1 {
            gpu.transition(output, UNDEFINED, DST);
        }
        gpu.fill(color, value);
        gpu.fill(depth, [0.5, 0., 0., 0.]);
        gpu.fill(motion, [0.; 4]);
        gpu.fill(bias, [0.65, 0., 0., 0.]);
        for input in [color, depth, motion, bias] {
            gpu.transition(input, DST, READ);
        }
        frame.bias_current_color = if (2..12).contains(&index) {
            laid_out(bias, READ)
        } else {
            unsafe { std::mem::zeroed() }
        };
        frame.struct_size = if index == 1 {
            std::mem::offset_of!(Frame, bias_current_color)
        } else {
            size_of::<Frame>()
        } as u32;
        record.frame_id = u64::from(index);
        record.flags = if index == 1 || index == 7 {
            RSF_FRAME_FLAG_RESET
        } else {
            0
        };
        frame.record = &record;
        let evaluate = |frame: &Frame| unsafe { (api.evaluate)(context, commands, frame) };
        let device = frame.color.device;
        frame.color.device = ptr::null_mut();
        check(evaluate(&frame) == INVALID, "foreign device image refusal")?;
        frame.color.device = device;
        if index > 1 {
            let valid_bias = frame.bias_current_color;
            frame.bias_current_color = frame.color;
            check(evaluate(&frame) == INVALID, "wrong bias format refusal")?;
            frame.bias_current_color = laid_out(bias, READ);
            frame.bias_current_color.device = ptr::null_mut();
            check(evaluate(&frame) == INVALID, "foreign bias device refusal")?;
            frame.bias_current_color = valid_bias;
        }
        let result = evaluate(&frame);
        println!(
            "backend={backend} frame={index} dispatch={result} reset={} sharpening={}",
            record.flags, frame.sharpening.mode
        );
        check(result == 0, "Vulkan SR dispatch")?;
        gpu.read(output);
        gpu.finish()?;
        let valid = gpu.pixels(|pixels| {
            (16..H - 16).all(|y| {
                (16..W - 16).all(|x| {
                    pixels[(y * W + x) * 4..][..3]
                        .iter()
                        .zip(value)
                        .all(|(&bits, target)| (half_to_f32(bits) - target).abs() < 0.025)
                })
            })
        })?;
        check(valid, "GPU output disagrees with known linear scene colour")?;
    }
    check(
        fallback_logs == 1,
        "optional failure logs once while frames remain valid",
    )?;
    unsafe { gpu.device().device_wait_idle()? };
    check(unsafe { (api.destroy)(context) } == 0, "retire Vulkan SR")?;
    owner.value = ptr::null_mut();
    println!("PASS: Vulkan SR backend {backend} GPU pixels, reset and image-owner guards");
    Ok(())
}

unsafe extern "C" fn no_proc(_: Handle, _: *const c_char) -> rsf_vk_function {
    None
}

fn contract(api: &Calls, enabled: bool) -> TestResult {
    // These bindgen records contain only scalars, pointers and nullable callbacks.
    let mut fallback: Fallback = unsafe { std::mem::zeroed() };
    fallback.struct_size = size_of::<Fallback>() as u32 - 1;
    check(
        unsafe { (api.fallback)(ptr::null()) } == INVALID
            && unsafe { (api.fallback)(&fallback) } == INVALID,
        "fallback null/short ABI refusal",
    )?;
    fallback.struct_size += 1;
    fallback.abi_version = RSF_SRVK_ABI_VERSION + 1;
    check(
        unsafe { (api.fallback)(&fallback) } == ABI_MISMATCH,
        "fallback ABI mismatch refusal",
    )?;

    let mut fg = ptr::dangling_mut();
    let mut chain = 1;
    check(
        unsafe { (api.fg_create)(ptr::null(), &mut fg, &mut chain) } == INVALID
            && fg.is_null()
            && chain == 0,
        "FG null setup refusal",
    )?;
    let mut short: FgSetup = unsafe { std::mem::zeroed() };
    short.struct_size = size_of::<FgSetup>() as u32 - 1;
    let mut create_fg = |call: &FgSetup| unsafe { (api.fg_create)(call, &mut fg, &mut chain) };
    check(create_fg(&short) == INVALID, "FG short setup refusal")?;
    short.struct_size += 1;
    short.abi_version = RSF_FGVK_ABI_VERSION + 1;
    check(create_fg(&short) == ABI_MISMATCH, "FG ABI mismatch refusal")?;
    short.abi_version = RSF_FGVK_ABI_VERSION;
    short.graphics.struct_size = size_of::<Setup>() as u32 - 1;
    check(
        create_fg(&short) == INVALID,
        "FG embedded graphics size refusal",
    )?;
    short.graphics.struct_size += 1;
    short.graphics.abi_version = RSF_SRVK_ABI_VERSION + 1;
    check(
        create_fg(&short) == ABI_MISMATCH,
        "FG embedded graphics ABI refusal",
    )?;
    check(
        unsafe { (api.fg_destroy)(ptr::null_mut()) } == 0
            && unsafe { (api.fg_drain)(ptr::null_mut()) } == 0
            && unsafe { (api.fg_prepare)(ptr::null_mut(), ptr::null_mut(), ptr::null()) }
                == INVALID,
        "FG null lifecycle contract",
    )?;

    let mut context = ptr::dangling_mut();
    check(
        unsafe { (api.create)(ptr::null(), &mut context) } == INVALID && context.is_null(),
        "null setup refusal",
    )?;
    let mut short: Setup = unsafe { std::mem::zeroed() };
    short.struct_size = size_of::<Setup>() as u32 - 1;
    check(
        unsafe { (api.create)(&short, &mut context) } == INVALID && context.is_null(),
        "short setup refusal",
    )?;
    short.struct_size += 1;
    short.abi_version = RSF_SRVK_ABI_VERSION + 1;
    check(
        unsafe { (api.create)(&short, &mut context) } == ABI_MISMATCH,
        "ABI mismatch refusal",
    )?;
    check(
        unsafe { (api.plan)(ptr::null_mut(), ptr::null_mut(), ptr::null_mut()) } == INVALID
            && unsafe { (api.evaluate)(ptr::null_mut(), ptr::null_mut(), ptr::null()) } == INVALID
            && unsafe { (api.preset)(ptr::null_mut(), 0) } == INVALID
            && unsafe { (api.destroy)(ptr::null_mut()) } == 0,
        "null lifecycle contract",
    )?;
    if enabled {
        capture::run()?;
    } else {
        let mut object = 0u32;
        let object = ptr::from_mut(&mut object).cast();
        let call = Setup {
            struct_size: size_of::<Setup>() as u32,
            abi_version: RSF_SRVK_ABI_VERSION,
            instance: object,
            physical_device: object,
            device: object,
            instance_proc: Some(no_proc),
            device_proc: Some(no_proc),
            backend: RSF_SR_FSR3,
            quality: 1,
            output_width: 128,
            output_height: 72,
            depth_inverted: 1,
            depth_infinite: 1,
            motion_jittered: 0,
            runtime_directory_utf8: c"unused".as_ptr(),
        };
        check(
            unsafe { (api.create)(&call, &mut context) } == NOT_COMPILED && context.is_null(),
            "feature-disabled provider refusal",
        )?;
    }
    println!("PASS: native Vulkan SR C ABI guards");
    Ok(())
}

struct Session(unsafe extern "C" fn() -> i32);
impl Drop for Session {
    fn drop(&mut self) {
        unsafe { (self.0)() };
    }
}

fn device(api: &Calls, directory: &CStr, streamline: Option<&CStr>) -> TestResult {
    let session = if let Some(streamline) = streamline {
        let setup = rsf_slvk_setup {
            struct_size: size_of::<rsf_slvk_setup>() as u32,
            abi_version: RSF_SLVK_ABI_VERSION,
            runtime_directory_utf8: streamline.as_ptr(),
            project_id: c"D9435FE3-13D4-4D81-B880-CCB98271F876".as_ptr(),
            features: RSF_SLVK_DLSS_SR,
        };
        check(
            unsafe { (api.sl_initialize)(&setup) } == 0,
            "initialize shared Vulkan DLSS/PCL session",
        )?;
        Some(Session(api.sl_shutdown))
    } else {
        None
    };
    let mut gpu = Gpu::open(streamline.and(api.sl_lookup), session)?;
    if streamline.is_some() {
        check(
            unsafe { (api.sl_options)(0, 0) } == 0,
            "bind shared PCL on Vulkan device",
        )
        .and_then(|()| device_case(&mut gpu, api, directory, RSF_SR_DLSS))
    } else {
        fallback_case(&mut gpu, api)
            .and_then(|()| device_case(&mut gpu, api, directory, RSF_SR_FSR2))
            .and_then(|()| device_case(&mut gpu, api, directory, RSF_SR_FSR3))
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_native_sr_vulkan_fixture(
    api: &Calls,
    enabled: i32,
    argc: i32,
    argument: *const c_char,
) -> i32 {
    let result = if argc == 1 {
        contract(api, enabled != 0)
    } else {
        let Ok(directory) = std::env::var("RSF_VULKAN_SR_RUNTIME") else {
            println!("SKIP: set RSF_VULKAN_SR_RUNTIME for the Vulkan device fixture");
            return 77;
        };
        if enabled == 0 {
            println!("SKIP: built without Vulkan SDK headers");
            return 77;
        }
        let dlss = !argument.is_null() && unsafe { CStr::from_ptr(argument) } == c"--dlss";
        let streamline = if dlss {
            let Ok(directory) = std::env::var("RSF_VULKAN_SL_RUNTIME") else {
                println!("SKIP: set RSF_VULKAN_SL_RUNTIME for the shared DLSS SDK session");
                return 77;
            };
            Some(directory)
        } else {
            None
        };
        let run = || -> TestResult {
            let directory = CString::new(directory)?;
            let streamline = streamline.map(CString::new).transpose()?;
            device(api, &directory, streamline.as_deref())
        };
        run()
    };
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("Vulkan SR fixture FAILED: {error}");
            1
        }
    }
}

#[path = "native_sr_vulkan_capture.rs"]
mod capture;
