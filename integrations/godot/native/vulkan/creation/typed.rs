// SPDX-License-Identifier: MIT
//! The loader's typed entry points and borrowed device negotiation operations.

use super::VK_ERROR_INITIALIZATION_FAILED;
use super::abi::*;
use crate::platform::vulkan_negotiation::{
    VK_XESS_ENABLED_FEATURES, VK_XESS_FEATURES2, VK_XESS_FLOAT16_INT8, VK_XESS_INTEGER_DOT_PRODUCT,
    VK_XESS_SCALAR_BLOCK_LAYOUT,
};
use crate::platform::{
    VK_CREATION_VERSION, VK_ROUTE_CREATE_DEVICE, VK_ROUTE_CREATE_INSTANCE, VK_ROUTE_DESTROY_DEVICE,
    VkAdapter, VkCreationLeaves, VkDeviceCall, VkDeviceDerive, VkFunction, VkInstanceCall,
    VkLatencyRequirements, VkProc,
};
use std::ffi::{c_char, c_void};
use std::ptr;
use std::sync::OnceLock;

mod chain;
use chain::{Chain, chain_facts};

const NOT_SUPPORTED: i32 = -7;
const MAX_EXTENSIONS: u32 = 4096;

/// The linked Runtime services, with the native table version checked at resolver startup.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Requirements {
    size: u32,
    version: u32,
    xess_instance: unsafe extern "C" fn(*mut u32, *mut *const *const c_char, *mut u32) -> i32,
    xess_device: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut u32,
        *mut *const *const c_char,
        *mut *mut c_void,
    ) -> i32,
    latency_require: unsafe extern "C" fn(
        *const rsf_latencyvk_device_query,
        *mut rsf_latencyvk_device_requirements,
    ) -> i32,
    latency_created: unsafe extern "C" fn(*const rsf_latencyvk_created_device) -> i32,
    latency_destroyed: unsafe extern "C" fn(*mut c_void),
}

#[cfg(all(windows, not(test)))]
impl Requirements {
    pub(crate) fn linked() -> Self {
        Self {
            size: size_of::<Self>() as u32,
            version: 1,
            xess_instance: rsf_xessvk_instance_requirements,
            xess_device: rsf_xessvk_device_requirements,
            latency_require: rsf_latencyvk_require_device,
            latency_created: rsf_latencyvk_device_created,
            latency_destroyed: rsf_latencyvk_device_destroyed,
        }
    }
}

const _: () = {
    assert!(size_of::<Requirements>() == 48);
    assert!(std::mem::offset_of!(Requirements, latency_require) == 24);
};

static REQUIREMENTS: OnceLock<Requirements> = OnceLock::new();

/// The caller supplies at least a size word and lends the advertised bytes for this call.
/// The functions stay mapped for the lifetime of the engine's loader resolver.
pub(crate) unsafe fn bind_requirements(table: *const Requirements) -> bool {
    if table.is_null() || unsafe { table.cast::<u32>().read() } < size_of::<Requirements>() as u32 {
        return false;
    }
    let table = unsafe { &*table };
    if table.version != 1 {
        return false;
    }
    let _ = REQUIREMENTS.set(*table);
    true
}

#[derive(Default)]
struct InstanceRecord {
    app: VkApplicationInfo,
    request: VkInstanceCreateInfo,
}

#[derive(Default)]
struct DeviceRecord {
    xess: Chain,
    latency: Chain,
    base: VkPhysicalDeviceFeatures2,
    scalar: VkPhysicalDeviceScalarBlockLayoutFeatures,
    dot: VkPhysicalDeviceShaderIntegerDotProductFeatures,
    int8: VkPhysicalDeviceShaderFloat16Int8Features,
    stages: [VkDeviceCreateInfo; 3],
}

pub(crate) fn entry(route: u32) -> Option<VkFunction> {
    // The loader uses PFN_vkVoidFunction as the transport for named entry points on Win64.
    unsafe {
        match route {
            VK_ROUTE_CREATE_INSTANCE => std::mem::transmute::<
                PFN_vkCreateInstance,
                std::option::Option<unsafe extern "C" fn()>,
            >(Some(create_instance)),
            VK_ROUTE_CREATE_DEVICE => std::mem::transmute::<
                PFN_vkCreateDevice,
                std::option::Option<unsafe extern "C" fn()>,
            >(Some(create_device)),
            VK_ROUTE_DESTROY_DEVICE => std::mem::transmute::<
                PFN_vkDestroyDevice,
                std::option::Option<unsafe extern "C" fn()>,
            >(Some(destroy_device)),
            _ => None,
        }
    }
}

