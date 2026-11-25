// SPDX-License-Identifier: MIT
//! The Vulkan host's device creation. `resolver` wraps the engine loader's `vkGetInstanceProcAddr` and
//! intercepts `vkCreateInstance`, `vkCreateDevice` and `vkDestroyDevice`. A prepared XeSS session
//! adds the SDK's instance and device requirements to private copies of the engine's requests; the
//! device request then gets the Win32 interop extensions when the device has them and the
//! Runtime's optional latency requirements. A driver refusal of an optional stage retries the
//! request without it. Each successful device keeps a receipt of the stages it accepted and its
//! adapter, which provider admission reads for the rendering device. No lock is held across a
//! loader call.

use crate::sys::*;
use ash::vk::{self, Handle, StructureType};
use rsf_game_sys::backend_result;
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_game_sys::rsf_game_log_fn;
use rsf_game_sys::vulkan_extensions::{WIN32_INTEROP, merge};
use std::ffi::{CStr, CString, c_char, c_void};
use std::mem::transmute;
use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

mod chain;
use chain::Chain;

// The Runtime's XeSS and latency services (crate::sys); tests.rs mocks them.
#[cfg(test)]
use tests::{
    rsf_latencyvk_device_created, rsf_latencyvk_device_destroyed, rsf_latencyvk_require_device,
    rsf_xessvk_device_requirements, rsf_xessvk_instance_requirements,
};

/// `PFN_vkVoidFunction`.
pub(crate) type VkFunction = unsafe extern "C" fn();
/// `rsf_vk_get_proc`: `vkGetInstanceProcAddr` or `vkGetDeviceProcAddr`.
pub(crate) type VkProc = unsafe extern "C" fn(*mut c_void, *const c_char) -> Option<VkFunction>;

const MAX_EXTENSIONS: u32 = 4096;

/// The created device's physical device, as `vkGetPhysicalDeviceProperties` read it. `software`
/// is a CPU device type.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Adapter {
    pub(crate) vendor: u32,
    pub(crate) device: u32,
    pub(crate) software: u32,
}

/// What one successful `vkCreateDevice` accepted.
#[derive(Clone, Copy, Default)]
pub(crate) struct DeviceReceipt {
    pub(crate) xess: bool,
    pub(crate) interop: bool,
    pub(crate) adapter: Option<Adapter>,
}

#[derive(Clone, Copy)]
struct Binding {
    underlying: Option<VkProc>,
    log: rsf_game_log_fn,
    log_user: usize,
}

static BINDING: Mutex<Binding> = Mutex::new(Binding {
    underlying: None,
    log: None,
    log_user: 0,
});
/// The instance `vkCreateInstance` created or `vkCreateDevice` was resolved for.
static OWNER: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static PREPARE_XESS: AtomicBool = AtomicBool::new(false);
static RECEIPTS: Mutex<Vec<(usize, DeviceReceipt)>> = Mutex::new(Vec::new());
static RENDER_DEVICE: AtomicUsize = AtomicUsize::new(0);

fn binding() -> Binding {
    *lock_or_recover(&BINDING)
}

fn owner() -> *mut c_void {
    OWNER.load(Ordering::Acquire)
}

fn diagnostic(message: &CStr) {
    let binding = binding();
    if let Some(log) = binding.log {
        // SAFETY: the engine's logger lives through the session's setup contract.
        unsafe { log(binding.log_user as *mut c_void, message.as_ptr()) };
    }
}

/// `name` of the owner instance through the underlying resolver.
fn owner_proc(underlying: VkProc, name: &CStr) -> Option<VkFunction> {
    // SAFETY: the underlying resolver is Streamline's or the loader's vkGetInstanceProcAddr.
    unsafe { underlying(owner(), name.as_ptr()) }
}

