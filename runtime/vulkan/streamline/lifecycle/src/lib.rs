// SPDX-License-Identifier: MIT
//! Lifecycle owner for the Streamline Vulkan interposer.
//!
//! `runtime/vulkan/streamline/bridge.cpp` keeps the `VulkanStorySl*` exports, the
//! typed SDK entry points, the slInit preferences and the verified-file leaf. This
//! crate decides when the interposer is admitted, loaded, initialized, shut down
//! and unloaded, which features slInit loads, and which Vulkan dispatch entry
//! points stay usable after SDK shutdown.

use std::cell::Cell;
use std::ffi::{CStr, c_char, c_void};
use std::ptr::{NonNull, null, null_mut, without_provenance_mut};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

#[cfg(test)]
mod tests;

// Local results of VulkanStorySlInitializeFeatures. Other values are slInit
// results or the sticky slShutdown result.
const INVALID_ARGUMENT: i32 = -1;
const PATH_TOO_LONG: i32 = -2;
const LOAD_FAILED: i32 = -3;
const MISSING_EXPORT: i32 = -4;
const UNVERIFIED: i32 = -5;
const OTHER_DIRECTORY: i32 = -6;
// A lifecycle call made from inside another one on the same thread.
const REENTERED: i32 = -7;

// The bridge formatted the interposer path into a MAX_PATH buffer.
const MAX_PATH: usize = 260;
const INTERPOSER: &str = "\\sl.interposer.dll";
const COMMON: &str = "\\sl.common.dll";
const INVALID_HANDLE: *mut c_void = without_provenance_mut(usize::MAX);
const LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR: u32 = 0x100;
const LOAD_LIBRARY_SEARCH_DEFAULT_DIRS: u32 = 0x1000;

// Feature bits for the native slInit leaf, which maps them to sl::Feature in
// this order.
const FEATURE_PCL: u32 = 1;
const FEATURE_REFLEX: u32 = 1 << 1;
const FEATURE_DLSS_G: u32 = 1 << 2;
const FEATURE_DLSS: u32 = 1 << 3;

/// Selects the slInit feature set. PCL is always loaded; DLSS-G also needs Reflex.
fn feature_mask(reflex: bool, frame_generation: bool, super_resolution: bool) -> u32 {
    let mut features = FEATURE_PCL;
    if reflex || frame_generation {
        features |= FEATURE_REFLEX;
    }
    if frame_generation {
        features |= FEATURE_DLSS_G;
    }
    if super_resolution {
        features |= FEATURE_DLSS;
    }
    features
}

/// Native leaves of the bridge. They own the SDK types, the typed entry points
/// and the Authenticode and SHA-256 check of the interposer file.
#[repr(C)]
pub struct Platform {
    /// Opens the file without write or delete sharing and verifies it through
    /// that handle. Returns the open handle or INVALID_HANDLE_VALUE.
    pub open_verified: unsafe extern "C" fn(*const u16) -> *mut c_void,
    /// Resolves the typed SDK exports from the module. Zero when all resolve.
    pub resolve: unsafe extern "C" fn(*mut c_void) -> i32,
    /// Calls slInit with the feature bits, project identity and the plugin
    /// path array, which this crate keeps alive until the next initialization.
    pub init: unsafe extern "C" fn(u32, *const c_char, *const *const u16) -> i32,
    /// Calls slShutdown when it was resolved and writes its result. Returns
    /// nonzero when the SDK was called.
    pub shutdown: unsafe extern "C" fn(*mut i32) -> u32,
    /// Clears the SDK entry points and frame token. Nonzero also clears the
    /// frame-generation configuration and the pending presentation error.
    pub clear: unsafe extern "C" fn(u32),
}

const _: () = assert!(size_of::<Platform>() == 5 * size_of::<*mut c_void>());

trait System {
    unsafe fn load(&self, path: *const u16) -> *mut c_void;
    unsafe fn free(&self, module: NonNull<c_void>);
    unsafe fn close(&self, handle: *mut c_void);
    unsafe fn symbol(&self, module: *mut c_void, name: *const c_char) -> *mut c_void;
    unsafe fn loaded(&self, path: *const u16) -> *mut c_void;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}