unsafe extern "C" fn create_instance(
    info: *const VkInstanceCreateInfo,
    allocator: *const VkAllocationCallbacks,
    output: *mut VkInstance,
) -> VkResult {
    let Some(info) = (unsafe { info.as_ref() }) else {
        return VK_ERROR_INITIALIZATION_FAILED;
    };
    let mut record = InstanceRecord::default();
    let call = VkInstanceCall {
        call: (&raw mut record).cast(),
        request: ptr::from_ref(info).cast(),
        allocator: allocator.cast(),
        output: output.cast(),
        extensions: info.ppEnabledExtensionNames,
        extension_count: info.enabledExtensionCount,
        reserved: 0,
    };
    crate::CONTROL
        .vulkan
        .creation()
        .create_instance(&LEAVES, &call)
}

unsafe extern "C" fn create_device(
    physical: VkPhysicalDevice,
    info: *const VkDeviceCreateInfo,
    allocator: *const VkAllocationCallbacks,
    output: *mut VkDevice,
) -> VkResult {
    let Some(info) = (unsafe { info.as_ref() }) else {
        return VK_ERROR_INITIALIZATION_FAILED;
    };
    let mut record = DeviceRecord::default();
    let call = VkDeviceCall {
        call: (&raw mut record).cast(),
        physical_device: physical.cast(),
        request: ptr::from_ref(info).cast(),
        allocator: allocator.cast(),
        output: output.cast(),
        extensions: info.ppEnabledExtensionNames,
        extension_count: info.enabledExtensionCount,
        reserved: 0,
    };
    crate::CONTROL
        .vulkan
        .creation()
        .create_device(&LEAVES, &call)
}

unsafe extern "C" fn destroy_device(device: VkDevice, allocator: *const VkAllocationCallbacks) {
    crate::CONTROL
        .vulkan
        .creation()
        .destroy_device(&LEAVES, device.cast(), allocator.cast());
}

unsafe extern "C" fn xess_instance(
    count: *mut u32,
    names: *mut *const *const c_char,
    minimum: *mut u32,
) -> i32 {
    REQUIREMENTS.get().map_or(NOT_SUPPORTED, |ops| unsafe {
        (ops.xess_instance)(count, names, minimum)
    })
}

unsafe extern "C" fn derive_instance(
    call: *mut c_void,
    base: *const c_void,
    minimum: u32,
    extensions: *const *const c_char,
    count: u32,
) -> *const c_void {
    let record = unsafe { &mut *call.cast::<InstanceRecord>() };
    let info = unsafe { *base.cast::<VkInstanceCreateInfo>() };
    record.app = unsafe { info.pApplicationInfo.as_ref() }
        .copied()
        .unwrap_or(VkApplicationInfo {
            sType: TAG_APPLICATION_INFO,
            ..VkApplicationInfo::default()
        });
    record.app.apiVersion = record.app.apiVersion.max(minimum);
    record.request = VkInstanceCreateInfo {
        pApplicationInfo: &raw const record.app,
        enabledExtensionCount: count,
        ppEnabledExtensionNames: extensions,
        ..info
    };
    (&raw const record.request).cast()
}

unsafe extern "C" fn call_create_instance(
    create: VkFunction,
    info: *const c_void,
    allocator: *const c_void,
    output: *mut c_void,
) -> i32 {
    let create = unsafe {
        std::mem::transmute::<std::option::Option<unsafe extern "C" fn()>, PFN_vkCreateInstance>(
            Some(create),
        )
    };
    unsafe {
        create.expect("resolved vkCreateInstance")(info.cast(), allocator.cast(), output.cast())
    }
}

