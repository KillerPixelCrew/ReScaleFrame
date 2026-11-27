// SPDX-License-Identifier: MIT
use std::ffi::{CStr, c_void};
use std::ptr::NonNull;

#[cfg(windows)]
mod platform {
    use super::*;
    use windows_sys::Win32::Foundation::{GetLastError, INVALID_HANDLE_VALUE, MAX_PATH};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FindClose, FindFirstFileW, FindNextFileW, WIN32_FIND_DATAW,
    };
    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryA, LoadLibraryExW,
    };
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

    pub(crate) const NAME: &CStr = c"nvngx.dll";
    const REPOSITORY: &str = "\\DriverStore\\FileRepository\\";

    fn search_path(
        system: &[u16],
        suffix: impl IntoIterator<Item = u16>,
    ) -> Option<[u16; MAX_PATH as usize]> {
        let mut path = [0; MAX_PATH as usize];
        for (index, unit) in system
            .iter()
            .copied()
            .chain(REPOSITORY.encode_utf16())
            .chain(suffix)
            .chain([0])
            .enumerate()
        {
            *path.get_mut(index)? = unit;
        }
        Some(path)
    }

    fn load_from_driver_store() -> Option<NonNull<c_void>> {
        let mut system = [0u16; MAX_PATH as usize];
        let len = unsafe { GetSystemDirectoryW(system.as_mut_ptr(), MAX_PATH) } as usize;
        if len == 0 || len >= system.len() {
            return None;
        }
        let system = &system[..len];
        let pattern = search_path(system, "nv*.inf_amd64_*".encode_utf16())?;
        let mut entry = WIN32_FIND_DATAW::default();
        let search = unsafe { FindFirstFileW(pattern.as_ptr(), &mut entry) };
        if search == INVALID_HANDLE_VALUE {
            return None;
        }
        let module = loop {
            if entry.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                let name = entry.cFileName.into_iter().take_while(|&unit| unit != 0);
                let suffix = name.chain("\\nvngx.dll".encode_utf16());
                let Some(path) = search_path(system, suffix) else {
                    break None;
                };
                let module = unsafe {
                    LoadLibraryExW(
                        path.as_ptr(),
                        std::ptr::null_mut(),
                        LOAD_WITH_ALTERED_SEARCH_PATH,
                    )
                };
                if let Some(module) = NonNull::new(module) {
                    break Some(module);
                }
            }
            if unsafe { FindNextFileW(search, &mut entry) } == 0 {
                break None;
            }
        };
        unsafe { FindClose(search) };
        module
    }

    pub(crate) fn open() -> Result<NonNull<c_void>, Vec<u8>> {
        let module = unsafe { LoadLibraryA(NAME.as_ptr().cast()) };
        NonNull::new(module)
            .or_else(load_from_driver_store)
            .ok_or_else(|| {
                let error = unsafe { GetLastError() };
                format!("LoadLibrary failed, GetLastError={error}").into_bytes()
            })
    }

    pub(crate) fn symbol(module: NonNull<c_void>, name: &CStr) -> *mut c_void {
        unsafe { GetProcAddress(module.as_ptr(), name.as_ptr().cast()) }
            .map_or(std::ptr::null_mut(), |entry| {
                (entry as *const ()).cast_mut().cast()
            })
    }
}

#[cfg(unix)]
mod platform {
    use super::*;

    pub(crate) const NAME: &CStr = c"libnvidia-ngx.so.1";

    pub(crate) fn open() -> Result<NonNull<c_void>, Vec<u8>> {
        unsafe {
            libc::dlerror();
            let module = libc::dlopen(NAME.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
            NonNull::new(module).ok_or_else(|| {
                let error = libc::dlerror();
                if error.is_null() {
                    b"the runtime could not be loaded".to_vec()
                } else {
                    CStr::from_ptr(error).to_bytes().to_vec()
                }
            })
        }
    }

    pub(crate) fn symbol(module: NonNull<c_void>, name: &CStr) -> *mut c_void {
        unsafe { libc::dlsym(module.as_ptr(), name.as_ptr()) }
    }
}

pub(crate) use platform::{NAME, open, symbol};