struct Win32;

impl System for Win32 {
    unsafe fn load(&self, path: *const u16) -> *mut c_void {
        let flags = LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS;
        unsafe { LoadLibraryExW(path, null_mut(), flags) }
    }

    unsafe fn free(&self, module: NonNull<c_void>) {
        unsafe { FreeLibrary(module.as_ptr()) };
    }

    unsafe fn close(&self, handle: *mut c_void) {
        unsafe { CloseHandle(handle) };
    }

    unsafe fn symbol(&self, module: *mut c_void, name: *const c_char) -> *mut c_void {
        unsafe { GetProcAddress(module, name) }
    }

    unsafe fn loaded(&self, path: *const u16) -> *mut c_void {
        unsafe { GetModuleHandleW(path) }
    }
}

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
}

// Refuses a lifecycle call reentered from an SDK or log callback on the same
// thread instead of deadlocking on the session lock.
struct Entry;

impl Entry {
    fn enter() -> Option<Self> {
        ACTIVE.with(|active| (!active.replace(true)).then_some(Self))
    }
}

impl Drop for Entry {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.set(false));
    }
}

struct Paths {
    // NUL-terminated plugin directory.
    directory: Vec<u16>,
    project: Box<CStr>,
    // The SDK receives this array's address as pathsToPlugins.
    plugin: [*const u16; 1],
}

struct Session {
    module: Option<NonNull<c_void>>,
    // True between a successful slInit and the matching slShutdown. The module
    // can stay loaded after shutdown.
    initialized: bool,
    shutdown_attempted: bool,
    shutdown_result: i32,
    paths: Option<Box<Paths>>,
}

// The module handle and path pointers name process-wide loader and heap state.
// The session lock serializes every use.
unsafe impl Send for Session {}

// Lock-free copies for the presentation and Vulkan dispatch paths, which must
// not wait behind slInit or slShutdown.
struct Published {
    module: AtomicPtr<c_void>,
    initialized: AtomicBool,
    instance: AtomicPtr<c_void>,
    device: AtomicPtr<c_void>,
}

struct Lifecycle {
    session: Mutex<Session>,
    published: Published,
    // sl.common.dll path for presentation lookups, held only around loader queries.
    common: Mutex<Vec<u16>>,
}

static LIFECYCLE: Lifecycle = Lifecycle::new();

unsafe fn terminated(text: *const u16) -> Vec<u16> {
    let mut length = 0;
    while unsafe { *text.add(length) } != 0 {
        length += 1;
    }
    unsafe { std::slice::from_raw_parts(text, length) }.to_vec()
}

// Admits only the interposer verified through the handle that stays open while
// the loader maps it. The handle denies writes, renames and deletes.
unsafe fn admit(
    system: &impl System,
    platform: &Platform,
    directory: &[u16],
) -> Result<NonNull<c_void>, i32> {
    let mut path = directory.to_vec();
    path.extend(INTERPOSER.encode_utf16());
    if path.len() >= MAX_PATH {
        return Err(PATH_TOO_LONG);
    }
    path.push(0);
    let verified = unsafe { (platform.open_verified)(path.as_ptr()) };
    if verified.is_null() || verified == INVALID_HANDLE {
        return Err(UNVERIFIED);
    }
    let module = unsafe { system.load(path.as_ptr()) };
    unsafe { system.close(verified) };
    NonNull::new(module).ok_or(LOAD_FAILED)
}

impl Lifecycle {
    const fn new() -> Self {
        Self {
            session: Mutex::new(Session {
                module: None,
                initialized: false,
                shutdown_attempted: false,
                shutdown_result: 0,
                paths: None,
            }),
            published: Published {
                module: AtomicPtr::new(null_mut()),
                initialized: AtomicBool::new(false),
                instance: AtomicPtr::new(null_mut()),
                device: AtomicPtr::new(null_mut()),
            },
            common: Mutex::new(Vec::new()),
        }
    }

