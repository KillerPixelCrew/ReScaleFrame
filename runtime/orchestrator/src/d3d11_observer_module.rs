// SPDX-License-Identifier: MIT
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Read as _;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::OpenOptionsExt;
use std::ptr;

const FILE_SHARE_READ: u32 = 1;
const MAX_FILE: u64 = 1 << 28;
const OK: u32 = 0;
const OPEN_FAILED: u32 = 1;
const READ_FAILED: u32 = 2;
const INVALID_HEADERS: u32 = 3;

#[path = "../../common/src/pe32_plus.rs"]
mod pe32_plus;
use pe32_plus::Pe32Plus;

struct ModuleFile {
    bytes: Vec<u8>,
}

fn open(path: &[u16]) -> Result<ModuleFile, u32> {
    let path = OsString::from_wide(path);
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)
        .map_err(|_| OPEN_FAILED)?;
    let length = file.metadata().map_err(|_| READ_FAILED)?.len();
    if !(1..MAX_FILE).contains(&length) {
        return Err(READ_FAILED);
    }
    let length = length as usize;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(|_| READ_FAILED)?;
    bytes.resize(length, 0);
    file.read_exact(&mut bytes).map_err(|_| READ_FAILED)?;
    Pe32Plus::parse(&bytes).ok_or(INVALID_HEADERS)?;
    Ok(ModuleFile { bytes })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer11_module_open(
    path: *const u16,
    length: u32,
    out: *mut *mut ModuleFile,
) -> u32 {
    if out.is_null() {
        return READ_FAILED;
    }
    unsafe { out.write(ptr::null_mut()) };
    if path.is_null() || length == 0 {
        return OPEN_FAILED;
    }
    let path = unsafe { std::slice::from_raw_parts(path, length as usize) };
    let result = match open(path) {
        Ok(file) => crate::allocate_handle(file),
        Err(error) => return error,
    };
    if result.is_null() {
        return READ_FAILED;
    }
    unsafe { out.write(result) };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer11_module_preferred_base(file: &ModuleFile) -> u64 {
    Pe32Plus::parse(&file.bytes).map_or(0, |image| image.preferred_base())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer11_module_span(
    file: &ModuleFile,
    rva: usize,
    count: usize,
) -> *const u8 {
    Pe32Plus::parse(&file.bytes)
        .and_then(|image| image.span(rva, count))
        .map_or(ptr::null(), |(bytes, _)| bytes.as_ptr())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_observer11_module_close(file: *mut ModuleFile) {
    if !file.is_null() {
        drop(unsafe { Box::from_raw(file) });
    }
}
