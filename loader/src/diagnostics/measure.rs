// SPDX-License-Identifier: MIT
use super::entropy::{entropy, sampled};
use super::memory::{read, read_guarded};
use rsf_game_sys::pe32_plus::Mapped;
use std::ffi::c_void;
use std::ptr::null;
use windows_sys::Win32::System::{
    Diagnostics::Debug::IMAGE_SCN_MEM_EXECUTE, LibraryLoader::GetModuleHandleW,
    SystemInformation::GetTickCount64, Threading::Sleep,
};

const OK: i32 = 0;
const INVALID_ARGUMENT: i32 = -1;
const NOT_A_PE: i32 = -3;
const WRITE_FAILED: i32 = -4;
const STILL_ENCRYPTED: i32 = -5;
const WINDOW: usize = 65_536;
const SAMPLES: usize = 16;

/// The guarded reader `Mapped` takes, over the module at `base`.
pub(super) fn image(base: usize) -> impl FnMut(usize, *mut u8, usize) -> bool {
    move |offset, out, size| {
        base.checked_add(offset).is_some_and(|address| {
            let (ok, copied) = unsafe { read(address, out, size) };
            ok && copied == size
        })
    }
}

fn executable_span(base: usize) -> Option<(usize, usize)> {
    let mut read = image(base);
    let pe = Mapped::read(&mut read)?;
    if !pe.sections_fit() {
        return None;
    }
    let image_size = pe.image_size();
    let mut largest = (0, 0);
    for index in 0..pe.section_count() {
        let section = pe.section(&mut read, index)?;
        if section.Characteristics & IMAGE_SCN_MEM_EXECUTE != 0 {
            let size = unsafe { section.Misc.VirtualSize } as usize;
            if size > largest.1 {
                largest = (section.VirtualAddress as usize, size);
            }
        }
    }
    let (rva, size) = largest;
    if size == 0 || rva.checked_add(size)? > image_size {
        return None;
    }
    let address = base.checked_add(rva)?;
    address.checked_add(size)?;
    Some((address, size))
}

/// The module at module, or the executable when that is null.
pub(super) fn module_base(module: *const c_void) -> usize {
    if module.is_null() {
        (unsafe { GetModuleHandleW(null()) }) as usize
    } else {
        module as usize
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_measure_module_code(
    module: *const c_void,
    output: *mut f64,
) -> i32 {
    if output.is_null() {
        return INVALID_ARGUMENT;
    }
    let Some((address, size)) = executable_span(module_base(module)) else {
        return NOT_A_PE;
    };
    let mut buffer = Vec::new();
    if buffer.try_reserve_exact(WINDOW.min(size)).is_err() {
        return WRITE_FAILED;
    }
    buffer.resize(WINDOW.min(size), 0);
    let result = sampled(size, WINDOW, SAMPLES, |offset, length| {
        read_guarded(address + offset, &mut buffer[..length]);
        entropy(&buffer[..length])
    });
    unsafe { output.write(result) };
    OK
}

pub(super) fn rsf_dump_poll_code(
    threshold: f64,
    poll_ms: u32,
    timeout_ms: u32,
    required_stable: u32,
    last_entropy: &mut f64,
) -> i32 {
    if poll_ms == 0 || required_stable == 0 {
        return INVALID_ARGUMENT;
    }
    let start = unsafe { GetTickCount64() };
    let mut stable = 0;
    loop {
        let measured = unsafe { rsf_measure_module_code(null(), last_entropy) };
        if measured != OK {
            return measured;
        }
        stable = if *last_entropy < threshold {
            stable + 1
        } else {
            0
        };
        if stable >= required_stable {
            return OK;
        }
        if timeout_ms != 0 && unsafe { GetTickCount64() } - start >= u64::from(timeout_ms) {
            return STILL_ENCRYPTED;
        }
        unsafe { Sleep(poll_ms) };
    }
}
