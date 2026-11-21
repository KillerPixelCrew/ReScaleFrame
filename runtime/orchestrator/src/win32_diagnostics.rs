// SPDX-License-Identifier: MIT
use std::ptr::null_mut;
use windows_sys::Win32::{
    Foundation::MAX_PATH,
    System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        GetModuleFileNameW, GetModuleHandleExW,
    },
};

pub(crate) fn module_location(address: usize) -> Option<(String, usize)> {
    let mut module = null_mut();
    let flags =
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
    if unsafe { GetModuleHandleExW(flags, address as *const u16, &mut module) } == 0
        || module.is_null()
    {
        return None;
    }
    let mut path = [0; MAX_PATH as usize];
    let len = unsafe { GetModuleFileNameW(module, path.as_mut_ptr(), MAX_PATH) } as usize;
    if len == 0 || len >= path.len() {
        return None;
    }
    let path = &path[..len];
    let leaf = path
        .iter()
        .rposition(|&ch| ch == u16::from(b'\\'))
        .map_or(path, |separator| &path[separator + 1..]);
    let offset = address.checked_sub(module.addr())?;
    Some((String::from_utf16_lossy(leaf), offset))
}
