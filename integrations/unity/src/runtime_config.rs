// SPDX-License-Identifier: MIT
//! The [UnitySR] settings both Unity routes read: the game plugin's runtime setup and the Unity
//! package, which links this archive, reads its identity through package_state/presentation.rs
//! and its Log path through package_state/services.rs. A relative path therefore resolves the same
//! way in both.

const SLASH: u16 = b'/' as u16;
const BACKSLASH: u16 = b'\\' as u16;
const COLON: u16 = b':' as u16;
/// The path buffer limit, in characters.
pub const PATH_CAPACITY: usize = 32768;

fn separator(unit: &u16) -> bool {
    *unit == SLASH || *unit == BACKSLASH
}

/// A drive-qualified or rooted value is kept as written. Anything else is joined to the ini's
/// directory and canonicalised by `full`; without a directory in the ini path it is empty.
pub fn resolve(
    ini: &[u16],
    value: Vec<u16>,
    full: impl FnOnce(&[u16]) -> Option<Vec<u16>>,
) -> Vec<u16> {
    if value.is_empty() || value.first().is_some_and(separator) || value.get(1) == Some(&COLON) {
        return value;
    }
    let Some(end) = ini.iter().rposition(separator) else {
        return Vec::new();
    };
    let mut containing = ini[..=end].to_vec();
    containing.extend_from_slice(&value);
    full(&containing).unwrap_or_default()
}

/// Provider-independent root beside the startup INI.
pub fn runtime_root(ini: &[u16]) -> Vec<u16> {
    let end = ini.iter().rposition(separator).map_or(0, |index| index + 1);
    let mut root = ini[..end].to_vec();
    root.extend("ReScaleFrame".encode_utf16());
    root
}

/// `<ini directory>\ReScaleFrame\preferences.ini`, where the overlay saves next-start choices.
pub fn preferences_path(ini: &[u16]) -> Vec<u16> {
    let mut preferences = runtime_root(ini);
    preferences.extend("\\preferences.ini".encode_utf16());
    preferences
}

/// The text after the last path separator.
pub fn file_name(path: &[u16]) -> &[u16] {
    path.iter()
        .rposition(separator)
        .map_or(path, |end| &path[end + 1..])
}

/// UTF-16 to UTF-8; empty for invalid UTF-16, as WC_ERR_INVALID_CHARS made it.
pub fn utf8(value: &[u16]) -> Vec<u8> {
    String::from_utf16(value)
        .map(String::into_bytes)
        .unwrap_or_default()
}

/// `value` with a terminator, for a `const char*` or `const wchar_t*` field. An empty value is
/// an empty string, never a null pointer.
pub fn terminated<T: Default>(mut value: Vec<T>) -> Vec<T> {
    value.push(T::default());
    value
}

#[cfg(windows)]
pub use platform::*;

#[cfg(windows)]
mod platform {
    use super::{PATH_CAPACITY, resolve};
    use std::ptr::null_mut;
    use std::slice;
    use windows_sys::Win32::Storage::FileSystem::GetFullPathNameW;
    use windows_sys::Win32::System::WindowsProgramming::{
        GetPrivateProfileIntW, GetPrivateProfileStringW,
    };
    use windows_sys::core::{PCWSTR, w};

    const SECTION: PCWSTR = w!("UnitySR");

    /// The configuration path handed to an entry point, with its terminator.
    pub struct Ini(Vec<u16>);

    impl Ini {
        /// # Safety
        /// `path` is null or a NUL-terminated UTF-16 string.
        pub unsafe fn new(path: *const u16) -> Option<Self> {
            if path.is_null() {
                return None;
            }
            let length = (0..).take_while(|&i| unsafe { *path.add(i) } != 0).count();
            let mut text = unsafe { slice::from_raw_parts(path, length) }.to_vec();
            text.push(0);
            Some(Self(text))
        }

        pub fn chars(&self) -> &[u16] {
            &self.0[..self.0.len() - 1]
        }

        pub fn as_ptr(&self) -> PCWSTR {
            self.0.as_ptr()
        }
    }

    /// GetPrivateProfileIntW with its INT result taken as UINT.
    pub fn profile_int(section: PCWSTR, name: PCWSTR, default: u32, file: PCWSTR) -> u32 {
        (unsafe { GetPrivateProfileIntW(section, name, default as i32, file) }) as u32
    }

