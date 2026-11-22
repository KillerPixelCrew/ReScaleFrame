// SPDX-License-Identifier: MIT
use super::{null, null_mut};
use std::cell::UnsafeCell;
use std::ffi::c_void;
use windows_sys::Win32::Foundation::{CloseHandle, HMODULE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_APPEND_DATA, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_ALWAYS, WriteFile,
};
use windows_sys::Win32::System::Diagnostics::Debug::{
    GetErrorMode, GetThreadErrorMode, SEM_FAILCRITICALERRORS, SEM_NOOPENFILEERRORBOX,
};
use windows_sys::Win32::System::Environment::GetEnvironmentVariableW;
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::{GetCurrentThreadId, Sleep};
use windows_sys::core::{BOOL, GUID, HRESULT, w};

const PATH_CAPACITY: usize = 32768;

// Process attach is serialized by Windows, and the only adapter entry caller
// runs after attach and once per isolated test subprocess.
struct EventPath(UnsafeCell<[u16; PATH_CAPACITY]>);
unsafe impl Sync for EventPath {}
static EVENT_PATH: EventPath = EventPath(UnsafeCell::new([0; PATH_CAPACITY]));

fn value<'a>(name: *const u16, output: &'a mut [u16]) -> Option<&'a [u16]> {
    let count =
        unsafe { GetEnvironmentVariableW(name, output.as_mut_ptr(), output.len() as u32) } as usize;
    (count != 0 && count < output.len()).then(|| &output[..count])
}

fn number(name: *const u16, base: u32) -> Option<usize> {
    let mut buffer = [0; 64];
    let text = String::from_utf16(value(name, &mut buffer)?).ok()?;
    usize::from_str_radix(&text, base).ok()
}

fn record(event: u8) {
    let path = EVENT_PATH.0.get().cast::<u16>();
    let count = unsafe {
        GetEnvironmentVariableW(w!("RSF_THIN_FIXTURE_EVENTS"), path, PATH_CAPACITY as u32)
    };
    if count == 0 || count as usize >= PATH_CAPACITY {
        return;
    }
    unsafe {
        let file = CreateFileW(
            path,
            FILE_APPEND_DATA,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        );
        if file != INVALID_HANDLE_VALUE {
            let mut written = 0;
            WriteFile(file, &event, 1, &mut written, null_mut());
            CloseHandle(file);
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_thin_fixture_attach(event: u32, marker: u8) -> BOOL {
    if event == DLL_PROCESS_ATTACH {
        let mut buffer = [0; 8];
        if value(w!("RSF_THIN_FIXTURE_CHECK_ERROR_MODE"), &mut buffer).is_some() {
            let flags = SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX;
            if unsafe { GetThreadErrorMode() } & flags != flags {
                record(b'H');
            }
        }
        record(marker);
    }
    1
}

#[unsafe(no_mangle)]
unsafe extern "system" fn rsf_test_thin_fixture_start(configuration_path: *const u16) -> u32 {
    let mut buffer = vec![0; PATH_CAPACITY];
    let same_path = value(w!("RSF_THIN_FIXTURE_CONFIG"), &mut buffer).is_some_and(|expected| {
        if configuration_path.is_null() {
            return false;
        }
        for (index, unit) in expected.iter().copied().chain([0]).enumerate() {
            if unsafe { configuration_path.add(index).read() } != unit {
                return false;
            }
        }
        true
    });
    if !same_path {
        record(b'X');
        return 1;
    }
    if let Some(address) = number(w!("RSF_THIN_FIXTURE_OUTPUT"), 16)
        && !unsafe { (address as *const *mut c_void).read_volatile() }.is_null()
    {
        record(b'O');
        return 2;
    }
    if number(w!("RSF_THIN_FIXTURE_THREAD"), 10)
        .is_some_and(|expected| expected != unsafe { GetCurrentThreadId() } as usize)
    {
        record(b'T');
        return 3;
    }
    if number(w!("RSF_THIN_FIXTURE_THREAD_MODE"), 10)
        .is_some_and(|expected| expected != unsafe { GetThreadErrorMode() } as usize)
    {
        record(b'M');
        return 4;
    }
    if number(w!("RSF_THIN_FIXTURE_PROCESS_MODE"), 10)
        .is_some_and(|expected| expected != unsafe { GetErrorMode() } as usize)
    {
        record(b'P');
        return 5;
    }
    if let Some(address) = number(w!("RSF_THIN_FIXTURE_REENTER"), 16) {
        type Create = unsafe extern "system" fn(
            HMODULE,
            u32,
            *const GUID,
            *mut *mut c_void,
            *mut c_void,
        ) -> HRESULT;
        let create = unsafe { std::mem::transmute::<usize, Create>(address) };
        let mut unused = null_mut();
        if unsafe { create(null_mut(), 0, &GUID::from_u128(0), &mut unused, null_mut()) } >= 0 {
            record(b'I');
            return 6;
        }
    }
    // A premature asynchronous return must be visible to the caller.
    unsafe { Sleep(80) };
    record(b'E');
    let mut refusal = [0; 8];
    if value(w!("RSF_THIN_FIXTURE_REFUSE"), &mut refusal).is_some() {
        7
    } else {
        0
    }
}
