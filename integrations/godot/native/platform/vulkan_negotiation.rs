// SPDX-License-Identifier: MIT
//! Optional features for an engine-owned Vulkan device request.
//! The consumer lends typed Vulkan callbacks. The negotiated receipt describes only
//! the request accepted by the loader, including fallback without each optional stage.

use std::ffi::{CStr, c_char, c_void};
use std::ptr;

use rsf_game_sys::vulkan_extensions::WIN32_INTEROP;
pub(crate) use rsf_game_sys::vulkan_extensions::merge;

/// `PFN_vkVoidFunction`.
pub(crate) type VkFunction = unsafe extern "C" fn();
/// `rsf_godot_vk_proc`: `vkGetInstanceProcAddr` or `vkGetDeviceProcAddr`.
pub(crate) type VkProc = unsafe extern "C" fn(*mut c_void, *const c_char) -> Option<VkFunction>;

pub(crate) const VK_CREATION_VERSION: u32 = 2;
const VK_SUCCESS: i32 = 0;
const VK_ERROR_OUT_OF_HOST_MEMORY: i32 = -1;
const VK_ERROR_INITIALIZATION_FAILED: i32 = -3;
const VK_ERROR_EXTENSION_NOT_PRESENT: i32 = -7;
pub(crate) const VK_ERROR_FEATURE_NOT_PRESENT: i32 = -8;
const BACKEND_OK: i32 = 0;

/// `RSF_VK_XESS_*`.
pub(crate) const VK_XESS_FEATURES2: u32 = 1;
pub(crate) const VK_XESS_ENABLED_FEATURES: u32 = 2;
pub(crate) const VK_XESS_FLOAT16_INT8: u32 = 4;
pub(crate) const VK_XESS_SCALAR_BLOCK_LAYOUT: u32 = 8;
pub(crate) const VK_XESS_INTEGER_DOT_PRODUCT: u32 = 16;
/// `RSF_VK_STAGE_*`.
pub(crate) const VK_STAGE_XESS: u32 = 0;
pub(crate) const VK_STAGE_INTEROP: u32 = 1;
pub(crate) const VK_STAGE_LATENCY: u32 = 2;

/// `rsf_vk_chain_facts`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct VkChainFacts {
    pub(crate) cloneable: u32,
    pub(crate) features2: u32,
    pub(crate) vulkan12: u32,
    pub(crate) vulkan13: u32,
    pub(crate) float16_int8: u32,
    pub(crate) scalar_block_layout: u32,
    pub(crate) integer_dot_product: u32,
    pub(crate) enabled_features: u32,
    pub(crate) timeline_semaphore: u32,
    pub(crate) reserved: u32,
}

/// `rsf_vk_device_derive`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct VkDeviceDerive {
    pub(crate) base: *const c_void,
    pub(crate) extensions: *const *const c_char,
    pub(crate) extension_count: u32,
    pub(crate) replace_next: u32,
    pub(crate) next: *mut c_void,
    pub(crate) clear_enabled_features: u32,
    pub(crate) reserved: u32,
}

/// `rsf_vk_adapter`: the created device's physical device, as `vkGetPhysicalDeviceProperties`
/// read it. `software` is a CPU device type.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct VkAdapter {
    pub(crate) vendor: u32,
    pub(crate) device: u32,
    pub(crate) software: u32,
    pub(crate) reserved: u32,
}

/// `rsf_vk_latency_requirements`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct VkLatencyRequirements {
    pub(crate) result: i32,
    pub(crate) extension_count: u32,
    pub(crate) extensions: *const *const c_char,
    pub(crate) features: *mut c_void,
}

/// `rsf_vk_instance_call`: one intercepted `vkCreateInstance`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct VkInstanceCall {
    pub(crate) call: *mut c_void,
    pub(crate) request: *const c_void,
    pub(crate) allocator: *const c_void,
    pub(crate) output: *mut c_void,
    pub(crate) extensions: *const *const c_char,
    pub(crate) extension_count: u32,
    pub(crate) reserved: u32,
}

/// `rsf_vk_device_call`: one intercepted `vkCreateDevice`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct VkDeviceCall {
    pub(crate) call: *mut c_void,
    pub(crate) physical_device: *mut c_void,
    pub(crate) request: *const c_void,
    pub(crate) allocator: *const c_void,
    pub(crate) output: *mut c_void,
    pub(crate) extensions: *const *const c_char,
    pub(crate) extension_count: u32,
    pub(crate) reserved: u32,
}

