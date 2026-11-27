// SPDX-License-Identifier: MIT
use crate::sys::{rsf_srvk_image, rsf_srvk_setup, rsf_vk_function};
use crate::xess_sr::{
    Failure, require, rsf_xessvk_device_requirements, rsf_xessvk_instance_requirements,
};
use ash::{
    Entry, StaticFn,
    vk::{self, Handle as _},
};
use rsf_test_support::Library;
use std::{
    ffi::{CStr, c_char, c_void},
    mem, ptr,
    slice::from_ref,
    sync::OnceLock,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

const WIDTH: u32 = 320;
const HEIGHT: u32 = 180;
const CHANNELS: usize = 4;
const COLOR: vk::Format = vk::Format::R16G16B16A16_SFLOAT;
const TRANSFER: vk::ImageLayout = vk::ImageLayout::TRANSFER_DST_OPTIMAL;
const READ: vk::ImageLayout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
const COPY: vk::ImageLayout = vk::ImageLayout::TRANSFER_SRC_OPTIMAL;
const RANGE: vk::ImageSubresourceRange = vk::ImageSubresourceRange {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    base_mip_level: 0,
    level_count: 1,
    base_array_layer: 0,
    layer_count: 1,
};

static PROCS: OnceLock<(vk::PFN_vkGetInstanceProcAddr, vk::PFN_vkGetDeviceProcAddr)> =
    OnceLock::new();

unsafe extern "C" fn instance_proc(object: *mut c_void, name: *const c_char) -> rsf_vk_function {
    let (instance, _) = PROCS.get().unwrap();
    // Vulkan and C use the same Windows x64 calling convention.
    unsafe { mem::transmute(instance(vk::Instance::from_raw(object as u64), name)) }
}

unsafe extern "C" fn device_proc(object: *mut c_void, name: *const c_char) -> rsf_vk_function {
    let (_, device) = PROCS.get().unwrap();
    unsafe { mem::transmute(device(vk::Device::from_raw(object as u64), name)) }
}

#[derive(Default)]
pub(super) struct Gpu {
    library: Option<Library>,
    instance: Option<ash::Instance>,
    device: Option<ash::Device>,
    memory: vk::PhysicalDeviceMemoryProperties,
    queue: vk::Queue,
    pool: vk::CommandPool,
    command: vk::CommandBuffer,
    fence: vk::Fence,
    readback: vk::Buffer,
    readback_memory: vk::DeviceMemory,
    allocations: Vec<(vk::Image, vk::DeviceMemory)>,
    images: [vk::Image; 4],
}

impl Gpu {
    pub(super) fn open(setup: &mut rsf_srvk_setup) -> Result<Self, Failure> {
        let library = Library::vulkan(true).map_err(|_| "Vulkan loader")?;
        // Gpu releases its resources on drop, so it is filled in place rather than by update.
        let mut gpu = Self::default();
        gpu.library = Some(library);
        gpu.initialize(setup)?;
        Ok(gpu)
    }

    fn initialize(&mut self, setup: &mut rsf_srvk_setup) -> Result<(), Failure> {
        let get_instance_proc_addr = self
            .library
            .as_ref()
            .and_then(Library::vulkan_instance_proc)
            .ok_or("Vulkan instance dispatch")?;
        require(
            unsafe { get_instance_proc_addr(vk::Instance::null(), c"vkCreateInstance".as_ptr()) }
                .is_some(),
            "create instance entry",
        )?;
        let entry = unsafe {
            Entry::from_static_fn(StaticFn {
                get_instance_proc_addr,
            })
        };
        let mut count = 0;
        let mut names = ptr::null();
        let mut minimum = 0;
        require(
            unsafe { rsf_xessvk_instance_requirements(&mut count, &mut names, &mut minimum) } == 0,
            "XeSS instance requirements",
        )?;
        let app = vk::ApplicationInfo::default()
            .application_name(c"ReScaleFrame Vulkan SR fixture")
            .api_version(vk::API_VERSION_1_3.max(minimum));
        let mut create = vk::InstanceCreateInfo::default().application_info(&app);
        create.enabled_extension_count = count;
        create.pp_enabled_extension_names = names;
        self.instance = Some(
            unsafe { entry.create_instance(&create, None) }
                .map_err(|_| "create Vulkan instance")?,
        );
        let instance = self.instance.as_ref().unwrap();
        if std::env::var_os("VK_INSTANCE_LAYERS").is_some_and(|layers| {
            layers
                .to_string_lossy()
                .contains("VK_LAYER_KHRONOS_validation")
        }) {
            let layer = rsf_game_sys::ffi_util::wide("VkLayer_khronos_validation.dll");
            require(
                !unsafe { GetModuleHandleW(layer.as_ptr()) }.is_null(),
                "requested Khronos validation layer must be loaded",
            )?;
            println!("Khronos validation DLL is loaded");
        }

        let devices = unsafe { instance.enumerate_physical_devices() }.map_err(|_| "read GPUs")?;
        let first = *devices.first().ok_or("enumerate GPUs")?;
        let properties = |device| unsafe { instance.get_physical_device_properties(device) };
        let physical = devices
            .into_iter()
            .find(|&device| properties(device).device_type == vk::PhysicalDeviceType::DISCRETE_GPU)
            .unwrap_or(first);
        let description = properties(physical);
        let name = unsafe { CStr::from_ptr(description.device_name.as_ptr()) };
        println!("Vulkan SR fixture GPU: {}", name.to_string_lossy());
        let family = unsafe { instance.get_physical_device_queue_family_properties(physical) }
            .iter()
            .position(|item| {
                item.queue_flags
                    .contains(vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE)
            })
            .ok_or("graphics/compute family")? as u32;

        let mut features12 = vk::PhysicalDeviceVulkan12Features::default();
        let mut features11 = vk::PhysicalDeviceVulkan11Features::default();
        let mut features = vk::PhysicalDeviceFeatures2::default()
            .push_next(&mut features12)
            .push_next(&mut features11);
        unsafe { instance.get_physical_device_features2(physical, &mut features) };
        let instance_handle = instance.handle().as_raw() as *mut c_void;
        let physical_handle = physical.as_raw() as *mut c_void;
        let mut chain = (&raw mut features).cast();
        require(
            unsafe {
                rsf_xessvk_device_requirements(
                    instance_handle,
                    physical_handle,
                    &mut count,
                    &mut names,
                    &mut chain,
                )
            } == 0,
            "XeSS device requirements",
        )?;
        println!("XeSS Vulkan minimum API={minimum} device extensions={count}");
        for index in 0..count as usize {
            let name = unsafe { CStr::from_ptr(*names.add(index)) };
            println!("XeSS device extension: {}", name.to_string_lossy());
        }
        let priority = 1.0;
        let queue = vk::DeviceQueueCreateInfo::default()
            .queue_family_index(family)
            .queue_priorities(from_ref(&priority));
        let mut create = vk::DeviceCreateInfo::default().queue_create_infos(from_ref(&queue));
        create.p_next = chain;
        create.enabled_extension_count = count;
        create.pp_enabled_extension_names = names;
        self.device = Some(
            unsafe { instance.create_device(physical, &create, None) }
                .map_err(|_| "create Vulkan device")?,
        );
        let device = self.device.as_ref().unwrap();
        self.queue = unsafe { device.get_device_queue(family, 0) };
        self.memory = unsafe { instance.get_physical_device_memory_properties(physical) };
        PROCS.get_or_init(|| {
            (
                get_instance_proc_addr,
                instance.fp_v1_0().get_device_proc_addr,
            )
        });
        setup.instance = instance_handle;
        setup.physical_device = physical_handle;
        setup.device = device.handle().as_raw() as *mut c_void;
        setup.instance_proc = Some(instance_proc);
        setup.device_proc = Some(device_proc);
        self.allocate_work(family)
    }

    fn allocate_work(&mut self, family: u32) -> Result<(), Failure> {
        let device = self.device.as_ref().unwrap();
        unsafe {
            self.pool = device
                .create_command_pool(
                    &vk::CommandPoolCreateInfo::default().queue_family_index(family),
                    None,
                )
                .map_err(|_| "create command pool")?;
            self.command = device
                .allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default()
                        .command_pool(self.pool)
                        .level(vk::CommandBufferLevel::PRIMARY)
                        .command_buffer_count(1),
                )
                .map_err(|_| "allocate commands")?[0];
            self.fence = device
                .create_fence(&vk::FenceCreateInfo::default(), None)
                .map_err(|_| "create fence")?;
            self.readback = device
                .create_buffer(
                    &vk::BufferCreateInfo::default()
                        .size(u64::from(WIDTH * HEIGHT * 8))
                        .usage(
                            vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::TRANSFER_SRC,
                        ),
                    None,
                )
                .map_err(|_| "create readback")?;
            let requirements = device.get_buffer_memory_requirements(self.readback);
            let index = memory_index(
                &self.memory,
                requirements.memory_type_bits,
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            )?;
            self.readback_memory = device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(requirements.size)
                        .memory_type_index(index),
                    None,
                )
                .map_err(|_| "allocate readback")?;
            device
                .bind_buffer_memory(self.readback, self.readback_memory, 0)
                .map_err(|_| "allocate readback")
        }
    }

    pub(super) fn allocate_images(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<[rsf_srvk_image; 4], Failure> {
        let device = self.device.as_ref().unwrap();
        let transfer = vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::TRANSFER_DST;
        let formats = [
            COLOR,
            vk::Format::R32_SFLOAT,
            vk::Format::R16G16_SFLOAT,
            COLOR,
        ];
        let mut descriptions: [rsf_srvk_image; 4] = unsafe { mem::zeroed() };
        for (index, (format, description)) in formats.into_iter().zip(&mut descriptions).enumerate()
        {
            let output = index == 3;
            let (width, height) = if output {
                (WIDTH, HEIGHT)
            } else {
                (width, height)
            };
            let usage = if output {
                transfer
            } else {
                transfer | vk::ImageUsageFlags::SAMPLED
            };
            let create = vk::ImageCreateInfo::default()
                .image_type(vk::ImageType::TYPE_2D)
                .format(format)
                .extent(vk::Extent3D {
                    width,
                    height,
                    depth: 1,
                })
                .mip_levels(1)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .usage(usage);
            unsafe {
                let image = device
                    .create_image(&create, None)
                    .map_err(|_| "create image")?;
                self.allocations.push((image, vk::DeviceMemory::null()));
                let requirements = device.get_image_memory_requirements(image);
                let memory_type = memory_index(
                    &self.memory,
                    requirements.memory_type_bits,
                    vk::MemoryPropertyFlags::DEVICE_LOCAL,
                )?;
                let memory = device
                    .allocate_memory(
                        &vk::MemoryAllocateInfo::default()
                            .allocation_size(requirements.size)
                            .memory_type_index(memory_type),
                        None,
                    )
                    .map_err(|_| "allocate image")?;
                self.allocations.last_mut().unwrap().1 = memory;
                device
                    .bind_image_memory(image, memory, 0)
                    .map_err(|_| "allocate image")?;
                self.images[index] = image;
                *description = rsf_srvk_image {
                    device: device.handle().as_raw() as *mut c_void,
                    image: image.as_raw(),
                    format: format.as_raw() as u32,
                    width,
                    height,
                    layout: (if output { TRANSFER } else { READ }).as_raw() as u32,
                    copy_source: 1,
                };
            }
        }
        Ok(descriptions)
    }

    pub(super) fn begin(&self) -> Result<*mut c_void, Failure> {
        let device = self.device.as_ref().unwrap();
        unsafe {
            device
                .reset_command_pool(self.pool, vk::CommandPoolResetFlags::empty())
                .map_err(|_| "reset command pool")?;
            device
                .begin_command_buffer(
                    self.command,
                    &vk::CommandBufferBeginInfo::default()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                )
                .map_err(|_| "begin commands")?;
        }
        Ok(self.command.as_raw() as *mut c_void)
    }

    pub(super) fn record(&self, first: bool, color: [f32; 4]) {
        let device = self.device.as_ref().unwrap();
        let initial = if first {
            vk::ImageLayout::UNDEFINED
        } else {
            READ
        };
        for &image in &self.images[..3] {
            self.transition(image, initial, TRANSFER);
        }
        if first {
            self.transition(self.images[3], vk::ImageLayout::UNDEFINED, TRANSFER);
        }
        for (&image, value) in self
            .images
            .iter()
            .zip([color, [0.5, 0.0, 0.0, 0.0], [0.0; 4]])
        {
            unsafe {
                device.cmd_clear_color_image(
                    self.command,
                    image,
                    TRANSFER,
                    &vk::ClearColorValue { float32: value },
                    from_ref(&RANGE),
                )
            };
        }
        for &image in &self.images[..3] {
            self.transition(image, TRANSFER, READ);
        }
    }

    fn transition(&self, image: vk::Image, from: vk::ImageLayout, to: vk::ImageLayout) {
        let access = vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE;
        let barrier = vk::ImageMemoryBarrier::default()
            .image(image)
            .subresource_range(RANGE)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .src_access_mask(if from == vk::ImageLayout::UNDEFINED {
                vk::AccessFlags::empty()
            } else {
                access
            })
            .dst_access_mask(access)
            .old_layout(from)
            .new_layout(to);
        let all = vk::PipelineStageFlags::ALL_COMMANDS;
        unsafe {
            self.device.as_ref().unwrap().cmd_pipeline_barrier(
                self.command,
                all,
                all,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                from_ref(&barrier),
            )
        };
    }

    pub(super) fn finish_and_verify(&self, expected: [f32; 4]) -> Result<(), Failure> {
        let device = self.device.as_ref().unwrap();
        let output = self.images[3];
        self.transition(output, TRANSFER, COPY);
        let region = vk::BufferImageCopy::default()
            .image_subresource(
                vk::ImageSubresourceLayers::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .layer_count(1),
            )
            .image_extent(vk::Extent3D {
                width: WIDTH,
                height: HEIGHT,
                depth: 1,
            });
        unsafe {
            device.cmd_copy_image_to_buffer(
                self.command,
                output,
                COPY,
                self.readback,
                from_ref(&region),
            )
        };
        self.transition(output, COPY, TRANSFER);
        let barrier = vk::BufferMemoryBarrier::default()
            .buffer(self.readback)
            .size(vk::WHOLE_SIZE)
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::HOST_READ)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED);
        unsafe {
            device.cmd_pipeline_barrier(
                self.command,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::HOST,
                vk::DependencyFlags::empty(),
                &[],
                from_ref(&barrier),
                &[],
            );
            device
                .end_command_buffer(self.command)
                .map_err(|_| "end commands")?;
            let submit = vk::SubmitInfo::default().command_buffers(from_ref(&self.command));
            device
                .queue_submit(self.queue, from_ref(&submit), self.fence)
                .map_err(|_| "submit and complete Vulkan work")?;
            device
                .wait_for_fences(from_ref(&self.fence), true, 15_000_000_000)
                .map_err(|_| "submit and complete Vulkan work")?;
            device
                .reset_fences(from_ref(&self.fence))
                .map_err(|_| "reset fence")?;
            let pixels = device
                .map_memory(
                    self.readback_memory,
                    0,
                    vk::WHOLE_SIZE,
                    vk::MemoryMapFlags::empty(),
                )
                .map_err(|_| "map output")?;
            let valid = !pixels.is_null()
                && verify_pixels(
                    std::slice::from_raw_parts(
                        pixels.cast(),
                        WIDTH as usize * HEIGHT as usize * CHANNELS,
                    ),
                    expected,
                );
            device.unmap_memory(self.readback_memory);
            require(valid, "GPU output disagrees with known linear scene colour")
        }
    }

    pub(super) fn drain(&self) -> Result<(), vk::Result> {
        unsafe { self.device.as_ref().unwrap().device_wait_idle() }
    }
}

