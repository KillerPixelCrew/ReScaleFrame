// SPDX-License-Identifier: MIT
use super::{abi::*, bundles, contents, entry, root, utf8};
use ash::vk::{self, Handle as _};
use rsf_game_sys::{
    RSF_FRAME_FLAG_NO_FG, RSF_FRAME_FLAG_NO_SR, RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record,
};
use std::ffi::{c_char, c_void};
use std::sync::OnceLock;
use std::{fs, mem, path::Path, ptr, slice};
use windows_sys::Win32::System::Environment::SetEnvironmentVariableW;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

static INSTANCE_PROC: OnceLock<vk::PFN_vkGetInstanceProcAddr> = OnceLock::new();
static DEVICE_PROC: OnceLock<vk::PFN_vkGetDeviceProcAddr> = OnceLock::new();

unsafe extern "C" fn instance_proc(object: *mut c_void, name: *const c_char) -> rsf_vk_function {
    let proc = INSTANCE_PROC.get()?;
    unsafe {
        mem::transmute::<vk::PFN_vkVoidFunction, rsf_vk_function>(proc(
            vk::Instance::from_raw(object as u64),
            name,
        ))
    }
}

unsafe extern "C" fn device_proc(object: *mut c_void, name: *const c_char) -> rsf_vk_function {
    let proc = DEVICE_PROC.get()?;
    unsafe {
        mem::transmute::<vk::PFN_vkVoidFunction, rsf_vk_function>(proc(
            vk::Device::from_raw(object as u64),
            name,
        ))
    }
}

struct Device {
    api: &'static rsf_fixture_vk_capture_api,
    instance: ash::Instance,
    physical: vk::PhysicalDevice,
    device: Option<ash::Device>,
    queue: vk::Queue,
    family: u32,
    pool: vk::CommandPool,
    image: vk::Image,
    memory: vk::DeviceMemory,
    capture: *mut rsf_input_dump_vk,
}

