// SPDX-License-Identifier: MIT
//! Append-only file diagnostics for Windows plugin carriers.
use std::ffi::{CStr, OsString, c_char};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, PoisonError};

#[derive(Default)]
pub struct CarrierLog {
    path: OnceLock<PathBuf>,
    writer: Mutex<()>,
}

impl CarrierLog {
    pub const fn new() -> Self {
        Self {
            path: OnceLock::new(),
            writer: Mutex::new(()),
        }
    }

    pub fn set_path(&self, path: &[u16]) {
        let path = path.strip_suffix(&[0]).unwrap_or(path);
        let _ = self.path.set(PathBuf::from(OsString::from_wide(path)));
    }

    pub fn write(&self, tick: u64, message: impl AsRef<[u8]>) {
        let Some(path) = self.path.get() else { return };
        let _writer = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = write!(file, "[{tick}] ");
            let _ = file.write_all(message.as_ref());
            let _ = file.write_all(b"\n");
        }
    }

    /// # Safety
    /// A non-null message must be a readable, NUL-terminated C string.
    pub unsafe fn write_c_message(&self, tick: u64, message: *const c_char) {
        if !message.is_null() {
            self.write(tick, unsafe { CStr::from_ptr(message) }.to_bytes());
        }
    }
}
