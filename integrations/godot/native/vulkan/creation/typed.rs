// SPDX-License-Identifier: MIT
//! The loader's typed entry points and borrowed device negotiation operations.

use super::VK_ERROR_INITIALIZATION_FAILED;
use crate::platform::vulkan_negotiation::{
    VK_XESS_ENABLED_FEATURES, VK_XESS_FEATURES2, VK_XESS_FLOAT16_INT8, VK_XESS_INTEGER_DOT_PRODUCT,
    VK_XESS_SCALAR_BLOCK_LAYOUT,
};
use crate::platform::{
    VK_CREATION_VERSION, VK_ROUTE_CREATE_DEVICE, VK_ROUTE_CREATE_INSTANCE, VK_ROUTE_DESTROY_DEVICE,
    VkAdapter, VkCreationLeaves, VkDeviceCall, VkDeviceDerive, VkFunction, VkInstanceCall,
    VkLatencyRequirements, VkProc,
};
use crate::sys::*;
use crate::vulkan::handle;
use ash::vk::{self, Handle};
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
    app: vk::ApplicationInfo<'static>,
    request: vk::InstanceCreateInfo<'static>,
}

#[derive(Default)]
struct DeviceRecord {
    xess: Chain,
    latency: Chain,
    base: vk::PhysicalDeviceFeatures2<'static>,
    scalar: vk::PhysicalDeviceScalarBlockLayoutFeatures<'static>,
    dot: vk::PhysicalDeviceShaderIntegerDotProductFeatures<'static>,
    int8: vk::PhysicalDeviceShaderFloat16Int8Features<'static>,
    stages: [vk::DeviceCreateInfo<'static>; 3],
}

pub(crate) fn entry(route: u32) -> Option<VkFunction> {
    // The loader uses PFN_vkVoidFunction as the transport for named entry points on Win64.
    unsafe {
        match route {
            VK_ROUTE_CREATE_INSTANCE => Some(std::mem::transmute::<
                vk::PFN_vkCreateInstance,
                VkFunction,
            >(create_instance)),
            VK_ROUTE_CREATE_DEVICE => Some(
                std::mem::transmute::<vk::PFN_vkCreateDevice, VkFunction>(create_device),
            ),
            VK_ROUTE_DESTROY_DEVICE => Some(std::mem::transmute::<
                vk::PFN_vkDestroyDevice,
                VkFunction,
            >(destroy_device)),
            _ => None,
        }
    }
}

unsafe extern "system" fn create_instance(
    info: *const vk::InstanceCreateInfo<'_>,
    allocator: *const vk::AllocationCallbacks<'_>,
    output: *mut vk::Instance,
) -> vk::Result {
    let Some(info) = (unsafe { info.as_ref() }) else {
        return vk::Result::from_raw(VK_ERROR_INITIALIZATION_FAILED);
    };
    let mut record = InstanceRecord::default();
    let call = VkInstanceCall {
        call: (&raw mut record).cast(),
        request: ptr::from_ref(info).cast(),
        allocator: allocator.cast(),
        output: output.cast(),
        extensions: info.pp_enabled_extension_names,
        extension_count: info.enabled_extension_count,
    };
    vk::Result::from_raw(
        crate::CONTROL
            .vulkan
            .creation()
            .create_instance(&LEAVES, &call),
    )
}

unsafe extern "system" fn create_device(
    physical: vk::PhysicalDevice,
    info: *const vk::DeviceCreateInfo<'_>,
    allocator: *const vk::AllocationCallbacks<'_>,
    output: *mut vk::Device,
) -> vk::Result {
    let Some(info) = (unsafe { info.as_ref() }) else {
        return vk::Result::from_raw(VK_ERROR_INITIALIZATION_FAILED);
    };
    let mut record = DeviceRecord::default();
    let call = VkDeviceCall {
        call: (&raw mut record).cast(),
        physical_device: ptr::without_provenance_mut(physical.as_raw() as usize),
        request: ptr::from_ref(info).cast(),
        allocator: allocator.cast(),
        output: output.cast(),
        extensions: info.pp_enabled_extension_names,
        extension_count: info.enabled_extension_count,
    };
    vk::Result::from_raw(
        crate::CONTROL
            .vulkan
            .creation()
            .create_device(&LEAVES, &call),
    )
}

unsafe extern "system" fn destroy_device(
    device: vk::Device,
    allocator: *const vk::AllocationCallbacks<'_>,
) {
    crate::CONTROL.vulkan.creation().destroy_device(
        &LEAVES,
        ptr::without_provenance_mut(device.as_raw() as usize),
        allocator.cast(),
    );
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
    let mut request = unsafe { *base.cast::<vk::InstanceCreateInfo<'static>>() };
    record.app = unsafe { request.p_application_info.as_ref() }
        .copied()
        .unwrap_or_default();
    record.app.api_version = record.app.api_version.max(minimum);
    request.p_application_info = &raw const record.app;
    request.enabled_extension_count = count;
    request.pp_enabled_extension_names = extensions;
    record.request = request;
    (&raw const record.request).cast()
}

unsafe extern "C" fn call_create_instance(
    create: VkFunction,
    info: *const c_void,
    allocator: *const c_void,
    output: *mut c_void,
) -> i32 {
    let create = unsafe { std::mem::transmute::<VkFunction, vk::PFN_vkCreateInstance>(create) };
    unsafe { create(info.cast(), allocator.cast(), output.cast()) }.as_raw()
}

unsafe extern "C" fn xess_features(
    call: *mut c_void,
    request: *const c_void,
    flags: u32,
    out: *mut *mut c_void,
) -> u32 {
    let record = unsafe { &mut *call.cast::<DeviceRecord>() };
    let info = unsafe { &*request.cast::<vk::DeviceCreateInfo<'static>>() };
    unsafe { out.write(ptr::null_mut()) };
    record.xess = Chain::default();
    let mut chain = match unsafe { Chain::copy(info.p_next.cast()) } {
        Ok(Some(chain)) => chain,
        Ok(None) => return 1,
        Err(()) => return 0,
    };

    // The record's nodes are default-initialized with their structure types.
    if flags & VK_XESS_ENABLED_FEATURES != 0 && !info.p_enabled_features.is_null() {
        record.base.features = unsafe { *info.p_enabled_features };
        unsafe { chain.prepend((&raw mut record.base).cast()) };
    } else if flags & VK_XESS_FEATURES2 != 0 {
        unsafe { chain.prepend((&raw mut record.base).cast()) };
    }
    if flags & VK_XESS_FLOAT16_INT8 != 0 {
        unsafe { chain.prepend((&raw mut record.int8).cast()) };
    }
    if flags & VK_XESS_SCALAR_BLOCK_LAYOUT != 0 {
        unsafe { chain.prepend((&raw mut record.scalar).cast()) };
    }
    if flags & VK_XESS_INTEGER_DOT_PRODUCT != 0 {
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
        std::mem::transmute::<VkFunction, vk::PFN_vkEnumerateDeviceExtensionProperties>(enumerate)
    };
    let physical = handle::<vk::PhysicalDevice>(physical);
    let mut n = 0;
    if unsafe { enumerate(physical, ptr::null(), &mut n, ptr::null_mut()) } != vk::Result::SUCCESS
        || n > MAX_EXTENSIONS
    {
        return 0;
    }

    let mut available = Vec::new();
    if available.try_reserve_exact(n as usize).is_err() {
        return 0;
    }
    available.resize(n as usize, vk::ExtensionProperties::default());
    let entries = if n == 0 {
        ptr::null_mut()
    } else {
        available.as_mut_ptr()
    };
    if unsafe { enumerate(physical, ptr::null(), &mut n, entries) } != vk::Result::SUCCESS
        || n as usize > available.len()
    {
        return 0;
    }
    available.truncate(n as usize);
    let names = unsafe { super::extension_list(names, count) };
    u32::from(names.iter().all(|&name| {
        !name.is_null()
            && available.iter().any(|entry| {
                entry.extension_name_as_c_str().ok()
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
    let info = unsafe { &*request.cast::<vk::DeviceCreateInfo<'static>>() };
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
    *latency = match unsafe { Chain::copy(info.p_next.cast()) } {
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
    let mut request = unsafe { *derive.base.cast::<vk::DeviceCreateInfo<'static>>() };
    if derive.replace_next != 0 {
        request.p_next = derive.next;
    }
    if derive.clear_enabled_features != 0 {
        request.p_enabled_features = ptr::null();
    }
    request.enabled_extension_count = derive.extension_count;
    request.pp_enabled_extension_names = derive.extensions;
    let stages = unsafe { &raw mut (*call.cast::<DeviceRecord>()).stages };
    if stage >= 3 {
        return ptr::null();
    }
    let slot = unsafe {
        stages
            .cast::<vk::DeviceCreateInfo<'static>>()
            .add(stage as usize)
    };
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
    let create = unsafe { std::mem::transmute::<VkFunction, vk::PFN_vkCreateDevice>(create) };
    unsafe {
        create(
            handle(physical),
            request.cast(),
            allocator.cast(),
            output.cast(),
        )
    }
    .as_raw()
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
    let destroy = unsafe { std::mem::transmute::<VkFunction, vk::PFN_vkDestroyDevice>(destroy) };
    unsafe { destroy(handle(device), allocator.cast()) };
}

unsafe extern "C" fn adapter(
    function: VkFunction,
    physical: *mut c_void,
    out: *mut VkAdapter,
) -> u32 {
    let properties = unsafe {
        std::mem::transmute::<VkFunction, vk::PFN_vkGetPhysicalDeviceProperties>(function)
    };
    let mut value = vk::PhysicalDeviceProperties::default();
    unsafe { properties(handle(physical), &mut value) };
    unsafe {
        out.write(VkAdapter {
            vendor: value.vendor_id,
            device: value.device_id,
            software: u32::from(value.device_type == vk::PhysicalDeviceType::CPU),
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
