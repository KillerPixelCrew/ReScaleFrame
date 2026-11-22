// SPDX-License-Identifier: MIT
//! Root admission shared by Windows plugin, SDK and diagnostics paths.

/// Accepts a rooted ASCII drive or a backslash UNC root with server and share
/// components. Also accepts Win32 namespace paths with the same two components.
pub fn absolute_path(mut units: impl Iterator<Item = u16>) -> bool {
    let [colon, backslash, slash] = [b':', b'\\', b'/'].map(u16::from);
    let (Some(first), Some(second), Some(third)) = (units.next(), units.next(), units.next())
    else {
        return false;
    };
    if matches!(first, 65..=90 | 97..=122) && second == colon {
        return third == backslash || third == slash;
    }
    if first != backslash || second != backslash || [0, backslash, slash].contains(&third) {
        return false;
    }
    let separator = units.find(|&unit| [0, backslash, slash].contains(&unit));
    matches!(separator, Some(unit) if unit != 0)
        && units
            .next()
            .is_some_and(|share| ![0, backslash, slash].contains(&share))
}

/// Checks the root of a borrowed, NUL-terminated Windows path within a unit limit.
///
/// # Safety
/// A non-null pointer must be readable through its first terminating NUL.
pub unsafe fn absolute_wide_path(path: *const u16, limit: usize) -> bool {
    if path.is_null() {
        return false;
    }
    absolute_path(
        (0..limit)
            .map(|index| unsafe { path.add(index).read() })
            .take_while(|&unit| unit != 0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_a_rooted_drive_or_unc() {
        for path in [
            r"C:\plugin.dll",
            "d:/vendor",
            r"\\server\share",
            r"\\?\C:\vendor",
        ] {
            assert!(absolute_path(path.encode_utf16()), "{path}");
        }
        for path in [
            "",
            "1:/vendor",
            r"C:plugin.dll",
            r"\vendor",
            r"\\server",
            r"\\server\",
        ] {
            assert!(!absolute_path(path.encode_utf16()), "{path}");
        }
    }
}
