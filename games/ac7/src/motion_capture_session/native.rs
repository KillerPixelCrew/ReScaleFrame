// SPDX-License-Identifier: MIT
//! Platform effects for the F9 present-relative scheduler.

use super::{
    CaptureEffects, CaptureInterval, CaptureStamp, Stats, rsf_ac7_capture_failures,
    rsf_ac7_capture_present, rsf_ac7_capture_shutdown,
};
use crate::motion_capture_records::artifacts::rsf_ac7_capture_publish_checkpoint;
use std::ffi::{c_char, c_void};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;
use windows::Win32::Storage::FileSystem::{
    self as file_system, CREATE_ALWAYS, CreateDirectoryA, CreateFileA, FILE_ATTRIBUTE_NORMAL,
    FILE_GENERIC_WRITE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use windows::Win32::System::Performance::QueryPerformanceFrequency;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_F9};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
use windows::core::{Interface, PCSTR};

unsafe extern "C" {
    fn rsf_ac7_capture_engine_installed() -> bool;
    fn rsf_ac7_capture_current_command_address() -> *const c_void;
    fn rsf_ac7_capture_set_sample(enabled: bool);
}

unsafe extern "C" fn poll(chain: *mut c_void, pressed: *mut bool) -> bool {
    let Some(chain) = (unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }) else {
        return false;
    };
    if unsafe { chain.GetDesc() }.is_err() {
        return false;
    }
    let mut process = 0;
    unsafe {
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut process));
        pressed.write(process == GetCurrentProcessId() && GetAsyncKeyState(i32::from(VK_F9.0)) < 0);
    }
    true
}

unsafe extern "C" fn stamp(stamp: *mut CaptureStamp) {
    let time = unsafe { GetLocalTime() };
    unsafe {
        stamp.write(CaptureStamp {
            year: u32::from(time.wYear),
            month: u32::from(time.wMonth),
            day: u32::from(time.wDay),
            hour: u32::from(time.wHour),
            minute: u32::from(time.wMinute),
            second: u32::from(time.wSecond),
            process: GetCurrentProcessId(),
        })
    };
}

unsafe extern "C" fn create_directory(path: *const c_char) -> bool {
    unsafe { CreateDirectoryA(PCSTR(path.cast()), None) }.is_ok()
}

/// Replaces the file at the narrow (ANSI) path with ytes, sharing as open(path, "wb"); false
/// on an open failure, a short write or a failed close.
unsafe extern "C" fn write_file(path: *const c_char, data: *const u8, bytes: usize) -> bool {
    let Ok(file) = (unsafe {
        CreateFileA(
            PCSTR(path.cast()),
            FILE_GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            CREATE_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }) else {
        return false;
    };
    let data = if bytes == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(data, bytes) }
    };
    let written = data.chunks(u32::MAX as usize).all(|chunk| {
        let mut count = 0;
        unsafe { file_system::WriteFile(file, Some(chunk), Some(&mut count), None) }.is_ok()
            && count as usize == chunk.len()
    });
    unsafe { CloseHandle(file) }.is_ok() && written
}

extern "C" fn stats() -> Stats {
    let mut frequency = 0;
    let _ = unsafe { QueryPerformanceFrequency(&mut frequency) };
    Stats::from_native(
        frequency,
        unsafe { rsf_ac7_capture_engine_installed() },
        !unsafe { rsf_ac7_capture_current_command_address() }.is_null(),
        rsf_ac7_capture_failures(),
    )
}

const EFFECTS: CaptureEffects = CaptureEffects {
    poll,
    stamp,
    create_directory,
    set_sample: rsf_ac7_capture_set_sample,
    write: write_file,
    stats,
};

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_motion_capture_present(
    chain: *mut c_void,
    prefix: *mut c_char,
    capacity: u32,
) -> i32 {
    // The public scheduler validates the optional pointers and publishes only a bounded prefix.
    i32::from(unsafe { rsf_ac7_capture_present(chain, prefix, capacity, &EFFECTS) })
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_end_session() {
    unsafe { rsf_ac7_capture_shutdown(&EFFECTS) };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_checkpoint_native(capture: CaptureInterval) {
    rsf_ac7_capture_publish_checkpoint(write_file, capture);
}
