// SPDX-License-Identifier: MIT
//! The capture worker polls requests, retires GPU leases and publishes detached artifacts.
use super::bundle::{self, Bundle, FormatScalar, Report};
use super::gpu_leases::drop_idle;
use super::shots::{self, Completed, DeviceRemoved, WriteImage};
use super::{Capture, install, poll_request, readback_written, timeout};
use std::ffi::c_void;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, Sleep};

#[repr(C)]
struct PollOps {
    lock: unsafe extern "C" fn(*mut c_void),
    unlock: unsafe extern "C" fn(*mut c_void),
    completed: Completed,
    removed: DeviceRemoved,
    write: WriteImage,
    timeout: unsafe extern "C" fn(u32),
    format_scalar: FormatScalar,
    report: Report,
}

const _: () = assert!(size_of::<PollOps>() == 64 && std::mem::offset_of!(PollOps, report) == 56);

struct Lock<'a>(&'a PollOps, *mut c_void);

impl Drop for Lock<'_> {
    fn drop(&mut self) {
        unsafe { (self.0.unlock)(self.1) };
    }
}

impl PollOps {
    unsafe fn lock(&self, context: *mut c_void) -> Lock<'_> {
        unsafe { (self.lock)(context) };
        Lock(self, context)
    }
}

/// One process-lived worker per owner. The native context and paths live as long as this call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_poll(
    owner: &Capture,
    context: *mut c_void,
    ops: &PollOps,
    request: *const u16,
    directory: *const u16,
    directory_len: usize,
    base: u64,
    fixture: bool,
) {
    let bundle = Bundle {
        directory,
        directory_len,
        base,
        pid: unsafe { GetCurrentProcessId() },
    };
    loop {
        unsafe { Sleep(100) };
        if fixture && install::fixture_stopping() {
            return;
        }
        unsafe { poll_request(owner, request) };
        let (expired, retired) = {
            let _lock = unsafe { ops.lock(context) };
            let expired = unsafe { timeout(owner, GetTickCount64()) }
                .then(|| unsafe { (*owner.ledger.get()).sequence });
            let retired = unsafe { shots::collect_ready(owner, ops.completed, ops.removed) };
            (expired, retired)
        };
        if let Some(sequence) = expired {
            unsafe { (ops.timeout)(sequence) };
        }
        drop_idle(retired);
        for index in 0..unsafe { shots::ready_count(owner) } {
            let saved = unsafe { shots::write_ready(owner, index, ops.write) };
            let _lock = unsafe { ops.lock(context) };
            unsafe { readback_written(owner, saved) };
        }
        let detached = {
            let _lock = unsafe { ops.lock(context) };
            unsafe { bundle::take_bundle(owner) }
        };
        if detached {
            let sequence =
                unsafe { bundle::publish_bundle(owner, &bundle, ops.format_scalar, ops.report) };
            let _lock = unsafe { ops.lock(context) };
            unsafe { super::mark_published(owner, sequence) };
        }
        unsafe { shots::drop_ready(owner) };
    }
}
