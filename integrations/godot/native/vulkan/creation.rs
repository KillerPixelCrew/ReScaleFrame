// SPDX-License-Identifier: MIT
//! Intercepts the engine loader, lends device requests to Runtime negotiation and
//! publishes a receipt for each successful device. The primary swapchain and SR
//! passes supply their device identity. Resolver callbacks run outside the graphics
//! guard, and the binding and receipt locks are released before calling the loader.

use crate::platform::vulkan_negotiation::{DeviceNegotiation, extension_list, merge};
use crate::platform::{
    VK_ROUTE_CREATE_DEVICE, VK_ROUTE_CREATE_INSTANCE, VK_ROUTE_DESTROY_DEVICE,
    VK_ROUTE_DEVICE_PROC, VK_ROUTE_FORWARD, VK_ROUTE_NONE, VK_SCOPE_DEVICE, VkAdapter,
    VkCreationLeaves, VkDeviceCall, VkFunction, VkInstanceCall, VkProc,
};
use rsf_game_sys::rsf_game_log_fn;
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

const VK_SUCCESS: i32 = 0;
const VK_ERROR_OUT_OF_HOST_MEMORY: i32 = -1;
const VK_ERROR_INITIALIZATION_FAILED: i32 = -3;
const VK_ERROR_EXTENSION_NOT_PRESENT: i32 = -7;
const VK_ERROR_INCOMPATIBLE_DRIVER: i32 = -9;

#[derive(Clone, Copy)]
struct Binding {
    underlying: Option<VkProc>,
    log: rsf_game_log_fn,
    log_user: usize,
}

#[derive(Clone, Copy, Default)]
pub(super) struct DeviceReceipt {
    pub(super) xess: bool,
    pub(super) interop: bool,
    pub(super) adapter: Option<VkAdapter>,
}

pub(crate) struct Creation {
    binding: Mutex<Binding>,
    /// The instance `vkCreateInstance` created or `vkCreateDevice` was resolved for.
    owner: AtomicPtr<c_void>,
    prepare_xess: AtomicBool,
    receipts: Mutex<HashMap<usize, DeviceReceipt>>,
    render_device: AtomicUsize,
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
            receipts: Mutex::new(HashMap::new()),
            render_device: AtomicUsize::new(0),
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
        self.reset_readiness();
        underlying.is_some()
    }

    /// The stop leaf retired XeSS. The resolver keeps forwarding for the engine's teardown.
    pub(crate) fn stop(&self) {
        self.prepare_xess.store(false, Ordering::Release);
        self.reset_readiness();
    }

    fn receipts(&self) -> std::sync::MutexGuard<'_, HashMap<usize, DeviceReceipt>> {
        self.receipts.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn reset_readiness(&self) {
        self.render_device.store(0, Ordering::Release);
        for receipt in self.receipts().values_mut() {
            receipt.xess = false;
            receipt.interop = false;
        }
    }

    pub(crate) fn report_render_device(&self, device: *mut c_void) {
        self.render_device.store(device.addr(), Ordering::Release);
    }

    pub(super) fn render_receipt(&self) -> DeviceReceipt {
        self.device_receipt(self.render_device.load(Ordering::Acquire))
    }

    pub(super) fn device_receipt(&self, device: usize) -> DeviceReceipt {
        let receipts = self.receipts();
        // Before a primary pass or swapchain identifies its device, the sole successful
        // creation is unambiguous. Multiple candidates require an explicit device report.
        if device == 0 && receipts.len() == 1 {
            return receipts.values().next().copied().unwrap_or_default();
        }
        #[cfg(test)]
        if let Some(receipt) = receipts.get(&0) {
            return *receipt;
        }
        receipts.get(&device).copied().unwrap_or_default()
    }

    #[cfg(test)]
    pub(crate) fn xess_ready(&self) -> bool {
        self.render_receipt().xess
    }

    pub(crate) fn interop_ready(&self) -> bool {
        self.render_receipt().interop
    }

    #[cfg(test)]
    pub(crate) fn adapter(&self) -> Option<VkAdapter> {
        self.render_receipt().adapter
    }

    #[cfg(test)]
    pub(crate) fn report_ready(&self, xess: bool, interop: bool) {
        let mut receipts = self.receipts();
        let receipt = receipts.entry(0).or_default();
        receipt.xess = xess;
        receipt.interop = interop;
    }

    #[cfg(test)]
    pub(crate) fn report_adapter(&self, adapter: Option<VkAdapter>) {
        self.receipts().entry(0).or_default().adapter = adapter;
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
        if call.request.is_null()
            || call.output.is_null()
            || (call.extension_count != 0 && call.extensions.is_null())
        {
            return VK_ERROR_INITIALIZATION_FAILED;
        }
        // SAFETY: the entry point read the engine's list for the call.
        let engine = unsafe { extension_list(call.extensions, call.extension_count) };
        let merged;
        let mut request = call.request;
        if self.prepare_xess.load(Ordering::Acquire) {
            match leaves.xess_instance_requirements() {
                Some((names, minimum)) => {
                    let Some(list) = (unsafe { merge(engine, names) }) else {
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
    /// driver's feature or extension check, creates the engine's own request instead. The Runtime
    /// negotiates optional stages and returns the features accepted for this device.
    pub(crate) fn create_device(&self, leaves: &VkCreationLeaves, call: &VkDeviceCall) -> i32 {
        let binding = self.binding();
        let (result, accepted) = DeviceNegotiation::new(
            self.owner(),
            binding.underlying,
            self.prepare_xess.load(Ordering::Acquire),
            |message| self.diagnostic(message),
        )
        .create(leaves, call);
        if result == VK_SUCCESS {
            // SAFETY: a successful create wrote the dispatchable VkDevice handle.
            let device = unsafe { call.output.cast::<*mut c_void>().read() };
            if !device.is_null() {
                let receipt = DeviceReceipt {
                    xess: accepted.xess,
                    interop: accepted.interop,
                    adapter: self.read_adapter(leaves, call.physical_device),
                };
                self.receipts().insert(device.addr(), receipt);
            }
        }
        result
    }

    /// `vkGetPhysicalDeviceProperties` of the created device's physical device through the owner
    /// instance. Without the entry point, or when the leaf threw, no facts are kept rather than a
    /// previous device's.
    fn read_adapter(
        &self,
        leaves: &VkCreationLeaves,
        physical_device: *mut c_void,
    ) -> Option<VkAdapter> {
        let properties = self.binding().underlying.and_then(|underlying| {
            Self::resolve_with(underlying, self.owner(), c"vkGetPhysicalDeviceProperties")
        });
        let adapter = properties.and_then(|properties| leaves.adapter(properties, physical_device));
        let message = match adapter {
            Some(value) => format!(
                "Vulkan device adapter vendor={:#x} device={:#x} software={}.",
                value.vendor, value.device, value.software
            ),
            None => "Vulkan device adapter facts unavailable.".to_owned(),
        };
        if let Ok(message) = CString::new(message) {
            self.diagnostic(&message);
        }
        adapter
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
        self.receipts().remove(&device.addr());
        leaves.latency_destroyed(device);
        if let Some(destroy) = destroy {
            leaves.destroy_device(destroy, device, allocator);
        }
    }
}

#[cfg(test)]
mod tests;

mod abi;
pub(crate) mod typed;
