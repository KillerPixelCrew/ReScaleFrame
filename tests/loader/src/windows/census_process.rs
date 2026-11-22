// SPDX-License-Identifier: MIT
// Process and module facts consumed by the native census fixture.
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use windows_sys::Win32::{
    Foundation::{GetLastError, HMODULE, MAX_PATH, SetLastError},
    System::{
        Diagnostics::Debug::{
            IMAGE_NT_HEADERS64, IMAGE_NT_OPTIONAL_HDR64_MAGIC, ReadProcessMemory,
        },
        LibraryLoader::{GetModuleFileNameW, GetModuleHandleW, LoadLibraryW},
        SystemServices::{IMAGE_DOS_HEADER, IMAGE_DOS_SIGNATURE, IMAGE_NT_SIGNATURE},
        Threading::GetCurrentProcess,
    },
};

fn image_base() -> HMODULE {
    unsafe { GetModuleHandleW(null()) }
}

// PE records used here have no Rust validity restrictions on their bit patterns.
unsafe fn image_record<T: Default>(address: usize) -> Option<T> {
    let mut record = T::default();
    let mut copied = 0;
    let ok = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            address as _,
            (&raw mut record).cast(),
            size_of::<T>(),
            &mut copied,
        )
    };
    (ok != 0 && copied == size_of::<T>()).then_some(record)
}

pub(super) fn executable_anchor(anchor: *const c_void) -> Option<(u32, u64)> {
    let base = image_base() as usize;
    if base == 0 || anchor.is_null() {
        return None;
    }
    let dos: IMAGE_DOS_HEADER = unsafe { image_record(base) }?;
    if dos.e_magic != IMAGE_DOS_SIGNATURE {
        return None;
    }
    let displacement = usize::try_from(dos.e_lfanew).ok()?;
    let headers: IMAGE_NT_HEADERS64 = unsafe { image_record(base.checked_add(displacement)?) }?;
    if headers.Signature != IMAGE_NT_SIGNATURE
        || headers.OptionalHeader.Magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC
    {
        return None;
    }
    let size = headers.OptionalHeader.SizeOfImage;
    let rva = (anchor as usize).checked_sub(base)?;
    (rva < size as usize).then_some((size, rva as u64))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_census_load_fixture(file: *const u16) -> HMODULE {
    let mut path = [0u16; MAX_PATH as usize];
    let length = unsafe { GetModuleFileNameW(null_mut(), path.as_mut_ptr(), MAX_PATH) } as usize;
    if file.is_null() || length == 0 || length >= path.len() {
        return null_mut();
    }
    let directory = path[..length]
        .iter()
        .rposition(|&unit| unit == u16::from(b'\\'))
        .unwrap_or(length);
    let Some(tail) = (0..MAX_PATH as usize).find(|&i| unsafe { file.add(i).read() } == 0) else {
        return null_mut();
    };
    let file = unsafe { std::slice::from_raw_parts(file, tail) };
    let mut full_path = Vec::with_capacity(directory + tail + 2);
    full_path.extend_from_slice(&path[..directory]);
    full_path.push(u16::from(b'\\'));
    full_path.extend_from_slice(file);
    full_path.push(0);
    let module = unsafe { LoadLibraryW(full_path.as_ptr()) };
    let error = unsafe { GetLastError() };
    drop(full_path);
    unsafe { SetLastError(error) };
    module
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_census_loaded_module(name: *const u16) -> HMODULE {
    unsafe { GetModuleHandleW(name) }
}
