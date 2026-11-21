// SPDX-License-Identifier: MIT
use crate::overlay_host_abi::{
    RSF_OVERLAY_HOST_PANEL_LOADED, RSF_OVERLAY_HOST_PANEL_MISSING_EXPORTS,
    RSF_OVERLAY_HOST_PANEL_NO_PATH, RSF_OVERLAY_HOST_PANEL_NOT_LOADED, rsf_overlay_host_panel,
};
use std::ffi::{CStr, c_void};
use std::ptr::null_mut;
use std::sync::Mutex;
use windows_sys::Win32::{
    Foundation::{FARPROC, FreeLibrary, GetLastError, HMODULE},
    System::{
        Environment::GetEnvironmentVariableW,
        LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            GetModuleFileNameW, GetModuleHandleExW, GetProcAddress, LoadLibraryW,
        },
    },
};
use windows_sys::core::w;

const PATH_CAPACITY: usize = 32768;
const PANEL_DLL: &str = "rescaleframe_overlay.dll";

// Keep one reference per published DLL. A window message can still reach
// panel code after a renderer has stopped.
static PUBLISHED: Mutex<Vec<usize>> = Mutex::new(Vec::new());

impl rsf_overlay_host_panel {
    pub(crate) fn complete(&self) -> bool {
        self.create.is_some()
            && self.destroy.is_some()
            && self.frame.is_some()
            && self.texture_updates.is_some()
            && self.textures_to_free.is_some()
    }
}

fn panel_path() -> Option<Vec<u16>> {
    let mut path = vec![0; PATH_CAPACITY];
    let capacity = path.len() as u32;
    let configured =
        unsafe { GetEnvironmentVariableW(w!("RSF_OVERLAY_DLL"), path.as_mut_ptr(), capacity) };
    if configured >= capacity {
        return None;
    }
    if configured != 0 {
        path.truncate(configured as usize + 1);
        return Some(path);
    }

    let mut owner = null_mut();
    let flags =
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
    let address = rsf_core_overlay_panel_open as *const c_void;
    if unsafe { GetModuleHandleExW(flags, address.cast(), &mut owner) } == 0 {
        return None;
    }
    let length = unsafe { GetModuleFileNameW(owner, path.as_mut_ptr(), capacity) };
    if length == 0 || length >= capacity {
        return None;
    }
    let separator = path[..length as usize]
        .iter()
        .rposition(|&unit| unit == u16::from(b'\\') || unit == u16::from(b'/'))?;
    path.truncate(separator + 1);
    path.extend(PANEL_DLL.encode_utf16().chain([0]));
    (path.len() <= PATH_CAPACITY).then_some(path)
}

fn exports(module: HMODULE) -> rsf_overlay_host_panel {
    let get = |name: &CStr| unsafe { GetProcAddress(module, name.as_ptr().cast()) };
    // The panel's five C exports have the signatures declared in overlay.h.
    unsafe {
        rsf_overlay_host_panel {
            create: std::mem::transmute::<FARPROC, _>(get(c"rsf_overlay_create")),
            destroy: std::mem::transmute::<FARPROC, _>(get(c"rsf_overlay_destroy")),
            frame: std::mem::transmute::<FARPROC, _>(get(c"rsf_overlay_frame")),
            texture_updates: std::mem::transmute::<FARPROC, _>(get(c"rsf_overlay_texture_updates")),
            textures_to_free: std::mem::transmute::<FARPROC, _>(get(
                c"rsf_overlay_textures_to_free",
            )),
        }
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_overlay_panel_open(
    out: *mut rsf_overlay_host_panel,
    error: *mut u32,
    returned_module: *mut HMODULE,
) -> u32 {
    let Some(path) = panel_path() else {
        return RSF_OVERLAY_HOST_PANEL_NO_PATH as u32;
    };
    let module = unsafe { LoadLibraryW(path.as_ptr()) };
    if module.is_null() {
        if !error.is_null() {
            unsafe { error.write(GetLastError()) };
        }
        return RSF_OVERLAY_HOST_PANEL_NOT_LOADED as u32;
    }
    let panel = exports(module);
    if !panel.complete() {
        unsafe { rsf_core_overlay_panel_release(module) };
        return RSF_OVERLAY_HOST_PANEL_MISSING_EXPORTS as u32;
    }
    if returned_module.is_null() {
        unsafe { rsf_core_overlay_panel_retain(module) };
    } else {
        unsafe { returned_module.write(module) };
    }
    unsafe { out.write(panel) };
    RSF_OVERLAY_HOST_PANEL_LOADED as u32
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_overlay_panel_release(module: HMODULE) {
    unsafe { FreeLibrary(module) };
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_core_overlay_panel_retain(module: HMODULE) {
    {
        let mut published = PUBLISHED.lock().unwrap();
        if !published.contains(&module.addr()) {
            published.push(module.addr());
            return;
        }
    }
    // Do not call into the Windows loader while holding the registry lock.
    unsafe { rsf_core_overlay_panel_release(module) };
}
