// SPDX-License-Identifier: MIT
//! The Vulkan creation owner of the Godot session, formerly the state and policy of
//! `vulkan_creation.cpp`: the underlying resolver and session log the start leaf hands over, the
//! resolver's routes (`vkCreateInstance`, `vkGetDeviceProcAddr`, `vkCreateDevice` and the
//! device-level `vkDestroyDevice` are intercepted, every other name is forwarded), the owner
//! instance, the XeSS instance and device requirements with their fallbacks, the Win32 interop
//! extensions, the order of the device stages (XeSS, interop, core latency, the native create), the
//! extension merge, the feature structures the XeSS clone gets before the SDK patches it, every
//! retry and diagnostic, and the XeSS and interop readiness the session's admission and the
//! presenter host options read.
//!
//! Vulkan structures stay in the leaves (`VkCreationLeaves`): the chain facts and clones, the
//! request copies, the XeSS and core latency requirement calls and the Vulkan create and destroy
//! calls. Requests are passed here as addresses. The core latency requirements stay the Runtime's
//! (`rsf_latencyvk_require_device`); this owner only orders and retries around them.
//!
//! The creation calls come from the engine's loader outside `vulkan.cpp`'s guard, as before. The
//! flags are atomics and the binding a short lock that is never held across a leaf, so a nested
//! resolver call cannot deadlock. The start and stop leaves set and clear this state from inside
//! the session's guarded boot and shutdown.
use crate::platform::{
    VK_ROUTE_CREATE_DEVICE, VK_ROUTE_CREATE_INSTANCE, VK_ROUTE_DESTROY_DEVICE,
    VK_ROUTE_DEVICE_PROC, VK_ROUTE_FORWARD, VK_ROUTE_NONE, VK_SCOPE_DEVICE, VK_STAGE_INTEROP,
    VK_STAGE_LATENCY, VK_STAGE_XESS, VK_XESS_ENABLED_FEATURES, VK_XESS_FEATURES2,
    VK_XESS_FLOAT16_INT8, VK_XESS_INTEGER_DOT_PRODUCT, VK_XESS_SCALAR_BLOCK_LAYOUT, VkChainFacts,
    VkCreationLeaves, VkDeviceCall, VkDeviceDerive, VkFunction, VkInstanceCall, VkProc,
    extension_list,
};
use rsf_game_sys::rsf_game_log_fn;
use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Mutex, PoisonError};

const VK_SUCCESS: i32 = 0;
const VK_ERROR_OUT_OF_HOST_MEMORY: i32 = -1;
const VK_ERROR_INITIALIZATION_FAILED: i32 = -3;
const VK_ERROR_EXTENSION_NOT_PRESENT: i32 = -7;
const VK_ERROR_FEATURE_NOT_PRESENT: i32 = -8;
const VK_ERROR_INCOMPATIBLE_DRIVER: i32 = -9;
const BACKEND_OK: i32 = 0;

/// The device extensions the shared Win32 interop (FSR4 SR and FG, XeSS FG) exports and imports
/// memory and semaphores through. Asked only when the engine enables timeline semaphores.
const INTEROP_EXTENSIONS: [*const c_char; 2] = [
    c"VK_KHR_external_memory_win32".as_ptr(),
    c"VK_KHR_external_semaphore_win32".as_ptr(),
];

/// A create result for which a refused optional requirement is retried without it.
fn refused(result: i32) -> bool {
    result == VK_ERROR_FEATURE_NOT_PRESENT || result == VK_ERROR_EXTENSION_NOT_PRESENT
}

/// `base` followed by each of `required` it does not name yet, in order; `None` when the list
/// cannot be allocated, which ends the creation call as the entry point's catch did.
fn merge(base: &[*const c_char], required: &[*const c_char]) -> Option<Vec<*const c_char>> {
    let mut list = Vec::new();
    list.try_reserve(base.len() + required.len()).ok()?;
    list.extend_from_slice(base);
    for &name in required {
        // SAFETY: engine and SDK extension names are NUL-terminated for the creation call.
        let present = list
            .iter()
            .any(|&value| unsafe { CStr::from_ptr(value) == CStr::from_ptr(name) });
        if !present {
            list.push(name);
        }
    }
    Some(list)
}