impl Device {
    fn open(api: &'static rsf_fixture_vk_capture_api) -> Result<Self, vk::Result> {
        let module = unsafe { LoadLibraryW(windows::core::w!("vulkan-1.dll").as_ptr()) };
        if module.is_null() {
            return Err(vk::Result::ERROR_INITIALIZATION_FAILED);
        }
        // Resolver callbacks outlive this fixture's Vulkan objects. Pin the loader
        // for the process, which runs exactly one fixture.
        let entry = unsafe {
            let get = GetProcAddress(module, c"vkGetInstanceProcAddr".as_ptr().cast())
                .ok_or(vk::Result::ERROR_INITIALIZATION_FAILED)?;
            let get = mem::transmute::<
                unsafe extern "system" fn() -> isize,
                vk::PFN_vkGetInstanceProcAddr,
            >(get);
            let _ = INSTANCE_PROC.set(get);
            ash::Entry::from_static_fn(ash::StaticFn {
                get_instance_proc_addr: get,
            })
        };
        let app = vk::ApplicationInfo::default()
            .application_name(c"ReScaleFrame Vulkan capture fixture")
            .api_version(vk::API_VERSION_1_2);
        let instance = unsafe {
            entry.create_instance(
                &vk::InstanceCreateInfo::default().application_info(&app),
                None,
            )
        }?;
        let mut owner = Self {
            api,
            instance,
            physical: vk::PhysicalDevice::null(),
            device: None,
            queue: vk::Queue::null(),
            family: 0,
            pool: vk::CommandPool::null(),
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            capture: ptr::null_mut(),
        };
        owner.physical = *unsafe { owner.instance.enumerate_physical_devices() }?
            .first()
            .ok_or(vk::Result::ERROR_INITIALIZATION_FAILED)?;
        let queue_props = unsafe {
            owner
                .instance
                .get_physical_device_queue_family_properties(owner.physical)
        };
        owner.family = queue_props
            .iter()
            .position(|q| q.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .ok_or(vk::Result::ERROR_INITIALIZATION_FAILED)? as u32;
        let family = vk::DeviceQueueCreateInfo::default()
            .queue_family_index(owner.family)
            .queue_priorities(&[1.0]);
        let extensions = [
            ash::khr::external_memory_win32::NAME.as_ptr(),
            ash::khr::external_semaphore_win32::NAME.as_ptr(),
        ];
        let mut timeline =
            vk::PhysicalDeviceTimelineSemaphoreFeatures::default().timeline_semaphore(true);
        let create = vk::DeviceCreateInfo::default()
            .queue_create_infos(slice::from_ref(&family))
            .enabled_extension_names(&extensions)
            .push_next(&mut timeline);
        let device = unsafe { owner.instance.create_device(owner.physical, &create, None) }?;
        let _ = DEVICE_PROC.set(owner.instance.fp_v1_0().get_device_proc_addr);
        owner.queue = unsafe { device.get_device_queue(owner.family, 0) };
        owner.device = Some(device);
        Ok(owner)
    }

    fn prepare(&mut self) -> Result<(), vk::Result> {
        let device = self.device.as_ref().unwrap();
        let format = vk::Format::R8G8B8A8_UNORM;
        unsafe {
            self.image = device.create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(format)
                    .extent(vk::Extent3D {
                        width: 4,
                        height: 2,
                        depth: 1,
                    })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(
                        vk::ImageUsageFlags::SAMPLED
                            | vk::ImageUsageFlags::TRANSFER_SRC
                            | vk::ImageUsageFlags::TRANSFER_DST,
                    ),
                None,
            )?;
            let requirements = device.get_image_memory_requirements(self.image);
            let properties = self
                .instance
                .get_physical_device_memory_properties(self.physical);
            let index = (0..properties.memory_type_count)
                .find(|&index| {
                    requirements.memory_type_bits & (1 << index) != 0
                        && properties.memory_types[index as usize]
                            .property_flags
                            .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
                })
                .ok_or(vk::Result::ERROR_OUT_OF_DEVICE_MEMORY)?;
            self.memory = device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(requirements.size)
                    .memory_type_index(index),
                None,
            )?;
            device.bind_image_memory(self.image, self.memory, 0)?;
            self.pool = device.create_command_pool(
                &vk::CommandPoolCreateInfo::default().queue_family_index(self.family),
                None,
            )?;
            let commands = device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(self.pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )?[0];
            device.begin_command_buffer(commands, &vk::CommandBufferBeginInfo::default())?;
            let range = vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .level_count(1)
                .layer_count(1);
            let mut barrier = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .image(self.image)
                .subresource_range(range)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED);
            device.cmd_pipeline_barrier(
                commands,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                slice::from_ref(&barrier),
            );
            device.cmd_clear_color_image(
                commands,
                self.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &vk::ClearColorValue {
                    float32: [0.25, 0.5, 1.0, 0.0],
                },
                &[range],
            );
            barrier = barrier
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ);
            device.cmd_pipeline_barrier(
                commands,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            );
            device.end_command_buffer(commands)?;
            let info = vk::SubmitInfo::default().command_buffers(slice::from_ref(&commands));
            device.queue_submit(self.queue, &[info], vk::Fence::null())
        }
    }

    fn setup(&mut self) -> rsf_input_dump_vk_setup {
        let device = self.device.as_ref().unwrap().handle().as_raw() as *mut c_void;
        rsf_input_dump_vk_setup {
            struct_size: size_of::<rsf_input_dump_vk_setup>() as u32,
            abi_version: 1,
            graphics: rsf_srvk_setup {
                struct_size: size_of::<rsf_srvk_setup>() as u32,
                abi_version: RSF_SRVK_ABI_VERSION,
                instance: self.instance.handle().as_raw() as *mut c_void,
                physical_device: self.physical.as_raw() as *mut c_void,
                device,
                instance_proc: Some(instance_proc),
                device_proc: Some(device_proc),
                ..Default::default()
            },
            queue: self.queue.as_raw() as *mut c_void,
            queue_family: self.family,
            submit: Some(submit),
            user: ptr::from_mut(self).cast(),
        }
    }
}