/// The owner instance's `vkGetDeviceProcAddr`.
fn owner_device_proc(underlying: VkProc) -> Option<VkProc> {
    // SAFETY: vkGetDeviceProcAddr has the rsf_vk_get_proc signature.
    owner_proc(underlying, c"vkGetDeviceProcAddr")
        .map(|get| unsafe { transmute::<VkFunction, VkProc>(get) })
}

/// The creation resolver over `underlying` for a new Vulkan session, or `None` without one. The
/// owner instance survives; readiness starts again.
pub(crate) fn resolver(
    underlying: Option<VkProc>,
    xess: bool,
    log: rsf_game_log_fn,
    log_user: *mut c_void,
) -> Option<VkProc> {
    *lock_or_recover(&BINDING) = Binding {
        underlying,
        log,
        log_user: log_user as usize,
    };
    PREPARE_XESS.store(xess, Ordering::Release);
    reset_readiness();
    underlying.map(|_| instance_proc as VkProc)
}

/// The session retired XeSS. The resolver keeps forwarding for the engine's teardown.
pub(crate) fn stop() {
    PREPARE_XESS.store(false, Ordering::Release);
    reset_readiness();
}

fn reset_readiness() {
    RENDER_DEVICE.store(0, Ordering::Release);
    for (_, receipt) in lock_or_recover(&RECEIPTS).iter_mut() {
        receipt.xess = false;
        receipt.interop = false;
    }
}

/// The device the primary swapchain or an SR pass renders with.
pub(crate) fn report_render_device(device: *mut c_void) {
    RENDER_DEVICE.store(device.addr(), Ordering::Release);
}

pub(crate) fn render_receipt() -> DeviceReceipt {
    device_receipt(RENDER_DEVICE.load(Ordering::Acquire))
}

pub(crate) fn device_receipt(device: usize) -> DeviceReceipt {
    let receipts = lock_or_recover(&RECEIPTS);
    // Before a primary pass or swapchain identifies its device, the sole successful creation is
    // unambiguous. Several candidates need an explicit device report.
    if device == 0 && receipts.len() == 1 {
        return receipts[0].1;
    }
    receipts
        .iter()
        .find(|(owner, _)| *owner == device)
        .map_or_else(DeviceReceipt::default, |(_, receipt)| *receipt)
}

/// An extension list as a slice; empty for a zero count or a null list.
///
/// # Safety
/// `names` is null or readable for `count` entries for the returned lifetime.
unsafe fn extension_list<'a>(names: *const *const c_char, count: u32) -> &'a [*const c_char] {
    if count == 0 || names.is_null() {
        &[]
    } else {
        // SAFETY: the caller's guarantee.
        unsafe { std::slice::from_raw_parts(names, count as usize) }
    }
}

/// A create result for which a refused optional requirement is retried without it.
fn refused(result: vk::Result) -> bool {
    result == vk::Result::ERROR_FEATURE_NOT_PRESENT
        || result == vk::Result::ERROR_EXTENSION_NOT_PRESENT
}

/// The resolver handed to the engine: the three intercepted entries, everything else forwarded.
unsafe extern "C" fn instance_proc(
    instance: *mut c_void,
    name: *const c_char,
) -> Option<VkFunction> {
    let underlying = binding().underlying?;
    if name.is_null() {
        return None;
    }
    // SAFETY: a non-null name is NUL-terminated for the call. The loader uses PFN_vkVoidFunction
    // as the transport for named entry points on Win64.
    unsafe {
        match CStr::from_ptr(name).to_bytes() {
            b"vkCreateInstance" => Some(transmute::<vk::PFN_vkCreateInstance, VkFunction>(
                create_instance,
            )),
            b"vkGetDeviceProcAddr" => Some(transmute::<VkProc, VkFunction>(device_proc)),
            b"vkCreateDevice" => {
                if !instance.is_null() {
                    OWNER.store(instance, Ordering::Release);
                }
                Some(transmute::<vk::PFN_vkCreateDevice, VkFunction>(
                    create_device,
                ))
            }
            _ => underlying(instance, name),
        }
    }
}

