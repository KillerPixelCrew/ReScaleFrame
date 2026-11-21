// SPDX-License-Identifier: MIT
//! ASI diagnostic sampling and the optional RenderDoc request worker.
use crate::camera_probe::rsf_rdr2_camera_request;
use crate::runtime_worker::{append_path, log};
use std::ffi::{CString, c_char, c_void};
use std::ptr::{null, null_mut};
use std::slice;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Storage::FileSystem::DeleteFileW;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::{CreateThread, GetCurrentProcessId, Sleep};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_F10};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

const MODULE_MARKS_MS: [u32; 5] = [5_000, 15_000, 30_000, 60_000, 120_000];

#[repr(C)]
#[derive(Default)]
struct DumpResult {
    result: i32,
    sections: u32,
    imports: u32,
    iat_refs: u32,
    entropy: f64,
    base: u64,
    bytes: u64,
}

#[repr(C)]
#[derive(Default)]
struct CensusResult {
    calls: u64,
    written: u64,
    presents: u64,
    dropped: u64,
    sites: u32,
    hooks: u32,
    conflicts: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DiagnosticsLeaves {
    measure: unsafe extern "C" fn() -> f64,
    dump: unsafe extern "C" fn(*const c_char, f64, u32, u32, u32, u32, *mut DumpResult),
    modules: unsafe extern "C" fn(*const c_char, *const c_char),
    census: unsafe extern "C" fn(*mut CensusResult) -> bool,
}

const _: () = {
    assert!(size_of::<DumpResult>() == 40);
    assert!(size_of::<CensusResult>() == 48);
    assert!(size_of::<DiagnosticsLeaves>() == 32);
};

struct Diagnostics {
    modules: CString,
    dumps: CString,
    leaves: DiagnosticsLeaves,
}

type Worker = unsafe extern "system" fn(*mut c_void) -> u32;
type FrameInitialise = unsafe extern "C" fn(*const c_char, *const c_char) -> i32;
type FrameTrigger = unsafe extern "C" fn(u32) -> i32;

/// The entry consumes the owned pointer exactly once, after successful thread creation.
unsafe fn start<T: Send>(work: T, entry: Worker) -> bool {
    let work = Box::into_raw(Box::new(work));
    let thread = unsafe { CreateThread(null(), 0, Some(entry), work.cast(), 0, null_mut()) };
    if thread.is_null() {
        drop(unsafe { Box::from_raw(work) });
        return false;
    }
    unsafe { CloseHandle(thread) };
    true
}

/// The native caller passes a NUL-terminated UTF-16 path, or null to omit it.
unsafe fn path_units<'a>(path: *const u16) -> &'a [u16] {
    if path.is_null() {
        return &[];
    }
    let mut len = 0;
    while unsafe { *path.add(len) } != 0 {
        len += 1;
    }
    unsafe { slice::from_raw_parts(path, len) }
}

fn utf8(text: &[u16]) -> CString {
    CString::new(String::from_utf16_lossy(text)).unwrap_or_default()
}

unsafe extern "system" fn diagnostics(work: *mut c_void) -> u32 {
    let worker = unsafe { Box::from_raw(work.cast::<Diagnostics>()) };
    let leaves = worker.leaves;
    let entropy = unsafe { (leaves.measure)() };
    log(format!("Code entropy at entry {entropy:.3}."));
    if !worker.dumps.is_empty() {
        let mut report = DumpResult::default();
        unsafe { (leaves.dump)(worker.dumps.as_ptr(), 7.0, 1, 250, 180_000, 2, &mut report) };
        log(format!(
            "Module dump result={} entropy={:.3} sections={} imports={} iat_refs={} base=0x{:x} bytes={}",
            report.result,
            report.entropy,
            report.sections,
            report.imports,
            report.iat_refs,
            report.base,
            report.bytes
        ));
    }
    let begin = unsafe { GetTickCount64() };
    for mark in MODULE_MARKS_MS {
        let elapsed = unsafe { GetTickCount64() }.wrapping_sub(begin);
        if elapsed < u64::from(mark) {
            unsafe { Sleep(mark - elapsed as u32) };
        }
        let label = CString::new(format!("t+{}s", mark / 1000)).unwrap_or_default();
        unsafe { (leaves.modules)(worker.modules.as_ptr(), label.as_ptr()) };
    }
    log("Module sampling finished.");
    let mut report = CensusResult::default();
    if unsafe { (leaves.census)(&mut report) } {
        log(format!(
            "Census at 120 s: calls={} written={} sites={} hooks={} conflicts={} presents={} dropped={}",
            report.calls,
            report.written,
            report.sites,
            report.hooks,
            report.conflicts,
            report.presents,
            report.dropped
        ));
    }
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_start_diagnostics(
    modules: *const u16,
    dumps: *const u16,
    leaves: &DiagnosticsLeaves,
) {
    let worker = Diagnostics {
        modules: utf8(unsafe { path_units(modules) }),
        dumps: utf8(unsafe { path_units(dumps) }),
        leaves: *leaves,
    };
    let _ = unsafe { start(worker, diagnostics) };
}

pub(crate) fn foreground() -> bool {
    let mut process = 0;
    unsafe {
        GetWindowThreadProcessId(GetForegroundWindow(), &mut process);
        process == GetCurrentProcessId()
    }
}

pub(crate) fn capture_key_down() -> bool {
    (unsafe { GetAsyncKeyState(i32::from(VK_F10)) }) < 0
}

struct FrameRequests {
    request: Vec<u16>,
    trigger: FrameTrigger,
}

unsafe extern "system" fn frame_requests(work: *mut c_void) -> u32 {
    let work = unsafe { Box::from_raw(work.cast::<FrameRequests>()) };
    let mut previous_key = false;
    loop {
        unsafe { Sleep(250) };
        let key = foreground() && capture_key_down();
        let requested = unsafe { DeleteFileW(work.request.as_ptr()) } != 0;
        if requested || (key && !previous_key) {
            rsf_rdr2_camera_request();
            unsafe { (work.trigger)(1) };
        }
        previous_key = key;
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_start_frame_probe(
    library: *const u16,
    prefix: *const u16,
    initialise: FrameInitialise,
    trigger: FrameTrigger,
) -> i32 {
    let library = utf8(unsafe { path_units(library) });
    let prefix = unsafe { path_units(prefix) };
    let output = utf8(prefix);
    let result = unsafe { initialise(library.as_ptr(), output.as_ptr()) };
    if result == 0 {
        let work = FrameRequests {
            request: append_path(prefix, ".request"),
            trigger,
        };
        let _ = unsafe { start(work, frame_requests) };
    }
    result
}
