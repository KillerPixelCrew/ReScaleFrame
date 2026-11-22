// SPDX-License-Identifier: MIT
//! Portable interposer lifecycle state machine. The loader and the native SDK
//! leaves are supplied by the caller.

use crate::diagnostics::Diagnostics;
#[cfg(windows)]
use crate::frame_generation::{FrameGeneration, Swapchain};
#[cfg(windows)]
use crate::frame_token::FrameToken;
use crate::sdk_records;
use crate::sdk_records_abi::{rsf_slvk_sdk_log, rsf_slvk_sdk_preferences};
use std::ffi::{CStr, c_char, c_void};
use std::ptr::{NonNull, null, null_mut, without_provenance_mut};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};

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
// SDK callbacks can run host code on SDK threads. Lifecycle operations
// refuse overlapping SDK calls instead of waiting.
pub(crate) const BUSY: i32 = -7;
const OTHER_CONFIGURATION: i32 = -8;

// The bridge formatted the interposer path into a MAX_PATH buffer.
const MAX_PATH: usize = 260;
const INTERPOSER: &str = "\\sl.interposer.dll";
const COMMON: &str = "\\sl.common.dll";
const INVALID_HANDLE: *mut c_void = without_provenance_mut(usize::MAX);

// Feature bits for the native slInit leaf, which maps them to sl::Feature in
// this order.
const FEATURE_PCL: u32 = 1;
const FEATURE_REFLEX: u32 = 1 << 1;
const FEATURE_DLSS_G: u32 = 1 << 2;
const FEATURE_DLSS: u32 = 1 << 3;

