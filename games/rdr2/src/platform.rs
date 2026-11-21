// SPDX-License-Identifier: MIT
//! Typed native leaves supplied by `native_platform.cpp`. The table carries MinHook, D3D12/DXGI,
//! Win32 input/memory and caller-capture operations only; every RDR2 decision stays in this crate.
//! The archive imports no native symbol, so fixtures can link it without the plugin's leaves.
use std::ffi::{c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Hook callbacks must not unwind; a poisoned lock keeps its last consistent state.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The native routine MinHook or a cloned table stored in `slot`.
///
/// # Safety
/// `F` must be the function pointer type of the routine stored in `slot`.
pub(crate) unsafe fn trampoline<F: Copy>(slot: &AtomicPtr<c_void>) -> Option<F> {
    const { assert!(size_of::<F>() == size_of::<*mut c_void>()) };
    let pointer = slot.load(Ordering::Acquire);
    (!pointer.is_null()).then(|| unsafe { std::mem::transmute_copy::<*mut c_void, F>(&pointer) })
}

pub(crate) const PLATFORM_VERSION: u32 = 1;

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub(crate) struct ResourceFacts {
    pub(crate) width: u64,
    pub(crate) height: u32,
    pub(crate) format: u32,
    pub(crate) samples: u32,
    _reserved: u32,
}

#[repr(C)]
pub(crate) struct Platform {
    _struct_size: u32,
    _version: u32,
    // Process, image and memory. `read` is a non-faulting ReadProcessMemory copy.
    pub(crate) main_module: unsafe extern "C" fn() -> usize,
    pub(crate) module_handle: unsafe extern "C" fn(name: *const c_char) -> usize,
    pub(crate) export_address: unsafe extern "C" fn(module: usize, name: *const c_char) -> usize,
    pub(crate) read: unsafe extern "C" fn(address: usize, out: *mut c_void, size: usize) -> bool,
    pub(crate) thread_slots: unsafe extern "C" fn() -> usize,
    pub(crate) qpc: unsafe extern "C" fn(now: *mut u64, frequency: *mut u64),
    pub(crate) module_path: unsafe extern "C" fn(
        address: usize,
        base: *mut usize,
        path: *mut c_char,
        capacity: usize,
    ) -> bool,
    // MinHook.
    pub(crate) hook_initialize: unsafe extern "C" fn() -> bool,
    pub(crate) hook_create: unsafe extern "C" fn(
        target: usize,
        detour: *const c_void,
        original: *mut *mut c_void,
    ) -> bool,
    pub(crate) hook_enable: unsafe extern "C" fn(target: usize) -> bool,
    pub(crate) hook_disable: unsafe extern "C" fn(target: usize) -> bool,
    pub(crate) hook_remove: unsafe extern "C" fn(target: usize) -> bool,
    // Detours that capture the native return address before forwarding to this crate.
    pub(crate) pop_detour: *const c_void,
    pub(crate) output_detour: *const c_void,
    pub(crate) blit_detour: *const c_void,
    pub(crate) fsr_dispatch_detour: *const c_void,
    // D3D12/DXGI. Attach clones the object's own table and retains one reference on success.
    pub(crate) attach_queue: unsafe extern "C" fn(
        queue: *mut c_void,
        detour: *const c_void,
        original: *mut *mut c_void,
    ) -> bool,
    pub(crate) attach_present: unsafe extern "C" fn(
        swapchain: *mut c_void,
        detour: *const c_void,
        original: *mut *mut c_void,
    ) -> bool,
    pub(crate) resource_facts:
        unsafe extern "C" fn(resource: *mut c_void, out: *mut ResourceFacts) -> bool,
    pub(crate) barrier:
        unsafe extern "C" fn(list: *mut c_void, resource: *mut c_void, before: u32, after: u32),
    pub(crate) swapchain_facts:
        unsafe extern "C" fn(swapchain: *mut c_void, buffers: *mut u32, window: *mut u64) -> bool,
    /// Returns a referenced buffer or null.
    pub(crate) swapchain_buffer:
        unsafe extern "C" fn(swapchain: *mut c_void, index: u32) -> *mut c_void,
    pub(crate) back_buffer_index: unsafe extern "C" fn(swapchain: *mut c_void) -> u32,
    pub(crate) release: unsafe extern "C" fn(object: *mut c_void),
    // Window-message and XInput read facts on the game's input thread.
    pub(crate) input_install: unsafe extern "C" fn(
        message: unsafe extern "C" fn(u32, u32),
        controller: unsafe extern "C" fn(),
    ),
    pub(crate) input_disable: unsafe extern "C" fn(),
    pub(crate) input_idle: unsafe extern "C" fn() -> bool,
    pub(crate) input_unhook: unsafe extern "C" fn(),
    pub(crate) input_remove: unsafe extern "C" fn() -> bool,
}

const _: () = {
    assert!(size_of::<ResourceFacts>() == 24);
    assert!(size_of::<Platform>() == 8 + 29 * size_of::<usize>());
    assert!(std::mem::offset_of!(Platform, pop_detour) == 8 + 12 * 8);
    assert!(std::mem::offset_of!(Platform, attach_queue) == 8 + 16 * 8);
    assert!(std::mem::offset_of!(Platform, input_install) == 8 + 24 * 8);
};

// The native table is a constant with static storage in the plugin DLL.
unsafe impl Sync for Platform {}

static PLATFORM: AtomicPtr<Platform> = AtomicPtr::new(ptr::null_mut());

/// Accepts the plugin's native table once its size and version match this crate.
pub(crate) fn register(platform: *const Platform) {
    if platform.is_null() {
        return;
    }
    // Only the leading header is readable from a short or foreign table.
    let header = platform.cast::<u32>();
    let (size, version) = unsafe { (header.read(), header.add(1).read()) };
    if size as usize == size_of::<Platform>() && version == PLATFORM_VERSION {
        PLATFORM.store(platform.cast_mut(), Ordering::Release);
    }
}

pub(crate) fn get() -> Option<&'static Platform> {
    // A registered table lives in the plugin DLL's constant data until unload.
    unsafe { PLATFORM.load(Ordering::Acquire).as_ref() }
}

