// SPDX-License-Identifier: MIT
use crate::dx12_vulkan_interop_abi::rsf_dx12vk_shared_image;
use crate::vulkan_sr_abi::{RSF_SRVK_ABI_VERSION, rsf_srvk_setup, rsf_vk_function};
use crate::xess_bridge_abi::{
    RSF_XESS_BRIDGE_ABI_MISMATCH, RSF_XESS_BRIDGE_ABI_VERSION, RSF_XESS_BRIDGE_INVALID_ARGUMENT,
    rsf_xess_bridge_create,
};
use ash::{Entry, StaticFn, vk, vk::Handle as _};
use std::{
    ffi::{c_char, c_void},
    mem,
    ptr::{self, null_mut},
    sync::OnceLock,
};
use windows_sys::Win32::{
    Foundation::FreeLibrary,
    System::LibraryLoader::{GetProcAddress, LoadLibraryW},
};

type Handle = *mut c_void;
static INSTANCE_PROC: OnceLock<vk::PFN_vkGetInstanceProcAddr> = OnceLock::new();
static DEVICE_PROC: OnceLock<vk::PFN_vkGetDeviceProcAddr> = OnceLock::new();

unsafe extern "C" {
    fn RsfXessBridgeCreate(create: *const rsf_xess_bridge_create, output: *mut Handle) -> i32;
    fn VulkanStoryXessFgWaitIdle(bridge: Handle) -> i32;
    fn VulkanStoryXessFgDestroyChecked(bridge: Handle) -> i32;
    fn rsf_xessvk_interop_create(
        setup: *const rsf_srvk_setup,
        bridge: Handle,
        timeline_enabled: u32,
        output: *mut Handle,
    ) -> i32;
    fn rsf_xessvk_interop_image(
        owner: Handle,
        format: u32,
        width: u32,
        height: u32,
        output: *mut rsf_dx12vk_shared_image,
    ) -> i32;
    fn rsf_xessvk_interop_destroy(owner: Handle, drained: u32) -> i32;
}

unsafe extern "C" fn instance_proc(object: Handle, name: *const c_char) -> rsf_vk_function {
    // The RSF callback erases the function type. The x64 Windows Vulkan ABI
    // uses the same calling convention for extern "system" and extern "C".
    unsafe {
        mem::transmute(INSTANCE_PROC.get().unwrap()(
            vk::Instance::from_raw(object as u64),
            name,
        ))
    }
}

unsafe extern "C" fn device_proc(object: Handle, name: *const c_char) -> rsf_vk_function {
    unsafe {
        mem::transmute(DEVICE_PROC.get().unwrap()(
            vk::Device::from_raw(object as u64),
            name,
        ))
    }
}

fn contract() -> i32 {
    let mut output = ptr::without_provenance_mut(1);
    let mut create = rsf_xess_bridge_create {
        struct_size: size_of::<rsf_xess_bridge_create>() as u32 - 1,
        abi_version: RSF_XESS_BRIDGE_ABI_VERSION,
        adapter_luid: 1,
        runtime_directory: ptr::null(),
    };
    if unsafe { RsfXessBridgeCreate(&create, &mut output) } != RSF_XESS_BRIDGE_INVALID_ARGUMENT
        || !output.is_null()
    {
        return 10;
    }
    create.struct_size += 1;
    create.abi_version += 1;
    if unsafe { RsfXessBridgeCreate(&create, &mut output) } != RSF_XESS_BRIDGE_ABI_MISMATCH
        || !output.is_null()
    {
        return 11;
    }
    create.abi_version = RSF_XESS_BRIDGE_ABI_VERSION;
    let relative: Vec<_> = "relative\0".encode_utf16().collect();
    create.runtime_directory = relative.as_ptr();
    if unsafe { RsfXessBridgeCreate(&create, &mut output) } != RSF_XESS_BRIDGE_INVALID_ARGUMENT
        || !output.is_null()
    {
        return 12;
    }
    // Only the aligned size word is initialized for this preflight call.
    let mut prefix = mem::MaybeUninit::<rsf_xess_bridge_create>::uninit();
    unsafe {
        prefix
            .as_mut_ptr()
            .cast::<u32>()
            .write(size_of::<u32>() as u32)
    };
    output = ptr::without_provenance_mut(1);
    if unsafe { RsfXessBridgeCreate(prefix.as_ptr(), &mut output) }
        != RSF_XESS_BRIDGE_INVALID_ARGUMENT
        || !output.is_null()
    {
        return 14;
    }
    0
}