unsafe extern "C" fn xess_features(
    call: *mut c_void,
    request: *const c_void,
    flags: u32,
    out: *mut *mut c_void,
) -> u32 {
    let record = unsafe { &mut *call.cast::<DeviceRecord>() };
    let info = unsafe { &*request.cast::<VkDeviceCreateInfo>() };
    unsafe { out.write(ptr::null_mut()) };
    record.xess = Chain::default();
    let mut chain = match unsafe { Chain::copy(info.pNext.cast()) } {
        Ok(Some(chain)) => chain,
        Ok(None) => return 1,
        Err(()) => return 0,
    };

    record.base = VkPhysicalDeviceFeatures2 {
        sType: TAG_PHYSICAL_DEVICE_FEATURES_2,
        ..Default::default()
    };
    if flags & VK_XESS_ENABLED_FEATURES != 0 && !info.pEnabledFeatures.is_null() {
        record.base.features = unsafe { *info.pEnabledFeatures };
        unsafe { chain.prepend((&raw mut record.base).cast()) };
    } else if flags & VK_XESS_FEATURES2 != 0 {
        unsafe { chain.prepend((&raw mut record.base).cast()) };
    }
    if flags & VK_XESS_FLOAT16_INT8 != 0 {
        record.int8 = VkPhysicalDeviceShaderFloat16Int8Features {
            sType: TAG_PHYSICAL_DEVICE_SHADER_FLOAT16_INT8_FEATURES,
            ..Default::default()
        };
        unsafe { chain.prepend((&raw mut record.int8).cast()) };
    }
    if flags & VK_XESS_SCALAR_BLOCK_LAYOUT != 0 {
        record.scalar = VkPhysicalDeviceScalarBlockLayoutFeatures {
            sType: TAG_PHYSICAL_DEVICE_SCALAR_BLOCK_LAYOUT_FEATURES,
            ..Default::default()
        };
        unsafe { chain.prepend((&raw mut record.scalar).cast()) };
    }
    if flags & VK_XESS_INTEGER_DOT_PRODUCT != 0 {
        record.dot = VkPhysicalDeviceShaderIntegerDotProductFeatures {
            sType: TAG_PHYSICAL_DEVICE_SHADER_INTEGER_DOT_PRODUCT_FEATURES,
            ..Default::default()
        };
        unsafe { chain.prepend((&raw mut record.dot).cast()) };
    }
    unsafe { chain.feature_root_first() };
    unsafe { out.write(chain.head.cast()) };
    record.xess = chain;
    1
}

unsafe extern "C" fn xess_device(
    instance: *mut c_void,
    physical: *mut c_void,
    count: *mut u32,
    names: *mut *const *const c_char,
    features: *mut *mut c_void,
) -> i32 {
    REQUIREMENTS.get().map_or(NOT_SUPPORTED, |ops| unsafe {
        (ops.xess_device)(instance, physical, count, names, features)
    })
}

unsafe extern "C" fn device_extensions(
    enumerate: VkFunction,
    physical: *mut c_void,
    names: *const *const c_char,
    count: u32,
) -> u32 {
    let enumerate = unsafe {
        std::mem::transmute::<
            std::option::Option<unsafe extern "C" fn()>,
            PFN_vkEnumerateDeviceExtensionProperties,
        >(Some(enumerate))
    }
    .expect("resolved vkEnumerateDeviceExtensionProperties");
    let mut n = 0;
    if unsafe { enumerate(physical.cast(), ptr::null(), &mut n, ptr::null_mut()) } != VK_SUCCESS
        || n > MAX_EXTENSIONS
    {
        return 0;
    }

    let mut available = Vec::new();
    if available.try_reserve_exact(n as usize).is_err() {
        return 0;
    }
    available.resize(n as usize, VkExtensionProperties::default());
    let entries = if n == 0 {
        ptr::null_mut()
    } else {
        available.as_mut_ptr()
    };
    if unsafe { enumerate(physical.cast(), ptr::null(), &mut n, entries) } != VK_SUCCESS
        || n as usize > available.len()
    {
        return 0;
    }
    available.truncate(n as usize);
    let names = unsafe { super::extension_list(names, count) };
    u32::from(names.iter().all(|&name| {
        !name.is_null()
            && available.iter().any(|entry| {
                let bytes = entry.extensionName.map(|byte| byte as u8);
                std::ffi::CStr::from_bytes_until_nul(&bytes).ok()
                    == Some(unsafe { std::ffi::CStr::from_ptr(name) })
            })
    }))
}

unsafe extern "C" fn latency_requirements(
    call: *mut c_void,
    instance: *mut c_void,
    physical: *mut c_void,
    proc: Option<VkProc>,
    request: *const c_void,
    out: *mut VkLatencyRequirements,
) -> u32 {
    let info = unsafe { &*request.cast::<VkDeviceCreateInfo>() };
    let latency = unsafe { &mut (*call.cast::<DeviceRecord>()).latency };
    unsafe {
        out.write(VkLatencyRequirements {
            result: NOT_SUPPORTED,
            extension_count: 0,
            extensions: ptr::null(),
            features: ptr::null_mut(),
        })
    };
    *latency = Chain::default();
    let Some(ops) = REQUIREMENTS.get() else {
        return 1;
    };
    *latency = match unsafe { Chain::copy(info.pNext.cast()) } {
        Ok(Some(chain)) => chain,
        Ok(None) => return 1,
        Err(()) => return 0,
    };
    let query = rsf_latencyvk_device_query {
        struct_size: size_of::<rsf_latencyvk_device_query>() as u32,
        abi_version: RSF_LATENCYVK_DEVICE_ABI_VERSION,
        instance,
        physical_device: physical,
        instance_proc: proc,
        features: latency.head.cast(),
    };
    let mut answer = rsf_latencyvk_device_requirements {
        struct_size: size_of::<rsf_latencyvk_device_requirements>() as u32,
        abi_version: RSF_LATENCYVK_DEVICE_ABI_VERSION,
        ..Default::default()
    };
    let result = unsafe { (ops.latency_require)(&query, &mut answer) };
    unsafe {
        out.write(VkLatencyRequirements {
            result,
            extension_count: answer.extension_count,
            extensions: answer.extensions,
            features: answer.features,
        })
    };
    1
}

