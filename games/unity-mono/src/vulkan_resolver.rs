// SPDX-License-Identifier: MIT
//! Unity's Vulkan resolver: the resolver keys and their separate instance and device routes, the
//! interceptors native code registers at initialization, the loader functions they replace, the
//! Win32 interop device-extension selection and the interop readiness of the created device.
//! Native leaves keep the Vulkan types and perform the typed loader, enumeration and create calls.
use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::slice;
use std::sync::{Mutex, MutexGuard, PoisonError};

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

const SUCCESS: i32 = 0;
const OUT_OF_HOST_MEMORY: i32 = -1;
const INITIALIZATION_FAILED: i32 = -3;

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
const INTEROP: [&CStr; 2] = [
    c"VK_KHR_external_memory_win32",
    c"VK_KHR_external_semaphore_win32",
];
/// Their import entry points, checked when the created device's request is not on record.
const INTEROP_FUNCTIONS: [&CStr; 2] = [
    c"vkImportSemaphoreWin32HandleKHR",
    c"vkGetMemoryWin32HandlePropertiesKHR",
];

const EXTENSION_NAME_SIZE: usize = 256;

/// VkExtensionProperties, which vulkan_bridge.cpp checks against the same size.
#[repr(C)]
#[derive(Clone, Copy)]
struct ExtensionProperties {
    name: [u8; EXTENSION_NAME_SIZE],
    #[allow(dead_code)] // written by the native enumeration
    spec_version: u32,
}
const _: () = assert!(size_of::<ExtensionProperties>() == 260);

impl ExtensionProperties {
    const EMPTY: Self = Self {
        name: [0; EXTENSION_NAME_SIZE],
        spec_version: 0,
    };

    /// An unterminated name matches nothing.
    fn name(&self) -> Option<&CStr> {
        CStr::from_bytes_until_nul(&self.name).ok()
    }
}

type Function = unsafe extern "system" fn();
type Enumerate = unsafe extern "C" fn(*mut c_void, *mut u32, *mut ExtensionProperties) -> i32;
type Create =
    unsafe extern "C" fn(*mut c_void, Function, *const *const c_char, u32, *mut *mut c_void) -> i32;
type Resolves = unsafe extern "C" fn(*mut c_void, *const c_char) -> bool;
type ResolveDevice = unsafe extern "C" fn(*mut c_void, *const c_char) -> Option<Function>;

struct Resolver {
    /// Native interceptors by slot, published with the loader's instance resolver. FORWARD has none.
    interceptors: [Option<Function>; SLOTS],
    /// The loader functions the interceptors replace, published before an interceptor is returned.
    originals: [Option<Function>; SLOTS],
    /// The device created with both interop extensions in its request.
    interop_device: usize,
}

static RESOLVER: Mutex<Resolver> = Mutex::new(Resolver::NEW);

fn resolver() -> MutexGuard<'static, Resolver> {
    RESOLVER.lock().unwrap_or_else(PoisonError::into_inner)
}

fn route(name: &CStr, from_instance: bool) -> u32 {
    match KEYS.iter().find(|(_, key)| *key == name) {
        Some(&(DEVICE_PROC | CREATE_SURFACE | DESTROY_SURFACE, _)) if !from_instance => FORWARD,
        Some(&(slot, _)) => slot,
        None => FORWARD,
    }
}

fn key(slot: u32) -> Option<&'static CStr> {
    KEYS.iter()
        .find(|(candidate, _)| *candidate == slot)
        .map(|&(_, key)| key)
}

impl Resolver {
    const NEW: Self = Self {
        interceptors: [None; SLOTS],
        originals: [None; SLOTS],
        interop_device: 0,
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
        self.originals[INSTANCE_PROC as usize] = Some(original);
        self.interceptors[INSTANCE_PROC as usize]
    }