    fn session(&self) -> MutexGuard<'_, Session> {
        self.session.lock().unwrap_or_else(PoisonError::into_inner)
    }

    unsafe fn initialize(
        &self,
        system: &impl System,
        platform: &Platform,
        directory: *const u16,
        project: *const c_char,
        features: u32,
    ) -> i32 {
        let Some(_entry) = Entry::enter() else {
            return REENTERED;
        };
        let mut session = self.session();
        if session.shutdown_attempted && session.shutdown_result != 0 {
            return session.shutdown_result;
        }
        if session.initialized {
            return 0;
        }
        if directory.is_null() || project.is_null() {
            return INVALID_ARGUMENT;
        }
        let directory = unsafe { terminated(directory) };
        // A module kept after shutdown may still back Vulkan dispatch tables.
        // Only a module loaded by this call is released on failure.
        let loaded_here = session.module.is_none();
        let module = match session.module {
            Some(module) => {
                let same = session.paths.as_ref().is_some_and(|paths| {
                    paths.directory.strip_suffix(&[0]) == Some(directory.as_slice())
                });
                if !same {
                    return OTHER_DIRECTORY;
                }
                module
            }
            None => match unsafe { admit(system, platform, &directory) } {
                Ok(module) => {
                    session.module = Some(module);
                    self.published
                        .module
                        .store(module.as_ptr(), Ordering::Release);
                    module
                }
                Err(result) => return result,
            },
        };
        let resolved = unsafe { (platform.resolve)(module.as_ptr()) } == 0;
        let instance = unsafe { system.symbol(module.as_ptr(), c"vkGetInstanceProcAddr".as_ptr()) };
        let device = unsafe { system.symbol(module.as_ptr(), c"vkGetDeviceProcAddr".as_ptr()) };
        if !resolved || instance.is_null() || device.is_null() {
            unsafe { self.abandon(system, platform, &mut session, loaded_here) };
            return MISSING_EXPORT;
        }
        self.published.instance.store(instance, Ordering::Release);
        self.published.device.store(device, Ordering::Release);

        let mut paths = Box::new(Paths {
            directory: directory.iter().copied().chain([0]).collect(),
            project: unsafe { CStr::from_ptr(project) }.into(),
            plugin: [null()],
        });
        paths.plugin[0] = paths.directory.as_ptr();
        let (project, plugin) = (paths.project.as_ptr(), paths.plugin.as_ptr());
        // The previous strings belong to a session that has already shut down.
        session.paths = Some(paths);
        self.set_common(&directory);
        let result = unsafe { (platform.init)(features, project, plugin) };
        if result != 0 {
            unsafe { self.abandon(system, platform, &mut session, loaded_here) };
            return result;
        }
        session.initialized = true;
        session.shutdown_attempted = false;
        session.shutdown_result = 0;
        self.published.initialized.store(true, Ordering::Release);
        0
    }

    unsafe fn shutdown(&self, platform: &Platform) -> i32 {
        let Some(_entry) = Entry::enter() else {
            return REENTERED;
        };
        let mut session = self.session();
        if session.module.is_none() {
            return 0;
        }
        if session.shutdown_attempted {
            return session.shutdown_result;
        }
        // An SDK failure may follow partial plugin destruction. Spend the attempt
        // before calling it; neither feature entry points nor retries remain valid.
        session.shutdown_attempted = true;
        if session.initialized {
            let mut result = 0;
            if unsafe { (platform.shutdown)(&mut result) } != 0 {
                session.shutdown_result = result;
            }
        }
        session.initialized = false;
        self.published.initialized.store(false, Ordering::Release);
        // Only the Vulkan dispatch entry points remain valid after slShutdown.
        unsafe { (platform.clear)(1) };
        session.shutdown_result
    }

    // Call only after shutdown and after every Vulkan object and API table that
    // references the interposer is destroyed. A failed shutdown keeps the module.
    unsafe fn unload(&self, system: &impl System, platform: &Platform) {
        let Some(_entry) = Entry::enter() else {
            return;
        };
        let mut session = self.session();
        if session.shutdown_attempted && session.shutdown_result != 0 {
            return;
        }
        session.initialized = false;
        unsafe { self.release(system, platform, &mut session) };
    }

    unsafe fn abandon(
        &self,
        system: &impl System,
        platform: &Platform,
        session: &mut Session,
        loaded_here: bool,
    ) {
        if loaded_here {
            unsafe { self.release(system, platform, session) };
        } else {
            unsafe { (platform.clear)(0) };
        }
    }

    // Withdraws the published entry points before the loader can unmap them.
    unsafe fn release(&self, system: &impl System, platform: &Platform, session: &mut Session) {
        self.published.initialized.store(false, Ordering::Release);
        self.published.instance.store(null_mut(), Ordering::Release);
        self.published.device.store(null_mut(), Ordering::Release);
        self.published.module.store(null_mut(), Ordering::Release);
        if let Some(module) = session.module.take() {
            unsafe { (platform.clear)(0) };
            unsafe { system.free(module) };
        }
    }

    fn set_common(&self, directory: &[u16]) {
        let slash = u16::from(b'/');
        let path = directory
            .iter()
            .map(|&unit| {
                if unit == slash {
                    u16::from(b'\\')
                } else {
                    unit
                }
            })
            .chain(COMMON.encode_utf16())
            .chain([0])
            .collect();
        *self.common.lock().unwrap_or_else(PoisonError::into_inner) = path;
    }

    fn module(&self) -> *mut c_void {
        if self.published.initialized.load(Ordering::Acquire) {
            self.published.module.load(Ordering::Acquire)
        } else {
            null_mut()
        }
    }

    unsafe fn interposer_symbol(&self, system: &impl System, name: *const c_char) -> *mut c_void {
        let module = self.published.module.load(Ordering::Acquire);
        if module.is_null() || name.is_null() {
            return null_mut();
        }
        unsafe { system.symbol(module, name) }
    }

    unsafe fn common_symbol(&self, system: &impl System, name: *const c_char) -> *mut c_void {
        let path = self.common.lock().unwrap_or_else(PoisonError::into_inner);
        if path.is_empty() || name.is_null() {
            return null_mut();
        }
        let common = unsafe { system.loaded(path.as_ptr()) };
        if common.is_null() {
            return null_mut();
        }
        unsafe { system.symbol(common, name) }
    }
}

