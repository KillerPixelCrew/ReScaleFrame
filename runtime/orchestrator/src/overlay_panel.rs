// SPDX-License-Identifier: MIT
// The production entry points live in `native`, which unit tests do not build.
#![cfg_attr(test, allow(dead_code))]
use crate::sys::{rsf_overlay, rsf_overlay_draw_data, rsf_overlay_input};
use crate::sys::{rsf_overlay_intent, rsf_overlay_stats, rsf_overlay_texture_update};
use rsf_game_sys::win32::typed_export;
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::Mutex;
use windows_sys::Win32::{
    Foundation::{FreeLibrary, GetLastError, HMODULE},
    System::{
        Environment::GetEnvironmentVariableW,
        LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            GetModuleFileNameW, GetModuleHandleExW, LoadLibraryW,
        },
    },
};
use windows_sys::core::w;

const PATH_CAPACITY: usize = 32768;
const PANEL_DLL: &str = "rescaleframe_overlay.dll";

// A first frame, a clock jump or a frame delayed by loading should advance the
// panel by one nominal frame.
pub(super) fn elapsed_seconds(seconds: f64) -> f32 {
    if 0.0 < seconds && seconds <= 0.25 {
        seconds as f32
    } else {
        1.0 / 60.0
    }
}

#[cfg(test)]
mod elapsed_tests {
    use super::elapsed_seconds;

    #[test]
    fn elapsed_uses_the_nominal_frame_for_first_clock_jumps_and_loading_gaps() {
        assert_eq!(elapsed_seconds(0.0), 1.0 / 60.0);
        assert_eq!(elapsed_seconds(-1.0), 1.0 / 60.0);
        assert_eq!(elapsed_seconds(0.25), 0.25);
        assert_eq!(elapsed_seconds(1.0), 1.0 / 60.0);
    }
}

// Keep one reference per published DLL. A window message can still reach
// panel code after a renderer has stopped.
static PUBLISHED: Mutex<Vec<usize>> = Mutex::new(Vec::new());

type Create = unsafe extern "C" fn(u32) -> *mut rsf_overlay;
type Destroy = unsafe extern "C" fn(*mut rsf_overlay);
type Frame = unsafe extern "C" fn(
    *mut rsf_overlay,
    *const rsf_overlay_input,
    *const rsf_overlay_stats,
    *mut rsf_overlay_draw_data,
    *mut rsf_overlay_intent,
) -> i32;
type TextureUpdates =
    unsafe extern "C" fn(*mut rsf_overlay, *mut rsf_overlay_texture_update, u32) -> u32;
type TexturesToFree = unsafe extern "C" fn(*mut rsf_overlay, *mut u64, u32) -> u32;

#[derive(Clone, Copy)]
pub(super) struct PanelEntries {
    pub create: Create,
    pub destroy: Destroy,
    pub frame: Frame,
    pub texture_updates: TextureUpdates,
    pub textures_to_free: TexturesToFree,
}

pub(super) enum LoadError {
    NoPath,
    CouldNotLoad(u32),
    MissingExports,
}

pub(super) struct OpenPanel {
    pub entries: PanelEntries,
    module: HMODULE,
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
    let address = open_panel as *const c_void;
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

fn exports(module: HMODULE) -> Option<PanelEntries> {
    // The panel's five C exports have the signatures declared in overlay.h.
    unsafe {
        Some(PanelEntries {
            create: typed_export(module, c"rsf_overlay_create")?,
            destroy: typed_export(module, c"rsf_overlay_destroy")?,
            frame: typed_export(module, c"rsf_overlay_frame")?,
            texture_updates: typed_export(module, c"rsf_overlay_texture_updates")?,
            textures_to_free: typed_export(module, c"rsf_overlay_textures_to_free")?,
        })
    }
}

pub(super) fn open_panel() -> Result<OpenPanel, LoadError> {
    let path = panel_path().ok_or(LoadError::NoPath)?;
    let module = unsafe { LoadLibraryW(path.as_ptr()) };
    if module.is_null() {
        return Err(LoadError::CouldNotLoad(unsafe { GetLastError() }));
    }
    let Some(entries) = exports(module) else {
        unsafe { FreeLibrary(module) };
        return Err(LoadError::MissingExports);
    };
    Ok(OpenPanel { entries, module })
}

impl OpenPanel {
    pub(super) fn publish(mut self) -> PanelEntries {
        let mut published = PUBLISHED.lock().unwrap();
        if !published.contains(&self.module.addr()) {
            published.push(self.module.addr());
            self.module = null_mut();
        }
        // A duplicate reference is released by Drop after the lock is released.
        drop(published);
        self.entries
    }
}

impl Drop for OpenPanel {
    fn drop(&mut self) {
        if !self.module.is_null() {
            unsafe { FreeLibrary(self.module) };
        }
    }
}