    /// A null resolution stays null and is never saved. A routed key saves its original before its
    /// interceptor is returned; without a registered interceptor it forwards unchanged.
    fn resolve(
        &mut self,
        name: Option<&CStr>,
        original: Option<Function>,
        from_instance: bool,
    ) -> Option<Function> {
        let original = original?;
        let Some(name) = name else {
            return Some(original);
        };
        let slot = route(name, from_instance) as usize;
        let Some(interceptor) = self.interceptors[slot] else {
            return Some(original);
        };
        // Device-command continuations are looked up for the dispatch owner at call time.
        // Caching these even on an instance route would make a later device's lookup global.
        if slot == CREATE_DEVICE as usize && from_instance {
            self.originals[slot] = Some(original);
        }
        Some(interceptor)
    }

    fn original(&self, slot: u32) -> Option<Function> {
        self.originals.get(slot as usize).copied().flatten()
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vk_bind_resolver(
    original: Option<Function>,
    interceptors: *const Option<Function>,
    count: u32,
) -> Option<Function> {
    let table = if interceptors.is_null() {
        &[]
    } else {
        // The native table is a constant array of count entries.
        unsafe { slice::from_raw_parts(interceptors, count as usize) }
    };
    resolver().bind(original, table)
}

/// Unity lends a NUL-terminated loader name, or null, for the call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vk_resolver_resolve(
    name: *const c_char,
    original: Option<Function>,
    from_instance: bool,
) -> Option<Function> {
    let name = (!name.is_null()).then(|| unsafe { CStr::from_ptr(name) });
    resolver().resolve(name, original, from_instance)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vk_resolver_original(slot: u32) -> Option<Function> {
    resolver().original(slot)
}

/// Look up a device command on the device supplied by its caller, outside the state mutex.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vk_resolver_device_function(
    slot: u32,
    device: *mut c_void,
    resolve: ResolveDevice,
) -> Option<Function> {
    let name = key(slot)?;
    if device.is_null() || !matches!(slot, CREATE_SWAPCHAIN | DESTROY_SWAPCHAIN | PRESENT) {
        return None;
    }
    unsafe { resolve(device, name.as_ptr()) }
}

/// The recorded request short-circuits the lookups, which run in order outside the lock.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vk_interop_ready(device: *mut c_void, resolves: Resolves) -> bool {
    let requested = !device.is_null() && resolver().interop_device == device as usize;
    requested || unsafe { functions_resolve(device, resolves) }
}

unsafe fn functions_resolve(device: *mut c_void, resolves: Resolves) -> bool {
    INTEROP_FUNCTIONS
        .iter()
        .all(|name| unsafe { resolves(device, name.as_ptr()) })
}

/// Whether the physical device offers every required name. The two-call enumeration fails on an
/// unsuccessful query, a failed allocation or a second count above the first.
unsafe fn available(context: *mut c_void, enumerate: Enumerate, required: &[&CStr]) -> bool {
    let mut count = 0;
    if unsafe { enumerate(context, &mut count, ptr::null_mut()) } != SUCCESS {
        return false;
    }
    let mut properties = Vec::new();
    if properties.try_reserve_exact(count as usize).is_err() {
        return false;
    }
    properties.resize(count as usize, ExtensionProperties::EMPTY);
    // An empty list repeats the count query, as the native empty vector did.
    let target = if properties.is_empty() {
        ptr::null_mut()
    } else {
        properties.as_mut_ptr()
    };
    if unsafe { enumerate(context, &mut count, target) } != SUCCESS
        || count as usize > properties.len()
    {
        return false;
    }
    properties.truncate(count as usize);
    required
        .iter()
        .all(|&name| properties.iter().any(|item| item.name() == Some(name)))
}

/// Unity's names followed by each interop name it lacks. None when the list cannot be allocated.
/// Each supplied name is null or NUL-terminated.
unsafe fn merge(supplied: &[*const c_char]) -> Option<Vec<*const c_char>> {
    let mut extensions = Vec::new();
    if supplied.len() > u32::MAX as usize - INTEROP.len()
        || extensions
            .try_reserve_exact(supplied.len() + INTEROP.len())
            .is_err()
    {
        return None;
    }
    extensions.extend_from_slice(supplied);
    for name in INTEROP {
        let has_name = extensions
            .iter()
            .any(|&candidate| !candidate.is_null() && unsafe { CStr::from_ptr(candidate) } == name);
        if !has_name {
            extensions.push(name.as_ptr());
        }
    }
    Some(extensions)
}