/// Selects the slInit feature set. PCL is always loaded; DLSS-G also needs Reflex.
pub(crate) fn feature_mask(reflex: bool, frame_generation: bool, super_resolution: bool) -> u32 {
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
pub(crate) struct Platform {
    /// Opens the file without write or delete sharing and verifies it through
    /// that handle. Returns the open handle or INVALID_HANDLE_VALUE.
    pub open_verified: unsafe extern "C" fn(*const u16) -> *mut c_void,
    /// Resolves the typed SDK exports from the module. Zero when all resolve.
    pub resolve: unsafe extern "C" fn(*mut c_void) -> i32,
    /// Calls slInit with the packed preferences. The project and plugin-path
    /// array stay live until the next initialization.
    pub init: unsafe extern "C" fn(*const rsf_slvk_sdk_preferences, u64) -> i32,
    /// Calls slShutdown when it was resolved and writes its result. Returns
    /// nonzero when the SDK was called.
    pub shutdown: unsafe extern "C" fn(*mut i32) -> u32,
    /// Clears the typed SDK entry points.
    pub clear: unsafe extern "C" fn(),
    pub log: rsf_slvk_sdk_log,
}

const _: () = assert!(size_of::<Platform>() == 6 * size_of::<*mut c_void>());

pub(crate) trait System {
    unsafe fn load(&self, path: *const u16) -> *mut c_void;
    unsafe fn free(&self, module: NonNull<c_void>);
    unsafe fn close(&self, handle: *mut c_void);
    unsafe fn symbol(&self, module: *mut c_void, name: *const c_char) -> *mut c_void;
    unsafe fn loaded(&self, path: *const u16) -> *mut c_void;
    fn verbose(&self) -> bool;
}

struct Paths {
    // NUL-terminated plugin directory.
    directory: Vec<u16>,
    project: Box<CStr>,
    features: u32,
    // The SDK receives this array's address as pathsToPlugins.
    plugin: [*const u16; 1],
}

struct Session {
    module: Option<NonNull<c_void>>,
    // True between a successful slInit and the matching slShutdown. The module
    // can stay loaded after shutdown.
    initialized: bool,
    shutdown_result: Option<i32>,
    paths: Option<Box<Paths>>,
    #[cfg(windows)]
    frame_generation: FrameGeneration,
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

pub(crate) struct Lifecycle {
    session: Mutex<Session>,
    pub diagnostics: Diagnostics,
    #[cfg(windows)]
    swapchain: Mutex<Swapchain>,
    #[cfg(windows)]
    pub frame_token: FrameToken,
    published: Published,
    // sl.common.dll path for presentation lookups, held only around loader queries.
    common: Mutex<Vec<u16>>,
}

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
    pub(crate) const fn new() -> Self {
        Self {
            session: Mutex::new(Session {
                module: None,
                initialized: false,
                shutdown_result: None,
                paths: None,
                #[cfg(windows)]
                frame_generation: FrameGeneration::new(),
            }),
            diagnostics: Diagnostics::new(),
            #[cfg(windows)]
            swapchain: Mutex::new(Swapchain::new()),
            #[cfg(windows)]
            frame_token: FrameToken::new(),
            published: Published {
                module: AtomicPtr::new(null_mut()),
                initialized: AtomicBool::new(false),
                instance: AtomicPtr::new(null_mut()),
                device: AtomicPtr::new(null_mut()),
            },
            common: Mutex::new(Vec::new()),
        }
    }

    // Never blocks: see BUSY.
    fn session(&self) -> Option<MutexGuard<'_, Session>> {
        match self.session.try_lock() {
            Ok(session) => Some(session),
            Err(TryLockError::Poisoned(session)) => Some(session.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        }
    }

    fn with_initialized(&self, inactive: i32, call: impl FnOnce(&mut Session) -> i32) -> i32 {
        let Some(mut session) = self.session() else {
            return BUSY;
        };
        if !session.initialized {
            return inactive;
        }
        call(&mut session)
    }

    pub(crate) fn with_sdk(&self, inactive: i32, call: impl FnOnce() -> i32) -> i32 {
        self.with_initialized(inactive, |_| call())
    }

    #[cfg(windows)]
    pub(crate) fn swapchain(&self) -> MutexGuard<'_, Swapchain> {
        self.swapchain
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    #[cfg(windows)]
    pub(crate) fn with_frame_generation(
        &self,
        inactive: i32,
        call: impl FnOnce(&mut FrameGeneration, u64) -> i32,
    ) -> i32 {
        self.with_initialized(inactive, |session| {
            // Release the WSI guard before the SDK can call back on any thread.
            let revision = self.swapchain().revision;
            call(&mut session.frame_generation, revision)
        })
    }

    #[cfg(windows)]
    fn reset_frame_generation(&self, session: &mut Session) {
        session.frame_generation = FrameGeneration::new();
        *self.swapchain() = Swapchain::new();
    }

    pub(crate) unsafe fn initialize(
        &self,
        system: &impl System,
        platform: &Platform,
        directory: *const u16,
        project: *const c_char,
        features: u32,
    ) -> i32 {
        let Some(mut session) = self.session() else {
            return BUSY;
        };
        if let Some(result) = session.shutdown_result.filter(|&result| result != 0) {
            return result;
        }
        if directory.is_null() || project.is_null() {
            return INVALID_ARGUMENT;
        }
        let directory = unsafe { terminated(directory) };
        let project = unsafe { CStr::from_ptr(project) };
        // A module kept after shutdown may still back Vulkan dispatch tables.
        // Only a module loaded by this call is released on failure.
        let loaded_here = session.module.is_none();
        let module = match session.module {
            Some(module) => {
                let Some(paths) = session.paths.as_ref().filter(|paths| {
                    paths.directory.strip_suffix(&[0]) == Some(directory.as_slice())
                }) else {
                    return OTHER_DIRECTORY;
                };
                if session.initialized {
                    return if *paths.project == *project && paths.features == features {
                        0
                    } else {
                        OTHER_CONFIGURATION
                    };
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
            project: project.into(),
            features,
            plugin: [null()],
        });
        paths.plugin[0] = paths.directory.as_ptr();
        let (project, plugin) = (paths.project.as_ptr(), paths.plugin.as_ptr());
        // The previous strings belong to a session that has already shut down.
        session.paths = Some(paths);
        self.set_common(&directory);
        let mut selected = [0; 4];
        let mut count = 0;
        for feature in sdk_records::features(features) {
            selected[count] = feature;
            count += 1;
        }
        let verbose = system.verbose();
        self.diagnostics.set_verbose(verbose);
        let preferences =
            sdk_records::preferences(&selected[..count], plugin, project, verbose, platform.log);
        let result = unsafe { (platform.init)(&preferences, sdk_records::VERSION) };
        if result != 0 {
            unsafe { self.abandon(system, platform, &mut session, loaded_here) };
            return result;
        }
        session.initialized = true;
        session.shutdown_result = None;
        #[cfg(windows)]
        self.frame_token.open();
        self.published.initialized.store(true, Ordering::Release);
        0
    }

    pub(crate) unsafe fn shutdown(&self, platform: &Platform) -> i32 {
        let Some(mut session) = self.session() else {
            return BUSY;
        };
        if session.module.is_none() {
            return 0;
        }
        if let Some(result) = session.shutdown_result {
            return result;
        }
        #[cfg(windows)]
        if !self.frame_token.try_close() {
            return BUSY;
        }
        // An SDK failure may follow partial plugin destruction. Spend the attempt
        // before calling it; neither feature entry points nor retries remain valid.
        session.shutdown_result = Some(0);
        let mut result = 0;
        if session.initialized {
            let mut sdk_result = 0;
            if unsafe { (platform.shutdown)(&mut sdk_result) } != 0 {
                result = sdk_result;
                session.shutdown_result = Some(result);
            }
        }
        session.initialized = false;
        self.published.initialized.store(false, Ordering::Release);
        // Only the Vulkan dispatch entry points remain valid after slShutdown.
        #[cfg(windows)]
        self.reset_frame_generation(&mut session);
        unsafe { (platform.clear)() };
        self.diagnostics.present_error(0);
        result
    }

    // Call only after shutdown and after every Vulkan object and API table that
    // references the interposer is destroyed. A failed shutdown keeps the module.
    pub(crate) unsafe fn unload(&self, system: &impl System, platform: &Platform) -> i32 {
        let Some(mut session) = self.session() else {
            return BUSY;
        };
        if let Some(result) = session.shutdown_result.filter(|&result| result != 0) {
            return result;
        }
        if session.initialized {
            return BUSY;
        }
        #[cfg(windows)]
        if !self.frame_token.try_close() {
            return BUSY;
        }
        unsafe { self.release(system, platform, &mut session) };
        0
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
            unsafe { (platform.clear)() };
        }
    }

    // Withdraws the published entry points before the loader can unmap them.
    unsafe fn release(&self, system: &impl System, platform: &Platform, session: &mut Session) {
        self.published.initialized.store(false, Ordering::Release);
        self.published.instance.store(null_mut(), Ordering::Release);
        self.published.device.store(null_mut(), Ordering::Release);
        self.published.module.store(null_mut(), Ordering::Release);
        if let Some(module) = session.module.take() {
            #[cfg(windows)]
            self.reset_frame_generation(session);
            unsafe { (platform.clear)() };
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

    pub(crate) fn initialized(&self) -> bool {
        self.published.initialized.load(Ordering::Acquire)
    }

    pub(crate) fn instance_proc(&self) -> *mut c_void {
        self.published.instance.load(Ordering::Acquire)
    }

    pub(crate) fn device_proc(&self) -> *mut c_void {
        self.published.device.load(Ordering::Acquire)
    }

    pub(crate) fn module(&self) -> *mut c_void {
        if self.initialized() {
            self.published.module.load(Ordering::Acquire)
        } else {
            null_mut()
        }
    }

    pub(crate) unsafe fn interposer_symbol(
        &self,
        system: &impl System,
        name: *const c_char,
    ) -> *mut c_void {
        let module = self.published.module.load(Ordering::Acquire);
        if module.is_null() || name.is_null() {
            return null_mut();
        }
        unsafe { system.symbol(module, name) }
    }

    pub(crate) unsafe fn common_symbol(
        &self,
        system: &impl System,
        name: *const c_char,
    ) -> *mut c_void {
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