struct Device {
    physical: vk::PhysicalDevice,
    logical: ash::Device,
    family: u32,
    luid: u64,
    properties: vk::PhysicalDeviceProperties,
}

fn open_device(instance: &ash::Instance) -> Result<Device, i32> {
    let enumerate = instance.fp_v1_0().enumerate_physical_devices;
    let mut count = 0;
    if unsafe { enumerate(instance.handle(), &mut count, null_mut()) } != vk::Result::SUCCESS
        || count == 0
    {
        return Err(23);
    }
    let mut physicals = vec![vk::PhysicalDevice::null(); count as usize];
    if unsafe { enumerate(instance.handle(), &mut count, physicals.as_mut_ptr()) }
        != vk::Result::SUCCESS
        || count == 0
    {
        return Err(24);
    }
    let physical = physicals[0];
    let mut identity = vk::PhysicalDeviceIDProperties::default();
    let mut properties = vk::PhysicalDeviceProperties2::default().push_next(&mut identity);
    unsafe { instance.get_physical_device_properties2(physical, &mut properties) };
    let properties = properties.properties;
    let luid = u64::from_ne_bytes(identity.device_luid);
    if identity.device_luid_valid == 0 || luid == 0 {
        return Err(25);
    }
    let family = unsafe { instance.get_physical_device_queue_family_properties(physical) }
        .iter()
        .position(|item| item.queue_flags.contains(vk::QueueFlags::GRAPHICS))
        .ok_or(26)? as u32;
    let priorities = [1.0];
    let queues = [vk::DeviceQueueCreateInfo::default()
        .queue_family_index(family)
        .queue_priorities(&priorities)];
    let extensions = [
        ash::khr::external_memory_win32::NAME.as_ptr(),
        ash::khr::external_semaphore_win32::NAME.as_ptr(),
    ];
    let mut timeline =
        vk::PhysicalDeviceTimelineSemaphoreFeatures::default().timeline_semaphore(true);
    let create = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queues)
        .enabled_extension_names(&extensions)
        .push_next(&mut timeline);
    let logical = unsafe { instance.create_device(physical, &create, None) }.map_err(|_| 27)?;
    DEVICE_PROC.get_or_init(|| instance.fp_v1_0().get_device_proc_addr);
    Ok(Device {
        physical,
        logical,
        family,
        luid,
        properties,
    })
}

