// SPDX-License-Identifier: MIT

use super::observations;
use crate::motion_capture_session::{CaptureInterval, rsf_ac7_capture_enabled};
use crate::velocity_container::borrowed;
use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::offset_of;

const MAX_BUFFER_BYTES: usize = 8192;
const MAX_UPLOADS: usize = 2048;
const REGULAR_READS: u32 = 120;
const LIGHTING_READS: u32 = 8;

#[derive(Default)]
struct Upload {
    bytes: Vec<u8>,
    serial: u64,
}

impl Upload {
    fn replace(&mut self, bytes: &[u8], serial: u64) -> bool {
        let extra = bytes.len().saturating_sub(self.bytes.len());
        if self.bytes.try_reserve_exact(extra).is_err() {
            return false;
        }
        self.bytes.clear();
        self.bytes.extend_from_slice(bytes);
        self.serial = serial;
        true
    }
}

#[derive(Default)]
struct Readbacks {
    interval: Option<u32>,
    used: u32,
    lighting_used: u32,
    attempted: u32,
    failed: u32,
}

impl Readbacks {
    fn has_budget(&self, lighting: bool) -> bool {
        if lighting {
            self.used < REGULAR_READS + LIGHTING_READS && self.lighting_used < LIGHTING_READS
        } else {
            self.used < REGULAR_READS
        }
    }
}

#[derive(Default)]
pub(super) struct ConstantBuffers {
    uploads: HashMap<usize, Upload>,
    serial: u64,
    readbacks: Readbacks,
}

impl ConstantBuffers {
    pub(super) fn reset(&mut self) {
        self.uploads.clear();
        self.readbacks = Readbacks::default();
    }

    pub(super) fn readback_counts(&self) -> (u32, u32) {
        (self.readbacks.attempted, self.readbacks.failed)
    }

    fn store(&mut self, identity: usize, bytes: &[u8]) -> bool {
        let serial = self.serial.wrapping_add(1);
        if let Some(upload) = self.uploads.get_mut(&identity) {
            if !upload.replace(bytes, serial) {
                return false;
            }
        } else {
            if self.uploads.len() >= MAX_UPLOADS {
                return true;
            }
            let mut upload = Upload::default();
            if self.uploads.try_reserve(1).is_err() || !upload.replace(bytes, serial) {
                return false;
            }
            self.uploads.insert(identity, upload);
        }
        self.serial = serial;
        true
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_capture_store_upload(
    capture: CaptureInterval,
    buffer: *mut c_void,
    bytes: *const u8,
    size: u32,
    created: bool,
) -> bool {
    if !rsf_ac7_capture_enabled() || buffer.is_null() {
        return true;
    }
    let size = size as usize;
    let snapshot = capture.active() && !bytes.is_null() && (1..=MAX_BUFFER_BYTES).contains(&size);
    if !snapshot && !created {
        return true;
    }

    let mut observations = observations();
    let current = CaptureInterval::load();
    if current.session != capture.session {
        return true;
    }
    let constants = &mut observations.constants;
    let identity = buffer.addr();
    if created {
        constants.uploads.remove(&identity);
    }
    if !snapshot || !current.active() {
        return true;
    }
    // SAFETY: The creation or upload callback lends the exact source span for this call.
    let Some(bytes) = (unsafe { borrowed(bytes, size) }) else {
        return true;
    };
    constants.store(identity, bytes)
}

#[repr(C)]
struct ConstantRecord {
    buffer: u64,
    upload_serial: u64,
}

const _: () = {
    assert!(size_of::<ConstantRecord>() == 16 && offset_of!(ConstantRecord, upload_serial) == 8);
};

#[repr(u32)]
enum ConstantSource {
    Stopped,
    Uploaded,
    TryRead,
    Missing,
    Failed,
}

type CopyUpload = unsafe extern "C" fn(*mut c_void, *const u8, usize) -> bool;

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_capture_constant_source(
    capture: CaptureInterval,
    buffer: *mut c_void,
    lighting: bool,
    record: &mut ConstantRecord,
    user: *mut c_void,
    copy: CopyUpload,
) -> ConstantSource {
    let mut observations = observations();
    if !capture.active() || CaptureInterval::load() != capture {
        return ConstantSource::Stopped;
    }
    let constants = &mut observations.constants;
    let readbacks = &mut constants.readbacks;
    if readbacks.interval != Some(capture.interval) {
        readbacks.interval = Some(capture.interval);
        readbacks.used = 0;
        readbacks.lighting_used = 0;
    }

    let identity = buffer.addr();
    record.buffer = identity as u64;
    record.upload_serial = 0;
    if !lighting && let Some(upload) = constants.uploads.get(&identity) {
        // SAFETY: Native only copies into its local byte vector while the cache lends this span.
        if !unsafe { copy(user, upload.bytes.as_ptr(), upload.bytes.len()) } {
            return ConstantSource::Failed;
        }
        record.upload_serial = upload.serial;
        return ConstantSource::Uploaded;
    }
    if readbacks.has_budget(lighting) {
        ConstantSource::TryRead
    } else {
        ConstantSource::Missing
    }
}

#[repr(u32)]
enum ConstantRead {
    Stopped,
    Skipped,
    Reserved,
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_reserve_constant_read(
    capture: CaptureInterval,
    width: u32,
    lighting: bool,
) -> ConstantRead {
    if !(1..=MAX_BUFFER_BYTES).contains(&(width as usize)) {
        return ConstantRead::Skipped;
    }
    let mut observations = observations();
    if !capture.active() || CaptureInterval::load() != capture {
        return ConstantRead::Stopped;
    }
    let readbacks = &mut observations.constants.readbacks;
    if !readbacks.has_budget(lighting) {
        return ConstantRead::Skipped;
    }
    readbacks.attempted += 1;
    readbacks.used += 1;
    readbacks.lighting_used += u32::from(lighting);
    ConstantRead::Reserved
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ac7_capture_constant_read_failed(capture: CaptureInterval) {
    let mut observations = observations();
    let current = CaptureInterval::load();
    if current.active() && current.session == capture.session {
        observations.constants.readbacks.failed += 1;
    }
}