unsafe extern "C" fn derive_device(
    call: *mut c_void,
    stage: u32,
    derive: *const VkDeviceDerive,
) -> *const c_void {
    let derive = unsafe { &*derive };
    let mut request = unsafe { *derive.base.cast::<VkDeviceCreateInfo>() };
    if derive.replace_next != 0 {
        request.pNext = derive.next;
    }
    if derive.clear_enabled_features != 0 {
        request.pEnabledFeatures = ptr::null();
    }
    request.enabledExtensionCount = derive.extension_count;
    request.ppEnabledExtensionNames = derive.extensions;
    let stages = unsafe { &raw mut (*call.cast::<DeviceRecord>()).stages };
    if stage >= 3 {
        return ptr::null();
    }
    let slot = unsafe { stages.cast::<VkDeviceCreateInfo>().add(stage as usize) };
    unsafe { slot.write(request) };
    slot.cast()
}

unsafe extern "C" fn call_create_device(
    create: VkFunction,
    physical: *mut c_void,
    request: *const c_void,
    allocator: *const c_void,
    output: *mut c_void,
) -> i32 {
    let create = unsafe {
        std::mem::transmute::<std::option::Option<unsafe extern "C" fn()>, PFN_vkCreateDevice>(
            Some(create),
        )
    };
    unsafe {
        create.expect("resolved vkCreateDevice")(
            physical.cast(),
            request.cast(),
            allocator.cast(),
            output.cast(),
        )
    }
}

unsafe extern "C" fn latency_created(
    instance: *mut c_void,
    physical: *mut c_void,
    device: *mut c_void,
    proc: Option<VkProc>,
    request: *const c_void,
) -> i32 {
    let facts = rsf_latencyvk_created_device {
        struct_size: size_of::<rsf_latencyvk_created_device>() as u32,
        abi_version: RSF_LATENCYVK_DEVICE_ABI_VERSION,
        instance,
        physical_device: physical,
        device,
        instance_proc: proc,
        create_info: request,
    };
    REQUIREMENTS.get().map_or(NOT_SUPPORTED, |ops| unsafe {
        (ops.latency_created)(&facts)
    })
}

unsafe extern "C" fn latency_destroyed(device: *mut c_void) {
    if let Some(ops) = REQUIREMENTS.get() {
        unsafe { (ops.latency_destroyed)(device) };
    }
}

unsafe extern "C" fn call_destroy_device(
    destroy: VkFunction,
    device: *mut c_void,
    allocator: *const c_void,
) {
    let destroy = unsafe {
        std::mem::transmute::<std::option::Option<unsafe extern "C" fn()>, PFN_vkDestroyDevice>(
            Some(destroy),
        )
    };
    unsafe { destroy.expect("resolved vkDestroyDevice")(device.cast(), allocator.cast()) };
}

unsafe extern "C" fn adapter(
    function: VkFunction,
    physical: *mut c_void,
    out: *mut VkAdapter,
) -> u32 {
    let properties = unsafe {
        std::mem::transmute::<
            std::option::Option<unsafe extern "C" fn()>,
            PFN_vkGetPhysicalDeviceProperties,
        >(Some(function))
    };
    let mut value = VkPhysicalDeviceProperties::default();
    unsafe {
        properties.expect("resolved vkGetPhysicalDeviceProperties")(physical.cast(), &mut value)
    };
    unsafe {
        out.write(VkAdapter {
            vendor: value.vendorID,
            device: value.deviceID,
            software: u32::from(value.deviceType == VK_PHYSICAL_DEVICE_TYPE_CPU),
            reserved: 0,
        })
    };
    1
}

static LEAVES: VkCreationLeaves = VkCreationLeaves {
    struct_size: size_of::<VkCreationLeaves>() as u32,
    version: VK_CREATION_VERSION,
    xess_instance_requirements: xess_instance,
    derive_instance,
    create_instance: call_create_instance,
    chain_facts,
    xess_features,
    xess_device_requirements: xess_device,
    device_extensions,
    latency_requirements,
    derive_device,
    create_device: call_create_device,
    latency_created,
    latency_destroyed,
    destroy_device: call_destroy_device,
    adapter,
};

#[cfg(test)]
mod tests;
