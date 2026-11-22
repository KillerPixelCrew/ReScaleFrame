// SPDX-License-Identifier: MIT
use std::ffi::{CStr, c_char, c_void};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{NonNull, null_mut};
use windows_sys::Win32::{
    Foundation::{FARPROC, FreeLibrary, GetLastError},
    System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN, GetModuleHandleExW,
        GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
        LoadLibraryExW,
    },
};

const INVALID_ARGUMENT: i32 = -1;
const LOAD_FAILED: i32 = -4;
pub(super) const FIDELITYFX_DLL: &str = "amd_fidelityfx_vk.dll";
pub(super) const FFX_CONTEXT_EXPORTS: [&CStr; 4] = [
    c"ffxCreateContext",
    c"ffxDestroyContext",
    c"ffxQuery",
    c"ffxDispatch",
];

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Loader {
    pub load: unsafe extern "C" fn(*const u16) -> *mut c_void,
    pub unload: unsafe extern "C" fn(*mut c_void),
}

const _: () = assert!(size_of::<Loader>() == 2 * size_of::<*mut c_void>());

// The caller lends a NUL-terminated UTF-8 path for this call.
pub(super) unsafe fn wide_path(path: *const c_char) -> Option<Vec<u16>> {
    if path.is_null() {
        return None;
    }
    let path = unsafe { CStr::from_ptr(path) }.to_str().ok()?;
    (!path.is_empty()).then(|| path.encode_utf16().chain([0]).collect())
}

// D3D12 provider packages use a bounded absolute drive or UNC directory and
// append their DLL name verbatim. The bound includes the terminating NUL.
pub(super) fn d3d12_library_path(
    directory: &str,
    name: impl Iterator<Item = u16>,
) -> Option<Vec<u16>> {
    const DIRECTORY_CAPACITY: usize = 1024;
    const PATH_CAPACITY: usize = 1200;
    const COLON: u16 = b':' as u16;
    const SLASH: u16 = b'/' as u16;
    const BACKSLASH: u16 = b'\\' as u16;
    let mut path: Vec<u16> = directory.encode_utf16().take(DIRECTORY_CAPACITY).collect();
    if path.len() == DIRECTORY_CAPACITY
        || !matches!(
            path.as_slice(),
            [_, COLON, BACKSLASH | SLASH, ..] | [BACKSLASH, BACKSLASH, ..]
        )
    {
        return None;
    }
    path.push(BACKSLASH);
    path.extend(name.take(PATH_CAPACITY - path.len()));
    if path.len() == PATH_CAPACITY {
        return None;
    }
    path.push(0);
    Some(path)
}

// The caller supplies a NUL-terminated absolute path from d3d12_library_path.
pub(super) unsafe fn load_system_library(path: &[u16]) -> (*mut c_void, u32) {
    unsafe { load_absolute_library(path.as_ptr()) }
}

// The caller supplies an absolute NUL-terminated UTF-16 DLL path.
pub(super) unsafe fn load_absolute_library(path: *const u16) -> (*mut c_void, u32) {
    let module = unsafe { rsf_core_vk_sdk_load(path) };
    let error = if module.is_null() {
        unsafe { GetLastError() }
    } else {
        0
    };
    (module, error)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_load_absolute_library(
    path: *const u16,
    error: &mut u32,
) -> *mut c_void {
    let (module, last_error) = unsafe { load_absolute_library(path) };
    *error = last_error;
    module
}

pub(super) struct SdkModule {
    handle: NonNull<c_void>,
    unload: unsafe extern "C" fn(*mut c_void),
}

impl SdkModule {
    // Adopt a system loader reference from a native leaf that uses its own path policy.
    pub(super) fn adopt_system(handle: *mut c_void) -> Option<Self> {
        Self::adopt(handle, rsf_core_vk_sdk_unload)
    }

    pub(super) fn adopt(
        handle: *mut c_void,
        unload: unsafe extern "C" fn(*mut c_void),
    ) -> Option<Self> {
        Some(Self {
            handle: NonNull::new(handle)?,
            unload,
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

    pub(super) fn symbol(&self, name: &CStr) -> FARPROC {
        unsafe { GetProcAddress(self.as_ptr(), name.as_ptr().cast()) }
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

// Shared C callback for the FidelityFX SR and FG platform tables.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vkffx_symbol(
    module: *mut c_void,
    name: *const c_char,
) -> crate::vulkan_sr_abi::rsf_vk_function {
    // Windows x64 uses the same ABI for system and C procedure pointers.
    unsafe {
        std::mem::transmute::<FARPROC, crate::vulkan_sr_abi::rsf_vk_function>(GetProcAddress(
            module,
            name.cast(),
        ))
    }
}
