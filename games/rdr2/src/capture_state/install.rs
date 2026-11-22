// SPDX-License-Identifier: MIT
//! Process-lived capture owner and registration with the loader census.
use super::{Capture, Progress, detours, ngx, poll};
use crate::{
    diagnostics::capture_directory,
    platform::{lock, read_process_memory},
};
use rsf_game_sys::ffi_util::try_box;
use std::ffi::{OsString, c_char, c_void};
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::CreateThread;
#[cfg(feature = "capture-fixture")]
use windows_sys::Win32::System::Threading::WaitForSingleObject;

type Object = unsafe extern "C" fn(*mut c_void);
type Submission = unsafe extern "C" fn(*mut c_void, u32, *const *mut c_void);
type ColorSpace = unsafe extern "C" fn(*mut c_void, u32);
type Feature = unsafe extern "C" fn(*mut c_void, *const c_void, u32);
type Evaluation = unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void, u32, u64);

// loader/census/include/rescaleframe/census_d3d12_observer.h
type ObserveD3d12 =
    unsafe extern "C" fn(Object, Submission, Submission, Object, Object, ColorSpace);
type ObserveNgx = unsafe extern "C" fn(Feature, Evaluation, Object);
pub(super) type ReadNgx =
    unsafe extern "C" fn(*const c_void, u32, *const c_char, *mut c_void) -> bool;

pub(super) struct Native {
    pub(super) capture: Capture,
    pub(super) observations: Mutex<()>,
    pub(super) directory: Vec<u16>,
    pub(super) request: Vec<u16>,
    pub(super) base: usize,
    pub(super) fixture: bool,
    pub(super) selected_chain: AtomicPtr<c_void>,
    pub(super) stop: AtomicBool,
    pub(super) read_ngx: ReadNgx,
    log: PathBuf,
}

impl Native {
    pub(super) fn read<T: Copy + Default>(&self, address: usize) -> Option<T> {
        let mut value = T::default();
        (!self.fixture
            && unsafe {
                read_process_memory(address, (&mut value as *mut T).cast(), size_of::<T>())
            })
        .then_some(value)
    }

    pub(super) fn progress(&self) -> Progress {
        // Callers hold observations.
        unsafe { (*self.capture.ledger.get()).progress() }
    }

    pub(super) fn log(&self, message: std::fmt::Arguments<'_>) {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log)
        {
            let line = format!("[{}] {message}\n", unsafe { GetTickCount64() });
            let _ = file.write_all(line.as_bytes());
        }
    }
}

static OWNER: AtomicPtr<Native> = AtomicPtr::new(null_mut());
// The numeric HANDLE can cross threads; only the fixture removes and closes it.
static FIXTURE_THREAD: Mutex<usize> = Mutex::new(0);

pub(super) fn owner() -> &'static Native {
    let owner = OWNER.load(Ordering::Acquire);
    assert!(!owner.is_null(), "capture registered before publication");
    unsafe { &*owner }
}

/// The ASI and fixture lend their linked census and exception-boundary functions.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_start(
    directory: *const u16,
    fixture: bool,
    observe_d3d12: ObserveD3d12,
    observe_ngx: ObserveNgx,
    read_ngx: ReadNgx,
) -> bool {
    let mut thread = lock(&FIXTURE_THREAD);
    if !OWNER.load(Ordering::Acquire).is_null() {
        return false;
    }
    let Some(directory) = (unsafe { capture_directory(directory) }) else {
        return false;
    };
    let root = PathBuf::from(OsString::from_wide(directory));
    let request = directory
        .iter()
        .copied()
        .chain("\\native.request".encode_utf16())
        .chain([0])
        .collect();
    let Some(state) = try_box(Native {
        capture: Capture::new(),
        observations: Mutex::new(()),
        directory: directory.to_vec(),
        request,
        base: unsafe { GetModuleHandleW(std::ptr::null()) } as usize,
        fixture,
        selected_chain: AtomicPtr::new(null_mut()),
        stop: AtomicBool::new(false),
        read_ngx,
        log: root.join("native-probe.log"),
    }) else {
        return false;
    };
    let state = Box::into_raw(state);
    OWNER.store(state, Ordering::Release);
    let handle = unsafe {
        CreateThread(
            std::ptr::null(),
            0,
            Some(poll::worker),
            state.cast(),
            0,
            null_mut(),
        )
    };
    if handle.is_null() {
        OWNER.store(null_mut(), Ordering::Release);
        // Census cannot have reached the unpublished hooks.
        drop(unsafe { Box::from_raw(state) });
        return false;
    }
    unsafe {
        observe_d3d12(
            detours::device_created,
            detours::before_submit,
            detours::after_submit,
            detours::before_present,
            detours::swapchain_created,
            detours::color_space_set,
        );
        observe_ngx(ngx::created, ngx::evaluate, ngx::released);
    }
    if fixture {
        *thread = handle as usize;
    } else {
        unsafe { CloseHandle(handle) };
    }
    owner().log(format_args!("native capture installed"));
    true
}

#[cfg(feature = "capture-fixture")]
pub(super) fn arm_fixture(chain: *mut c_void) {
    let state = owner();
    let _guard = lock(&state.observations);
    state.selected_chain.store(chain, Ordering::Release);
    super::rsf_rdr2_capture_request(&state.capture);
}

#[cfg(feature = "capture-fixture")]
pub(super) fn stop_fixture() {
    let handle = std::mem::take(&mut *lock(&FIXTURE_THREAD)) as *mut c_void;
    if !handle.is_null() {
        owner().stop.store(true, Ordering::Release);
        // Keep process-lived callback storage even on a deliberately bounded fixture wait.
        unsafe {
            WaitForSingleObject(handle, 5000);
            CloseHandle(handle);
        }
    }
}