fn exercise(
    instance: &ash::Instance,
    gpu: &Device,
    runtime: *const u16,
    present: bool,
) -> (i32, bool) {
    let setup = rsf_srvk_setup {
        struct_size: size_of::<rsf_srvk_setup>() as u32,
        abi_version: RSF_SRVK_ABI_VERSION,
        instance: instance.handle().as_raw() as Handle,
        physical_device: gpu.physical.as_raw() as Handle,
        device: gpu.logical.handle().as_raw() as Handle,
        instance_proc: Some(instance_proc),
        device_proc: Some(device_proc),
        backend: 0,
        quality: 0,
        output_width: 0,
        output_height: 0,
        depth_inverted: 0,
        depth_infinite: 0,
        motion_jittered: 0,
        runtime_directory_utf8: ptr::null(),
    };
    let create = rsf_xess_bridge_create {
        struct_size: size_of::<rsf_xess_bridge_create>() as u32,
        abi_version: RSF_XESS_BRIDGE_ABI_VERSION,
        adapter_luid: gpu.luid,
        runtime_directory: runtime,
    };
    let mut bridge = null_mut();
    let bridge_result = unsafe { RsfXessBridgeCreate(&create, &mut bridge) };
    let mut interop = null_mut();
    let mut import = -1;
    let mut images = [-1; 3];
    if bridge_result == 0 {
        import = unsafe { rsf_xessvk_interop_create(&setup, bridge, 1, &mut interop) };
        if import == 0 {
            let formats = [
                vk::Format::B8G8R8A8_UNORM,
                vk::Format::R32_SFLOAT,
                vk::Format::R16G16_SFLOAT,
            ];
            for (result, format) in images.iter_mut().zip(formats) {
                let mut image = mem::MaybeUninit::zeroed();
                *result = unsafe {
                    rsf_xessvk_interop_image(
                        interop,
                        format.as_raw() as u32,
                        640,
                        360,
                        image.as_mut_ptr(),
                    )
                };
            }
        }
    }
    let vk_drain = unsafe { gpu.logical.device_wait_idle() }
        .err()
        .map_or(0, vk::Result::as_raw);
    let dx_drain = if bridge.is_null() {
        0
    } else {
        unsafe { VulkanStoryXessFgWaitIdle(bridge) }
    };
    let release = if vk_drain == 0 && dx_drain == 0 {
        unsafe { rsf_xessvk_interop_destroy(interop, 1) }
    } else {
        -1
    };
    let bridge_release = if release == 0 {
        unsafe { VulkanStoryXessFgDestroyChecked(bridge) }
    } else {
        -1
    };
    println!(
        concat!(
            "{{\"contract_checks\":\"passed\",\"device_test\":true,\"vendor_id\":{},\"device_id\":{},",
            "\"bridge_result\":{},\"timeline_import\":{},\"bgra8_import\":{},\"r32f_import\":{},\"rg16f_import\":{},",
            "\"vulkan_drain\":{},\"dx12_drain\":{},\"interop_release\":{},\"bridge_release\":{}}}"
        ),
        gpu.properties.vendor_id,
        gpu.properties.device_id,
        bridge_result,
        import,
        images[0],
        images[1],
        images[2],
        vk_drain,
        dx_drain,
        release,
        bridge_release,
    );
    let mut retired = release == 0 && bridge_release == 0;
    let present_result = if retired && present {
        crate::xess_present::run(&setup, gpu.family, runtime, &mut retired)
    } else {
        0
    };
    let failed =
        bridge_result != 0 || import != 0 || images != [0; 3] || !retired || present_result != 0;
    (i32::from(failed), retired)
}

fn device(runtime: *const u16, present: bool) -> i32 {
    let name: Vec<_> = "vulkan-1.dll\0".encode_utf16().collect();
    let loader = unsafe { LoadLibraryW(name.as_ptr()) };
    if loader.is_null() {
        return 20;
    }
    let result = (|| {
        let Some(get_proc) =
            (unsafe { GetProcAddress(loader, c"vkGetInstanceProcAddr".as_ptr().cast()) })
        else {
            return (21, true);
        };
        let get_instance_proc_addr = unsafe { mem::transmute(get_proc) };
        INSTANCE_PROC.get_or_init(|| get_instance_proc_addr);
        let entry = unsafe {
            Entry::from_static_fn(StaticFn {
                get_instance_proc_addr,
            })
        };
        let app = vk::ApplicationInfo::default()
            .application_name(c"ReScaleFrame XeSS FG interop fixture")
            .api_version(vk::API_VERSION_1_2);
        let create = vk::InstanceCreateInfo::default().application_info(&app);
        let Ok(instance) = (unsafe { entry.create_instance(&create, None) }) else {
            return (22, true);
        };
        let gpu = match open_device(&instance) {
            Ok(gpu) => gpu,
            Err(result) => {
                unsafe { instance.destroy_instance(None) };
                return (result, true);
            }
        };
        let (result, retired) = exercise(&instance, &gpu, runtime, present);
        if retired {
            unsafe {
                gpu.logical.destroy_device(None);
                instance.destroy_instance(None);
            }
        }
        (result, retired)
    })();
    if result.1 {
        unsafe { FreeLibrary(loader) };
    }
    result.0
}

fn argument_matches(argument: *const u16, expected: &str) -> bool {
    let mut argument = argument;
    for unit in expected.encode_utf16().chain([0]) {
        if unsafe { *argument } != unit {
            return false;
        }
        argument = unsafe { argument.add(1) };
    }
    true
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fixture_xess_interop(argc: i32, argv: *const *const u16) -> i32 {
    let result = contract();
    if result != 0 {
        return result;
    }
    if argc == 1 {
        println!(r#"{{"contract_checks":"passed","device_test":false}}"#);
        return 0;
    }
    if !(3..=4).contains(&argc) {
        return 13;
    }
    let args = unsafe { std::slice::from_raw_parts(argv, argc as usize) };
    if !argument_matches(args[1], "--device")
        || (argc == 4 && !argument_matches(args[3], "--present"))
    {
        return 13;
    }
    device(args[2], argc == 4)
}
