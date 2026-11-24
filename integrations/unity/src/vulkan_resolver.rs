// SPDX-License-Identifier: MIT
//! Unity's Vulkan resolver: the resolver keys and their separate instance and device routes, the
//! interceptors native code registers at initialization, the loader functions they replace, the
//! Win32 interop device-extension selection and the interop readiness of the created device.
//! Native code supplies stateless Vulkan loader entry points and the DXGI presentation callback.
use ash::vk;
use rsf_game_sys::ffi_util::lock_or_recover;
use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::slice;
use std::sync::{Mutex, MutexGuard};

#[path = "vulkan_wsi.rs"]
mod wsi;

const FORWARD: u32 = 0;
const INSTANCE_PROC: u32 = 1;
const DEVICE_PROC: u32 = 2;
const CREATE_DEVICE: u32 = 3;
const CREATE_SWAPCHAIN: u32 = 4;
const DESTROY_SWAPCHAIN: u32 = 5;
const PRESENT: u32 = 6;
const CREATE_SURFACE: u32 = 7;
const DESTROY_SURFACE: u32 = 8;
const SLOTS: usize = DESTROY_SURFACE as usize + 1;

pub(crate) const SUCCESS: i32 = vk::Result::SUCCESS.as_raw();
const OUT_OF_HOST_MEMORY: i32 = vk::Result::ERROR_OUT_OF_HOST_MEMORY.as_raw();
const OUT_OF_DEVICE_MEMORY: i32 = vk::Result::ERROR_OUT_OF_DEVICE_MEMORY.as_raw();
const INITIALIZATION_FAILED: i32 = vk::Result::ERROR_INITIALIZATION_FAILED.as_raw();
pub(crate) const DEVICE_LOST: i32 = vk::Result::ERROR_DEVICE_LOST.as_raw();

/// Resolver keys by slot. vkGetInstanceProcAddr itself is never routed, and vkGetDeviceProcAddr
/// only from the instance resolver.
const KEYS: [(u32, &CStr); 7] = [
    (DEVICE_PROC, c"vkGetDeviceProcAddr"),
    (CREATE_DEVICE, c"vkCreateDevice"),
    (CREATE_SWAPCHAIN, c"vkCreateSwapchainKHR"),
    (DESTROY_SWAPCHAIN, c"vkDestroySwapchainKHR"),
    (PRESENT, c"vkQueuePresentKHR"),
    (CREATE_SURFACE, c"vkCreateWin32SurfaceKHR"),
    (DESTROY_SURFACE, c"vkDestroySurfaceKHR"),
];

/// The device extensions the shared memory and fence imports need.
use rsf_game_sys::vulkan_extensions::{self, WIN32_INTEROP as INTEROP};

const INTEROP_FUNCTIONS: [&CStr; 2] = [
    c"vkImportSemaphoreWin32HandleKHR",
    c"vkGetMemoryWin32HandlePropertiesKHR",
];

type Function = unsafe extern "system" fn();
pub(crate) type Lookup = unsafe extern "system" fn(*mut c_void, *const c_char) -> Option<Function>;
type Enumerate = unsafe extern "system" fn(
    *mut c_void,
    *const c_char,
    *mut u32,
    *mut vk::ExtensionProperties,
) -> i32;
type CreateDevice = unsafe extern "system" fn(
    *mut c_void,
    *const vk::DeviceCreateInfo<'static>,
    *const c_void,
    *mut *mut c_void,
) -> i32;

#[repr(C)]
#[derive(Clone, Copy)]
struct NativeHooks {
    present: Function,
    device_proc: Lookup,
    enumerate: Enumerate,
    create_device: CreateDevice,
    create_semaphore: wsi::CreateSemaphore,
    destroy_semaphore: wsi::Destroy,
    wait_idle: wsi::WaitIdle,
    retirement_fault: extern "C" fn(),
}

struct Resolver {
    /// Native interceptors by slot, published with the loader's instance resolver. FORWARD has none.
    interceptors: [Option<Function>; SLOTS],
    /// The Unity loader resolver. Vulkan commands dispatch on their supplied owner.
    original_instance: Option<Function>,
    /// The device created with both interop extensions in its request.
    interop_device: usize,
    native: Option<NativeHooks>,
}

static RESOLVER: Mutex<Resolver> = Mutex::new(Resolver::NEW);

fn resolver() -> MutexGuard<'static, Resolver> {
    lock_or_recover(&RESOLVER)
}

