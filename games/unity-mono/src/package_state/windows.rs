// SPDX-License-Identifier: MIT
//! Win32 window ownership for the Unity package.
use super::services::{Handle, report_window_message};
use std::ffi::c_void;
use std::ptr::{null_mut, without_provenance};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering::SeqCst};
use std::sync::{Mutex, PoisonError};
use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GetModuleHandleExW,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, EnumWindows, GWLP_WNDPROC, GetClassNameW, GetWindowLongPtrW,
    GetWindowThreadProcessId, SetWindowLongPtrW, WM_NCDESTROY,
};

struct Receipt {
    window: Handle,
    owner: u32,
    module: Handle,
}

// Attachment and retirement take the mutex. A WNDPROC only uses atomics so forwarding can
// continue while the session shuts down, including when a later subclass calls our procedure.
static RECEIPT: Mutex<Option<Receipt>> = Mutex::new(None);
static PREVIOUS: AtomicIsize = AtomicIsize::new(0);
static REPORTING: AtomicBool = AtomicBool::new(false);
static DESTROYED: AtomicBool = AtomicBool::new(false);
static IN_FLIGHT: AtomicU32 = AtomicU32::new(0);

type WindowProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

unsafe fn forward(
    previous: isize,
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // The nonzero value from GWLP_WNDPROC can be a Win32 thunk. Only CallWindowProcW may use it.
    let procedure = unsafe { std::mem::transmute::<isize, WindowProc>(previous) };
    unsafe { CallWindowProcW(Some(procedure), hwnd, message, wparam, lparam) }
}

unsafe extern "system" fn input_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    IN_FLIGHT.fetch_add(1, SeqCst);
    if REPORTING.load(SeqCst) {
        report_window_message(message, lparam);
    }
    let value = unsafe { forward(PREVIOUS.load(SeqCst), hwnd, message, wparam, lparam) };
    if message == WM_NCDESTROY {
        DESTROYED.store(true, SeqCst);
    }
    IN_FLIGHT.fetch_sub(1, SeqCst);
    value
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_package_subclass(hwnd: HWND) -> u32 {
    let owner = unsafe { GetCurrentThreadId() };
    if unsafe { GetWindowThreadProcessId(hwnd, null_mut()) } != owner {
        return 0;
    }
    let mut receipt = RECEIPT.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(active) = receipt.as_ref() {
        if !DESTROYED.load(SeqCst) {
            if active.window.0 != hwnd || active.owner != owner {
                return 0;
            }
            REPORTING.store(true, SeqCst);
            return 1;
        }
        if active.owner != owner || IN_FLIGHT.load(SeqCst) != 0 {
            return 0;
        }
        // A Unity export supplies the outer module lifetime during this owner-thread call.
        unsafe { FreeLibrary(active.module.0) };
        *receipt = None;
    }

    let ours = input_proc as *const () as isize;
    let previous = unsafe { GetWindowLongPtrW(hwnd, GWLP_WNDPROC) };
    if previous == 0 || previous == ours {
        return 0;
    }
    let mut module: HMODULE = null_mut();
    let address = without_provenance(ours as usize);
    if unsafe { GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, address, &mut module) }
        == 0
    {
        return 0;
    }
    PREVIOUS.store(previous, SeqCst);
    let replaced = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, ours) };
    if replaced == 0 {
        unsafe { FreeLibrary(module) };
        return 0;
    }
    PREVIOUS.store(replaced, SeqCst);
    *receipt = Some(Receipt {
        window: Handle(hwnd),
        owner,
        module: Handle(module),
    });
    DESTROYED.store(false, SeqCst);
    REPORTING.store(true, SeqCst);
    1
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_package_restore(hwnd: HWND) {
    REPORTING.store(false, SeqCst);
    let mut receipt = RECEIPT.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(active) = receipt.as_ref() else {
        return;
    };
    if active.window.0 != hwnd
        || active.owner != unsafe { GetCurrentThreadId() }
        || IN_FLIGHT.load(SeqCst) != 0
    {
        return;
    }
    if !DESTROYED.load(SeqCst) {
        let ours = input_proc as *const () as isize;
        if unsafe { GetWindowLongPtrW(hwnd, GWLP_WNDPROC) } != ours
            || unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, PREVIOUS.load(SeqCst)) } != ours
        {
            return;
        }
    }
    PREVIOUS.store(0, SeqCst);
    unsafe { FreeLibrary(active.module.0) };
    *receipt = None;
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_package_window_class(hwnd: HWND) -> u32 {
    let mut process = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut process) };
    if process != unsafe { GetCurrentProcessId() } {
        return 0;
    }
    let mut class = [0; 64];
    let count = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) } as usize;
    if count == 0 {
        0
    } else if class[..count]
        .iter()
        .copied()
        .eq("UnityWndClass".encode_utf16())
    {
        1
    } else {
        2
    }
}

unsafe extern "system" fn find_player_window(hwnd: HWND, output: LPARAM) -> i32 {
    if rsf_unity_package_window_class(hwnd) == 1 {
        unsafe { (output as *mut HWND).write(hwnd) };
        0
    } else {
        1
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_package_player_window() -> *mut c_void {
    let mut hwnd: HWND = null_mut();
    unsafe { EnumWindows(Some(find_player_window), (&raw mut hwnd) as LPARAM) };
    hwnd
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_MESSAGE, SendMessageW, WM_APP,
    };
    use windows_sys::core::w;

    static UPPER_PREVIOUS: AtomicIsize = AtomicIsize::new(0);
    const REPLY: LRESULT = 0x5040;

    unsafe extern "system" fn original(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_APP {
            return REPLY;
        }
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    unsafe extern "system" fn upper(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe { forward(UPPER_PREVIOUS.load(SeqCst), hwnd, message, wparam, lparam) }
    }

    #[test]
    #[ignore = "Win32 message dispatch fixture"]
    fn later_subclass_keeps_forwarding_during_and_after_shutdown() {
        unsafe {
            let window = CreateWindowExW(
                0,
                w!("STATIC"),
                w!(""),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                null_mut(),
                null_mut(),
                null_mut(),
            );
            assert!(!window.is_null());
            assert_ne!(
                SetWindowLongPtrW(window, GWLP_WNDPROC, original as *const () as isize),
                0
            );
            assert_eq!(rsf_unity_package_subclass(window), 1);
            let previous = SetWindowLongPtrW(window, GWLP_WNDPROC, upper as *const () as isize);
            assert_eq!(previous, input_proc as *const () as isize);
            UPPER_PREVIOUS.store(previous, SeqCst);
            assert_eq!(SendMessageW(window, WM_APP, 0, 0), REPLY);

            rsf_unity_package_restore(window);
            assert_eq!(
                GetWindowLongPtrW(window, GWLP_WNDPROC),
                upper as *const () as isize
            );
            assert!(!REPORTING.load(SeqCst));
            assert!(RECEIPT.lock().unwrap().is_some());
            assert_eq!(SendMessageW(window, WM_APP, 0, 0), REPLY);

            assert_ne!(SetWindowLongPtrW(window, GWLP_WNDPROC, previous), 0);
            rsf_unity_package_restore(window);
            assert!(RECEIPT.lock().unwrap().is_none());
            assert_eq!(SendMessageW(window, WM_APP, 0, 0), REPLY);
            assert_ne!(DestroyWindow(window), 0);
        }
    }
}