/// `vkGetDeviceProcAddr`: `vkDestroyDevice` is intercepted; other names, null included, go to the
/// owner instance's resolver unchanged.
unsafe extern "C" fn device_proc(device: *mut c_void, name: *const c_char) -> Option<VkFunction> {
    let underlying = binding().underlying?;
    // SAFETY: a non-null name is NUL-terminated for the call.
    if !name.is_null() && unsafe { CStr::from_ptr(name) } == c"vkDestroyDevice" {
        return Some(unsafe { transmute::<vk::PFN_vkDestroyDevice, VkFunction>(destroy_device) });
    }
    unsafe { owner_device_proc(underlying)?(device, name) }
}

/// The intercepted `vkCreateInstance`: the XeSS instance extensions and minimum API version when
/// XeSS is prepared, and the engine's own request when the driver refuses them. A refusal ends
/// XeSS preparation for the session.
unsafe extern "system" fn create_instance(
    info: *const vk::InstanceCreateInfo<'_>,
    allocator: *const vk::AllocationCallbacks<'_>,
    output: *mut vk::Instance,
) -> vk::Result {
    let create = binding().underlying.and_then(|underlying| unsafe {
        underlying(ptr::null_mut(), c"vkCreateInstance".as_ptr())
    });
    let info = info.cast::<vk::InstanceCreateInfo<'static>>();
    let (Some(create), Some(info)) = (create, unsafe { info.as_ref() }) else {
        return vk::Result::ERROR_INITIALIZATION_FAILED;
    };
    if output.is_null()
        || (info.enabled_extension_count != 0 && info.pp_enabled_extension_names.is_null())
    {
        return vk::Result::ERROR_INITIALIZATION_FAILED;
    }
    let create = unsafe { transmute::<VkFunction, vk::PFN_vkCreateInstance>(create) };
    let mut app = vk::ApplicationInfo::default();
    let merged;
    let mut request = *info;
    if PREPARE_XESS.load(Ordering::Acquire) {
        let (mut count, mut minimum) = (0, 0);
        let mut names: *const *const c_char = ptr::null();
        if unsafe { rsf_xessvk_instance_requirements(&mut count, &mut names, &mut minimum) }
            == backend_result::OK
        {
            // SAFETY: the engine's list lives for the call; the SDK's names until its shutdown.
            let Some(list) = (unsafe {
                merge(
                    extension_list(
                        info.pp_enabled_extension_names,
                        info.enabled_extension_count,
                    ),
                    extension_list(names, count),
                )
            }) else {
                return vk::Result::ERROR_OUT_OF_HOST_MEMORY;
            };
            merged = list;
            if let Some(engine) = unsafe { info.p_application_info.as_ref() } {
                app = *engine;
            }
            app.api_version = app.api_version.max(minimum);
            request.p_application_info = &raw const app;
            request.enabled_extension_count = merged.len() as u32;
            request.pp_enabled_extension_names = merged.as_ptr();
        } else {
            PREPARE_XESS.store(false, Ordering::Release);
        }
    }
    let mut result = unsafe { create(&request, allocator, output) };
    if PREPARE_XESS.load(Ordering::Acquire)
        && (result == vk::Result::ERROR_EXTENSION_NOT_PRESENT
            || result == vk::Result::ERROR_INCOMPATIBLE_DRIVER)
    {
        PREPARE_XESS.store(false, Ordering::Release);
        diagnostic(
            c"XeSS Vulkan instance requirements unavailable; retrying the original engine instance request.",
        );
        result = unsafe { create(info, allocator, output) };
    }
    if result == vk::Result::SUCCESS {
        // SAFETY: a successful create wrote the dispatchable VkInstance handle.
        let instance = unsafe { output.read() };
        OWNER.store(
            ptr::without_provenance_mut(instance.as_raw() as usize),
            Ordering::Release,
        );
    }
    result
}

