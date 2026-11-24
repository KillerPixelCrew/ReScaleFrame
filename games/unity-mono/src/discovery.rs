// SPDX-License-Identifier: MIT
//! Finds the Unity Mono player the adapter needs in the files beside the running executable:
//! UnityPlayer.dll, the MonoBleedingEdge runtime, the URP managed assemblies and the Unity version
//! the player's globalgamemanagers header records. No executable build is pinned. The URP types,
//! methods and fields the Harmony patches target are resolved by name in the managed helper
//! (`UrpAdapter.Contracts`), which refuses naming the missing one.
use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::Read;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

pub(crate) struct Player {
    /// As the player records it, e.g. `6000.3.14f1`. Evidence and the vendor engine string; it
    /// gates nothing.
    pub(crate) unity_version: CString,
}

/// The player around `executable`, or what is missing, naming the file.
pub(crate) fn discover(executable: &Path) -> Result<Player, String> {
    let (Some(directory), Some(stem)) = (executable.parent(), executable.file_stem()) else {
        return Err(format!("{} has no player directory", executable.display()));
    };
    let mut data = stem.to_owned();
    data.push("_Data");
    let data = directory.join(data);
    let managed = data.join("Managed");
    for path in [
        directory.join("UnityPlayer.dll"),
        directory.join(r"MonoBleedingEdge\EmbedRuntime\mono-2.0-bdwgc.dll"),
        managed.join("UnityEngine.CoreModule.dll"),
        managed.join("Unity.RenderPipelines.Core.Runtime.dll"),
        managed.join("Unity.RenderPipelines.Universal.Runtime.dll"),
    ] {
        if !path.is_file() {
            return Err(format!("{} is missing", path.display()));
        }
    }
    let settings = data.join("globalgamemanagers");
    let mut header = Vec::with_capacity(HEADER);
    File::open(&settings)
        .and_then(|file| file.take(HEADER as u64).read_to_end(&mut header))
        .map_err(|error| format!("{} is unreadable: {error}", settings.display()))?;
    let unity_version = unity_version(&header)
        .ok_or_else(|| format!("{} records no Unity version", settings.display()))?;
    Ok(Player { unity_version })
}

const HEADER: usize = 128;

/// The version string after a Unity serialized-file header: 20 bytes before format 22, 48 from
/// format 22 on. The format is the big-endian word at offset 8.
pub(crate) fn unity_version(header: &[u8]) -> Option<CString> {
    let format = u32::from_be_bytes(header.get(8..12)?.try_into().ok()?);
    let text = header.get(if format >= 22 { 48 } else { 20 }..)?;
    let version = CStr::from_bytes_until_nul(text).ok()?.to_bytes();
    let plausible = version.first().is_some_and(u8::is_ascii_digit)
        && version.contains(&b'.')
        && version
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || b == b'.');
    plausible.then(|| CString::new(version).expect("no interior NUL"))
}

/// Whether the probe's executable name is `executable`'s file name.
pub(crate) fn names(executable: &Path, name: &CStr) -> bool {
    executable.file_name().is_some_and(|file| {
        file.as_encoded_bytes()
            .eq_ignore_ascii_case(name.to_bytes())
    })
}

/// The running process's executable and its player, discovered once.
#[cfg(windows)]
pub(crate) fn running() -> &'static Result<(PathBuf, Player), String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::sync::OnceLock;
    static RUNNING: OnceLock<Result<(PathBuf, Player), String>> = OnceLock::new();
    RUNNING.get_or_init(|| {
        let executable = unsafe { rsf_game_sys::win32::module_path(std::ptr::null_mut()) }
            .ok_or("the executable path is unavailable")?;
        let executable = PathBuf::from(OsString::from_wide(executable.units()));
        discover(&executable).map(|player| (executable, player))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// The header of Drag'n Wash's globalgamemanagers (format 22), up to the version's NUL.
    fn format_22(version: &[u8]) -> Vec<u8> {
        let mut header = vec![0; 48];
        header[11] = 22;
        header.extend_from_slice(version);
        header.extend_from_slice(&[0, 0x13, 0, 0]);
        header
    }

    #[test]
    fn version_follows_the_serialized_header() {
        assert_eq!(
            unity_version(&format_22(b"6000.3.14f1")).as_deref(),
            Some(c"6000.3.14f1")
        );
        let mut older = vec![0; 20];
        older[11] = 21;
        older.extend_from_slice(b"2021.3.45f1\0");
        assert_eq!(unity_version(&older).as_deref(), Some(c"2021.3.45f1"));
        assert_eq!(unity_version(&format_22(b"not a version")), None);
        assert_eq!(unity_version(&format_22(b"")), None);
        assert_eq!(unity_version(&[0; 10]), None);
        let unterminated = &format_22(b"6000.3.14f1")[..55];
        assert_eq!(unity_version(unterminated), None);
    }

    #[test]
    fn a_player_needs_every_mono_and_urp_file() {
        let root = std::env::temp_dir().join(format!("rsf-unity-discovery-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let managed = root.join(r"Game_Data\Managed");
        let runtime = root.join(r"MonoBleedingEdge\EmbedRuntime");
        fs::create_dir_all(&managed).unwrap();
        fs::create_dir_all(&runtime).unwrap();
        let executable = root.join("Game.exe");
        let files = [
            root.join("UnityPlayer.dll"),
            runtime.join("mono-2.0-bdwgc.dll"),
            managed.join("UnityEngine.CoreModule.dll"),
            managed.join("Unity.RenderPipelines.Core.Runtime.dll"),
            managed.join("Unity.RenderPipelines.Universal.Runtime.dll"),
        ];
        for file in &files {
            fs::write(file, b"").unwrap();
        }
        let settings = root.join(r"Game_Data\globalgamemanagers");
        let refusal = discover(&executable).err().unwrap();
        assert!(refusal.contains("globalgamemanagers"), "{refusal}");
        fs::write(&settings, format_22(b"6000.3.14f1")).unwrap();
        let player = discover(&executable).unwrap();
        assert_eq!(player.unity_version.as_c_str(), c"6000.3.14f1");
        for file in &files {
            fs::remove_file(file).unwrap();
            let refusal = discover(&executable).err().unwrap();
            assert!(refusal.contains(&file.display().to_string()), "{refusal}");
            fs::write(file, b"").unwrap();
        }
        assert!(discover(&root.join("Other.exe")).is_err());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_probe_names_the_executable_in_any_case() {
        let executable = Path::new(r"D:\Games\Drag'n Wash\DragNWash.exe");
        assert!(names(executable, c"dragnwash.EXE"));
        assert!(!names(executable, c"OtherGame.exe"));
        assert!(!names(executable, c"DragNWash"));
    }
}
