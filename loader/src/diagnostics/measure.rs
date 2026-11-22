// SPDX-License-Identifier: MIT
use super::entropy::{entropy, sampled};
use super::memory::{read, rsf_dump_read_guarded};
use std::ffi::c_void;
use std::mem::{offset_of, size_of_val};
use std::ptr::null;
use windows_sys::Win32::System::{
    Diagnostics::Debug::{
        IMAGE_NT_HEADERS64, IMAGE_NT_OPTIONAL_HDR64_MAGIC, IMAGE_SCN_MEM_EXECUTE,
        IMAGE_SECTION_HEADER,
    },
    LibraryLoader::GetModuleHandleW,
    SystemInformation::GetTickCount64,
    SystemServices::{IMAGE_DOS_HEADER, IMAGE_DOS_SIGNATURE, IMAGE_NT_SIGNATURE},
    Threading::Sleep,
};

const OK: i32 = 0;
const INVALID_ARGUMENT: i32 = -1;
const NOT_A_PE: i32 = -3;
const WRITE_FAILED: i32 = -4;
const STILL_ENCRYPTED: i32 = -5;
const WINDOW: usize = 65_536;
const SAMPLES: usize = 16;

// T must have no invalid bit patterns. Use the PE records from the Windows bindings.
unsafe fn record<T: Default>(base: usize, offset: usize) -> Option<T> {
    let address = base.checked_add(offset)?;
    let mut raw = T::default();
    let size = size_of_val(&raw);
    let (ok, copied) = unsafe { read(address, (&raw mut raw).cast(), size) };
    (ok && copied == size).then_some(raw)
}

fn executable_span(base: usize) -> Option<(usize, usize)> {
    let dos: IMAGE_DOS_HEADER = unsafe { record(base, 0) }?;
    if dos.e_magic != IMAGE_DOS_SIGNATURE {
        return None;
    }
    let nt = usize::try_from(dos.e_lfanew).ok()?;
    let headers: IMAGE_NT_HEADERS64 = unsafe { record(base, nt) }?;
    if headers.Signature != IMAGE_NT_SIGNATURE
        || headers.OptionalHeader.Magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC
    {
        return None;
    }
    let table = nt
        .checked_add(offset_of!(IMAGE_NT_HEADERS64, OptionalHeader))?
        .checked_add(headers.FileHeader.SizeOfOptionalHeader.into())?;
    let count = usize::from(headers.FileHeader.NumberOfSections);
    let stride = size_of::<IMAGE_SECTION_HEADER>();
    let image_size = headers.OptionalHeader.SizeOfImage as usize;
    if table.checked_add(count * stride)? > image_size {
        return None;
    }
    let mut largest = (0, 0);
    for index in 0..count {
        let section: IMAGE_SECTION_HEADER = unsafe { record(base, table + index * stride) }?;
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_measure_module_code(module: *const c_void, output: *mut f64) -> i32 {
    if output.is_null() {
        return INVALID_ARGUMENT;
    }
    let base = if module.is_null() {
        (unsafe { GetModuleHandleW(null()) }) as usize
    } else {
        module as usize
    };
    let Some((address, size)) = executable_span(base) else {
        return NOT_A_PE;
    };
    let mut buffer = Vec::new();
    if buffer.try_reserve_exact(WINDOW.min(size)).is_err() {
        return WRITE_FAILED;
    }
    buffer.resize(WINDOW.min(size), 0);
    let result = sampled(size, WINDOW, SAMPLES, |offset, length| {
        unsafe {
            rsf_dump_read_guarded((address + offset) as *const u8, buffer.as_mut_ptr(), length)
        };
        entropy(&buffer[..length])
    });
    unsafe { output.write(result) };
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_dump_poll_code(
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