/// The feature structures the XeSS clone adds before the SDK patches it, or `None` when the
/// engine's chain must stay as it is and XeSS is unavailable for the device. The SDK otherwise
/// appends Vulkan 1.2 and 1.3 aggregates, which conflict with the engine's individual promoted
/// feature structures.
fn xess_structures(facts: &VkChainFacts) -> Option<u32> {
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

#[derive(Clone, Copy)]
struct Binding {
    underlying: Option<VkProc>,
    log: rsf_game_log_fn,
    log_user: usize,
}

pub(crate) struct Creation {
    binding: Mutex<Binding>,
    /// The instance `vkCreateInstance` created or `vkCreateDevice` was resolved for.
    owner: AtomicPtr<c_void>,
    prepare_xess: AtomicBool,
    xess_ready: AtomicBool,
    interop_ready: AtomicBool,
}

impl Creation {
    pub(crate) fn new() -> Self {
        Self {
            binding: Mutex::new(Binding {
                underlying: None,
                log: None,
                log_user: 0,
            }),
            owner: AtomicPtr::new(ptr::null_mut()),
            prepare_xess: AtomicBool::new(false),
            xess_ready: AtomicBool::new(false),
            interop_ready: AtomicBool::new(false),
        }
    }

    /// A copy of the binding; the lock is never held across a leaf or a resolver call.
    fn binding(&self) -> Binding {
        *self.binding.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn owner(&self) -> *mut c_void {
        self.owner.load(Ordering::Acquire)
    }

    /// The start leaf picked `underlying`. The owner instance survives, as before.
    pub(crate) fn begin(
        &self,
        underlying: Option<VkProc>,
        xess: bool,
        log: rsf_game_log_fn,
        log_user: *mut c_void,
    ) -> bool {
        *self.binding.lock().unwrap_or_else(PoisonError::into_inner) = Binding {
            underlying,
            log,
            log_user: log_user as usize,
        };
        self.prepare_xess.store(xess, Ordering::Release);
        self.xess_ready.store(false, Ordering::Release);
        self.interop_ready.store(false, Ordering::Release);
        underlying.is_some()
    }

    /// The stop leaf retired XeSS. The resolver keeps forwarding for the engine's teardown.
    pub(crate) fn stop(&self) {
        self.prepare_xess.store(false, Ordering::Release);
        self.xess_ready.store(false, Ordering::Release);
        self.interop_ready.store(false, Ordering::Release);
    }

    pub(crate) fn xess_ready(&self) -> bool {
        self.xess_ready.load(Ordering::Acquire)
    }

    pub(crate) fn interop_ready(&self) -> bool {
        self.interop_ready.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn report_ready(&self, xess: bool, interop: bool) {
        self.xess_ready.store(xess, Ordering::Release);
        self.interop_ready.store(interop, Ordering::Release);
    }

    fn diagnostic(&self, message: &CStr) {
        let binding = self.binding();
        if let Some(log) = binding.log {
            // SAFETY: the engine module's logger lives while its library is loaded.
            unsafe { log(binding.log_user as *mut c_void, message.as_ptr()) };
        }
    }

    /// `name` through the underlying resolver for `object`.
    fn resolve_with(underlying: VkProc, object: *mut c_void, name: &CStr) -> Option<VkFunction> {
        // SAFETY: the underlying resolver is Streamline's or the loader's
        // vkGetInstanceProcAddr; the name is NUL-terminated.
        unsafe { underlying(object, name.as_ptr()) }
    }

    /// The owner instance's `vkGetDeviceProcAddr`, as a resolver.
    fn device_resolver(&self, underlying: VkProc) -> Option<VkProc> {
        let get = Self::resolve_with(underlying, self.owner(), c"vkGetDeviceProcAddr")?;
        // SAFETY: vkGetDeviceProcAddr has the rsf_godot_vk_proc signature.
        Some(unsafe { std::mem::transmute::<VkFunction, VkProc>(get) })
    }

    /// The route for `name`, and the function for `VK_ROUTE_FORWARD`.
    ///
    /// # Safety
    /// `name` is null or NUL-terminated for the call.
    pub(crate) unsafe fn resolve(
        &self,
        scope: u32,
        object: *mut c_void,
        name: *const c_char,
    ) -> (u32, Option<VkFunction>) {
        let Some(underlying) = self.binding().underlying else {
            return (VK_ROUTE_NONE, None);
        };
        // SAFETY: the caller's guarantee.
        let named = (!name.is_null()).then(|| unsafe { CStr::from_ptr(name) });
        if scope == VK_SCOPE_DEVICE {
            if named == Some(c"vkDestroyDevice") {
                return (VK_ROUTE_DESTROY_DEVICE, None);
            }
            // Other device names, null included, go to the owner instance's resolver unchanged.
            let forwarded = self.device_resolver(underlying).and_then(|get| {
                // SAFETY: as resolve_with; name is the caller's, forwarded unchanged.
                unsafe { get(object, name) }
            });
            return (VK_ROUTE_FORWARD, forwarded);
        }
        let Some(named) = named else {
            return (VK_ROUTE_NONE, None);
        };
        if named == c"vkCreateInstance" {
            (VK_ROUTE_CREATE_INSTANCE, None)
        } else if named == c"vkGetDeviceProcAddr" {
            (VK_ROUTE_DEVICE_PROC, None)
        } else if named == c"vkCreateDevice" {
            if !object.is_null() {
                self.owner.store(object, Ordering::Release);
            }
            (VK_ROUTE_CREATE_DEVICE, None)
        } else {
            (
                VK_ROUTE_FORWARD,
                Self::resolve_with(underlying, object, named),
            )
        }
    }

    /// The intercepted `vkCreateInstance`: the XeSS instance extensions and minimum API version
    /// when XeSS is prepared, and the engine's own request when the driver refuses them. A refusal
    /// ends XeSS preparation for the session.
    pub(crate) fn create_instance(&self, leaves: &VkCreationLeaves, call: &VkInstanceCall) -> i32 {
        let create = self.binding().underlying.and_then(|underlying| {
            Self::resolve_with(underlying, ptr::null_mut(), c"vkCreateInstance")
        });
        let Some(create) = create else {
            return VK_ERROR_INITIALIZATION_FAILED;
        };
        if call.request.is_null() || call.output.is_null() {
            return VK_ERROR_INITIALIZATION_FAILED;
        }
        // SAFETY: the entry point read the engine's list for the call.
        let engine = unsafe { extension_list(call.extensions, call.extension_count) };
        let merged;
        let mut request = call.request;
        if self.prepare_xess.load(Ordering::Acquire) {
            match leaves.xess_instance_requirements() {
                Some((names, minimum)) => {
                    let Some(list) = merge(engine, names) else {
                        return VK_ERROR_OUT_OF_HOST_MEMORY;
                    };
                    merged = list;
                    request = leaves.derive_instance(call.call, call.request, minimum, &merged);
                    if request.is_null() {
                        return VK_ERROR_OUT_OF_HOST_MEMORY;
                    }
                }
                None => self.prepare_xess.store(false, Ordering::Release),
            }
        }
        let mut result = leaves.create_instance(create, request, call.allocator, call.output);
        if self.prepare_xess.load(Ordering::Acquire)
            && (result == VK_ERROR_EXTENSION_NOT_PRESENT || result == VK_ERROR_INCOMPATIBLE_DRIVER)
        {
            self.prepare_xess.store(false, Ordering::Release);
            self.diagnostic(
                c"XeSS Vulkan instance requirements unavailable; retrying the original engine instance request.",
            );
            result = leaves.create_instance(create, call.request, call.allocator, call.output);
        }
        if result == VK_SUCCESS {
            // SAFETY: a successful create wrote the dispatchable VkInstance handle.
            let instance = unsafe { call.output.cast::<*mut c_void>().read() };
            self.owner.store(instance, Ordering::Release);
        }
        result
    }

    /// The intercepted `vkCreateDevice`. With XeSS prepared, the engine's chain is cloned with the
    /// structures XeSS needs and patched by the SDK; any refusal, of the clone, the SDK or the
    /// driver's feature or extension check, creates the engine's own request instead. Either way
    /// the interop stage runs next.
    pub(crate) fn create_device(&self, leaves: &VkCreationLeaves, call: &VkDeviceCall) -> i32 {
        if call.request.is_null() || call.output.is_null() {
            return VK_ERROR_INITIALIZATION_FAILED;
        }
        // SAFETY: the entry point read the engine's list for the call.
        let original = Request {
            handle: call.request,
            extensions: unsafe { extension_list(call.extensions, call.extension_count) },
        };
        self.xess_ready.store(false, Ordering::Release);
        if !self.prepare_xess.load(Ordering::Acquire) {
            return self.create_interop(leaves, call, original);
        }
        let fallback = || {
            self.diagnostic(
                c"XeSS Vulkan requirements unavailable; preserving the original engine device request.",
            );
            self.create_interop(leaves, call, original)
        };
        let Some(structures) = xess_structures(&leaves.chain_facts(original.handle)) else {
            return fallback();
        };
        let features = match leaves.xess_features(call.call, original.handle, structures) {
            None => return VK_ERROR_OUT_OF_HOST_MEMORY,
            Some(features) if features.is_null() => return fallback(),
            Some(features) => features,
        };
        let Some((names, features)) =
            leaves.xess_device_requirements(self.owner(), call.physical_device, features)
        else {
            return fallback();
        };
        let Some(merged) = merge(original.extensions, names) else {
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
        let result = self.create_interop(leaves, call, request);
        if refused(result) {
            return fallback();
        }
        if result == VK_SUCCESS {
            self.xess_ready.store(true, Ordering::Release);
            self.diagnostic(c"XeSS Vulkan device requirements enabled before device creation.");
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
    ) -> i32 {
        self.interop_ready.store(false, Ordering::Release);
        let enumerate = self.binding().underlying.and_then(|underlying| {
            Self::resolve_with(
                underlying,
                self.owner(),
                c"vkEnumerateDeviceExtensionProperties",
            )
        });
        let available = enumerate.is_some_and(|enumerate| {
            leaves.device_extensions(enumerate, call.physical_device, &INTEROP_EXTENSIONS)
        });
        if !available || leaves.chain_facts(request.handle).timeline_semaphore == 0 {
            return self.create_latency(leaves, call, request);
        }
        let Some(merged) = merge(request.extensions, &INTEROP_EXTENSIONS) else {
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
            self.diagnostic(
                c"Vulkan Win32 interop requirements unavailable; retrying the unchanged device request.",
            );
            return self.create_latency(leaves, call, request);
        }
        if result == VK_SUCCESS {
            self.interop_ready.store(true, Ordering::Release);
            self.diagnostic(
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
        let instance = self.owner();
        let underlying = self.binding().underlying;
        let create = underlying
            .and_then(|underlying| Self::resolve_with(underlying, instance, c"vkCreateDevice"));
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
            let Some(list) = merge(request.extensions, names) else {
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
            self.diagnostic(
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
                self.diagnostic(c"Core Vulkan latency device requirements enabled.");
            }
        }
        result
    }

    /// The intercepted `vkDestroyDevice`: the Runtime stops pacing the device first.
    pub(crate) fn destroy_device(
        &self,
        leaves: &VkCreationLeaves,
        device: *mut c_void,
        allocator: *const c_void,
    ) {
        let destroy = self
            .binding()
            .underlying
            .and_then(|underlying| self.device_resolver(underlying))
            .and_then(|get| {
                // SAFETY: as resolve_with; device is the engine's for the call.
                unsafe { get(device, c"vkDestroyDevice".as_ptr()) }
            });
        leaves.latency_destroyed(device);
        if let Some(destroy) = destroy {
            leaves.destroy_device(destroy, device, allocator);
        }
    }
}

#[cfg(test)]
mod tests;
