// SPDX-License-Identifier: MIT
//! The capture worker retires GPU leases and publishes detached artifacts.
use super::{
    bundle::{self, Bundle},
    d3d12,
    install::{Native, owner},
    mark_published, poll_request, readback_written, shots, timeout,
};
use crate::platform::lock;
use std::ffi::{c_char, c_void};
use std::sync::atomic::Ordering;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, Sleep};

unsafe fn write(
    leases: &super::gpu_leases::ReadbackLeases,
    image: &shots::Image,
    role: *const c_char,
) -> bool {
    let state = owner();
    let mut result = 0;
    let saved = unsafe {
        d3d12::d3d12_write(
            leases,
            image,
            state.directory.as_ptr(),
            state.directory.len(),
            GetCurrentProcessId(),
            role,
            &mut result,
        )
    };
    if result != 0 {
        state.log(format_args!(
            "readback Map failed hr={:08x} role={:?}",
            result as u32,
            unsafe { std::ffi::CStr::from_ptr(role) }
        ));
    }
    saved
}

pub(super) unsafe extern "system" fn worker(state: *mut c_void) -> u32 {
    let state = unsafe { &*state.cast::<Native>() };
    let capture = &state.capture;
    let bundle = Bundle {
        directory: state.directory.as_ptr(),
        directory_len: state.directory.len(),
        base: state.base as u64,
        pid: unsafe { GetCurrentProcessId() },
    };
    loop {
        unsafe { Sleep(100) };
        if state.stop.load(Ordering::Acquire) {
            return 0;
        }
        unsafe { poll_request(capture, state.request.as_ptr()) };
        let (expired, retired) = {
            let _lock = lock(&state.observations);
            let expired =
                unsafe { timeout(capture, GetTickCount64()) }.then(|| state.progress().sequence);
            let retired = unsafe {
                shots::collect_ready(capture, d3d12::d3d12_completed, d3d12::d3d12_removed)
            };
            (expired, retired)
        };
        if let Some(sequence) = expired {
            state.log(format_args!("capture {sequence} timed out waiting for scene or DLSS input; retaining explicit incomplete result"));
        }
        drop(retired);
        for index in 0..unsafe { shots::ready_count(capture) } {
            let saved = unsafe { shots::write_ready(capture, index, write) };
            let _lock = lock(&state.observations);
            unsafe { readback_written(capture, saved) };
        }
        let detached = {
            let _lock = lock(&state.observations);
            unsafe { bundle::take_bundle(capture) }
        };
        if detached {
            let published = unsafe { bundle::publish_bundle(capture, &bundle) };
            state.log(format_args!(
                "capture {} bundle saved={} input_frames={} draws={} readbacks={}",
                published.sequence,
                published.saved,
                published.inputs,
                published.draws,
                published.written,
            ));
            let _lock = lock(&state.observations);
            unsafe { mark_published(capture, published.sequence) };
        }
        unsafe { shots::drop_ready(capture) };
    }
}