/// `rsf_vk_creation_leaves`, the Vulkan-typed leaves of `vulkan_creation.cpp`.
#[repr(C)]
pub(crate) struct VkCreationLeaves {
    pub(crate) struct_size: u32,
    pub(crate) version: u32,
    pub(crate) xess_instance_requirements:
        unsafe extern "C" fn(*mut u32, *mut *const *const c_char, *mut u32) -> i32,
    pub(crate) derive_instance: unsafe extern "C" fn(
        *mut c_void,
        *const c_void,
        u32,
        *const *const c_char,
        u32,
    ) -> *const c_void,
    pub(crate) create_instance:
        unsafe extern "C" fn(VkFunction, *const c_void, *const c_void, *mut c_void) -> i32,
    pub(crate) chain_facts: unsafe extern "C" fn(*const c_void, *mut VkChainFacts),
    pub(crate) xess_features:
        unsafe extern "C" fn(*mut c_void, *const c_void, u32, *mut *mut c_void) -> u32,
    pub(crate) xess_device_requirements: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut u32,
        *mut *const *const c_char,
        *mut *mut c_void,
    ) -> i32,
    pub(crate) device_extensions:
        unsafe extern "C" fn(VkFunction, *mut c_void, *const *const c_char, u32) -> u32,
    pub(crate) latency_requirements: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut c_void,
        Option<VkProc>,
        *const c_void,
        *mut VkLatencyRequirements,
    ) -> u32,
    pub(crate) derive_device:
        unsafe extern "C" fn(*mut c_void, u32, *const VkDeviceDerive) -> *const c_void,
    pub(crate) create_device: unsafe extern "C" fn(
        VkFunction,
        *mut c_void,
        *const c_void,
        *const c_void,
        *mut c_void,
    ) -> i32,
    pub(crate) latency_created: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut c_void,
        Option<VkProc>,
        *const c_void,
    ) -> i32,
    pub(crate) latency_destroyed: unsafe extern "C" fn(*mut c_void),
    pub(crate) destroy_device: unsafe extern "C" fn(VkFunction, *mut c_void, *const c_void),
    pub(crate) adapter: unsafe extern "C" fn(VkFunction, *mut c_void, *mut VkAdapter) -> u32,
}

impl VkCreationLeaves {
    /// The XeSS instance extensions and minimum API version, when the SDK answers.
    pub(crate) fn xess_instance_requirements(&self) -> Option<(&[*const c_char], u32)> {
        let (mut count, mut minimum) = (0, 0);
        let mut names: *const *const c_char = std::ptr::null();
        let result =
            unsafe { (self.xess_instance_requirements)(&mut count, &mut names, &mut minimum) };
        // SAFETY: the SDK's names stay valid until shutdown.
        (result == 0).then(|| (unsafe { extension_list(names, count) }, minimum))
    }

    pub(crate) fn derive_instance(
        &self,
        call: *mut c_void,
        base: *const c_void,
        minimum_api: u32,
        extensions: &[*const c_char],
    ) -> *const c_void {
        unsafe {
            (self.derive_instance)(
                call,
                base,
                minimum_api,
                extensions.as_ptr(),
                extensions.len() as u32,
            )
        }
    }

    pub(crate) fn create_instance(
        &self,
        create: VkFunction,
        request: *const c_void,
        allocator: *const c_void,
        output: *mut c_void,
    ) -> i32 {
        unsafe { (self.create_instance)(create, request, allocator, output) }
    }

    pub(crate) fn chain_facts(&self, request: *const c_void) -> VkChainFacts {
        let mut facts = VkChainFacts::default();
        unsafe { (self.chain_facts)(request, &mut facts) };
        facts
    }

    /// The patched clone's head, null when the clone refused; `None` when the leaf threw.
    pub(crate) fn xess_features(
        &self,
        call: *mut c_void,
        request: *const c_void,
        structures: u32,
    ) -> Option<*mut c_void> {
        let mut features = std::ptr::null_mut();
        (unsafe { (self.xess_features)(call, request, structures, &mut features) } != 0)
            .then_some(features)
    }

    /// The XeSS device extensions and the chain to create with, when the SDK answers.
    pub(crate) fn xess_device_requirements(
        &self,
        instance: *mut c_void,
        physical_device: *mut c_void,
        features: *mut c_void,
    ) -> Option<(&[*const c_char], *mut c_void)> {
        let (mut count, mut features) = (0, features);
        let mut names: *const *const c_char = std::ptr::null();
        let result = unsafe {
            (self.xess_device_requirements)(
                instance,
                physical_device,
                &mut count,
                &mut names,
                &mut features,
            )
        };
        // SAFETY: the SDK's names stay valid until shutdown.
        (result == 0).then(|| (unsafe { extension_list(names, count) }, features))
    }

