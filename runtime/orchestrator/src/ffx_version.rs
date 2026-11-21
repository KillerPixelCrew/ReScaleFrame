// SPDX-License-Identifier: MIT
use std::ffi::c_char;

pub(super) const MAX_VERSIONS: usize = 32;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct Version {
    pub id: u64,
    pub name: *const c_char,
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

const _: () = {
    assert!(size_of::<Version>() == 32);
    assert!(std::mem::offset_of!(Version, major) == 16);
};

pub(super) fn select(versions: &[Version], major: u32, wanted: u64) -> Option<&Version> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
