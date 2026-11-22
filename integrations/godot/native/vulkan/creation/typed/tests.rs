// SPDX-License-Identifier: MIT
use super::super::VK_ERROR_EXTENSION_NOT_PRESENT;
use super::*;
use crate::platform::VkChainFacts;
use crate::platform::vulkan_negotiation::VK_ERROR_FEATURE_NOT_PRESENT;
use std::cell::RefCell;
use std::ffi::CStr;

const INSTANCE: usize = 0x11;
const PHYSICAL: usize = 0x12;
const DEVICE: usize = 0x13;
const API_12: u32 = (1 << 22) | (2 << 12);
const API_13: u32 = (1 << 22) | (3 << 12);

#[derive(Default)]
struct Calls {
    instance: Option<(u32, Vec<String>, usize)>,
    devices: Vec<ObservedDevice>,
    next_result: Option<VkResult>,
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
    static AMD: std::cell::Cell<VkPhysicalDeviceAntiLagFeaturesAMD> = const {
        std::cell::Cell::new(VkPhysicalDeviceAntiLagFeaturesAMD {
            sType: TAG_PHYSICAL_DEVICE_ANTI_LAG_FEATURES_AMD,
            pNext: ptr::null_mut(),
            antiLag: 1,
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

fn feature(mut node: *const VkBaseInStructure, tag: VkStructureType) -> bool {
    for _ in 0..64 {
        let Some(record) = (unsafe { node.as_ref() }) else {
            break;
        };
        if record.sType == tag {
            return true;
        }
        node = record.pNext;
    }
    false
}

unsafe extern "C" fn mock_create_instance(
    info: *const VkInstanceCreateInfo,
    allocator: *const VkAllocationCallbacks,
    out: *mut VkInstance,
) -> VkResult {
    let info = unsafe { &*info };
    let app = unsafe { &*info.pApplicationInfo };
    CALLS.with_borrow_mut(|calls| {
        calls.instance = Some((
            app.apiVersion,
            names(info.ppEnabledExtensionNames, info.enabledExtensionCount),
            allocator.addr(),
        ));
    });
    unsafe { out.write(ptr::without_provenance_mut(INSTANCE)) };
    VK_SUCCESS
}

unsafe extern "C" fn mock_create_device(
    _: VkPhysicalDevice,
    info: *const VkDeviceCreateInfo,
    allocator: *const VkAllocationCallbacks,
    out: *mut VkDevice,
) -> VkResult {
    let request = unsafe { &*info };
    let head = request.pNext.cast::<VkBaseInStructure>();
    let root = unsafe { head.as_ref() }.filter(|node| node.sType == TAG_PHYSICAL_DEVICE_FEATURES_2);
    let features = root.map(|_| unsafe { (*head.cast::<VkPhysicalDeviceFeatures2>()).features });
    let observation = ObservedDevice {
        request: info.addr(),
        chain: head.addr(),
        allocator: allocator.addr(),
        queue: request.pQueueCreateInfos.addr(),
        core_features: request.pEnabledFeatures.addr(),
        root: root.is_some(),
        inherited: features.is_some_and(|value| value.shaderClipDistance != 0),
        sdk_write: features.is_some_and(|value| value.shaderStorageImageWriteWithoutFormat != 0),
        individual_features: feature(head, TAG_PHYSICAL_DEVICE_SCALAR_BLOCK_LAYOUT_FEATURES)
            && feature(
                head,
                TAG_PHYSICAL_DEVICE_SHADER_INTEGER_DOT_PRODUCT_FEATURES,
            ),
        extensions: names(
            request.ppEnabledExtensionNames,
            request.enabledExtensionCount,
        ),
    };
    let result = CALLS.with_borrow_mut(|calls| {
        calls.devices.push(observation);
        calls.next_result.take().unwrap_or(VK_SUCCESS)
    });
    if result == VK_SUCCESS {
        unsafe { out.write(ptr::without_provenance_mut(DEVICE)) };
    }
    result
}

unsafe extern "C" fn mock_enumerate(
    _: VkPhysicalDevice,
    _: *const c_char,
    count: *mut u32,
    entries: *mut VkExtensionProperties,
) -> VkResult {
    if !CALLS.with_borrow(|calls| calls.interop) {
        unsafe { count.write(0) };
        return VK_SUCCESS;
    }
    if !entries.is_null() {
        for (index, name) in [
            c"VK_KHR_external_memory_win32",
            c"VK_KHR_external_semaphore_win32",
        ]
        .iter()
        .enumerate()
        {
            let mut entry = VkExtensionProperties::default();
            for (slot, &byte) in entry.extensionName.iter_mut().zip(name.to_bytes_with_nul()) {
                *slot = byte as c_char;
            }
            unsafe { entries.add(index).write(entry) };
        }
    }
    unsafe { count.write(2) };
    VK_SUCCESS
}

unsafe extern "C" fn mock_properties(_: VkPhysicalDevice, out: *mut VkPhysicalDeviceProperties) {
    unsafe {
        out.write(VkPhysicalDeviceProperties {
            vendorID: 0x8086,
            deviceID: 0x1234,
            ..Default::default()
        })
    };
}

unsafe extern "C" fn mock_destroy(device: VkDevice, allocator: *const VkAllocationCallbacks) {
    CALLS.with_borrow_mut(|calls| calls.destroy.extend([device.addr(), allocator.addr()]));
}

unsafe extern "C" fn mock_device_proc(_: *mut c_void, name: *const c_char) -> Option<VkFunction> {
    if unsafe { CStr::from_ptr(name) } != c"vkDestroyDevice" {
        return None;
    }
    unsafe { std::mem::transmute::<PFN_vkDestroyDevice, _>(Some(mock_destroy)) }
}

unsafe extern "C" fn mock_proc(_: *mut c_void, name: *const c_char) -> Option<VkFunction> {
    if name.is_null() {
        return None;
    }
    unsafe {
        match CStr::from_ptr(name).to_bytes() {
            b"vkCreateInstance" => std::mem::transmute::<
                PFN_vkCreateInstance,
                std::option::Option<unsafe extern "C" fn()>,
            >(Some(mock_create_instance)),
            b"vkCreateDevice" => std::mem::transmute::<
                PFN_vkCreateDevice,
                std::option::Option<unsafe extern "C" fn()>,
            >(Some(mock_create_device)),
            b"vkEnumerateDeviceExtensionProperties" => std::mem::transmute::<
                PFN_vkEnumerateDeviceExtensionProperties,
                std::option::Option<unsafe extern "C" fn()>,
            >(Some(mock_enumerate)),
            b"vkGetPhysicalDeviceProperties" => std::mem::transmute::<
                PFN_vkGetPhysicalDeviceProperties,
                std::option::Option<unsafe extern "C" fn()>,
            >(Some(mock_properties)),
            b"vkGetDeviceProcAddr" => std::mem::transmute::<
                Option<VkProc>,
                std::option::Option<unsafe extern "C" fn()>,
            >(Some(mock_device_proc)),
            _ => None,
        }
    }
}

unsafe extern "C" fn mock_xess_instance(
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

unsafe extern "C" fn mock_xess_device(
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
    let head = unsafe { &mut *chain.read().cast::<VkPhysicalDeviceFeatures2>() };
    if head.sType != TAG_PHYSICAL_DEVICE_FEATURES_2 {
        return NOT_SUPPORTED;
    }
    head.features.shaderStorageImageWriteWithoutFormat = 1;
    if refuse {
        return NOT_SUPPORTED;
    }
    unsafe {
        count.write(3);
        names.write(DEVICE_NAMES.with(|entries| entries.as_ptr()));
    }
    0
}

unsafe extern "C" fn mock_latency_require(
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
        node.set(VkPhysicalDeviceAntiLagFeaturesAMD {
            pNext: query.features,
            ..node.get()
        });
        node.as_ptr().cast()
    });
    unsafe { out.write(answer) };
    0
}

unsafe extern "C" fn mock_latency_created(facts: *const rsf_latencyvk_created_device) -> i32 {
    let facts = unsafe { &*facts };
    let request = unsafe { &*facts.create_info.cast::<VkDeviceCreateInfo>() };
    CALLS.with_borrow_mut(|calls| {
        calls.latency_created = Some((facts.create_info.addr(), request.pNext.addr()));
    });
    0
}

unsafe extern "C" fn mock_latency_destroyed(device: *mut c_void) {
    CALLS.with_borrow_mut(|calls| calls.destroy.push(device.addr()));
}

static SERVICES: Requirements = Requirements {
    size: size_of::<Requirements>() as u32,
    version: 1,
    xess_instance: mock_xess_instance,
    xess_device: mock_xess_device,
    latency_require: mock_latency_require,
    latency_created: mock_latency_created,
    latency_destroyed: mock_latency_destroyed,
};

#[test]
fn typed_creation_negotiates_on_owners_and_preserves_engine_records() {
    assert!(
        unsafe { crate::rsf_godot_vk_creation_resolver(None, 1, None, ptr::null_mut(), &SERVICES) }
            .is_none()
    );
    let begin = |xess| unsafe {
        crate::rsf_godot_vk_creation_resolver(
            Some(mock_proc),
            xess,
            None,
            ptr::null_mut(),
            &SERVICES,
        )
        .unwrap()
    };
    let gipa = begin(1);
    assert!(unsafe { gipa(ptr::null_mut(), ptr::null()) }.is_none());
    let create = unsafe {
        std::mem::transmute::<std::option::Option<unsafe extern "C" fn()>, PFN_vkCreateInstance>(
            gipa(ptr::null_mut(), c"vkCreateInstance".as_ptr()),
        )
    }
    .unwrap();
    let app = VkApplicationInfo {
        sType: TAG_APPLICATION_INFO,
        apiVersion: API_12,
        ..Default::default()
    };
    let instance_names = [c"VK_EXT_fixture".as_ptr()];
    let instance_info = VkInstanceCreateInfo {
        sType: TAG_INSTANCE_CREATE_INFO,
        pApplicationInfo: &app,
        enabledExtensionCount: 1,
        ppEnabledExtensionNames: instance_names.as_ptr(),
        ..Default::default()
    };
    let allocator = VkAllocationCallbacks::default();
    let mut instance = ptr::null_mut();
    assert_eq!(
        unsafe { create(&instance_info, &allocator, &mut instance) },
        VK_SUCCESS
    );
    assert_eq!(instance.addr(), INSTANCE);
    assert_eq!(app.apiVersion, API_12);
    assert_eq!(instance_info.pApplicationInfo, &raw const app);
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

    let mut half = VkPhysicalDeviceShaderFloat16Int8Features {
        sType: TAG_PHYSICAL_DEVICE_SHADER_FLOAT16_INT8_FEATURES,
        shaderFloat16: 1,
        ..Default::default()
    };
    let timeline = VkPhysicalDeviceTimelineSemaphoreFeatures {
        sType: TAG_PHYSICAL_DEVICE_TIMELINE_SEMAPHORE_FEATURES,
        timelineSemaphore: 1,
        pNext: (&raw mut half).cast(),
    };
    let enabled = VkPhysicalDeviceFeatures {
        shaderClipDistance: 1,
        ..Default::default()
    };
    let queue = VkDeviceQueueCreateInfo {
        sType: TAG_DEVICE_QUEUE_CREATE_INFO,
        queueFamilyIndex: 4,
        queueCount: 2,
        ..Default::default()
    };
    let device_names = [c"VK_KHR_swapchain".as_ptr()];
    let info = VkDeviceCreateInfo {
        sType: TAG_DEVICE_CREATE_INFO,
        pNext: (&raw const timeline).cast(),
        pEnabledFeatures: &enabled,
        queueCreateInfoCount: 1,
        pQueueCreateInfos: &queue,
        enabledExtensionCount: 1,
        ppEnabledExtensionNames: device_names.as_ptr(),
        ..Default::default()
    };
    let run = |gipa: VkProc, info: &VkDeviceCreateInfo| {
        let create = unsafe {
            std::mem::transmute::<std::option::Option<unsafe extern "C" fn()>, PFN_vkCreateDevice>(
                gipa(instance.cast(), c"vkCreateDevice".as_ptr()),
            )
        }
        .unwrap();
        let mut device = ptr::null_mut();
        let status = unsafe {
            create(
                ptr::without_provenance_mut(PHYSICAL),
                info,
                &allocator,
                &mut device,
            )
        };
        (status, device)
    };
    CALLS.with_borrow_mut(|calls| calls.interop = true);
    let (status, device) = run(gipa, &info);
    assert_eq!((status, device.addr()), (VK_SUCCESS, DEVICE));
    let receipt = crate::CONTROL.vulkan.creation().device_receipt(DEVICE);
    assert!(receipt.xess && receipt.interop);
    assert_eq!(receipt.adapter.unwrap().vendor, 0x8086);
    CALLS.with_borrow_mut(|calls| {
        let actual = calls.devices.pop().unwrap();
        assert_ne!(actual.chain, info.pNext.addr());
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
    assert_eq!(enabled.shaderStorageImageWriteWithoutFormat, 0);
    assert_eq!(timeline.pNext, (&raw mut half).cast());
    assert_eq!(half.shaderFloat16, 1);
    assert!(half.pNext.is_null());

    for refused in [VK_ERROR_FEATURE_NOT_PRESENT, VK_ERROR_EXTENSION_NOT_PRESENT] {
        CALLS.with_borrow_mut(|calls| {
            calls.interop = false;
            calls.next_result = Some(refused);
        });
        assert_eq!(run(begin(1), &info).0, VK_SUCCESS);
        CALLS.with_borrow_mut(|calls| {
            assert_eq!(calls.devices.len(), 2);
            assert_eq!(calls.devices[1].request, (&raw const info).addr());
            calls.devices.clear();
        });
    }
    CALLS.with_borrow_mut(|calls| calls.refuse_xess = true);
    assert_eq!(run(begin(1), &info).0, VK_SUCCESS);
    assert!(!crate::CONTROL.vulkan.creation().xess_ready());
    CALLS.with_borrow_mut(|calls| {
        assert_eq!(
            calls.devices.pop().unwrap().request,
            (&raw const info).addr()
        );
        calls.latency = true;
        calls.next_result = Some(VK_ERROR_EXTENSION_NOT_PRESENT);
    });
    assert_eq!(run(begin(0), &info).0, VK_SUCCESS);
    CALLS.with_borrow(|calls| {
        assert_ne!(calls.latency_clone, info.pNext.addr());
        assert!(
            calls.devices[0]
                .extensions
                .iter()
                .any(|name| name == "VK_AMD_anti_lag")
        );
        assert_eq!(
            calls.latency_created,
            Some(((&raw const info).addr(), info.pNext.addr()))
        );
    });
    let gdpa = unsafe {
        std::mem::transmute::<std::option::Option<unsafe extern "C" fn()>, Option<VkProc>>(gipa(
            instance.cast(),
            c"vkGetDeviceProcAddr".as_ptr(),
        ))
    }
    .unwrap();
    let destroy = unsafe {
        std::mem::transmute::<std::option::Option<unsafe extern "C" fn()>, PFN_vkDestroyDevice>(
            gdpa(device.cast(), c"vkDestroyDevice".as_ptr()),
        )
    }
    .unwrap();
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
    let mut root = VkPhysicalDeviceFeatures2 {
        sType: TAG_PHYSICAL_DEVICE_FEATURES_2,
        ..Default::default()
    };
    let mut v13 = VkPhysicalDeviceVulkan13Features {
        sType: TAG_PHYSICAL_DEVICE_VULKAN_1_3_FEATURES,
        pNext: (&raw mut root).cast(),
        ..Default::default()
    };
    let v12 = VkPhysicalDeviceVulkan12Features {
        sType: TAG_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES,
        pNext: (&raw mut v13).cast(),
        timelineSemaphore: 1,
        ..Default::default()
    };
    let info = VkDeviceCreateInfo {
        pNext: (&raw const v12).cast(),
        ..Default::default()
    };
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
    assert_eq!(
        unsafe { (*copied.cast::<VkBaseOutStructure>()).sType },
        TAG_PHYSICAL_DEVICE_FEATURES_2
    );
    assert_ne!(copied, (&raw mut root).cast());
    assert_eq!(v12.pNext, (&raw mut v13).cast());
    assert_eq!(v13.pNext, (&raw mut root).cast());
    assert!(root.pNext.is_null());

    v13.sType = TAG_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES;
    assert!(unsafe { Chain::copy(info.pNext.cast()) }.unwrap().is_none());
    v13.sType = 0x7ffffffe;
    assert!(unsafe { Chain::copy(info.pNext.cast()) }.unwrap().is_none());
}