    pub(crate) fn device_extensions(
        &self,
        enumerate: VkFunction,
        physical_device: *mut c_void,
        names: &[*const c_char],
    ) -> bool {
        unsafe {
            (self.device_extensions)(
                enumerate,
                physical_device,
                names.as_ptr(),
                names.len() as u32,
            ) != 0
        }
    }

    /// `None` when the leaf threw.
    pub(crate) fn latency_requirements(
        &self,
        call: *mut c_void,
        instance: *mut c_void,
        physical_device: *mut c_void,
        instance_proc: Option<VkProc>,
        request: *const c_void,
    ) -> Option<VkLatencyRequirements> {
        let mut requirements = VkLatencyRequirements {
            result: 0,
            extension_count: 0,
            extensions: std::ptr::null(),
            features: std::ptr::null_mut(),
        };
        (unsafe {
            (self.latency_requirements)(
                call,
                instance,
                physical_device,
                instance_proc,
                request,
                &mut requirements,
            )
        } != 0)
            .then_some(requirements)
    }

    pub(crate) fn derive_device(
        &self,
        call: *mut c_void,
        stage: u32,
        derive: &VkDeviceDerive,
    ) -> *const c_void {
        unsafe { (self.derive_device)(call, stage, derive) }
    }

    pub(crate) fn create_device(
        &self,
        create: VkFunction,
        physical_device: *mut c_void,
        request: *const c_void,
        allocator: *const c_void,
        output: *mut c_void,
    ) -> i32 {
        unsafe { (self.create_device)(create, physical_device, request, allocator, output) }
    }

    pub(crate) fn latency_created(
        &self,
        instance: *mut c_void,
        physical_device: *mut c_void,
        device: *mut c_void,
        instance_proc: Option<VkProc>,
        request: *const c_void,
    ) -> i32 {
        unsafe { (self.latency_created)(instance, physical_device, device, instance_proc, request) }
    }

    pub(crate) fn latency_destroyed(&self, device: *mut c_void) {
        unsafe { (self.latency_destroyed)(device) }
    }

    pub(crate) fn destroy_device(
        &self,
        destroy: VkFunction,
        device: *mut c_void,
        allocator: *const c_void,
    ) {
        unsafe { (self.destroy_device)(destroy, device, allocator) }
    }

    /// `properties` is `vkGetPhysicalDeviceProperties`; `None` when the leaf threw.
    pub(crate) fn adapter(
        &self,
        properties: VkFunction,
        physical_device: *mut c_void,
    ) -> Option<VkAdapter> {
        let mut adapter = VkAdapter::default();
        (unsafe { (self.adapter)(properties, physical_device, &mut adapter) } != 0)
            .then_some(adapter)
    }
}

/// An extension list as a slice; empty for a zero count or a null list.
///
/// # Safety
/// `names` is null or readable for `count` entries for the returned lifetime.
pub(crate) unsafe fn extension_list<'a>(
    names: *const *const c_char,
    count: u32,
) -> &'a [*const c_char] {
    if count == 0 || names.is_null() {
        &[]
    } else {
        // SAFETY: the caller's guarantee.
        unsafe { std::slice::from_raw_parts(names, count as usize) }
    }
}

/// A create result for which a refused optional requirement is retried without it.
fn refused(result: i32) -> bool {
    result == VK_ERROR_FEATURE_NOT_PRESENT || result == VK_ERROR_EXTENSION_NOT_PRESENT
}

/// The feature structures the XeSS clone adds before the SDK patches it, or `None` when the
/// engine's chain must stay as it is and XeSS is unavailable for the device. The SDK otherwise
/// appends Vulkan 1.2 and 1.3 aggregates, which conflict with the engine's individual promoted
/// feature structures.
pub(crate) fn xess_structures(facts: &VkChainFacts) -> Option<u32> {
    let on = |flag: u32| flag != 0;
    if !on(facts.cloneable) {
        return None;
    }
    let mut structures = 0;
    if on(facts.enabled_features) {
        // pEnabledFeatures and a Features2 in the chain are mutually exclusive.
        if on(facts.features2) {
            return None;
        }
        structures |= VK_XESS_ENABLED_FEATURES;
    } else if !on(facts.features2) {
        structures |= VK_XESS_FEATURES2;
    }
    if !on(facts.vulkan12) && !on(facts.float16_int8) {
        structures |= VK_XESS_FLOAT16_INT8;
    }
    if !on(facts.vulkan12) && !on(facts.scalar_block_layout) {
        structures |= VK_XESS_SCALAR_BLOCK_LAYOUT;
    }
    if !on(facts.vulkan13) && !on(facts.integer_dot_product) {
        structures |= VK_XESS_INTEGER_DOT_PRODUCT;
    }
    Some(structures)
}

