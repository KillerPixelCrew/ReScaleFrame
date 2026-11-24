// SPDX-License-Identifier: MIT
use super::chain::{ANTI_LAG_FEATURES, AntiLagFeatures};
use super::*;
use crate::platform::VkChainFacts;
use crate::platform::vulkan_negotiation::VK_ERROR_FEATURE_NOT_PRESENT;
use std::cell::RefCell;
use std::ffi::CStr;
use vk::StructureType as Tag;

const INSTANCE: usize = 0x11;
const PHYSICAL: usize = 0x12;
const DEVICE: usize = 0x13;
const API_12: u32 = (1 << 22) | (2 << 12);
const API_13: u32 = (1 << 22) | (3 << 12);

/// A default ash record with `edit` applied; ash's records keep a private marker field, so they
/// cannot be built with struct update syntax.
fn made<T: Default>(edit: impl FnOnce(&mut T)) -> T {
    let mut value = T::default();
    edit(&mut value);
    value
}

#[derive(Default)]
struct Calls {
    instance: Option<(u32, Vec<String>, usize)>,
    devices: Vec<ObservedDevice>,
    next_result: Option<vk::Result>,
    xess_count: usize,
    refuse_xess: bool,
    interop: bool,
    latency: bool,
    latency_clone: usize,
    latency_created: Option<(usize, usize)>,
    destroy: Vec<usize>,
}

struct ObservedDevice {
    request: usize,
    chain: usize,
    allocator: usize,
    queue: usize,
    core_features: usize,
    root: bool,
    inherited: bool,
    sdk_write: bool,
    individual_features: bool,
    extensions: Vec<String>,
}

thread_local! {
    static CALLS: RefCell<Calls> = RefCell::default();
    static INSTANCE_NAMES: [*const c_char; 2] = const { [c"VK_EXT_fixture".as_ptr(); 2] };
    static DEVICE_NAMES: [*const c_char; 3] = const {
        [
            c"VK_KHR_swapchain".as_ptr(),
            c"VK_EXT_mutable_descriptor_type".as_ptr(),
            c"VK_EXT_mutable_descriptor_type".as_ptr(),
        ]
    };
    static LATENCY_NAMES: [*const c_char; 1] = const { [c"VK_AMD_anti_lag".as_ptr()] };
    static AMD: std::cell::Cell<AntiLagFeatures> = const {
        std::cell::Cell::new(AntiLagFeatures {
            s_type: ANTI_LAG_FEATURES,
            p_next: ptr::null_mut(),
            anti_lag: 1,
        })
    };
}

