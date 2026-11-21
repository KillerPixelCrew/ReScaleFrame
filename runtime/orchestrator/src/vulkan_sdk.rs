// SPDX-License-Identifier: MIT
use std::ffi::{CStr, c_char, c_void};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{NonNull, null_mut};
use windows_sys::Win32::{
    Foundation::FreeLibrary,
    System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN, GetModuleHandleExW,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
    },
};

const INVALID_ARGUMENT: i32 = -1;
const LOAD_FAILED: i32 = -4;
pub(super) const FIDELITYFX_DLL: &str = "amd_fidelityfx_vk.dll";

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Loader {
    pub load: unsafe extern "C" fn(*const u16) -> *mut c_void,
    pub unload: unsafe extern "C" fn(*mut c_void),
}

const _: () = assert!(size_of::<Loader>() == 2 * size_of::<*mut c_void>());

pub(super) struct SdkModule {
    handle: NonNull<c_void>,
    unload: unsafe extern "C" fn(*mut c_void),
}

impl SdkModule {
    // Adopt a system loader reference from a native leaf that uses its own path policy.
    pub(super) fn adopt_system(handle: *mut c_void) -> Option<Self> {
        Some(Self {
            handle: NonNull::new(handle)?,
            unload: rsf_core_vk_sdk_unload,
        })
    }

    // The setup lends a NUL-terminated UTF-8 runtime directory for this call.
    pub(super) unsafe fn open(
        loader: Loader,
        directory: *const c_char,
        filename: &'static str,
    ) -> Result<Self, i32> {
        if directory.is_null() {
            return Err(INVALID_ARGUMENT);
        }
        let directory = unsafe { CStr::from_ptr(directory) }
            .to_str()
            .map_err(|_| INVALID_ARGUMENT)?;
        let directory = Path::new(directory);
        if !directory.is_absolute() {
            return Err(INVALID_ARGUMENT);
        }
        let path = directory.join(filename);
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let handle = NonNull::new(unsafe { (loader.load)(wide.as_ptr()) }).ok_or(LOAD_FAILED)?;
        Ok(Self {
            handle,
            unload: loader.unload,
        })
    }

    pub(super) fn as_ptr(&self) -> *mut c_void {
        self.handle.as_ptr()
    }
}

impl Drop for SdkModule {
    fn drop(&mut self) {
        unsafe { (self.unload)(self.as_ptr()) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vk_sdk_load(path: *const u16) -> *mut c_void {
    unsafe {
        LoadLibraryExW(
            path,
            null_mut(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vk_sdk_unload(module: *mut c_void) {
    unsafe { FreeLibrary(module) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vk_sdk_pin(module: *mut c_void) -> u32 {
    let mut pinned = null_mut();
    u32::from(
        unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_PIN | GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                module.cast(),
                &mut pinned,
            )
        } != 0,
    )
}
