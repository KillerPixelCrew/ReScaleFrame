// SPDX-License-Identifier: MIT
//! Window helpers the Windows fixture crates share.
#![cfg(windows)]

use std::ffi::{CStr, OsStr};
use windows::Win32::Foundation::{FreeLibrary, HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::{
    GetModuleHandleW, GetProcAddress, LOAD_LIBRARY_FLAGS, LoadLibraryExW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, UnregisterClassW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSW,
};
use windows::core::{HSTRING, PCSTR, PCWSTR, Result};

/// The windows crate wraps DefWindowProcW in a Rust function; a window class needs the system ABI.
pub unsafe extern "system" fn default_window_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// A window of its own class, destroyed and unregistered on drop.
pub struct Window {
    pub handle: HWND,
    class: PCWSTR,
    instance: HINSTANCE,
}

impl Window {
    pub fn new(
        class: PCWSTR,
        title: PCWSTR,
        style: WINDOW_STYLE,
        width: i32,
        height: i32,
    ) -> Result<Self> {
        let instance = HINSTANCE(unsafe { GetModuleHandleW(PCWSTR::null())? }.0);
        let description = WNDCLASSW {
            lpfnWndProc: Some(default_window_procedure),
            hInstance: instance,
            lpszClassName: class,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&description) } == 0 {
            return Err(windows::core::Error::from_thread());
        }
        let handle = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                title,
                style,
                0,
                0,
                width,
                height,
                None,
                None,
                Some(instance),
                None,
            )
        };
        match handle {
            Ok(handle) => Ok(Self {
                handle,
                class,
                instance,
            }),
            Err(error) => {
                let _ = unsafe { UnregisterClassW(class, Some(instance)) };
                Err(error)
            }
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.handle);
            let _ = UnregisterClassW(self.class, Some(self.instance));
        }
    }
}

/// A DLL loaded for the life of the value, for fixtures that call the exports of a shipped binary.
pub struct Library(HMODULE);

impl Library {
    /// Loads `path`; `LOAD_LIBRARY_FLAGS(0)` is the default search order.
    pub fn open(path: &OsStr, flags: LOAD_LIBRARY_FLAGS) -> Result<Self> {
        Ok(Self(unsafe {
            LoadLibraryExW(&HSTRING::from(path), None, flags)
        }?))
    }

    pub fn symbol(&self, name: &CStr) -> Option<unsafe extern "system" fn() -> isize> {
        unsafe { GetProcAddress(self.0, PCSTR(name.as_ptr().cast())) }
    }

}

impl Drop for Library {
    fn drop(&mut self) {
        let _ = unsafe { FreeLibrary(self.0) };
    }
}