/// The native caller lends the engine request and its extension names until creation returns.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_vk_resolver_create_device(
    names: *const *const c_char,
    count: u32,
    context: *mut c_void,
    enumerate: Enumerate,
    create: Create,
) -> i32 {
    let Some(original) = rsf_unity_vk_resolver_original(CREATE_DEVICE) else {
        return INITIALIZATION_FAILED;
    };
    let supplied = if count == 0 {
        &[]
    } else if names.is_null() {
        return INITIALIZATION_FAILED;
    } else {
        unsafe { slice::from_raw_parts(names, count as usize) }
    };
    let interop = unsafe { available(context, enumerate, &INTEROP) };
    let mut device = ptr::null_mut();
    let result = if interop {
        let Some(extensions) = (unsafe { merge(supplied) }) else {
            return OUT_OF_HOST_MEMORY;
        };
        unsafe {
            create(
                context,
                original,
                extensions.as_ptr(),
                extensions.len() as u32,
                &mut device,
            )
        }
    } else {
        unsafe { create(context, original, ptr::null(), 0, &mut device) }
    };
    if result == SUCCESS && !device.is_null() {
        let mut state = resolver();
        if interop {
            state.interop_device = device as usize;
        } else if state.interop_device == device as usize {
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
            Some(loader as usize)
        );
        assert_eq!(
            address(state.bind(Some(loader), &table()[1..])),
            Some(loader as usize)
        );
        assert!(state.interceptors.iter().all(Option::is_none));
        assert_eq!(address(state.original(INSTANCE_PROC)), None);

        let mut forward = table();
        forward[FORWARD as usize] = Some(other_loader);
        assert_eq!(
            address(state.bind(Some(loader), &forward)),
            Some(interceptor as usize)
        );
        assert!(state.interceptors[FORWARD as usize].is_none());
        assert_eq!(
            address(state.original(INSTANCE_PROC)),
            Some(loader as usize)
        );
    }

    #[test]
    fn disabled_and_null_resolutions_forward_without_saving() {
        let mut state = Resolver::NEW;
        let present = state.resolve(Some(c"vkQueuePresentKHR"), Some(loader), false);
        assert_eq!(address(present), Some(loader as usize));
        assert_eq!(address(state.original(PRESENT)), None);

        state.bind(Some(loader), &table());
        assert_eq!(
            address(state.resolve(Some(c"vkQueuePresentKHR"), None, false)),
            None
        );
        assert_eq!(address(state.original(PRESENT)), None);
        assert_eq!(
            address(state.resolve(None, Some(loader), true)),
            Some(loader as usize)
        );
        let device_proc = state.resolve(Some(c"vkGetDeviceProcAddr"), Some(other_loader), false);
        assert_eq!(address(device_proc), Some(other_loader as usize));
        assert_eq!(address(state.original(DEVICE_PROC)), None);
        assert_eq!(address(state.original(SLOTS as u32)), None);
    }

    #[test]
    fn device_resolutions_keep_the_continuation_on_its_dispatch_owner() {
        unsafe extern "C" fn lookup(device: *mut c_void, _: *const c_char) -> Option<Function> {
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
                rsf_unity_vk_resolver_device_function(
                    DESTROY_SWAPCHAIN,
                    ptr::without_provenance_mut(owner),
                    lookup,
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
        properties: *mut ExtensionProperties,
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
            let mut item = ExtensionProperties::EMPTY;
            item.name[..name.len()].copy_from_slice(name);
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
        let result = unsafe { available(context, enumerate, &INTEROP) };
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
        let mut unterminated = [b'A'; EXTENSION_NAME_SIZE];
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
        assert_eq!(empty, INTEROP.map(CStr::as_ptr));
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
            assert_eq!(unsafe { functions_resolve(device, resolves) }, found == 2);
            assert_eq!(lookups.0.len(), made);
            assert_eq!(lookups.0[0], b"vkImportSemaphoreWin32HandleKHR");
            if made == 2 {
                assert_eq!(lookups.0[1], b"vkGetMemoryWin32HandlePropertiesKHR");
            }
        }
    }
}