unsafe extern "C" fn submit(
    user: *mut c_void,
    count: u32,
    values: *const c_void,
    fence: u64,
) -> i32 {
    let owner = unsafe { &*user.cast::<Device>() };
    let values = if count == 0 {
        &[]
    } else {
        unsafe { slice::from_raw_parts(values.cast(), count as usize) }
    };
    unsafe {
        owner
            .device
            .as_ref()
            .unwrap()
            .queue_submit(owner.queue, values, vk::Fence::from_raw(fence))
    }
    .map_or_else(vk::Result::as_raw, |()| 0)
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            if let Some(device) = self.device.take() {
                let _ = device.device_wait_idle();
                if !self.capture.is_null() && (self.api.destroy.unwrap())(self.capture) != 0 {
                    // The fixture process will exit with the lease still intact.
                    return;
                }
                if self.pool != vk::CommandPool::null() {
                    device.destroy_command_pool(self.pool, None);
                }
                if self.image != vk::Image::null() {
                    device.destroy_image(self.image, None);
                }
                if self.memory != vk::DeviceMemory::null() {
                    device.free_memory(self.memory, None);
                }
                device.destroy_device(None);
            }
            self.instance.destroy_instance(None);
        }
    }
}

fn run(path: &Path, api: &'static rsf_fixture_vk_capture_api) -> i32 {
    unsafe {
        SetEnvironmentVariableW(
            windows::core::w!("RSF_VULKAN_FRAME_CAPTURE_DIR").as_ptr(),
            ptr::null(),
        )
    };
    let mut device = match Device::open(api) {
        Ok(device) => device,
        Err(result) => {
            println!("Vulkan capture device unavailable: {}", result.as_raw());
            return 77;
        }
    };
    device.prepare().expect("prepare Vulkan source image");
    let root = root(path, "core-dump-vulkan");
    let root_utf8 = utf8(&root);
    let setup = device.setup();
    let record = rsf_frame_record {
        struct_size: size_of::<rsf_frame_record>() as u32,
        abi_version: RSF_GAME_FRAME_ABI_VERSION,
        flags: RSF_FRAME_FLAG_NO_SR | RSF_FRAME_FLAG_NO_FG,
        session_id: 31,
        frame_id: 32,
        view_id: 33,
        render_width: 4,
        output_width: 4,
        render_height: 2,
        output_height: 2,
        ..unsafe { mem::zeroed() }
    };
    let input = rsf_srvk_image {
        device: setup.graphics.device,
        image: device.image.as_raw(),
        format: vk::Format::R8G8B8A8_UNORM.as_raw() as u32,
        width: 4,
        height: 2,
        layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL.as_raw() as u32,
        copy_source: 1,
    };
    unsafe {
        rsf_input_dump_configure(
            root_utf8.as_ptr(),
            c"FixtureVulkan".as_ptr(),
            c"fixture".as_ptr(),
            None,
            ptr::null_mut(),
        );
        rsf_input_dump_request();
        assert_ne!(rsf_input_dump_begin_record(&record, 2), 0);
        assert_eq!((api.create.unwrap())(&setup, &mut device.capture), 0);
        assert!(!device.capture.is_null());
        assert_eq!(
            (api.snapshot.unwrap())(device.capture, &record, &input, c"final_color".as_ptr()),
            0
        );
        rsf_input_dump_complete(&record, 0);
        let bundle = bundles(&root).next().unwrap();
        assert_eq!(
            fs::read(bundle.join("final_color.bin")).unwrap(),
            [64, 128, 255, 0].repeat(8)
        );
        assert!(contents(&bundle, "final_color_raw.json").contains("\"native_api\":3"));
        assert!(contents(&bundle, "manifest.json").contains("\"status\":\"complete\""));
        assert!(
            fs::read(bundle.join("final_color.png"))
                .unwrap()
                .starts_with(b"\x89PNG\r\n\x1a\n")
        );
        assert_eq!((api.destroy.unwrap())(device.capture), 0);
        device.capture = ptr::null_mut();
        println!("Vulkan core dump fixture: {}", bundle.display());
    }
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_input_dump_vulkan(
    path: *const c_char,
    api: &'static rsf_fixture_vk_capture_api,
) -> i32 {
    entry(path, |path| run(path, api))
}