/// Loads the verified interposer if needed and initializes the selected features.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slvk_life_initialize(
    platform: &Platform,
    directory: *const u16,
    project: *const c_char,
    reflex: u32,
    frame_generation: u32,
    super_resolution: u32,
) -> i32 {
    let features = feature_mask(reflex != 0, frame_generation != 0, super_resolution != 0);
    unsafe { LIFECYCLE.initialize(&Win32, platform, directory, project, features) }
}

/// Shuts the SDK down once. The result is sticky; the module stays loaded.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slvk_life_shutdown(platform: &Platform) -> i32 {
    unsafe { LIFECYCLE.shutdown(platform) }
}

/// Unloads the interposer unless shutdown failed.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slvk_life_unload(platform: &Platform) {
    unsafe { LIFECYCLE.unload(&Win32, platform) };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_slvk_life_initialized() -> u32 {
    u32::from(LIFECYCLE.published.initialized.load(Ordering::Acquire))
}

/// The interposer while the SDK is initialized, otherwise null.
#[unsafe(no_mangle)]
extern "C" fn rsf_slvk_life_module() -> *mut c_void {
    LIFECYCLE.module()
}

/// vkGetInstanceProcAddr of the loaded interposer. Kept after slShutdown.
#[unsafe(no_mangle)]
extern "C" fn rsf_slvk_life_instance_proc() -> *mut c_void {
    LIFECYCLE.published.instance.load(Ordering::Acquire)
}

/// vkGetDeviceProcAddr of the loaded interposer. Kept after slShutdown.
#[unsafe(no_mangle)]
extern "C" fn rsf_slvk_life_device_proc() -> *mut c_void {
    LIFECYCLE.published.device.load(Ordering::Acquire)
}

/// Resolves an export of the loaded interposer without another module reference.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slvk_life_interposer_symbol(name: *const c_char) -> *mut c_void {
    unsafe { LIFECYCLE.interposer_symbol(&Win32, name) }
}

/// Resolves an export of sl.common.dll from the plugin directory if it is loaded.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slvk_life_common_symbol(name: *const c_char) -> *mut c_void {
    unsafe { LIFECYCLE.common_symbol(&Win32, name) }
}
