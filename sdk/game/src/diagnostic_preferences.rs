// SPDX-License-Identifier: MIT
//! The Runtime diagnostics profile (%LOCALAPPDATA%\ReScaleFrame\Diagnostics.ini), read by the
//! Runtime and the standalone XeSS FG bridge.
use crate::ffi_util::wide;
use crate::profile_text;
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetPrivateProfileStringW(
        section: *const u16,
        key: *const u16,
        fallback: *const u16,
        output: *mut u16,
        capacity: u32,
        file: *const u16,
    ) -> u32;
}

pub const SECTION: &str = "Diagnostics";

pub fn profile() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").filter(|path| !path.is_empty())?;
    Some(
        PathBuf::from(local)
            .join("ReScaleFrame")
            .join("Diagnostics.ini"),
    )
}

pub fn value(variable: &str) -> Option<OsString> {
    std::env::var_os(variable).or_else(|| {
        let path: Vec<u16> = profile()?.as_os_str().encode_wide().chain([0]).collect();
        let (section, key) = (wide(SECTION), wide(variable));
        let mut buffer = vec![0u16; 32_768];
        let count = unsafe {
            GetPrivateProfileStringW(
                section.as_ptr(),
                key.as_ptr(),
                [0u16].as_ptr(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                path.as_ptr(),
            )
        } as usize;
        if count == 0 || count >= buffer.len() - 1 {
            return None;
        }
        buffer.truncate(count);
        Some(OsString::from_wide(&profile_text::decode(buffer)))
    })
}
