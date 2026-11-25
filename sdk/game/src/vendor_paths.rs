// SPDX-License-Identifier: MIT
use std::ffi::{CStr, CString};

/// The vendor runtime folders of one ReScaleFrame directory, as every host and adapter hands
/// them to the Runtime: `streamline`, `fidelityfx` (FSR2, FSR3 and FSR4 share it) and `xess`
/// under the root, the layout `eng/common.ps1` `Get-RsfVendorPayload` deploys, and the Vulkan
/// builds `eng/build-godot.ps1` adds beside them (`fidelityfx-vulkan`, `streamline-vulkan`).
#[derive(Clone, Default)]
pub struct VendorPaths {
    pub streamline: CString,
    pub interposer: CString,
    pub fidelityfx: CString,
    pub xess: CString,
    pub fidelityfx_vulkan: CString,
    pub streamline_vulkan: CString,
}

impl VendorPaths {
    pub fn new(root: &CStr) -> Self {
        let root = plain_directory(root.to_bytes());
        let join = |suffix: &[u8]| {
            let mut bytes = root.clone();
            bytes.extend_from_slice(suffix);
            CString::new(bytes).expect("SDK path suffixes contain no NUL")
        };
        Self {
            fidelityfx_vulkan: join(br"\fidelityfx-vulkan"),
            streamline_vulkan: join(br"\streamline-vulkan"),
            ..Self::from_folders(
                join(br"\streamline"),
                join(br"\fidelityfx"),
                join(br"\xess"),
            )
        }
    }

    /// The D3D folders an adapter already resolved (a host whose setup names each one); the
    /// interposer is the one beside Streamline's plugins.
    pub fn from_folders(streamline: CString, fidelityfx: CString, xess: CString) -> Self {
        let interposer = [streamline.as_bytes(), br"\sl.interposer.dll"].concat();
        Self {
            interposer: CString::new(interposer).expect("a CString holds no NUL"),
            streamline,
            fidelityfx,
            xess,
            ..Self::default()
        }
    }
}

/// A directory as Streamline, FidelityFX and XeSS can use it. A Win32 long-path name
/// ("\\?\D:\...", "\\?\UNC\server\share\...") disables the slash conversion Streamline relies on
/// when it appends "/sl.common.dll" to its plugin directory, so every plugin failed to load
/// (Anomaly wave 7 run, c1 log). Strips the prefix, uses backslashes only and drops trailing
/// separators, which `new` adds back with each folder.
fn plain_directory(path: &[u8]) -> Vec<u8> {
    let mut out = if let Some(rest) = path.strip_prefix(br"\\?\UNC\") {
        [br"\\".as_slice(), rest].concat()
    } else if let Some(rest) = path
        .strip_prefix(br"\\?\")
        .or_else(|| path.strip_prefix(br"\??\"))
    {
        rest.to_vec()
    } else {
        path.to_vec()
    };
    for byte in &mut out {
        if *byte == b'/' {
            *byte = b'\\';
        }
    }
    while out.last() == Some(&b'\\') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xess(root: &CStr) -> CString {
        VendorPaths::new(root).xess
    }

    #[test]
    fn every_folder_sits_under_the_plain_root() {
        let paths = VendorPaths::new(c"D:/Games/bin/ReScaleFrame/");
        assert_eq!(
            paths.streamline.as_c_str(),
            c"D:\\Games\\bin\\ReScaleFrame\\streamline"
        );
        assert_eq!(
            paths.interposer.as_c_str(),
            c"D:\\Games\\bin\\ReScaleFrame\\streamline\\sl.interposer.dll"
        );
        assert_eq!(
            paths.fidelityfx.as_c_str(),
            c"D:\\Games\\bin\\ReScaleFrame\\fidelityfx"
        );
        assert_eq!(xess(c"R").as_c_str(), c"R\\xess");
        assert_eq!(
            paths.fidelityfx_vulkan.as_c_str(),
            c"D:\\Games\\bin\\ReScaleFrame\\fidelityfx-vulkan"
        );
        assert_eq!(
            paths.streamline_vulkan.as_c_str(),
            c"D:\\Games\\bin\\ReScaleFrame\\streamline-vulkan"
        );
        let resolved = VendorPaths::from_folders(c"S".into(), c"F".into(), c"X".into());
        assert_eq!(resolved.interposer.as_c_str(), c"S\\sl.interposer.dll");
    }

    #[test]
    fn long_path_prefixes_are_removed() {
        assert_eq!(xess(c"\\\\?\\D:\\RSF\\").as_c_str(), c"D:\\RSF\\xess");
        assert_eq!(
            xess(c"\\\\?\\UNC\\server\\share/rsf").as_c_str(),
            c"\\\\server\\share\\rsf\\xess"
        );
        assert_eq!(xess(c"\\??\\C:\\rsf").as_c_str(), c"C:\\rsf\\xess");
        assert_eq!(xess(c"C:\\").as_c_str(), c"C:\\xess");
    }
}