fn names(list: *const *const c_char, count: u32) -> Vec<String> {
    unsafe { super::super::extension_list(list, count) }
        .iter()
        .map(|&name| {
            unsafe { CStr::from_ptr(name) }
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

fn feature(mut node: *const vk::BaseInStructure<'static>, tag: Tag) -> bool {
    for _ in 0..64 {
        let Some(record) = (unsafe { node.as_ref() }) else {
            break;
        };
        if record.s_type == tag {
            return true;
        }
        node = record.p_next;
    }
    false
}

unsafe extern "system" fn mock_create_instance(
    info: *const vk::InstanceCreateInfo<'_>,
    allocator: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::Instance,
) -> vk::Result {
    let info = unsafe { &*info };
    let app = unsafe { &*info.p_application_info };
    CALLS.with_borrow_mut(|calls| {
        calls.instance = Some((
            app.api_version,
            names(
                info.pp_enabled_extension_names,
                info.enabled_extension_count,
            ),
            allocator.addr(),
        ));
    });
    unsafe { out.write(vk::Instance::from_raw(INSTANCE as u64)) };
    vk::Result::SUCCESS
}

unsafe extern "system" fn mock_create_device(
    _: vk::PhysicalDevice,
    info: *const vk::DeviceCreateInfo<'_>,
    allocator: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::Device,
) -> vk::Result {
    let request = unsafe { &*info };
    let head = request.p_next.cast::<vk::BaseInStructure<'static>>();
    let root =
        unsafe { head.as_ref() }.filter(|node| node.s_type == Tag::PHYSICAL_DEVICE_FEATURES_2);
    let features =
        root.map(|_| unsafe { (*head.cast::<vk::PhysicalDeviceFeatures2<'static>>()).features });
    let observation = ObservedDevice {
        request: info.addr(),
        chain: head.addr(),
        allocator: allocator.addr(),
        queue: request.p_queue_create_infos.addr(),
        core_features: request.p_enabled_features.addr(),
        root: root.is_some(),
        inherited: features.is_some_and(|value| value.shader_clip_distance != 0),
        sdk_write: features
            .is_some_and(|value| value.shader_storage_image_write_without_format != 0),
        individual_features: feature(head, Tag::PHYSICAL_DEVICE_SCALAR_BLOCK_LAYOUT_FEATURES)
            && feature(
                head,
                Tag::PHYSICAL_DEVICE_SHADER_INTEGER_DOT_PRODUCT_FEATURES,
            ),
        extensions: names(
            request.pp_enabled_extension_names,
            request.enabled_extension_count,
        ),
    };
    let result = CALLS.with_borrow_mut(|calls| {
        calls.devices.push(observation);
        calls.next_result.take().unwrap_or(vk::Result::SUCCESS)
    });
    if result == vk::Result::SUCCESS {
        unsafe { out.write(vk::Device::from_raw(DEVICE as u64)) };
    }
    result
}

unsafe extern "system" fn mock_enumerate(
    _: vk::PhysicalDevice,
    _: *const c_char,
    count: *mut u32,
    entries: *mut vk::ExtensionProperties,
) -> vk::Result {
    if !CALLS.with_borrow(|calls| calls.interop) {
        unsafe { count.write(0) };
        return vk::Result::SUCCESS;
    }
    if !entries.is_null() {
        for (index, name) in [
            c"VK_KHR_external_memory_win32",
            c"VK_KHR_external_semaphore_win32",
        ]
        .iter()
        .enumerate()
        {
            let mut entry = vk::ExtensionProperties::default();
            for (slot, &byte) in entry
                .extension_name
                .iter_mut()
                .zip(name.to_bytes_with_nul())
            {
                *slot = byte as c_char;
            }
            unsafe { entries.add(index).write(entry) };
        }
    }
    unsafe { count.write(2) };
    vk::Result::SUCCESS
}

unsafe extern "system" fn mock_properties(
    _: vk::PhysicalDevice,
    out: *mut vk::PhysicalDeviceProperties,
) {
    unsafe {
        out.write(made(|properties: &mut vk::PhysicalDeviceProperties| {
            properties.vendor_id = 0x8086;
            properties.device_id = 0x1234;
        }))
    };
}

unsafe extern "system" fn mock_destroy(
    device: vk::Device,
    allocator: *const vk::AllocationCallbacks<'_>,
) {
    CALLS.with_borrow_mut(|calls| {
        calls
            .destroy
            .extend([device.as_raw() as usize, allocator.addr()])
    });
}

unsafe extern "C" fn mock_device_proc(_: *mut c_void, name: *const c_char) -> Option<VkFunction> {
    if unsafe { CStr::from_ptr(name) } != c"vkDestroyDevice" {
        return None;
    }
    Some(unsafe { std::mem::transmute::<vk::PFN_vkDestroyDevice, VkFunction>(mock_destroy) })
}

unsafe extern "C" fn mock_proc(_: *mut c_void, name: *const c_char) -> Option<VkFunction> {
    if name.is_null() {
        return None;
    }
    unsafe {
        match CStr::from_ptr(name).to_bytes() {
            b"vkCreateInstance" => Some(
                std::mem::transmute::<vk::PFN_vkCreateInstance, VkFunction>(mock_create_instance),
            ),
            b"vkCreateDevice" => Some(std::mem::transmute::<vk::PFN_vkCreateDevice, VkFunction>(
                mock_create_device,
            )),
            b"vkEnumerateDeviceExtensionProperties" => Some(std::mem::transmute::<
                vk::PFN_vkEnumerateDeviceExtensionProperties,
                VkFunction,
            >(mock_enumerate)),
            b"vkGetPhysicalDeviceProperties" => Some(std::mem::transmute::<
                vk::PFN_vkGetPhysicalDeviceProperties,
                VkFunction,
            >(mock_properties)),
            b"vkGetDeviceProcAddr" => {
                Some(std::mem::transmute::<VkProc, VkFunction>(mock_device_proc))
            }
            _ => None,
        }
    }
}

pub(super) unsafe fn rsf_xessvk_instance_requirements(
    count: *mut u32,
    names: *mut *const *const c_char,
    minimum: *mut u32,
) -> i32 {
    unsafe {
        count.write(2);
        names.write(INSTANCE_NAMES.with(|entries| entries.as_ptr()));
        minimum.write(API_13);
    }
    0
}

pub(super) unsafe fn rsf_xessvk_device_requirements(
    _: *mut c_void,
    _: *mut c_void,
    count: *mut u32,
    names: *mut *const *const c_char,
    chain: *mut *mut c_void,
) -> i32 {
    let refuse = CALLS.with_borrow_mut(|calls| {
        calls.xess_count += 1;
        calls.refuse_xess
    });
    let head = unsafe { &mut *chain.read().cast::<vk::PhysicalDeviceFeatures2<'static>>() };
    if head.s_type != Tag::PHYSICAL_DEVICE_FEATURES_2 {
        return NOT_SUPPORTED;
    }
    head.features.shader_storage_image_write_without_format = 1;
    if refuse {
        return NOT_SUPPORTED;
    }
    unsafe {
        count.write(3);
        names.write(DEVICE_NAMES.with(|entries| entries.as_ptr()));
    }
    0
}

pub(super) unsafe fn rsf_latencyvk_require_device(
    query: *const rsf_latencyvk_device_query,
    out: *mut rsf_latencyvk_device_requirements,
) -> i32 {
    let query = unsafe { &*query };
    if !CALLS.with_borrow(|calls| calls.latency) {
        return NOT_SUPPORTED;
    }
    CALLS.with_borrow_mut(|calls| calls.latency_clone = query.features.addr());
    let mut answer = unsafe { out.read() };
    answer.extension_count = 1;
    answer.extensions = LATENCY_NAMES.with(|entries| entries.as_ptr());
    answer.features = AMD.with(|node| {
        node.set(AntiLagFeatures {
            p_next: query.features,
            ..node.get()
        });
        node.as_ptr().cast()
    });
    unsafe { out.write(answer) };
    0
}

pub(super) unsafe fn rsf_latencyvk_device_created(
    facts: *const rsf_latencyvk_created_device,
) -> i32 {
    let facts = unsafe { &*facts };
    let request = unsafe { &*facts.create_info.cast::<vk::DeviceCreateInfo<'static>>() };
    CALLS.with_borrow_mut(|calls| {
        calls.latency_created = Some((facts.create_info.addr(), request.p_next.addr()));
    });
    0
}

pub(super) unsafe fn rsf_latencyvk_device_destroyed(device: *mut c_void) {
    CALLS.with_borrow_mut(|calls| calls.destroy.push(device.addr()));
}

#[test]
fn typed_creation_negotiates_on_owners_and_preserves_engine_records() {
    assert!(
        crate::vk_creation_resolver(None, true, None, ptr::null_mut()).is_none()
    );
    let begin = |xess| {
        crate::vk_creation_resolver(Some(mock_proc), xess, None, ptr::null_mut()).unwrap()
    };
    let gipa = begin(true);
    assert!(unsafe { gipa(ptr::null_mut(), ptr::null()) }.is_none());
    let create = unsafe {
        std::mem::transmute::<VkFunction, vk::PFN_vkCreateInstance>(
            gipa(ptr::null_mut(), c"vkCreateInstance".as_ptr()).unwrap(),
        )
    };
    let app = made(|app: &mut vk::ApplicationInfo| app.api_version = API_12);
    let instance_names = [c"VK_EXT_fixture".as_ptr()];
    let instance_info = made(|info: &mut vk::InstanceCreateInfo| {
        info.p_application_info = &app;
        info.enabled_extension_count = 1;
        info.pp_enabled_extension_names = instance_names.as_ptr();
    });
    let allocator = vk::AllocationCallbacks::default();
    let mut instance = vk::Instance::null();
    assert!(unsafe { create(&instance_info, &allocator, &mut instance) } == vk::Result::SUCCESS);
    assert_eq!(instance.as_raw() as usize, INSTANCE);
    assert_eq!(app.api_version, API_12);
    assert_eq!(instance_info.p_application_info, &raw const app);
    CALLS.with_borrow(|calls| {
        assert_eq!(
            calls.instance,
            Some((
                API_13,
                vec!["VK_EXT_fixture".into()],
                (&raw const allocator).addr()
            ))
        );
    });

    let mut half =
        made(|half: &mut vk::PhysicalDeviceShaderFloat16Int8Features| half.shader_float16 = 1);
    let timeline = made(
        |timeline: &mut vk::PhysicalDeviceTimelineSemaphoreFeatures| {
            timeline.timeline_semaphore = 1;
            timeline.p_next = (&raw mut half).cast();
        },
    );
    let enabled =
        made(|features: &mut vk::PhysicalDeviceFeatures| features.shader_clip_distance = 1);
    let queue = made(|queue: &mut vk::DeviceQueueCreateInfo| {
        queue.queue_family_index = 4;
        queue.queue_count = 2;
    });
    let device_names = [c"VK_KHR_swapchain".as_ptr()];
    let info = made(|info: &mut vk::DeviceCreateInfo| {
        info.p_next = (&raw const timeline).cast();
        info.p_enabled_features = &enabled;
        info.queue_create_info_count = 1;
        info.p_queue_create_infos = &queue;
        info.enabled_extension_count = 1;
        info.pp_enabled_extension_names = device_names.as_ptr();
    });
    let run = |gipa: VkProc, info: &vk::DeviceCreateInfo| {
        let create = unsafe {
            std::mem::transmute::<VkFunction, vk::PFN_vkCreateDevice>(
                gipa(
                    ptr::without_provenance_mut(instance.as_raw() as usize),
                    c"vkCreateDevice".as_ptr(),
                )
                .unwrap(),
            )
        };
        let mut device = vk::Device::null();
        let status = unsafe {
            create(
                vk::PhysicalDevice::from_raw(PHYSICAL as u64),
                info,
                &allocator,
                &mut device,
            )
        };
        (status, device)
    };
    CALLS.with_borrow_mut(|calls| calls.interop = true);
    let (status, device) = run(gipa, &info);
    assert!(status == vk::Result::SUCCESS && device.as_raw() as usize == DEVICE);
    let receipt = crate::CONTROL.vulkan.creation().device_receipt(DEVICE);
    assert!(receipt.xess && receipt.interop);
    assert_eq!(receipt.adapter.unwrap().vendor, 0x8086);
    CALLS.with_borrow_mut(|calls| {
        let actual = calls.devices.pop().unwrap();
        assert_ne!(actual.chain, info.p_next.addr());
        assert_eq!(
            (actual.queue, actual.allocator, actual.core_features),
            ((&raw const queue).addr(), (&raw const allocator).addr(), 0)
        );
        assert!(actual.root && actual.inherited && actual.sdk_write && actual.individual_features);
        assert_eq!(
            actual.extensions,
            [
                "VK_KHR_swapchain",
                "VK_EXT_mutable_descriptor_type",
                "VK_KHR_external_memory_win32",
                "VK_KHR_external_semaphore_win32",
            ]
        );
    });
    assert_eq!(enabled.shader_storage_image_write_without_format, 0);
    assert_eq!(timeline.p_next, (&raw mut half).cast());
    assert_eq!(half.shader_float16, 1);
    assert!(half.p_next.is_null());

    for refused in [
        vk::Result::from_raw(VK_ERROR_FEATURE_NOT_PRESENT),
        vk::Result::ERROR_EXTENSION_NOT_PRESENT,
    ] {
        CALLS.with_borrow_mut(|calls| {
            calls.interop = false;
            calls.next_result = Some(refused);
        });
        assert!(run(begin(true), &info).0 == vk::Result::SUCCESS);
        CALLS.with_borrow_mut(|calls| {
            assert_eq!(calls.devices.len(), 2);
            assert_eq!(calls.devices[1].request, (&raw const info).addr());
            calls.devices.clear();
        });
    }
    CALLS.with_borrow_mut(|calls| calls.refuse_xess = true);
    assert!(run(begin(true), &info).0 == vk::Result::SUCCESS);
    assert!(!crate::CONTROL.vulkan.creation().xess_ready());
    CALLS.with_borrow_mut(|calls| {
        assert_eq!(
            calls.devices.pop().unwrap().request,
            (&raw const info).addr()
        );
        calls.latency = true;
        calls.next_result = Some(vk::Result::ERROR_EXTENSION_NOT_PRESENT);
    });
    assert!(run(begin(false), &info).0 == vk::Result::SUCCESS);
    CALLS.with_borrow(|calls| {
        assert_ne!(calls.latency_clone, info.p_next.addr());
        assert!(
            calls.devices[0]
                .extensions
                .iter()
                .any(|name| name == "VK_AMD_anti_lag")
        );
        assert_eq!(
            calls.latency_created,
            Some(((&raw const info).addr(), info.p_next.addr()))
        );
    });
    let gdpa = unsafe {
        std::mem::transmute::<VkFunction, VkProc>(
            gipa(
                ptr::without_provenance_mut(instance.as_raw() as usize),
                c"vkGetDeviceProcAddr".as_ptr(),
            )
            .unwrap(),
        )
    };
    let destroy = unsafe {
        std::mem::transmute::<VkFunction, vk::PFN_vkDestroyDevice>(
            gdpa(
                ptr::without_provenance_mut(device.as_raw() as usize),
                c"vkDestroyDevice".as_ptr(),
            )
            .unwrap(),
        )
    };
    unsafe { destroy(device, &allocator) };
    CALLS.with_borrow(|calls| {
        assert_eq!(
            calls.destroy,
            [DEVICE, DEVICE, (&raw const allocator).addr()]
        )
    });
    assert!(
        crate::CONTROL
            .vulkan
            .creation()
            .device_receipt(DEVICE)
            .adapter
            .is_none()
    );
}

// The callee reads these records through the raw pointer it was given.
#[allow(unused_assignments)]
#[test]
fn typed_clone_promotes_existing_features2_without_editing_links() {
    let mut root = vk::PhysicalDeviceFeatures2::default();
    let mut v13 =
        made(|v13: &mut vk::PhysicalDeviceVulkan13Features| v13.p_next = (&raw mut root).cast());
    let v12 = made(|v12: &mut vk::PhysicalDeviceVulkan12Features| {
        v12.p_next = (&raw mut v13).cast();
        v12.timeline_semaphore = 1;
    });
    let info = made(|info: &mut vk::DeviceCreateInfo| info.p_next = (&raw const v12).cast());
    let mut facts = VkChainFacts::default();
    unsafe { chain_facts((&raw const info).cast(), &mut facts) };
    let flags = crate::platform::vulkan_negotiation::xess_structures(&facts).unwrap();
    assert_eq!(flags, 0);
    let mut record = DeviceRecord::default();
    let mut copied = ptr::null_mut();
    assert_eq!(
        unsafe {
            xess_features(
                (&raw mut record).cast(),
                (&raw const info).cast(),
                flags,
                &mut copied,
            )
        },
        1
    );
    assert!(!copied.is_null());
    assert!(
        unsafe { (*copied.cast::<vk::BaseOutStructure<'static>>()).s_type }
            == Tag::PHYSICAL_DEVICE_FEATURES_2
    );
    assert_ne!(copied, (&raw mut root).cast());
    assert_eq!(v12.p_next, (&raw mut v13).cast());
    assert_eq!(v13.p_next, (&raw mut root).cast());
    assert!(root.p_next.is_null());

    v13.s_type = Tag::PHYSICAL_DEVICE_VULKAN_1_2_FEATURES;
    assert!(
        unsafe { Chain::copy(info.p_next.cast()) }
            .unwrap()
            .is_none()
    );
    v13.s_type = Tag::from_raw(0x7ffffffe);
    assert!(
        unsafe { Chain::copy(info.p_next.cast()) }
            .unwrap()
            .is_none()
    );
}
