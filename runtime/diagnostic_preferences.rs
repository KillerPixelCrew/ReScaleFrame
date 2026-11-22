// SPDX-License-Identifier: MIT
//! The Runtime diagnostics profile is shared with the standalone bridge.
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;

// The standalone XeSS bridge is compiled directly with rustc.
#[allow(clippy::duplicate_mod)] // Shared with another crate or module through #[path].
#[path = "../sdk/game/src/profile_text.rs"]
#[allow(dead_code)]
mod profile_text;

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

pub(crate) const SECTION: &str = "Diagnostics";

pub(crate) fn profile() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").filter(|path| !path.is_empty())?;
    Some(
        PathBuf::from(local)
            .join("ReScaleFrame")
            .join("Diagnostics.ini"),
    )
}

pub(crate) fn value(variable: &str) -> Option<OsString> {
    std::env::var_os(variable).or_else(|| {
        let path: Vec<u16> = profile()?.as_os_str().encode_wide().chain([0]).collect();
        let section: Vec<u16> = SECTION.encode_utf16().chain([0]).collect();
        let key: Vec<u16> = variable.encode_utf16().chain([0]).collect();
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
