// SPDX-License-Identifier: MIT
use std::ffi::c_char;

pub use crate::sys::rsf_ffx_version as Version;
pub const MAX_VERSIONS: usize = crate::sys::RSF_FFX_MAX_VERSIONS as usize;

pub fn select(versions: &[Version], major: u32, wanted: u64) -> Option<&Version> {
    let mut best: Option<&Version> = None;
    for version in versions
        .iter()
        .filter(|version| version.major == major && (wanted == 0 || version.id == wanted))
    {
        // IDs are opaque. Keep the first entry on a tie, even if its ID is zero.
        if best.is_none_or(|best| (version.minor, version.patch) > (best.minor, best.patch)) {
            best = Some(version);
        }
    }
    best
}

#[cfg(windows)]
pub fn admitted(versions: &[Version], major: u32, wanted: u64) -> Option<Version> {
    select(versions, major, wanted)
        .filter(|version| version.id != 0)
        .copied()
}

/// SDK names start with a three-component version and may append a label.
///
/// # Safety
/// `name` is null or a NUL-terminated string readable for the call.
#[cfg(windows)]
pub unsafe fn from_sdk(id: u64, name: *const c_char) -> Version {
    unsafe { parse_sdk(id, name) }.0
}

/// Like [`from_sdk`], also reporting whether the name held a version.
///
/// # Safety
/// As for [`from_sdk`].
#[cfg(windows)]
pub unsafe fn parse_sdk(id: u64, name: *const c_char) -> (Version, bool) {
    let mut version = Version {
        id,
        name,
        ..Default::default()
    };
    if !name.is_null() {
        let text = unsafe { std::ffi::CStr::from_ptr(name) }.to_bytes();
        if let Some([major, minor, patch]) = version_components(text) {
            (version.major, version.minor, version.patch) = (major, minor, patch);
            return (version, true);
        }
    }
    (version, false)
}

#[cfg(windows)]
fn version_components(mut text: &[u8]) -> Option<[u32; 3]> {
    // Preserve the native %u.%u.%u parsing, including a suffix after the patch.
    let mut parts = [0; 3];
    for (index, part) in parts.iter_mut().enumerate() {
        if index != 0 {
            text = text.strip_prefix(b".")?;
        }
        text = text.trim_ascii_start();
        let negative = text.starts_with(b"-");
        if negative || text.starts_with(b"+") {
            text = &text[1..];
        }
        let digits = text.iter().take_while(|byte| byte.is_ascii_digit()).count();
        *part = std::str::from_utf8(&text[..digits])
            .ok()?
            .parse::<u32>()
            .ok()?;
        if negative {
            *part = part.wrapping_neg();
        }
        text = &text[digits..];
    }
    Some(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn provider_names_accept_numeric_prefixes_but_reject_incomplete_versions() {
        assert_eq!(version_components(b"4.1.2.4555 INT8"), Some([4, 1, 2]));
        assert_eq!(version_components(b" 3. 1.+5"), Some([3, 1, 5]));
        assert_eq!(version_components(b"4.1"), None);
        assert_eq!(version_components(b"FidelityFX 4.1.2"), None);
    }

    #[test]
    fn selects_by_family_and_revision_then_applies_the_opaque_id_contract() {
        let version = |id, major, minor, patch| Version {
            id,
            major,
            minor,
            patch,
            ..Version::default()
        };
        let versions = [
            version(99, 4, 9, 0),
            version(30, 3, 2, 3),
            version(17, 3, 2, 4),
            version(90, 3, 2, 4),
            version(11, 3, 1, 9),
        ];
        assert_eq!(select(&versions, 3, 0).map(|v| v.id), Some(17));
        assert_eq!(select(&versions, 3, 30).map(|v| v.id), Some(30));
        assert!(select(&versions, 3, 99).is_none());
        assert_eq!(
            select(&[version(0, 3, 2, 4), versions[2]], 3, 0).map(|v| v.id),
            Some(0)
        );
    }
}
