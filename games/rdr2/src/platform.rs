// SPDX-License-Identifier: MIT
//! Memory reads, MinHook calls and COM leaves shared by the renderer, the scene producer and the
//! ASI owners.
use rsf_minhook_sys::{
    MH_CreateHook, MH_DisableHook, MH_ERROR_DISABLED, MH_ERROR_NOT_CREATED, MH_EnableHook, MH_OK,
    MH_RemoveHook,
};
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};
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
    output_detour, pop_detour, read as read_process_memory, release, resource_facts,
    swapchain_buffer, swapchain_facts,
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

const _: () = assert!(size_of::<ResourceFacts>() == 24);

pub(crate) use rsf_game_sys::win32::{read as read_value, read_bytes};

pub(crate) fn read_pointer(address: usize) -> Option<usize> {
    read_value::<usize>(address)
}

pub(crate) fn matches(address: usize, expected: &[u8]) -> bool {
    let mut bytes = vec![0; expected.len()];
    read_bytes(address, &mut bytes) && bytes == expected
}

pub(crate) fn qpc() -> (u64, u64) {
    let (mut now, mut frequency) = (0, 0);
    unsafe {
        windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut now);
        windows_sys::Win32::System::Performance::QueryPerformanceFrequency(&mut frequency);
    }
    (now as u64, frequency as u64)
}

/// Creates a MinHook detour and publishes its trampoline into `original`.
pub(crate) fn create(target: usize, detour: *const c_void, original: &AtomicPtr<c_void>) -> bool {
    let mut trampoline = ptr::null_mut();
    if unsafe { MH_CreateHook(target as _, detour.cast_mut(), &mut trampoline) } != MH_OK {
        return false;
    }
    original.store(trampoline, Ordering::Release);
    true
}

pub(crate) fn enable(target: usize) -> bool {
    unsafe { MH_EnableHook(target as _) == MH_OK }
}

pub(crate) fn disable(target: usize) -> bool {
    matches!(
        unsafe { MH_DisableHook(target as _) },
        MH_OK | MH_ERROR_NOT_CREATED | MH_ERROR_DISABLED
    )
}

pub(crate) fn remove(target: usize) -> bool {
    matches!(
        unsafe { MH_RemoveHook(target as _) },
        MH_OK | MH_ERROR_NOT_CREATED
    )
}

/// PE header facts read through the non-faulting copy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ImageFacts {
    pub(crate) timestamp: u32,
    pub(crate) image_size: u32,
}

/// The NT header offset and facts of the loaded image at `module`.
pub(crate) fn image_header(module: usize) -> Option<(usize, ImageFacts)> {
    if module == 0 {
        return None;
    }
    let headers = rsf_game_sys::pe32_plus::Mapped::read(&mut |offset, out: *mut u8, size| {
        module
            .checked_add(offset)
            .is_some_and(|address| unsafe { read_process_memory(address, out.cast(), size) })
    })?;
    Some((
        module.checked_add(headers.nt_offset)?,
        ImageFacts {
            timestamp: headers.headers.FileHeader.TimeDateStamp,
            image_size: headers.headers.OptionalHeader.SizeOfImage,
        },
    ))
}

pub(crate) fn image_facts(module: usize) -> Option<ImageFacts> {
    image_header(module).map(|(_, facts)| facts)
}

/// Counts code executing in this module so quiesce and stop can refuse while it runs.
pub(crate) struct Scope<'a>(&'a AtomicU32);

impl<'a> Scope<'a> {
    pub(crate) fn enter(counter: &'a AtomicU32) -> Self {
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
pub(crate) struct Retained(pub(crate) *mut c_void);

impl Drop for Retained {
    fn drop(&mut self) {
        unsafe { release(self.0) };
    }
}
