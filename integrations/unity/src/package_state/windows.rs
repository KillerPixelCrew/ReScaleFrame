// SPDX-License-Identifier: MIT
//! Win32 platform operations and input-window ownership for the Unity package.
use super::services::{Handle, report_window_message};
use crate::runtime_config::{PATH_CAPACITY, file_name, terminated};
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_game_sys::win32;
use std::ffi::{CStr, c_void};
use std::ptr::{null, null_mut, without_provenance};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering::SeqCst};
use windows_sys::Win32::Foundation::{
    CloseHandle, FreeLibrary, HANDLE, HMODULE, HWND, INVALID_HANDLE_VALUE, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateDirectoryW, CreateFileW, FILE_APPEND_DATA, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_ALWAYS, WriteFile,
};
use windows_sys::Win32::System::Diagnostics::Debug::OutputDebugStringA;
use windows_sys::Win32::System::Environment::GetEnvironmentVariableW;
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleHandleExW,
};
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, EnumWindows, GWLP_WNDPROC, GetClassNameW, GetWindowLongPtrW,
    GetWindowThreadProcessId, SetWindowLongPtrW, WM_NCDESTROY,
};
use windows_sys::core::w;

/// The module's path without a terminator; empty when it cannot be read.
fn module_path(module: HMODULE) -> Vec<u16> {
    unsafe { win32::module_path(module) }.map_or_else(Vec::new, |path| path.units().to_vec())
}

pub(super) fn startup_paths() -> (bool, Vec<u16>) {
    let exe = module_path(null_mut());
    let editor = String::from_utf16(file_name(&exe))
        .is_ok_and(|name| name.eq_ignore_ascii_case("Unity.exe"));
    let mut module = null_mut();
    let anchor = (startup_paths as *const ()).cast();
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            anchor,
            &mut module,
        );
    }
    let mut ini = module_path(module);
    let file_len = file_name(&ini).len();
    ini.truncate(ini.len() - file_len);
    ini.extend("ReScaleFrame.Unity.ini".encode_utf16());
    (editor, terminated(ini))
}

pub(super) fn rsf_unity_package_session_seed() -> u64 {
    unsafe { GetTickCount64() }
}

pub(super) fn rsf_unity_package_timestamp() -> (u64, u64) {
    let (mut now, mut ticks) = (0, 0);
    unsafe {
        QueryPerformanceCounter(&mut now);
        QueryPerformanceFrequency(&mut ticks);
    }
    (now as u64, ticks as u64)
}

/// %LOCALAPPDATA%; None when it is unset, empty or longer than a path buffer.
pub(super) fn rsf_unity_package_local_app_data() -> Option<Vec<u16>> {
    let mut value = vec![0u16; PATH_CAPACITY];
    let length = unsafe {
        GetEnvironmentVariableW(w!("LOCALAPPDATA"), value.as_mut_ptr(), PATH_CAPACITY as u32)
    } as usize;
    (length != 0 && length < PATH_CAPACITY).then(|| {
        value.truncate(length);
        value
    })
}

pub(super) fn rsf_unity_package_create_directory(path: &[u16]) {
    unsafe { CreateDirectoryW(terminated(path.to_vec()).as_ptr(), null()) };
}

/// Null when the file cannot be opened.
pub(super) fn rsf_unity_package_log_open(path: &[u16]) -> HANDLE {
    let file = unsafe {
        CreateFileW(
            terminated(path.to_vec()).as_ptr(),
            FILE_APPEND_DATA,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    if file == INVALID_HANDLE_VALUE {
        null_mut()
    } else {
        file
    }
}

pub(super) fn rsf_unity_package_log_write(file: HANDLE, text: &CStr) {
    unsafe {
        OutputDebugStringA(text.as_ptr().cast());
        OutputDebugStringA(c"\n".as_ptr().cast());
        if !file.is_null() {
            let mut written = 0;
            let line = text.to_bytes();
            WriteFile(
                file,
                line.as_ptr(),
                line.len() as u32,
                &mut written,
                null_mut(),
            );
            WriteFile(file, b"\r\n".as_ptr(), 2, &mut written, null_mut());
        }
    }
}

pub(super) fn rsf_unity_package_log_close(file: HANDLE) {
    unsafe { CloseHandle(file) };
}

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

pub(super) fn rsf_unity_package_subclass(hwnd: HWND) -> bool {
    let owner = unsafe { GetCurrentThreadId() };
    if unsafe { GetWindowThreadProcessId(hwnd, null_mut()) } != owner {
        return false;
    }
    let mut receipt = lock_or_recover(&RECEIPT);
    if let Some(active) = receipt.as_ref() {
        if !DESTROYED.load(SeqCst) {
            if active.window.0 != hwnd || active.owner != owner {
                return false;
            }
            REPORTING.store(true, SeqCst);
            return true;
        }
        if active.owner != owner || IN_FLIGHT.load(SeqCst) != 0 {
            return false;
        }
        // A Unity export supplies the outer module lifetime during this owner-thread call.
        unsafe { FreeLibrary(active.module.0) };
        *receipt = None;
    }

    let ours = input_proc as *const () as isize;
    let previous = unsafe { GetWindowLongPtrW(hwnd, GWLP_WNDPROC) };
    if previous == 0 || previous == ours {
        return false;
    }
    let mut module: HMODULE = null_mut();
    let address = without_provenance(ours as usize);
    if unsafe { GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, address, &mut module) }
        == 0
    {
        return false;
    }
    PREVIOUS.store(previous, SeqCst);
    let replaced = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, ours) };
    if replaced == 0 {
        unsafe { FreeLibrary(module) };
        return false;
    }
    PREVIOUS.store(replaced, SeqCst);
    *receipt = Some(Receipt {
        window: Handle(hwnd),
        owner,
        module: Handle(module),
    });
    DESTROYED.store(false, SeqCst);
    REPORTING.store(true, SeqCst);
    true
}

pub(super) fn rsf_unity_package_restore(hwnd: HWND) {
    REPORTING.store(false, SeqCst);
    let mut receipt = lock_or_recover(&RECEIPT);
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

pub(super) fn rsf_unity_package_window_class(hwnd: HWND) -> u32 {
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

pub(super) fn rsf_unity_package_player_window() -> *mut c_void {
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
            assert!(rsf_unity_package_subclass(window));
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
