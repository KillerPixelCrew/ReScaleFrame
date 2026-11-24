// SPDX-License-Identifier: MIT
use super::memory::{read, u32_at};
use super::rsf_census_json;
use std::ffi::{CStr, c_char, c_void};
use std::fmt::Write;
use std::{ptr, slice};
use windows_sys::Win32::Globalization::{CP_UTF8, WideCharToMultiByte};

pub(super) unsafe fn emit_decimal(
    json: &mut rsf_census_json,
    key: *const c_char,
    value: f64,
    format: unsafe extern "C" fn(*mut c_char, usize, f64) -> i32,
) {
    if value.is_finite() {
        let mut text = [0; 40];
        let count = unsafe { format(text.as_mut_ptr(), text.len(), value) };
        if count >= 0 && (count as usize) < text.len() {
            unsafe { rsf_census_json_raw(json, key, text.as_ptr()) };
            return;
        }
    }
    unsafe { rsf_census_json_raw(json, key, ptr::null()) };
}

/// A census JSON key: a name, or `None` for an array element.
pub(super) trait Key {
    fn ptr(self) -> *const c_char;
}

impl Key for &CStr {
    fn ptr(self) -> *const c_char {
        self.as_ptr()
    }
}

impl Key for Option<&CStr> {
    fn ptr(self) -> *const c_char {
        self.map_or(ptr::null(), CStr::as_ptr)
    }
}

/// The writer every census module builds its records with.
pub(super) struct Json<'a>(pub(super) &'a mut rsf_census_json);

impl Json<'_> {
    pub(super) fn open(&mut self, key: impl Key, delimiter: u8) {
        unsafe { rsf_census_json_open(self.0, key.ptr(), delimiter as c_char) };
    }
    pub(super) fn close(&mut self, delimiter: u8) {
        rsf_census_json_close(self.0, delimiter as c_char);
    }
    pub(super) fn number(&mut self, key: impl Key, value: impl Into<u64>) {
        unsafe { rsf_census_json_u64(self.0, key.ptr(), value.into()) };
    }
    pub(super) fn signed(&mut self, key: impl Key, value: impl Into<i64>) {
        unsafe { rsf_census_json_i64(self.0, key.ptr(), value.into()) };
    }
    pub(super) fn hex(&mut self, key: impl Key, value: u64) {
        unsafe { rsf_census_json_hex(self.0, key.ptr(), value) };
    }
    pub(super) fn text(&mut self, key: impl Key, value: &CStr) {
        unsafe { rsf_census_json_string(self.0, key.ptr(), value.as_ptr()) };
    }
    pub(super) fn raw(&mut self, key: impl Key, value: &CStr) {
        unsafe { rsf_census_json_raw(self.0, key.ptr(), value.as_ptr()) };
    }
    pub(super) fn flag(&mut self, key: &CStr, on: bool) {
        self.raw(key, if on { c"true" } else { c"false" });
    }
    /// The 8-byte word at `address`, or null when it cannot be read.
    pub(super) fn output(&mut self, key: &CStr, address: u64) {
        if let Some(value) = read::<8>(address, 0) {
            self.hex(Some(key), u64::from_le_bytes(value));
        } else {
            self.raw(key, c"null");
        }
    }
    pub(super) fn fields(&mut self, data: &[u8], fields: &[(&CStr, usize)]) {
        for &(key, offset) in fields {
            self.number(key, u32_at(data, offset));
        }
    }
    pub(super) fn args(&mut self, args: &[u64]) {
        self.open(Some(c"raw_args"), b'[');
        for &value in args {
            self.hex(None, value);
        }
        self.close(b']');
    }
}

const HEX: &[u8; 16] = b"0123456789abcdef";