    /// [UnitySR] `name` as an unsigned integer.
    pub fn setting(ini: &Ini, name: PCWSTR, default: u32) -> u32 {
        profile_int(SECTION, name, default, ini.as_ptr())
    }

    fn full_path(path: &[u16]) -> Option<Vec<u16>> {
        let mut input = path.to_vec();
        input.push(0);
        let mut absolute = vec![0u16; PATH_CAPACITY];
        let count = unsafe {
            GetFullPathNameW(
                input.as_ptr(),
                PATH_CAPACITY as u32,
                absolute.as_mut_ptr(),
                null_mut(),
            )
        } as usize;
        (count != 0 && count < PATH_CAPACITY).then(|| {
            absolute.truncate(count);
            absolute
        })
    }

    /// [UnitySR] `name` from `ini` as written, cut as GetPrivateProfileStringW cuts it for a
    /// buffer of `capacity` characters. Empty when unset.
    pub fn string_setting(ini: &Ini, name: PCWSTR, capacity: usize) -> Vec<u16> {
        let read_capacity = rsf_game_sys::profile_text::read_capacity(capacity);
        let mut text = vec![0u16; read_capacity];
        let count = unsafe {
            GetPrivateProfileStringW(
                SECTION,
                name,
                w!(""),
                text.as_mut_ptr(),
                read_capacity as u32,
                ini.as_ptr(),
            )
        } as usize;
        text.truncate(count.min(read_capacity));
        if let Some(end) = text.iter().position(|&unit| unit == 0) {
            text.truncate(end);
        }
        let mut text = rsf_game_sys::profile_text::decode(text);
        text.truncate(capacity.saturating_sub(1));
        text
    }

    /// [UnitySR] `name` from `ini`, resolved by [`resolve`]. Empty when unset.
    pub fn path_setting(ini: &Ini, name: PCWSTR) -> Vec<u16> {
        resolve(
            ini.chars(),
            string_setting(ini, name, PATH_CAPACITY),
            full_path,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn rooted_and_drive_values_are_kept() {
        let ini = wide("C:\\Game\\ReScaleFrame.Unity.ini");
        let never = |_: &[u16]| -> Option<Vec<u16>> { panic!("no canonicalisation") };
        assert_eq!(resolve(&ini, wide("D:\\sdk"), never), wide("D:\\sdk"));
        assert_eq!(
            resolve(&ini, wide("\\\\server\\sdk"), never),
            wide("\\\\server\\sdk")
        );
        for rooted in ["\\", "/", "/sdk", r"\sdk"] {
            assert_eq!(resolve(&ini, wide(rooted), never), wide(rooted));
        }
        assert!(resolve(&ini, Vec::new(), never).is_empty());
    }

    #[test]
    fn relative_values_join_the_ini_directory() {
        let ini = wide("C:\\Game/ReScaleFrame.Unity.ini");
        let joined = resolve(&ini, wide("sdk\\xess"), |path| Some(path.to_vec()));
        assert_eq!(joined, wide("C:\\Game/sdk\\xess"));
        assert!(
            resolve(
                &wide("ReScaleFrame.Unity.ini"),
                wide("sdk"),
                |_| unreachable!()
            )
            .is_empty()
        );
        assert!(resolve(&ini, wide("sdk"), |_| None).is_empty());
    }

    #[test]
    fn preferences_sit_beside_the_ini() {
        assert_eq!(
            preferences_path(&wide("C:\\Game\\ReScaleFrame.Unity.ini")),
            wide("C:\\Game\\ReScaleFrame\\preferences.ini")
        );
        assert_eq!(
            preferences_path(&wide("ReScaleFrame.Unity.ini")),
            wide("ReScaleFrame\\preferences.ini")
        );
    }

    #[test]
    fn names_and_text_convert() {
        assert_eq!(
            file_name(&wide("C:\\Game/DragNWash.exe")),
            wide("DragNWash.exe")
        );
        assert_eq!(file_name(&wide("DragNWash.exe")), wide("DragNWash.exe"));
        assert_eq!(
            utf8(&wide("C:\\Spiele\\Größe")),
            "C:\\Spiele\\Größe".as_bytes()
        );
        assert!(utf8(&[0xd800]).is_empty());
        assert_eq!(terminated(Vec::<u8>::new()), vec![0]);
    }
}
