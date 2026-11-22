// SPDX-License-Identifier: MIT
#[path = "abi.rs"]
#[allow(dead_code)]
mod abi;
#[cfg(feature = "d3d11")]
mod d3d11;
#[cfg(feature = "d3d12")]
mod d3d12;
#[cfg(feature = "metadata")]
mod metadata;
#[cfg(feature = "texture")]
mod texture;
#[cfg(feature = "vulkan")]
mod vulkan;
#[cfg(feature = "vulkan-cpu")]
mod vulkan_cpu;

use std::ffi::{CStr, CString, c_char};
#[cfg(any(
    feature = "metadata",
    feature = "d3d12",
    feature = "d3d11",
    feature = "vulkan",
    feature = "vulkan-cpu"
))]
use std::fs;
use std::path::Path;
#[cfg(any(
    feature = "metadata",
    feature = "d3d12",
    feature = "d3d11",
    feature = "vulkan",
    feature = "vulkan-cpu"
))]
use std::path::PathBuf;
#[cfg(any(
    feature = "metadata",
    feature = "d3d12",
    feature = "d3d11",
    feature = "vulkan",
    feature = "vulkan-cpu"
))]
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
#[cfg(any(
    feature = "metadata",
    feature = "d3d12",
    feature = "d3d11",
    feature = "vulkan",
    feature = "vulkan-cpu"
))]
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

fn entry(path: *const c_char, fixture: impl FnOnce(&Path) -> i32 + std::panic::UnwindSafe) -> i32 {
    if path.is_null() {
        return 2;
    }
    let Ok(path) = (unsafe { CStr::from_ptr(path) }).to_str() else {
        return 2;
    };
    std::panic::catch_unwind(|| fixture(Path::new(path))).unwrap_or(1)
}

#[cfg(any(
    feature = "metadata",
    feature = "d3d12",
    feature = "d3d11",
    feature = "vulkan",
    feature = "vulkan-cpu"
))]
fn root(path: &Path, label: &str) -> PathBuf {
    let (process, tick) = unsafe { (GetCurrentProcessId(), GetTickCount64()) };
    path.join(format!("{label}-{process}-{tick}"))
}

fn utf8(path: &Path) -> CString {
    CString::new(path.to_str().expect("fixture directory is UTF-8")).unwrap()
}

#[cfg(any(
    feature = "metadata",
    feature = "d3d12",
    feature = "d3d11",
    feature = "vulkan",
    feature = "vulkan-cpu"
))]
fn bundles(root: &Path) -> impl Iterator<Item = PathBuf> {
    fs::read_dir(root.join("website"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
}

#[cfg(any(
    feature = "metadata",
    feature = "d3d12",
    feature = "d3d11",
    feature = "vulkan",
    feature = "vulkan-cpu"
))]
fn contents(bundle: &Path, name: &str) -> String {
    fs::read_to_string(bundle.join(name)).unwrap()
}