/// A device request and its extension list, which this owner built or read for the call.
#[derive(Clone, Copy)]
struct Request<'a> {
    handle: *const c_void,
    extensions: &'a [*const c_char],
}

#[derive(Default)]
pub(crate) struct Accepted {
    pub(crate) xess: bool,
    pub(crate) interop: bool,
}

pub(crate) struct DeviceNegotiation<D> {
    instance: *mut c_void,
    underlying: Option<VkProc>,
    prepare_xess: bool,
    diagnostic: D,
}

fn resolve_with(proc: VkProc, object: *mut c_void, name: &CStr) -> Option<VkFunction> {
    // SAFETY: the engine lends the original resolver; name lives through the call.
    unsafe { proc(object, name.as_ptr()) }
}

impl<D: Fn(&CStr)> DeviceNegotiation<D> {
    pub(crate) fn new(
        instance: *mut c_void,
        underlying: Option<VkProc>,
        prepare_xess: bool,
        diagnostic: D,
    ) -> Self {
        Self {
            instance,
            underlying,
            prepare_xess,
            diagnostic,
        }
    }

    pub(crate) fn create(&self, leaves: &VkCreationLeaves, call: &VkDeviceCall) -> (i32, Accepted) {
        let mut accepted = Accepted::default();
        let result = self.create_stages(leaves, call, &mut accepted);
        (result, accepted)
    }

    fn create_stages(
        &self,
        leaves: &VkCreationLeaves,
        call: &VkDeviceCall,
        receipt: &mut Accepted,
    ) -> i32 {
        if call.request.is_null()
            || call.output.is_null()
            || (call.extension_count != 0 && call.extensions.is_null())
        {
            return VK_ERROR_INITIALIZATION_FAILED;
        }
        // SAFETY: the entry point read the engine's list for the call.
        let original = Request {
            handle: call.request,
            extensions: unsafe { extension_list(call.extensions, call.extension_count) },
        };
        if !self.prepare_xess {
            return self.create_interop(leaves, call, original, receipt);
        }
        let fallback = |receipt: &mut Accepted| {
            (self.diagnostic)(
                c"XeSS Vulkan requirements unavailable; preserving the original engine device request.",
            );
            self.create_interop(leaves, call, original, receipt)
        };
        let Some(structures) = xess_structures(&leaves.chain_facts(original.handle)) else {
            return fallback(receipt);
        };
        let features = match leaves.xess_features(call.call, original.handle, structures) {
            None => return VK_ERROR_OUT_OF_HOST_MEMORY,
            Some(features) if features.is_null() => return fallback(receipt),
            Some(features) => features,
        };
        let Some((names, features)) =
            leaves.xess_device_requirements(self.instance, call.physical_device, features)
        else {
            return fallback(receipt);
        };
        let Some(merged) = (unsafe { merge(original.extensions, names) }) else {
            return VK_ERROR_OUT_OF_HOST_MEMORY;
        };
        let derive = VkDeviceDerive {
            base: original.handle,
            extensions: merged.as_ptr(),
            extension_count: merged.len() as u32,
            replace_next: 1,
            next: features,
            clear_enabled_features: 1,
            reserved: 0,
        };
        let handle = leaves.derive_device(call.call, VK_STAGE_XESS, &derive);
        if handle.is_null() {
            return VK_ERROR_OUT_OF_HOST_MEMORY;
        }
        let request = Request {
            handle,
            extensions: &merged,
        };
        let result = self.create_interop(leaves, call, request, receipt);
        if refused(result) {
            return fallback(receipt);
        }
        if result == VK_SUCCESS {
            receipt.xess = true;
            (self.diagnostic)(c"XeSS Vulkan device requirements enabled before device creation.");
        }
        result
    }

