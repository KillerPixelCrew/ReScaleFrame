// SPDX-License-Identifier: MIT
//! Player HWND callbacks and thread-affine creation for Unity's child presentation window.

use super::*;
use windows_sys::Win32::{
    Foundation::{ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, RECT, WPARAM},
    System::{
        LibraryLoader::GetModuleHandleW,
        Threading::{GetCurrentProcessId, GetCurrentThreadId},
    },
    UI::WindowsAndMessaging::*,
};

use windows_sys::core::w;

const UNITY_WINDOW: &[u8] = b"UnityWndClass";

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCHITTEST => HTTRANSPARENT as LRESULT,
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_ERASEBKGND => 1,
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

unsafe extern "system" fn find_window(candidate: HWND, result: LPARAM) -> i32 {
    let mut name = [0u16; 64];
    let mut process = 0;
    unsafe {
        GetWindowThreadProcessId(candidate, &mut process);
        if process == GetCurrentProcessId() {
            let length = GetClassNameW(candidate, name.as_mut_ptr(), name.len() as i32);
            if length == UNITY_WINDOW.len() as i32
                && name[..UNITY_WINDOW.len()]
                    .iter()
                    .copied()
                    .eq(UNITY_WINDOW.iter().copied().map(u16::from))
            {
                (result as *mut HWND).write(candidate);
                return 0;
            }
        }
    }
    1
}

#[unsafe(no_mangle)]
extern "C" fn rsf_uvk_bridge_create_window() -> bool {
    let mut window = rsf_unity_vulkan_surface_window();
    if window.is_null() {
        let mut parent: HWND = ptr::null_mut();
        unsafe { EnumWindows(Some(find_window), ptr::from_mut(&mut parent) as LPARAM) };
        if !on_window_thread(parent) {
            return false;
        }
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: unsafe { GetModuleHandleW(ptr::null()) },
            lpszClassName: w!("ReScaleFrame.Unity.Presentation"),
            ..Default::default()
        };
        if unsafe { RegisterClassW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS } {
            return false;
        }
        let mut rect = RECT::default();
        unsafe { GetClientRect(parent, &mut rect) };
        window = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
                class.lpszClassName,
                w!(""),
                WS_CHILD | WS_CLIPSIBLINGS,
                0,
                0,
                rect.right,
                rect.bottom,
                parent,
                ptr::null_mut(),
                class.hInstance,
                ptr::null(),
            )
        };
        if window.is_null() {
            return false;
        }
        if rsf_unity_vulkan_set_surface_window(window) == 0 {
            unsafe { DestroyWindow(window) };
            window = rsf_unity_vulkan_surface_window();
        }
    }
    // Release keeps the child window. Restart recreates the chain on its window thread.
    let mut rect = RECT::default();
    !window.is_null()
        && unsafe { GetClientRect(window, &mut rect) } != 0
        && rsf_uvk_bridge_update_window(rect.right as u32, rect.bottom as u32)
}

fn on_window_thread(window: HWND) -> bool {
    !window.is_null()
        && unsafe { GetWindowThreadProcessId(window, ptr::null_mut()) == GetCurrentThreadId() }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_uvk_bridge_owns_window(candidate: *mut c_void) -> bool {
    let window = rsf_unity_vulkan_surface_window();
    !window.is_null() && window == candidate
}

#[unsafe(no_mangle)]
extern "C" fn rsf_uvk_bridge_update_window(width: u32, height: u32) -> bool {
    let window = rsf_unity_vulkan_surface_window();
    if width == 0 || height == 0 || !on_window_thread(window) {
        return false;
    }
    let mut rect = RECT::default();
    if unsafe { GetClientRect(window, &mut rect) } == 0 {
        return false;
    }
    if (rect.right != width as i32 || rect.bottom != height as i32)
        && unsafe {
            SetWindowPos(
                window,
                ptr::null_mut(),
                0,
                0,
                width as i32,
                height as i32,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
        } == 0
    {
        return false;
    }
    let Some(_entry) = Entry::begin() else {
        return false;
    };
    if rsf_unity_vulkan_surface_ready() != 0 {
        return true;
    }
    // Never wait for a Present with the lease on the window thread.
    let Some(_lease) = Lease::try_begin() else {
        return false;
    };
    let mut graphics = DeviceHandles::EMPTY;
    rsf_unity_vulkan_surface_ready() != 0
        || (initialize(ptr::null(), &mut graphics) == DEVICE_READY
            && !configure_surface(width, height).is_null())
}
