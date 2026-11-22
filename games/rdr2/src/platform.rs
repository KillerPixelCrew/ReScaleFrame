// SPDX-License-Identifier: MIT
//! Platform services used by the game renderer and synthetic fixtures.
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

#[cfg(windows)]
#[path = "native_input.rs"]
mod input;
#[cfg(windows)]
#[path = "native_platform.rs"]
mod native;
#[cfg(windows)]
pub(crate) use input::{
    disable as input_disable, idle as input_idle, install as input_install, remove as input_remove,
    unhook as input_unhook,
};
#[cfg(windows)]
pub(crate) use native::{
    attach_present, attach_queue, back_buffer_index, barrier, blit_detour, export_address,
    fsr_dispatch_detour, initialize as hook_initialize, main_module, module_handle, module_path,
    output_detour, pop_detour, read as read_process_memory, resource_facts, swapchain_buffer,
    swapchain_facts,
};

#[cfg(all(windows, target_arch = "x86_64"))]
pub(crate) unsafe extern "C" fn worker_tls_slots() -> usize {
    let slots: usize;
    // Windows x64 TEB.ThreadLocalStoragePointer, equal to __readgsqword(0x58).
    unsafe {
        std::arch::asm!("mov {}, gs:[0x58]", out(reg) slots, options(nostack, readonly, preserves_flags))
    };
    slots
}

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

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub(crate) struct ResourceFacts {
    pub(crate) width: u64,
    pub(crate) height: u32,
    pub(crate) format: u32,
    pub(crate) samples: u32,
    _reserved: u32,
}

/// The Windows and COM services of the game DLL (`native_platform.rs`).
pub(crate) struct Platform;

const _: () = assert!(size_of::<ResourceFacts>() == 24);

/// The platform. Unit tests see none, so they never reach the game image.
pub(crate) fn get() -> Option<&'static Platform> {
    if cfg!(test) { None } else { Some(&Platform) }
}

impl Platform {
    pub(crate) fn read_bytes(&self, address: usize, out: &mut [u8]) -> bool {
        address != 0 && unsafe { native::read(address, out.as_mut_ptr().cast(), out.len()) }
    }

    /// Copies a plain-data value. `T` must be valid for every bit pattern.
    pub(crate) fn read_value<T: Copy + Default>(&self, address: usize) -> Option<T> {
        let mut value = T::default();
        let read = address != 0
            && unsafe { native::read(address, (&raw mut value).cast(), size_of::<T>()) };
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
        unsafe { native::qpc(&mut now, &mut frequency) };
        (now, frequency)
    }

    pub(crate) fn create(
        &self,
        target: usize,
        detour: *const c_void,
        original: &AtomicPtr<c_void>,
    ) -> bool {
        let mut trampoline = ptr::null_mut();
        if !unsafe { native::create(target, detour, &mut trampoline) } {
            return false;
        }
        original.store(trampoline, Ordering::Release);
        true
    }

    pub(crate) fn enable(&self, target: usize) -> bool {
        unsafe { native::enable(target) }
    }

    pub(crate) fn disable(&self, target: usize) -> bool {
        unsafe { native::disable(target) }
    }

    pub(crate) fn remove(&self, target: usize) -> bool {
        unsafe { native::remove(target) }
    }

    pub(crate) fn release(&self, object: *mut c_void) {
        if !object.is_null() {
            unsafe { native::release(object) };
        }
    }
}

/// PE header facts read through the non-faulting copy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ImageFacts {
    pub(crate) timestamp: u32,
    pub(crate) image_size: u32,
}

pub(crate) fn image_facts(module: usize) -> Option<ImageFacts> {
    image_header(native::read, module).map(|(_, facts)| facts)
}

pub(crate) const PE_SIGNATURE: u32 = 0x4550;

pub(crate) fn image_header(
    read: unsafe extern "C" fn(usize, *mut c_void, usize) -> bool,
    module: usize,
) -> Option<(usize, ImageFacts)> {
    if module == 0 {
        return None;
    }
    let headers = rsf_game_sys::pe32_plus::Mapped::read(&mut |offset, out: *mut u8, size| {
        module
            .checked_add(offset)
            .is_some_and(|address| unsafe { read(address, out.cast(), size) })
    })?;
    Some((
        module.checked_add(headers.nt_offset)?,
        ImageFacts {
            timestamp: headers.headers.FileHeader.TimeDateStamp,
            image_size: headers.headers.OptionalHeader.SizeOfImage,
        },
    ))
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
