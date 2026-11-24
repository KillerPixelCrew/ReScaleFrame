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

/// The last component of `path` (split at `\` or `/`) is `name`, compared ASCII case-insensitively.
pub fn leaf_is(path: &[u16], name: &std::ffi::CStr) -> bool {
    let leaf = path.rsplit(|&unit| matches!(unit, 0x5c | 0x2f)).next();
    leaf.is_some_and(|leaf| {
        leaf.len() == name.count_bytes()
            && leaf.iter().zip(name.to_bytes()).all(|(&unit, byte)| {
                u8::try_from(unit).is_ok_and(|unit| unit.eq_ignore_ascii_case(byte))
            })
    })
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
    fn leaf_matches_the_final_component_ignoring_ascii_case() {
        let name = c"Game.exe";
        assert!(leaf_is(&wide(r"C:\Games\GAME.EXE"), name));
        assert!(leaf_is(&wide("Game.exe"), name));
        assert!(!leaf_is(&wide(r"C:\Game.exe\other"), name));
        assert!(!leaf_is(&wide(r"C:\xGame.exe"), name));
        assert!(!leaf_is(&[0x0131; 8], name));
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

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
