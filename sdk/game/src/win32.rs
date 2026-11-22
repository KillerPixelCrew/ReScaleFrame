// SPDX-License-Identifier: MIT
//! Windows helpers for GameSDK adapters that borrow live modules.

use std::ffi::{CStr, c_void};
use std::mem::MaybeUninit;
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, GetProcAddress};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

const MODULE_PATH_CAPACITY: usize = 32768;

/// An owned Windows module path with one trailing NUL.
pub struct ModulePath(Vec<u16>);

impl ModulePath {
    pub fn units(&self) -> &[u16] {
        &self.0[..self.0.len() - 1]
    }

    pub fn terminated(&self) -> &[u16] {
        &self.0
    }
}

/// Reads a path into caller-supplied storage and returns its units without the NUL.
/// A null module selects the process executable.
///
/// # Safety
/// A non-null module must be a valid module handle kept loaded for this call.
pub unsafe fn module_path_in(module: HMODULE, buffer: &mut [u16]) -> Option<&[u16]> {
    let capacity = buffer.len().min(MODULE_PATH_CAPACITY);
    if capacity == 0 {
        return None;
    }
    let length =
        unsafe { GetModuleFileNameW(module, buffer.as_mut_ptr(), capacity as u32) } as usize;
    (length != 0 && length < capacity).then(|| &buffer[..length])
}

/// Reads a path with room for the maximum Windows path and rejects truncation.
///
/// # Safety
/// A non-null module must be a valid module handle kept loaded for this call.
pub unsafe fn module_path(module: HMODULE) -> Option<ModulePath> {
    let mut path = vec![0; MODULE_PATH_CAPACITY];
    let length = unsafe { module_path_in(module, &mut path) }?.len();
    path.truncate(length + 1);
    path[length] = 0;
    Some(ModulePath(path))
}

/// Resolves a function export from a borrowed module.
///
/// # Safety
/// The module must remain loaded through lookup and every use of the result.
/// F must be the function pointer with the exact ABI and signature of name.
pub unsafe fn typed_export<F: Copy>(module: HMODULE, name: &CStr) -> Option<F> {
    type Export = unsafe extern "system" fn() -> isize;
    const { assert!(size_of::<F>() == size_of::<Export>()) };
    if module.is_null() {
        return None;
    }
    unsafe { GetProcAddress(module, name.as_ptr().cast()) }
        .map(|address| unsafe { std::mem::transmute_copy::<Export, F>(&address) })
}

/// Copies `size` bytes of this process at `address` into `out`: all of them, or false and never a
/// fault when any byte is unreadable.
///
/// # Safety
/// A non-null `out` is writable for `size` bytes.
pub unsafe extern "C" fn read_process(address: usize, out: *mut c_void, size: usize) -> bool {
    let mut copied = 0;
    address != 0
        && !out.is_null()
        && unsafe {
            ReadProcessMemory(
                GetCurrentProcess(),
                address as *const c_void,
                out,
                size,
                &mut copied,
            )
        } != 0
        && copied == size
}

/// A guarded read of `out.len()` bytes of this process at `address`: all of them, or false.
pub fn read_bytes(address: usize, out: &mut [u8]) -> bool {
    // SAFETY: out is writable for its length.
    unsafe { read_process(address, out.as_mut_ptr().cast(), out.len()) }
}

/// Plain data that every bit pattern of its size represents.
///
/// # Safety
/// Implementors must accept any byte contents.
pub unsafe trait Plain: Copy {}
unsafe impl Plain for u8 {}
unsafe impl Plain for u16 {}
unsafe impl Plain for i32 {}
unsafe impl Plain for u32 {}
unsafe impl Plain for u64 {}
unsafe impl Plain for usize {}
unsafe impl Plain for f32 {}
unsafe impl<const N: usize> Plain for [u8; N] {}
unsafe impl<const N: usize> Plain for [f32; N] {}
unsafe impl Plain for windows_sys::Win32::UI::WindowsAndMessaging::MSG {}

/// A guarded read of one value of this process: the whole value or nothing, never a fault.
pub fn read<T: Plain>(address: usize) -> Option<T> {
    let mut value = MaybeUninit::<T>::uninit();
    // SAFETY: the read fills every byte or reports false, and T accepts any contents.
    unsafe { read_process(address, value.as_mut_ptr().cast(), size_of::<T>()) }
        .then(|| unsafe { value.assume_init() })
}