/// One intercepted `vkCreateDevice` and the stages it accepted.
struct DeviceCall {
    instance: *mut c_void,
    underlying: Option<VkProc>,
    physical: vk::PhysicalDevice,
    allocator: *const vk::AllocationCallbacks<'static>,
    output: *mut vk::Device,
    xess: bool,
    interop: bool,
}

/// The intercepted `vkCreateDevice`. With XeSS prepared, the engine's chain is cloned with the
/// structures XeSS needs and patched by the SDK; any refusal, of the clone, the SDK or the
/// driver's feature or extension check, creates the engine's own request instead.
unsafe extern "system" fn create_device(
    physical: vk::PhysicalDevice,
    info: *const vk::DeviceCreateInfo<'_>,
    allocator: *const vk::AllocationCallbacks<'_>,
    output: *mut vk::Device,
) -> vk::Result {
    let info = info.cast::<vk::DeviceCreateInfo<'static>>();
    let Some(info) = (unsafe { info.as_ref() }) else {
        return vk::Result::ERROR_INITIALIZATION_FAILED;
    };
    if output.is_null()
        || (info.enabled_extension_count != 0 && info.pp_enabled_extension_names.is_null())
    {
        return vk::Result::ERROR_INITIALIZATION_FAILED;
    }
    let mut call = DeviceCall {
        instance: owner(),
        underlying: binding().underlying,
        physical,
        allocator: allocator.cast(),
        output,
        xess: false,
        interop: false,
    };
    let result = if PREPARE_XESS.load(Ordering::Acquire) {
        unsafe { call.create_xess(info) }
    } else {
        unsafe { call.create_interop(info) }
    };
    if result == vk::Result::SUCCESS {
        // SAFETY: a successful create wrote the dispatchable VkDevice handle.
        let device = unsafe { output.read() }.as_raw() as usize;
        if device != 0 {
            let receipt = DeviceReceipt {
                xess: call.xess,
                interop: call.interop,
                adapter: read_adapter(physical),
            };
            let mut receipts = lock_or_recover(&RECEIPTS);
            receipts.retain(|(owner, _)| *owner != device);
            receipts.push((device, receipt));
        }
    }
    result
}

impl DeviceCall {
    fn proc(&self, name: &CStr) -> Option<VkFunction> {
        // SAFETY: the engine lends the original resolver; name lives through the call.
        self.underlying
            .and_then(|underlying| unsafe { underlying(self.instance, name.as_ptr()) })
    }