fn route(name: &CStr, from_instance: bool) -> u32 {
    match KEYS.iter().find(|(_, key)| *key == name) {
        Some(&(DEVICE_PROC | CREATE_SURFACE | DESTROY_SURFACE, _)) if !from_instance => FORWARD,
        Some(&(slot, _)) => slot,
        None => FORWARD,
    }
}

#[cfg(test)]
fn key(slot: u32) -> Option<&'static CStr> {
    KEYS.iter()
        .find(|(candidate, _)| *candidate == slot)
        .map(|&(_, key)| key)
}

impl Resolver {
    const NEW: Self = Self {
        interceptors: [None; SLOTS],
        original_instance: None,
        interop_device: 0,
        native: None,
    };

    /// The initialization route. A null loader resolver or an incomplete table leaves Unity on its
    /// own resolver and publishes nothing.
    fn bind(&mut self, original: Option<Function>, table: &[Option<Function>]) -> Option<Function> {
        let original = original?;
        if table.len() != SLOTS || table[INSTANCE_PROC as usize..].iter().any(Option::is_none) {
            return Some(original);
        }
        self.interceptors.copy_from_slice(table);
        self.interceptors[FORWARD as usize] = None;
        self.original_instance = Some(original);
        self.interceptors[INSTANCE_PROC as usize]
    }

    /// A null command stays null. An unhandled name forwards Unity's loader result.
    fn resolve(
        &self,
        name: Option<&CStr>,
        original: Option<Function>,
        from_instance: bool,
    ) -> Option<Function> {
        let original = original?;
        let Some(name) = name else {
            return Some(original);
        };
        self.interceptors[route(name, from_instance) as usize].or(Some(original))
    }

