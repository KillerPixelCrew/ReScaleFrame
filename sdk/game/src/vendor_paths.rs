// SPDX-License-Identifier: MIT
use std::ffi::{CStr, CString};

/// Owned C strings for the SDK directories shared by packaged D3D12 hosts.
#[derive(Clone, Default)]
pub struct VendorPaths {
    pub streamline: CString,
    pub interposer: CString,
    pub fidelityfx: CString,
    pub xess: CString,
}

impl VendorPaths {
    pub fn new(root: &CStr) -> Self {
        let join = |suffix: &[u8]| {
            let mut bytes = root.to_bytes().to_vec();
            bytes.extend_from_slice(suffix);
            CString::new(bytes).expect("SDK path suffixes contain no NUL")
        };
        Self {
            streamline: join(b"/streamline"),
            interposer: join(b"/streamline/sl.interposer.dll"),
            fidelityfx: join(b"/fidelityfx"),
            xess: join(b"/xess"),
        }
    }
}