    /// The XeSS stage.
    unsafe fn create_xess(&mut self, info: &vk::DeviceCreateInfo<'static>) -> vk::Result {
        let fallback = |call: &mut Self| {
            diagnostic(c"XeSS Vulkan requirements unavailable; preserving the original engine device request.");
            unsafe { call.create_interop(info) }
        };
        let mut chain = match unsafe { Chain::copy(info.p_next.cast()) } {
            Ok(Some(chain)) => chain,
            Ok(None) => return fallback(self),
            Err(()) => return vk::Result::ERROR_OUT_OF_HOST_MEMORY,
        };
        // The injected nodes live through the create.
        let mut base = vk::PhysicalDeviceFeatures2::default();
        let mut scalar = vk::PhysicalDeviceScalarBlockLayoutFeatures::default();
        let mut dot = vk::PhysicalDeviceShaderIntegerDotProductFeatures::default();
        let mut int8 = vk::PhysicalDeviceShaderFloat16Int8Features::default();
        if let Some(enabled) = unsafe { info.p_enabled_features.as_ref() } {
            // pEnabledFeatures and a Features2 in the chain are mutually exclusive.
            if chain.contains(StructureType::PHYSICAL_DEVICE_FEATURES_2) {
                return fallback(self);
            }
            base.features = *enabled;
            unsafe { chain.prepend((&raw mut base).cast()) };
        } else if !chain.contains(StructureType::PHYSICAL_DEVICE_FEATURES_2) {
            unsafe { chain.prepend((&raw mut base).cast()) };
        }
        // The SDK otherwise appends Vulkan 1.2 and 1.3 aggregates, which conflict with the
        // engine's individual promoted feature structures.
        let vulkan12 = chain.contains(StructureType::PHYSICAL_DEVICE_VULKAN_1_2_FEATURES);
        let vulkan13 = chain.contains(StructureType::PHYSICAL_DEVICE_VULKAN_1_3_FEATURES);
        if !vulkan12 && !chain.contains(StructureType::PHYSICAL_DEVICE_SHADER_FLOAT16_INT8_FEATURES)
        {
            unsafe { chain.prepend((&raw mut int8).cast()) };
        }
        if !vulkan12 && !chain.contains(StructureType::PHYSICAL_DEVICE_SCALAR_BLOCK_LAYOUT_FEATURES)
        {
            unsafe { chain.prepend((&raw mut scalar).cast()) };
        }
        if !vulkan13
            && !chain.contains(StructureType::PHYSICAL_DEVICE_SHADER_INTEGER_DOT_PRODUCT_FEATURES)
        {
            unsafe { chain.prepend((&raw mut dot).cast()) };
        }
        unsafe { chain.feature_root_first() };
        let mut features: *mut c_void = chain.head.cast();
        let mut count = 0;
        let mut names: *const *const c_char = ptr::null();
        let physical = ptr::without_provenance_mut(self.physical.as_raw() as usize);
        if unsafe {
            rsf_xessvk_device_requirements(
                self.instance,
                physical,
                &mut count,
                &mut names,
                &mut features,
            )
        } != backend_result::OK
        {
            return fallback(self);
        }
        // SAFETY: the engine's list lives for the call; the SDK's names until its shutdown.
        let Some(merged) = (unsafe {
            merge(
                extension_list(
                    info.pp_enabled_extension_names,
                    info.enabled_extension_count,
                ),
                extension_list(names, count),
            )
        }) else {
            return vk::Result::ERROR_OUT_OF_HOST_MEMORY;
        };
        let mut request = *info;
        request.p_next = features;
        request.p_enabled_features = ptr::null();
        request.enabled_extension_count = merged.len() as u32;
        request.pp_enabled_extension_names = merged.as_ptr();
        let result = unsafe { self.create_interop(&request) };
        if refused(result) {
            return fallback(self);
        }
        if result == vk::Result::SUCCESS {
            self.xess = true;
            diagnostic(c"XeSS Vulkan device requirements enabled before device creation.");
        }
        result
    }

    /// The interop stage: when the device lists both Win32 extensions and the request enables
    /// timeline semaphores, they are added; a refusal retries the request unchanged.
    unsafe fn create_interop(&mut self, request: &vk::DeviceCreateInfo<'static>) -> vk::Result {
        let names = WIN32_INTEROP.map(CStr::as_ptr);
        let available = self
            .proc(c"vkEnumerateDeviceExtensionProperties")
            .is_some_and(|enumerate| unsafe { self.device_has(enumerate, &names) });
        if !available || !unsafe { timeline(request) } {
            return unsafe { self.create_latency(request) };
        }
        // SAFETY: the request's list lives for the call.
        let Some(merged) = (unsafe {
            merge(
                extension_list(
                    request.pp_enabled_extension_names,
                    request.enabled_extension_count,
                ),
                &names,
            )
        }) else {
            return vk::Result::ERROR_OUT_OF_HOST_MEMORY;
        };
        let mut interop = *request;
        interop.enabled_extension_count = merged.len() as u32;
        interop.pp_enabled_extension_names = merged.as_ptr();
        let result = unsafe { self.create_latency(&interop) };
        if refused(result) {
            diagnostic(c"Vulkan Win32 interop requirements unavailable; retrying the unchanged device request.");
            return unsafe { self.create_latency(request) };
        }
        if result == vk::Result::SUCCESS {
            self.interop = true;
            diagnostic(c"Vulkan Win32 memory/semaphore interop enabled before device creation.");
        }
        result
    }

