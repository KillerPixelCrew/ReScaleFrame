// SPDX-License-Identifier: MIT
use std::cell::UnsafeCell;
use std::ffi::c_char;
use std::ptr::{null, null_mut};
use std::slice;
use std::sync::{
    OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{
        CREATE_ALWAYS, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ,
        MOVEFILE_REPLACE_EXISTING, MoveFileExW, WriteFile,
    },
};

const BUFFER_BYTES: usize = 256 << 10;
const FLUSH_US: u64 = 250_000;

struct Batch {
    handle: HANDLE,
    bytes: Vec<u8>,
    last_write_us: u64,
}

struct Output {
    batch: UnsafeCell<Batch>,
    budget: u64,
    written: AtomicU64,
    admitted_bytes: AtomicU64,
    dropped_budget: AtomicU64,
    exhausted: AtomicBool,
}

// The native census serializes all batch operations under the writer lock.
// Counts can be read independently while the batch is being drained.
unsafe impl Send for Output {}
unsafe impl Sync for Output {}
static OUTPUT: OnceLock<Output> = OnceLock::new();

fn open(path: *const u16) -> HANDLE {
    unsafe {
        CreateFileW(
            path,
            GENERIC_WRITE,
            FILE_SHARE_READ,
            null(),
            CREATE_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    }
}

impl Batch {
    fn flush(&mut self, now_us: u64) {
        if self.handle == INVALID_HANDLE_VALUE || self.bytes.is_empty() {
            return;
        }
        let mut done = 0;
        unsafe {
            WriteFile(
                self.handle,
                self.bytes.as_ptr(),
                self.bytes.len() as u32,
                &mut done,
                null_mut(),
            )
        };
        self.bytes.clear();
        self.last_write_us = now_us;
    }
}

pub(super) fn rsf_census_output_open(path: *const u16, budget: u64) -> bool {
    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(BUFFER_BYTES).is_err() {
        return false;
    }
    let handle = open(path);
    if handle == INVALID_HANDLE_VALUE {
        return false;
    }
    if OUTPUT
        .set(Output {
            batch: UnsafeCell::new(Batch {
                handle,
                bytes,
                last_write_us: 0,
            }),
            budget,
            written: AtomicU64::new(0),
            admitted_bytes: AtomicU64::new(0),
            dropped_budget: AtomicU64::new(0),
            exhausted: AtomicBool::new(false),
        })
        .is_err()
    {
        unsafe { CloseHandle(handle) };
        return false;
    }
    true
}

pub(super) fn rsf_census_output_close() {
    if let Some(output) = OUTPUT.get() {
        let batch = unsafe { &mut *output.batch.get() };
        if batch.handle != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(batch.handle) };
            batch.handle = INVALID_HANDLE_VALUE;
        }
    }
}

pub(super) unsafe fn rsf_census_output_append(data: *const c_char, size: usize, now_us: u64) {
    let Some(output) = OUTPUT.get() else { return };
    if size == 0 || size > BUFFER_BYTES {
        return;
    }
    if output
        .admitted_bytes
        .load(Relaxed)
        .saturating_add(size as u64)
        > output.budget
    {
        output.dropped_budget.fetch_add(1, Relaxed);
        output.exhausted.store(true, Relaxed);
        return;
    }
    let batch = unsafe { &mut *output.batch.get() };
    if batch.bytes.len() + size > BUFFER_BYTES {
        batch.flush(now_us);
    }
    batch
        .bytes
        .extend_from_slice(unsafe { slice::from_raw_parts(data.cast::<u8>(), size) });
    output.admitted_bytes.fetch_add(size as u64, Relaxed);
    output.written.fetch_add(1, Relaxed);
}

pub(super) fn rsf_census_output_flush(now_us: u64) {
    if let Some(output) = OUTPUT.get() {
        unsafe { &mut *output.batch.get() }.flush(now_us);
    }
}

pub(super) fn rsf_census_output_flush_due(now_us: u64) {
    if let Some(output) = OUTPUT.get() {
        let batch = unsafe { &mut *output.batch.get() };
        if now_us.saturating_sub(batch.last_write_us) > FLUSH_US {
            batch.flush(now_us);
        }
    }
}

pub(super) fn rsf_census_output_reset_records() {
    if let Some(output) = OUTPUT.get() {
        output.written.store(0, Relaxed);
    }
}

/// Records written, records dropped for the budget, bytes admitted and whether the budget is spent.
pub(super) fn rsf_census_output_counts() -> (u64, u64, u64, bool) {
    OUTPUT.get().map_or((0, 0, 0, false), |output| {
        (
            output.written.load(Relaxed),
            output.dropped_budget.load(Relaxed),
            output.admitted_bytes.load(Relaxed),
            output.exhausted.load(Relaxed),
        )
    })
}

pub(super) fn rsf_census_output_summary(
    temporary: *const u16,
    final_path: *const u16,
    data: *const c_char,
    size: usize,
) -> bool {
    let Ok(size) = u32::try_from(size) else {
        return false;
    };
    let handle = open(temporary);
    if handle == INVALID_HANDLE_VALUE {
        return false;
    }
    let mut done = 0;
    let ok = unsafe { WriteFile(handle, data.cast(), size, &mut done, null_mut()) } != 0;
    unsafe { CloseHandle(handle) };
    if ok && done == size {
        unsafe { MoveFileExW(temporary, final_path, MOVEFILE_REPLACE_EXISTING) };
    }
    true
}
