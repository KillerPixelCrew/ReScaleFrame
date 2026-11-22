// SPDX-License-Identifier: MIT
//! The parent owns input and focus. Only the child is handed to DXGI.
use std::ptr::{null, null_mut};
use windows_sys::Win32::{
    Foundation::{ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, WPARAM},
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::WindowsAndMessaging::*,
};
use windows_sys::w;

const RESIZE: u32 = WM_APP + 0x525;
const DESTROY: u32 = WM_APP + 0x526;
pub(super) const INITIAL_EXTENT: u32 = 32;

unsafe extern "system" fn surface_window(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCHITTEST => HTTRANSPARENT as LRESULT,
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_ERASEBKGND => 1,
        RESIZE => unsafe {
            SetWindowPos(
                hwnd, null_mut(), 0, 0, wparam as i32, lparam as i32,
                SWP_NOACTIVATE | SWP_NOZORDER,
            ) as LRESULT
        },
        DESTROY => unsafe { DestroyWindow(hwnd) as LRESULT },
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_gl_window_valid(window: HWND) -> i32 {
    unsafe { IsWindow(window) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_gl_window_create(parent: HWND) -> HWND {
    if unsafe { GetWindowThreadProcessId(parent, null_mut()) != GetCurrentThreadId() } {
        return null_mut();
    }
    let class = WNDCLASSW {
        lpfnWndProc: Some(surface_window),
        hInstance: unsafe { GetModuleHandleW(null()) },
        lpszClassName: w!("ReScaleFrame.OpenGL.Presentation"),
        ..Default::default()
    };
    unsafe {
        if RegisterClassW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return null_mut();
        }
        CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            class.lpszClassName, w!(""), WS_CHILD | WS_CLIPSIBLINGS, 0, 0,
            INITIAL_EXTENT as i32, INITIAL_EXTENT as i32, parent, null_mut(), class.hInstance, null(),
        )
    }
}

fn surface_message(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> bool {
    let mut result = 0;
    let sent = unsafe {
        SendMessageTimeoutW(
            hwnd, message, wparam, lparam, SMTO_BLOCK | SMTO_ABORTIFHUNG, 5000, &mut result,
        )
    };
    sent != 0 && result != 0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_gl_window_destroy(window: HWND) -> i32 {
    i32::from(rsf_core_gl_window_valid(window) == 0 || surface_message(window, DESTROY, 0, 0))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_gl_window_child_of(child: HWND, parent: HWND) -> i32 {
    i32::from(unsafe {
        GetParent(child) == parent
            && GetWindowThreadProcessId(child, null_mut()) == GetWindowThreadProcessId(parent, null_mut())
    })
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_gl_window_resize(window: HWND, width: u32, height: u32) -> i32 {
    i32::from(
        width <= i32::MAX as u32
            && height <= i32::MAX as u32
            && surface_message(window, RESIZE, width as WPARAM, height as LPARAM),
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_gl_window_show(window: HWND) {
    unsafe { ShowWindowAsync(window, SW_SHOWNOACTIVATE) };
}