    /// The physical device lists every extension in `names`.
    unsafe fn device_has(&self, enumerate: VkFunction, names: &[*const c_char]) -> bool {
        let enumerate = unsafe {
            transmute::<VkFunction, vk::PFN_vkEnumerateDeviceExtensionProperties>(enumerate)
        };
        let mut count = 0;
        if unsafe { enumerate(self.physical, ptr::null(), &mut count, ptr::null_mut()) }
            != vk::Result::SUCCESS
            || count > MAX_EXTENSIONS
        {
            return false;
        }
        let mut available = Vec::new();
        if available.try_reserve_exact(count as usize).is_err() {
            return false;
        }
        available.resize(count as usize, vk::ExtensionProperties::default());
        if unsafe {
            enumerate(
                self.physical,
                ptr::null(),
                &mut count,
                available.as_mut_ptr(),
            )
        } != vk::Result::SUCCESS
            || count as usize > available.len()
        {
            return false;
        }
        available.truncate(count as usize);
        names.iter().all(|&name| {
            available.iter().any(|entry| {
                entry.extension_name_as_c_str().ok() == Some(unsafe { CStr::from_ptr(name) })
            })
        })
    }

    /// The core latency stage and the native create: the Runtime's optional latency extensions
    /// and features, retried without them when refused, and the created device reported to it.
    unsafe fn create_latency(&mut self, request: &vk::DeviceCreateInfo<'static>) -> vk::Result {
        let Some(create) = self.proc(c"vkCreateDevice") else {
            return vk::Result::ERROR_INITIALIZATION_FAILED;
        };
        let create = unsafe { transmute::<VkFunction, vk::PFN_vkCreateDevice>(create) };
        let physical = ptr::without_provenance_mut(self.physical.as_raw() as usize);
        // The Runtime patches its own copy of the chain, which lives through the create.
        let latency = match unsafe { Chain::copy(request.p_next.cast()) } {
            Ok(chain) => chain,
            Err(()) => return vk::Result::ERROR_OUT_OF_HOST_MEMORY,
        };
        let merged;
        let mut derived = *request;
        let mut actual = request;
        if let Some(chain) = &latency {
            let query = rsf_latencyvk_device_query {
                struct_size: size_of::<rsf_latencyvk_device_query>() as u32,
                abi_version: RSF_LATENCYVK_DEVICE_ABI_VERSION,
                instance: self.instance,
                physical_device: physical,
                instance_proc: self.underlying,
                features: chain.head.cast(),
            };
            let mut answer = rsf_latencyvk_device_requirements {
                struct_size: size_of::<rsf_latencyvk_device_requirements>() as u32,
                abi_version: RSF_LATENCYVK_DEVICE_ABI_VERSION,
                ..Default::default()
            };
            if unsafe { rsf_latencyvk_require_device(&query, &mut answer) } == backend_result::OK
                && answer.extension_count != 0
            {
                // SAFETY: the Runtime's names stay valid through the synchronous create.
                let Some(list) = (unsafe {
                    merge(
                        extension_list(
                            request.pp_enabled_extension_names,
                            request.enabled_extension_count,
                        ),
                        extension_list(answer.extensions, answer.extension_count),
                    )
                }) else {
                    return vk::Result::ERROR_OUT_OF_HOST_MEMORY;
                };
                merged = list;
                derived.p_next = answer.features;
                derived.enabled_extension_count = merged.len() as u32;
                derived.pp_enabled_extension_names = merged.as_ptr();
                actual = &derived;
            }
        }
        let mut result = unsafe { create(self.physical, actual, self.allocator, self.output) };
        if !ptr::eq(actual, request) && refused(result) {
            diagnostic(c"Optional core latency device requirements refused; retrying the caller's unchanged request.");
            actual = request;
            result = unsafe { create(self.physical, actual, self.allocator, self.output) };
        }
        if result == vk::Result::SUCCESS {
            // SAFETY: a successful create wrote the dispatchable VkDevice handle.
            let device = unsafe { self.output.read() }.as_raw() as usize;
            let facts = rsf_latencyvk_created_device {
                struct_size: size_of::<rsf_latencyvk_created_device>() as u32,
                abi_version: RSF_LATENCYVK_DEVICE_ABI_VERSION,
                instance: self.instance,
                physical_device: physical,
                device: ptr::without_provenance_mut(device),
                instance_proc: self.underlying,
                create_info: ptr::from_ref(actual).cast(),
            };
            if unsafe { rsf_latencyvk_device_created(&facts) } == backend_result::OK {
                diagnostic(c"Core Vulkan latency device requirements enabled.");
            }
        }
        result
    }
}