impl Drop for Gpu {
    fn drop(&mut self) {
        unsafe {
            if let Some(device) = &self.device {
                let _ = device.device_wait_idle();
                for &(image, memory) in &self.allocations {
                    device.destroy_image(image, None);
                    device.free_memory(memory, None);
                }
                device.destroy_buffer(self.readback, None);
                device.free_memory(self.readback_memory, None);
                device.destroy_fence(self.fence, None);
                device.destroy_command_pool(self.pool, None);
                device.destroy_device(None);
            }
            if let Some(instance) = &self.instance {
                instance.destroy_instance(None);
            }
        }
    }
}

fn memory_index(
    memory: &vk::PhysicalDeviceMemoryProperties,
    bits: u32,
    flags: vk::MemoryPropertyFlags,
) -> Result<u32, Failure> {
    memory.memory_types[..memory.memory_type_count as usize]
        .iter()
        .enumerate()
        .find(|(index, entry)| bits & (1u32 << *index) != 0 && entry.property_flags.contains(flags))
        .map(|(index, _)| index as u32)
        .ok_or("Vulkan memory type")
}

fn verify_pixels(pixels: &[u16], expected: [f32; 4]) -> bool {
    for y in 16..164 {
        for x in 16..304 {
            for (channel, expected) in expected[..3].iter().enumerate() {
                let bits = pixels[(y * WIDTH as usize + x) * CHANNELS + channel];
                let exponent = (bits >> 10) & 31;
                if exponent == 31 {
                    return false;
                }
                let fraction = f32::from(bits & 1023);
                let value = if exponent == 0 {
                    fraction * 2_f32.powi(-24)
                } else {
                    (1.0 + fraction / 1024.0) * 2_f32.powi(i32::from(exponent) - 15)
                };
                let value = if bits & 0x8000 == 0 { value } else { -value };
                if (value - *expected).abs() >= 0.025 {
                    return false;
                }
            }
        }
    }
    true
}