impl Platform {
    pub(crate) fn read_bytes(&self, address: usize, out: &mut [u8]) -> bool {
        address != 0 && unsafe { (self.read)(address, out.as_mut_ptr().cast(), out.len()) }
    }

    /// Copies a plain-data value. `T` must be valid for every bit pattern.
    pub(crate) fn read_value<T: Copy + Default>(&self, address: usize) -> Option<T> {
        let mut value = T::default();
        let read = address != 0
            && unsafe { (self.read)(address, (&raw mut value).cast(), size_of::<T>()) };
        read.then_some(value)
    }

    pub(crate) fn read_pointer(&self, address: usize) -> Option<usize> {
        self.read_value::<usize>(address)
    }

    pub(crate) fn matches(&self, address: usize, expected: &[u8; 16]) -> bool {
        let mut bytes = [0; 16];
        self.read_bytes(address, &mut bytes) && bytes == *expected
    }

    pub(crate) fn qpc(&self) -> (u64, u64) {
        let (mut now, mut frequency) = (0, 0);
        unsafe { (self.qpc)(&mut now, &mut frequency) };
        (now, frequency)
    }

    pub(crate) fn create(
        &self,
        target: usize,
        detour: *const c_void,
        original: &AtomicPtr<c_void>,
    ) -> bool {
        unsafe { (self.hook_create)(target, detour, original.as_ptr()) }
    }

    pub(crate) fn enable(&self, target: usize) -> bool {
        unsafe { (self.hook_enable)(target) }
    }

    pub(crate) fn disable(&self, target: usize) -> bool {
        unsafe { (self.hook_disable)(target) }
    }

    pub(crate) fn remove(&self, target: usize) -> bool {
        unsafe { (self.hook_remove)(target) }
    }

    pub(crate) fn release(&self, object: *mut c_void) {
        if !object.is_null() {
            unsafe { (self.release)(object) };
        }
    }
}

/// PE header facts read through the non-faulting copy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ImageFacts {
    pub(crate) timestamp: u32,
    pub(crate) image_size: u32,
}

pub(crate) fn image_facts(platform: &Platform, module: usize) -> Option<ImageFacts> {
    const DOS_SIGNATURE: u16 = 0x5a4d;
    const PE_SIGNATURE: u32 = 0x4550;
    if module == 0 || platform.read_value::<u16>(module)? != DOS_SIGNATURE {
        return None;
    }
    let nt =
        module.checked_add(usize::try_from(platform.read_value::<i32>(module + 0x3c)?).ok()?)?;
    if platform.read_value::<u32>(nt)? != PE_SIGNATURE {
        return None;
    }
    // IMAGE_FILE_HEADER.TimeDateStamp and IMAGE_OPTIONAL_HEADER64.SizeOfImage.
    Some(ImageFacts {
        timestamp: platform.read_value(nt + 8)?,
        image_size: platform.read_value(nt + 0x50)?,
    })
}

/// Counts code executing in this module so quiesce and stop can refuse while it runs.
pub(crate) struct Scope<'a>(&'a std::sync::atomic::AtomicU32);

impl<'a> Scope<'a> {
    pub(crate) fn enter(counter: &'a std::sync::atomic::AtomicU32) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self(counter)
    }
}

impl Drop for Scope<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A referenced COM object released on every return path.
pub(crate) struct Retained<'a> {
    pub(crate) platform: &'a Platform,
    pub(crate) object: *mut c_void,
}

impl Drop for Retained<'_> {
    fn drop(&mut self) {
        self.platform.release(self.object);
    }
}