/// The request enables timeline semaphores, individually or through the Vulkan 1.2 aggregate.
unsafe fn timeline(request: &vk::DeviceCreateInfo<'static>) -> bool {
    let mut node = request.p_next.cast::<vk::BaseInStructure<'static>>();
    let mut enabled = false;
    for _ in 0..chain::LIMIT {
        let Some(value) = (unsafe { node.as_ref() }) else {
            break;
        };
        enabled |= match value.s_type {
            StructureType::PHYSICAL_DEVICE_TIMELINE_SEMAPHORE_FEATURES => unsafe {
                (*node.cast::<vk::PhysicalDeviceTimelineSemaphoreFeatures<'_>>()).timeline_semaphore
                    != 0
            },
            StructureType::PHYSICAL_DEVICE_VULKAN_1_2_FEATURES => unsafe {
                (*node.cast::<vk::PhysicalDeviceVulkan12Features<'_>>()).timeline_semaphore != 0
            },
            _ => false,
        };
        node = value.p_next;
    }
    enabled
}

/// `vkGetPhysicalDeviceProperties` of the created device's physical device through the owner
/// instance. Without the entry point no facts are kept rather than a previous device's.
fn read_adapter(physical: vk::PhysicalDevice) -> Option<Adapter> {
    let properties = binding()
        .underlying
        .and_then(|underlying| owner_proc(underlying, c"vkGetPhysicalDeviceProperties"));
    let adapter = properties.map(|properties| {
        let properties =
            unsafe { transmute::<VkFunction, vk::PFN_vkGetPhysicalDeviceProperties>(properties) };
        let mut value = vk::PhysicalDeviceProperties::default();
        unsafe { properties(physical, &mut value) };
        Adapter {
            vendor: value.vendor_id,
            device: value.device_id,
            software: u32::from(value.device_type == vk::PhysicalDeviceType::CPU),
        }
    });
    let message = match adapter {
        Some(value) => format!(
            "Vulkan device adapter vendor={:#x} device={:#x} software={}.",
            value.vendor, value.device, value.software
        ),
        None => "Vulkan device adapter facts unavailable.".to_owned(),
    };
    if let Ok(message) = CString::new(message) {
        diagnostic(&message);
    }
    adapter
}

/// The intercepted `vkDestroyDevice`: the Runtime stops pacing the device first.
unsafe extern "system" fn destroy_device(
    device: vk::Device,
    allocator: *const vk::AllocationCallbacks<'_>,
) {
    let handle = ptr::without_provenance_mut(device.as_raw() as usize);
    let destroy = binding()
        .underlying
        .and_then(owner_device_proc)
        // SAFETY: device is the engine's for the call.
        .and_then(|get| unsafe { get(handle, c"vkDestroyDevice".as_ptr()) });
    lock_or_recover(&RECEIPTS).retain(|(owner, _)| *owner != handle.addr());
    unsafe { rsf_latencyvk_device_destroyed(handle) };
    if let Some(destroy) = destroy {
        let destroy = unsafe { transmute::<VkFunction, vk::PFN_vkDestroyDevice>(destroy) };
        unsafe { destroy(device, allocator) };
    }
}

#[cfg(test)]
mod tests;