    /// The interop stage: when the device lists both Win32 extensions and the request enables
    /// timeline semaphores, they are added; a refusal retries the request unchanged.
    fn create_interop(
        &self,
        leaves: &VkCreationLeaves,
        call: &VkDeviceCall,
        request: Request<'_>,
        receipt: &mut Accepted,
    ) -> i32 {
        let enumerate = self.underlying.and_then(|underlying| {
            resolve_with(
                underlying,
                self.instance,
                c"vkEnumerateDeviceExtensionProperties",
            )
        });
        let interop_extensions = WIN32_INTEROP.map(CStr::as_ptr);
        let available = enumerate.is_some_and(|enumerate| {
            leaves.device_extensions(enumerate, call.physical_device, &interop_extensions)
        });
        if !available || leaves.chain_facts(request.handle).timeline_semaphore == 0 {
            return self.create_latency(leaves, call, request);
        }
        let Some(merged) = (unsafe { merge(request.extensions, &interop_extensions) }) else {
            return VK_ERROR_OUT_OF_HOST_MEMORY;
        };
        let derive = VkDeviceDerive {
            base: request.handle,
            extensions: merged.as_ptr(),
            extension_count: merged.len() as u32,
            replace_next: 0,
            next: ptr::null_mut(),
            clear_enabled_features: 0,
            reserved: 0,
        };
        let handle = leaves.derive_device(call.call, VK_STAGE_INTEROP, &derive);
        if handle.is_null() {
            return VK_ERROR_OUT_OF_HOST_MEMORY;
        }
        let interop = Request {
            handle,
            extensions: &merged,
        };
        let result = self.create_latency(leaves, call, interop);
        if refused(result) {
            (self.diagnostic)(
                c"Vulkan Win32 interop requirements unavailable; retrying the unchanged device request.",
            );
            return self.create_latency(leaves, call, request);
        }
        if result == VK_SUCCESS {
            receipt.interop = true;
            (self.diagnostic)(
                c"Vulkan Win32 memory/semaphore interop enabled before device creation.",
            );
        }
        result
    }

    /// The core latency stage and the native create: the Runtime's optional latency extensions
    /// and features, retried without them when refused, and the created device reported to it.
    fn create_latency(
        &self,
        leaves: &VkCreationLeaves,
        call: &VkDeviceCall,
        request: Request<'_>,
    ) -> i32 {
        let instance = self.instance;
        let underlying = self.underlying;
        let create =
            underlying.and_then(|underlying| resolve_with(underlying, instance, c"vkCreateDevice"));
        let Some(create) = create else {
            return VK_ERROR_INITIALIZATION_FAILED;
        };
        let Some(requirements) = leaves.latency_requirements(
            call.call,
            instance,
            call.physical_device,
            underlying,
            request.handle,
        ) else {
            return VK_ERROR_OUT_OF_HOST_MEMORY;
        };
        let merged;
        let mut actual = request.handle;
        if requirements.result == BACKEND_OK && requirements.extension_count != 0 {
            // SAFETY: the Runtime's names stay valid through the synchronous create.
            let names =
                unsafe { extension_list(requirements.extensions, requirements.extension_count) };
            let Some(list) = (unsafe { merge(request.extensions, names) }) else {
                return VK_ERROR_OUT_OF_HOST_MEMORY;
            };
            merged = list;
            let derive = VkDeviceDerive {
                base: request.handle,
                extensions: merged.as_ptr(),
                extension_count: merged.len() as u32,
                replace_next: 1,
                next: requirements.features,
                clear_enabled_features: 0,
                reserved: 0,
            };
            actual = leaves.derive_device(call.call, VK_STAGE_LATENCY, &derive);
            if actual.is_null() {
                return VK_ERROR_OUT_OF_HOST_MEMORY;
            }
        }
        let mut result = leaves.create_device(
            create,
            call.physical_device,
            actual,
            call.allocator,
            call.output,
        );
        if actual != request.handle && refused(result) {
            (self.diagnostic)(
                c"Optional core latency device requirements refused; retrying the caller's unchanged request.",
            );
            actual = request.handle;
            result = leaves.create_device(
                create,
                call.physical_device,
                actual,
                call.allocator,
                call.output,
            );
        }
        if result == VK_SUCCESS {
            // SAFETY: a successful create wrote the dispatchable VkDevice handle.
            let device = unsafe { call.output.cast::<*mut c_void>().read() };
            if leaves.latency_created(instance, call.physical_device, device, underlying, actual)
                == BACKEND_OK
            {
                (self.diagnostic)(c"Core Vulkan latency device requirements enabled.");
            }
        }
        result
    }
}