    #[cfg(test)]
    fn original(&self, slot: u32) -> Option<Function> {
        if slot == INSTANCE_PROC {
            self.original_instance
        } else {
            None
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vk_install(
    original: Option<Lookup>,
    hooks: *const NativeHooks,
) -> Option<Lookup> {
    let original = original?;
    let Some(&native) = (unsafe { hooks.as_ref() }) else {
        return Some(original);
    };
    // Vulkan defines PFN_vkVoidFunction for transport of any Vulkan function pointer.
    let cast = |lookup: Lookup| unsafe { std::mem::transmute::<Lookup, Function>(lookup) };
    let table = [
        None,
        Some(cast(get_instance_proc)),
        Some(cast(get_device_proc)),
        Some(unsafe { std::mem::transmute::<CreateDevice, Function>(intercept_create_device) }),
        Some(unsafe {
            std::mem::transmute::<wsi::CreateSwapchain, Function>(wsi::create_swapchain)
        }),
        Some(unsafe { std::mem::transmute::<wsi::Destroy, Function>(wsi::destroy_swapchain) }),
        Some(native.present),
        Some(unsafe { std::mem::transmute::<wsi::CreateSurface, Function>(wsi::create_surface) }),
        Some(unsafe { std::mem::transmute::<wsi::Destroy, Function>(wsi::destroy_surface) }),
    ];
    let mut state = resolver();
    state.native = Some(native);
    let _ = state.bind(Some(cast(original)), &table);
    Some(get_instance_proc)
}

unsafe fn instance_command(instance: *mut c_void, name: *const c_char) -> Option<Function> {
    let original = resolver().original_instance?;
    let lookup = unsafe { std::mem::transmute::<Function, Lookup>(original) };
    unsafe { lookup(instance, name) }
}

/// Unity lends a NUL-terminated loader name, or null, for the call.
unsafe extern "system" fn get_instance_proc(
    instance: *mut c_void,
    name: *const c_char,
) -> Option<Function> {
    if name.is_null() {
        return None;
    }
    let original = unsafe { instance_command(instance, name) };
    resolver().resolve(Some(unsafe { CStr::from_ptr(name) }), original, true)
}

unsafe extern "system" fn get_device_proc(
    device: *mut c_void,
    name: *const c_char,
) -> Option<Function> {
    if name.is_null() {
        return None;
    }
    let native = resolver().native?;
    let original = unsafe { (native.device_proc)(device, name) };
    resolver().resolve(Some(unsafe { CStr::from_ptr(name) }), original, false)
}

/// The recorded request short-circuits the lookups, which run in order outside the lock.
pub(crate) fn interop_ready(device: *mut c_void, lookup: Lookup) -> bool {
    if device.is_null() {
        return false;
    }
    let requested = resolver().interop_device == device as usize;
    requested || functions_resolve(|name| unsafe { lookup(device, name.as_ptr()) }.is_some())
}

fn functions_resolve(resolves: impl FnMut(&CStr) -> bool) -> bool {
    INTEROP_FUNCTIONS.into_iter().all(resolves)
}

/// Whether the physical device offers every required name. The two-call enumeration fails on an
/// unsuccessful query, a failed allocation or a second count above the first.
fn available(
    mut enumerate: impl FnMut(*mut u32, *mut vk::ExtensionProperties) -> i32,
    required: &[&CStr],
) -> bool {
    let mut count = 0;
    if enumerate(&mut count, ptr::null_mut()) != SUCCESS {
        return false;
    }
    let mut properties = Vec::new();
    if properties.try_reserve_exact(count as usize).is_err() {
        return false;
    }
    properties.resize(count as usize, vk::ExtensionProperties::default());
    // An empty list repeats the count query, as the native empty vector did.
    let target = if properties.is_empty() {
        ptr::null_mut()
    } else {
        properties.as_mut_ptr()
    };
    if enumerate(&mut count, target) != SUCCESS || count as usize > properties.len() {
        return false;
    }
    properties.truncate(count as usize);
    required.iter().all(|&name| {
        properties
            .iter()
            .any(|item| item.extension_name_as_c_str().ok() == Some(name))
    })
}

/// Unity's names followed by each interop name it lacks. None when the list cannot be allocated.
/// Each supplied name is null or NUL-terminated.
unsafe fn merge(supplied: &[*const c_char]) -> Option<Vec<*const c_char>> {
    unsafe { vulkan_extensions::merge(supplied, &INTEROP.map(CStr::as_ptr)) }
}

/// Unity lends the native create request and its borrowed pointers until the driver returns.
unsafe extern "system" fn intercept_create_device(
    physical: *mut c_void,
    info: *const vk::DeviceCreateInfo<'static>,
    allocator: *const c_void,
    output: *mut *mut c_void,
) -> i32 {
    let (Some(native), Some(info)) = (resolver().native, unsafe { info.as_ref() }) else {
        return INITIALIZATION_FAILED;
    };
    if output.is_null() {
        return INITIALIZATION_FAILED;
    }
    let supplied = if info.enabled_extension_count == 0 {
        &[]
    } else if info.pp_enabled_extension_names.is_null() {
        return INITIALIZATION_FAILED;
    } else {
        unsafe {
            slice::from_raw_parts(
                info.pp_enabled_extension_names,
                info.enabled_extension_count as usize,
            )
        }
    };
    let create = native.create_device;
    let interop = available(
        |count, properties| unsafe { (native.enumerate)(physical, ptr::null(), count, properties) },
        &INTEROP,
    );
    let result = if interop {
        let Some(extensions) = (unsafe { merge(supplied) }) else {
            return OUT_OF_HOST_MEMORY;
        };
        let configured = vk::DeviceCreateInfo {
            enabled_extension_count: extensions.len() as u32,
            pp_enabled_extension_names: extensions.as_ptr(),
            ..*info
        };
        unsafe { create(physical, &configured, allocator, output) }
    } else {
        unsafe { create(physical, info, allocator, output) }
    };
    if result == SUCCESS && !unsafe { *output }.is_null() {
        let device = unsafe { *output } as usize;
        let mut state = resolver();
        if interop {
            state.interop_device = device;
        } else if state.interop_device == device {
            state.interop_device = 0;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "system" fn loader() {}
    unsafe extern "system" fn other_loader() {}
    unsafe extern "system" fn interceptor() {}

    fn address(function: Option<Function>) -> Option<usize> {
        function.map(|function| function as usize)
    }

    fn table() -> [Option<Function>; SLOTS] {
        let mut table = [Some(interceptor as Function); SLOTS];
        table[FORWARD as usize] = None;
        table
    }

    #[test]
    fn keys_route_exactly_and_device_proc_only_from_the_instance() {
        assert_eq!(route(c"vkGetDeviceProcAddr", true), DEVICE_PROC);
        assert_eq!(route(c"vkGetDeviceProcAddr", false), FORWARD);
        for from_instance in [true, false] {
            assert_eq!(route(c"vkCreateDevice", from_instance), CREATE_DEVICE);
            assert_eq!(
                route(c"vkCreateSwapchainKHR", from_instance),
                CREATE_SWAPCHAIN
            );
            assert_eq!(
                route(c"vkDestroySwapchainKHR", from_instance),
                DESTROY_SWAPCHAIN
            );
            assert_eq!(route(c"vkQueuePresentKHR", from_instance), PRESENT);
            assert_eq!(route(c"vkGetInstanceProcAddr", from_instance), FORWARD);
            assert_eq!(route(c"vkQueuePresentKHR2", from_instance), FORWARD);
            assert_eq!(route(c"vkqueuepresentkhr", from_instance), FORWARD);
        }
        assert_eq!(key(DESTROY_SWAPCHAIN), Some(c"vkDestroySwapchainKHR"));
        assert_eq!(key(INSTANCE_PROC), None);
        assert_eq!(key(FORWARD), None);
    }

    #[test]
    fn initialization_route_publishes_only_a_complete_table() {
        let mut state = Resolver::NEW;
        assert_eq!(address(state.bind(None, &table())), None);
        assert_eq!(address(state.original(INSTANCE_PROC)), None);
        let mut incomplete = table();
        incomplete[PRESENT as usize] = None;
        assert_eq!(
            address(state.bind(Some(loader), &incomplete)),
            Some(loader as *const () as usize)
        );
        assert_eq!(
            address(state.bind(Some(loader), &table()[1..])),
            Some(loader as *const () as usize)
        );
        assert!(state.interceptors.iter().all(Option::is_none));
        assert_eq!(address(state.original(INSTANCE_PROC)), None);

        let mut forward = table();
        forward[FORWARD as usize] = Some(other_loader);
        assert_eq!(
            address(state.bind(Some(loader), &forward)),
            Some(interceptor as *const () as usize)
        );
        assert!(state.interceptors[FORWARD as usize].is_none());
        assert_eq!(
            address(state.original(INSTANCE_PROC)),
            Some(loader as *const () as usize)
        );
    }

    #[test]
    fn disabled_and_null_resolutions_forward_without_saving() {
        let mut state = Resolver::NEW;
        let present = state.resolve(Some(c"vkQueuePresentKHR"), Some(loader), false);
        assert_eq!(address(present), Some(loader as *const () as usize));
        assert_eq!(address(state.original(PRESENT)), None);

        state.bind(Some(loader), &table());
        assert_eq!(
            address(state.resolve(Some(c"vkQueuePresentKHR"), None, false)),
            None
        );
        assert_eq!(address(state.original(PRESENT)), None);
        assert_eq!(
            address(state.resolve(None, Some(loader), true)),
            Some(loader as *const () as usize)
        );
        let device_proc = state.resolve(Some(c"vkGetDeviceProcAddr"), Some(other_loader), false);
        assert_eq!(
            address(device_proc),
            Some(other_loader as *const () as usize)
        );
        assert_eq!(address(state.original(DEVICE_PROC)), None);
        assert_eq!(address(state.original(SLOTS as u32)), None);
    }

    #[test]
    fn device_resolutions_keep_the_continuation_on_its_dispatch_owner() {
        unsafe extern "system" fn lookup(
            device: *mut c_void,
            _: *const c_char,
        ) -> Option<Function> {
            match device as usize {
                1 => Some(loader),
                2 => Some(other_loader),
                _ => None,
            }
        }

        let mut state = Resolver::NEW;
        state.bind(Some(loader), &table());
        state.resolve(Some(c"vkDestroySwapchainKHR"), Some(loader), false);
        state.resolve(Some(c"vkDestroySwapchainKHR"), Some(other_loader), false);
        assert_eq!(address(state.original(DESTROY_SWAPCHAIN)), None);
        for (owner, expected) in [(1, loader as Function), (2, other_loader as Function)] {
            let continuation = unsafe {
                lookup(
                    ptr::without_provenance_mut(owner),
                    key(DESTROY_SWAPCHAIN).unwrap().as_ptr(),
                )
            };
            assert_eq!(address(continuation), Some(expected as usize));
        }
    }

    /// A physical device whose count may grow after the first query.
    struct Enumeration {
        names: Vec<Vec<u8>>,
        first: i32,
        second: i32,
        grow: u32,
        calls: Vec<(u32, bool)>,
    }

    unsafe extern "C" fn enumerate(
        context: *mut c_void,
        count: *mut u32,
        properties: *mut vk::ExtensionProperties,
    ) -> i32 {
        let state = unsafe { &mut *context.cast::<Enumeration>() };
        let capacity = unsafe { *count };
        state.calls.push((capacity, !properties.is_null()));
        let first = state.calls.len() == 1;
        if properties.is_null() {
            let grow = if first { 0 } else { state.grow };
            unsafe { *count = state.names.len() as u32 + grow };
            return if first { state.first } else { state.second };
        }
        let written = capacity.min(state.names.len() as u32);
        for (index, name) in state.names.iter().take(written as usize).enumerate() {
            let mut item = vk::ExtensionProperties::default();
            for (target, &byte) in item.extension_name.iter_mut().zip(name) {
                *target = byte as c_char;
            }
            unsafe { properties.add(index).write(item) };
        }
        unsafe { *count = written };
        state.second
    }

    fn enumeration(names: &[&[u8]]) -> Enumeration {
        Enumeration {
            names: names.iter().map(|name| name.to_vec()).collect(),
            first: SUCCESS,
            second: SUCCESS,
            grow: 0,
            calls: Vec::new(),
        }
    }

    fn offered(mut enumeration: Enumeration) -> (bool, Vec<(u32, bool)>) {
        let context = ptr::from_mut(&mut enumeration).cast();
        let result = available(
            |count, properties| unsafe { enumerate(context, count, properties) },
            &INTEROP,
        );
        (result, enumeration.calls)
    }

    const MEMORY: &[u8] = b"VK_KHR_external_memory_win32\0";
    const SEMAPHORE: &[u8] = b"VK_KHR_external_semaphore_win32\0";

    #[test]
    fn extension_selection_needs_both_exact_names() {
        let both = enumeration(&[b"VK_KHR_swapchain\0", SEMAPHORE, MEMORY]);
        assert_eq!(offered(both), (true, vec![(0, false), (3, true)]));
        assert!(!offered(enumeration(&[MEMORY])).0);
        assert!(
            !offered(enumeration(&[
                b"VK_KHR_external_memory_win32_x\0",
                SEMAPHORE
            ]))
            .0
        );
        let mut unterminated = [b'A'; vk::MAX_EXTENSION_NAME_SIZE];
        unterminated[..MEMORY.len() - 1].copy_from_slice(&MEMORY[..MEMORY.len() - 1]);
        assert!(!offered(enumeration(&[&unterminated, SEMAPHORE])).0);
    }

    #[test]
    fn extension_selection_refuses_failed_and_growing_enumerations() {
        let mut failed = enumeration(&[MEMORY, SEMAPHORE]);
        failed.first = INITIALIZATION_FAILED;
        assert_eq!(offered(failed), (false, vec![(0, false)]));
        let mut incomplete = enumeration(&[MEMORY, SEMAPHORE]);
        incomplete.second = 5;
        assert_eq!(offered(incomplete), (false, vec![(0, false), (2, true)]));
        // An empty list repeats the count query; a device that grew in between is refused.
        let mut grown = enumeration(&[]);
        grown.grow = 1;
        assert_eq!(offered(grown), (false, vec![(0, false), (0, false)]));
        assert_eq!(
            offered(enumeration(&[])),
            (false, vec![(0, false), (0, false)])
        );
    }

    #[test]
    fn merge_appends_only_missing_interop_names() {
        let swapchain = c"VK_KHR_swapchain".as_ptr();
        let memory = c"VK_KHR_external_memory_win32".as_ptr();
        let merged = unsafe { merge(&[swapchain, memory, ptr::null()]) }.unwrap();
        assert_eq!(merged.len(), 4);
        assert_eq!(merged[..3], [swapchain, memory, ptr::null()]);
        let added = unsafe { CStr::from_ptr(merged[3]) };
        assert_eq!(added, c"VK_KHR_external_semaphore_win32");
        let empty = unsafe { merge(&[]) }.unwrap();
        // Constants carry no address identity; compare the names.
        let names: Vec<&CStr> = empty
            .iter()
            .map(|&name| unsafe { CStr::from_ptr(name) })
            .collect();
        assert_eq!(names, INTEROP);
    }

    /// Lookups made and how many of them succeed.
    unsafe extern "C" fn resolves(device: *mut c_void, name: *const c_char) -> bool {
        let lookups = unsafe { &mut *device.cast::<(Vec<Vec<u8>>, usize)>() };
        let name = unsafe { CStr::from_ptr(name) };
        lookups.0.push(name.to_bytes().to_vec());
        lookups.0.len() <= lookups.1
    }

    #[test]
    fn interop_lookups_run_in_order_and_short_circuit() {
        for (found, made) in [(0, 1), (1, 2), (2, 2)] {
            let mut lookups: (Vec<Vec<u8>>, usize) = (Vec::new(), found);
            let device = ptr::from_mut(&mut lookups).cast();
            assert_eq!(
                functions_resolve(|name| unsafe { resolves(device, name.as_ptr()) }),
                found == 2
            );
            assert_eq!(lookups.0.len(), made);
            assert_eq!(lookups.0[0], b"vkImportSemaphoreWin32HandleKHR");
            if made == 2 {
                assert_eq!(lookups.0[1], b"vkGetMemoryWin32HandlePropertiesKHR");
            }
        }
    }
}