impl rsf_census_json {
    fn put(&mut self, bytes: &[u8]) {
        if self.overflow {
            return;
        }
        if bytes.len() >= self.capacity.saturating_sub(self.length) {
            self.overflow = true;
            return;
        }
        unsafe {
            ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                self.buffer.cast::<u8>().add(self.length),
                bytes.len(),
            );
        }
        self.length += bytes.len();
    }

    fn quoted(&mut self, text: &[u8]) {
        self.put(b"\"");
        for &byte in text {
            if self.overflow {
                return;
            }
            match byte {
                b'"' | b'\\' => self.put(&[b'\\', byte]),
                0..=0x1f => self.put(&[
                    b'\\',
                    b'u',
                    b'0',
                    b'0',
                    HEX[(byte >> 4) as usize],
                    HEX[(byte & 15) as usize],
                ]),
                _ => self.put(&[byte]),
            }
        }
        self.put(b"\"");
    }

    unsafe fn lead(&mut self, key: *const c_char) {
        if self.depth > 0 {
            let bit = 1 << (self.depth - 1);
            if self.comma & bit != 0 {
                self.put(b",");
            }
            self.comma |= bit;
        }
        if !key.is_null() {
            self.quoted(unsafe { CStr::from_ptr(key) }.to_bytes());
            self.put(b":");
        }
    }

    unsafe fn string(&mut self, key: *const c_char, value: *const c_char) {
        unsafe { self.lead(key) };
        let text = if value.is_null() {
            b""
        } else {
            unsafe { CStr::from_ptr(value) }.to_bytes()
        };
        self.quoted(text);
    }
}

impl Write for rsf_census_json {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.put(text.as_bytes());
        Ok(())
    }
}

pub(super) unsafe fn rsf_census_json_open(
    json: &mut rsf_census_json,
    key: *const c_char,
    delimiter: c_char,
) {
    if json.depth >= 31 {
        json.overflow = true;
        return;
    }
    unsafe { json.lead(key) };
    json.put(&[delimiter as u8]);
    json.comma &= !(1 << json.depth);
    json.depth += 1;
}

pub(super) fn rsf_census_json_close(json: &mut rsf_census_json, delimiter: c_char) {
    json.depth = json.depth.saturating_sub(1);
    json.put(&[delimiter as u8]);
}

pub(super) unsafe fn rsf_census_json_string(
    json: &mut rsf_census_json,
    key: *const c_char,
    value: *const c_char,
) {
    unsafe { json.string(key, value) };
}

pub(super) unsafe fn rsf_census_json_wide(
    json: &mut rsf_census_json,
    key: *const c_char,
    value: *const u16,
) {
    let mut utf8 = [0u8; 512];
    let len = if value.is_null() {
        0
    } else {
        unsafe {
            WideCharToMultiByte(
                CP_UTF8,
                0,
                value,
                -1,
                utf8.as_mut_ptr(),
                utf8.len() as i32,
                ptr::null(),
                ptr::null_mut(),
            )
        }
    };
    unsafe { json.lead(key) };
    json.quoted(&utf8[..len.saturating_sub(1).max(0) as usize]);
}

pub(super) unsafe fn rsf_census_json_u64(
    json: &mut rsf_census_json,
    key: *const c_char,
    value: u64,
) {
    unsafe { json.lead(key) };
    let _ = write!(json, "{value}");
}

pub(super) unsafe fn rsf_census_json_i64(
    json: &mut rsf_census_json,
    key: *const c_char,
    value: i64,
) {
    unsafe { json.lead(key) };
    let _ = write!(json, "{value}");
}

pub(super) unsafe fn rsf_census_json_hex(
    json: &mut rsf_census_json,
    key: *const c_char,
    value: u64,
) {
    unsafe { json.lead(key) };
    let _ = write!(json, "\"0x{value:x}\"");
}

pub(super) unsafe fn rsf_census_json_bytes(
    json: &mut rsf_census_json,
    key: *const c_char,
    data: *const c_void,
    size: usize,
) {
    unsafe { json.lead(key) };
    json.put(b"\"");
    if size != 0 {
        for &byte in unsafe { slice::from_raw_parts(data.cast::<u8>(), size) } {
            if json.overflow {
                return;
            }
            json.put(&[HEX[(byte >> 4) as usize], HEX[(byte & 15) as usize]]);
        }
    }
    json.put(b"\"");
}

pub(super) unsafe fn rsf_census_json_raw(
    json: &mut rsf_census_json,
    key: *const c_char,
    value: *const c_char,
) {
    unsafe { json.lead(key) };
    let text = if value.is_null() {
        b"null"
    } else {
        unsafe { CStr::from_ptr(value) }.to_bytes()
    };
    json.put(text);
}
