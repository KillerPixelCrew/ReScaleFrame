// SPDX-License-Identifier: MIT
//! Platform effects for the F9 present-relative scheduler.

use super::{
    CaptureEffects, CaptureInterval, CaptureStamp, Stats, failure_count, present, shutdown,
};
use crate::motion_capture_records::artifacts::publish_checkpoint;
use crate::motion_capture_records::draws::gpu::set_sample;
use crate::motion_capture_records::root_hooks::current_command_address;
use crate::motion_capture_records::root_hooks::engine_installed;
use std::ffi::{CStr, c_char, c_void};
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

unsafe fn poll(chain: *mut c_void) -> Option<bool> {
    let chain = unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }?;
    unsafe { chain.GetDesc() }.ok()?;
    let mut process = 0;
    unsafe {
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut process));
        Some(process == GetCurrentProcessId() && GetAsyncKeyState(i32::from(VK_F9.0)) < 0)
    }
}

fn stamp() -> CaptureStamp {
    let time = unsafe { GetLocalTime() };
    CaptureStamp {
        year: u32::from(time.wYear),
        month: u32::from(time.wMonth),
        day: u32::from(time.wDay),
        hour: u32::from(time.wHour),
        minute: u32::from(time.wMinute),
        second: u32::from(time.wSecond),
        process: unsafe { GetCurrentProcessId() },
    }
}

fn create_directory(path: &CStr) -> bool {
    unsafe { CreateDirectoryA(PCSTR(path.as_ptr().cast()), None) }.is_ok()
}

/// Replaces the file at the narrow (ANSI) path with `data`, sharing as open(path, "wb"); false
/// on an open failure, a short write or a failed close.
fn write_file(path: &CStr, data: &[u8]) -> bool {
    let Ok(file) = (unsafe {
        CreateFileA(
            PCSTR(path.as_ptr().cast()),
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
    let written = data.chunks(u32::MAX as usize).all(|chunk| {
        let mut count = 0;
        unsafe { file_system::WriteFile(file, Some(chunk), Some(&mut count), None) }.is_ok()
            && count as usize == chunk.len()
    });
    unsafe { CloseHandle(file) }.is_ok() && written
}

fn stats() -> Stats {
    let mut frequency = 0;
    let _ = unsafe { QueryPerformanceFrequency(&mut frequency) };
    Stats::from_native(
        frequency,
        engine_installed(),
        !current_command_address().is_null(),
        failure_count(),
    )
}

const EFFECTS: CaptureEffects = CaptureEffects {
    poll,
    stamp,
    create_directory,
    set_sample,
    write: write_file,
    stats,
};

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_motion_capture_present(
    chain: *mut c_void,
    prefix: *mut c_char,
    capacity: u32,
) -> i32 {
    if chain.is_null() || prefix.is_null() || capacity == 0 {
        return 0;
    }
    // SAFETY: The carrier lends `capacity` writable bytes for the returned narrow path.
    let prefix = unsafe { std::slice::from_raw_parts_mut(prefix, capacity as usize) };
    i32::from(unsafe { present(chain, prefix, &EFFECTS) })
}

pub(crate) fn end_session() {
    shutdown(&EFFECTS);
}

pub(crate) fn checkpoint_native(capture: CaptureInterval) {
    publish_checkpoint(write_file, capture);
}
