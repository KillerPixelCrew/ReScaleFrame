// SPDX-License-Identifier: MIT
#![cfg(windows)]

mod anomaly;
#[cfg(feature = "cpu-fixtures")]
mod antilag;
#[cfg(feature = "cpu-fixtures")]
mod antilag_abi;
mod contract;
#[cfg(any(
    feature = "cpu-fixtures",
    feature = "fg-cpu-fixture",
    feature = "vk-latency-fixtures"
))]
#[allow(dead_code)]
#[path = "../../../runtime/orchestrator/src/cpu_abi.rs"]
mod cpu_abi;
#[cfg(any(
    feature = "cpu-fixtures",
    feature = "fg-cpu-fixture",
    feature = "vk-latency-fixtures"
))]
#[allow(dead_code)]
mod cpu_calls;
#[cfg(feature = "cpu-fixtures")]
mod cpu_events;
#[cfg(feature = "d3d11-host-fixture")]
mod d3d11_host;
#[cfg(feature = "d3d11-host-fixture")]
mod d3d11_host_calls;
mod domains;
#[cfg(feature = "fg-cpu-fixture")]
mod fg_cpu;
#[cfg(feature = "fg-cpu-fixture")]
mod fg_cpu_calls;
#[cfg(feature = "cpu-fixtures")]
mod input_receipts;
#[cfg(feature = "fg-cpu-fixture")]
#[allow(dead_code)]
#[path = "../../../runtime/orchestrator/src/native_fg_cpu_abi.rs"]
mod native_fg_cpu_abi;
#[cfg(feature = "plugin-session-fixture")]
mod plugin_session_fixture;
#[cfg(feature = "fg-cpu-fixture")]
#[allow(dead_code, unused_imports)]
#[path = "../../../runtime/orchestrator/src/render_abi.rs"]
mod render_abi;
#[cfg(feature = "cpu-fixtures")]
mod reservation;
mod session;
#[cfg(feature = "vk-latency-fixtures")]
mod vk_latency;
#[cfg(feature = "vk-latency-fixtures")]
#[allow(dead_code)]
mod vk_latency_calls;
#[cfg(feature = "xell-fixture")]
mod xell;

use std::ffi::{CStr, OsStr};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{
    LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LoadLibraryExW,
};

const OK: i32 = 0;
const INVALID_ARGUMENT: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const NOT_READY: i32 = -3;
const BUSY: i32 = -4;
const NATIVE_REFUSED: i32 = -5;

fn wide(path: &OsStr) -> Vec<u16> {
    path.encode_wide().chain([0]).collect()
}

struct Module(HMODULE);

impl Module {
    fn beside_executable(leaf: &Path) -> Self {
        let mut path = std::env::current_exe().expect("fixture executable path");
        path.pop();
        path.push(leaf);
        let handle = unsafe {
            LoadLibraryExW(
                wide(path.as_os_str()).as_ptr(),
                null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            )
        };
        assert!(
            !handle.is_null(),
            "Loading {} failed: {}",
            path.display(),
            std::io::Error::last_os_error()
        );
        Self(handle)
    }

    /// F must be the export's exact ABI and signature.
    unsafe fn export<F: Copy>(&self, name: &CStr) -> Option<F> {
        unsafe { rsf_game_sys::win32::typed_export(self.0, name) }
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        unsafe { FreeLibrary(self.0) };
    }
}

fn run(check: impl FnOnce() -> i32 + std::panic::UnwindSafe) -> i32 {
    std::panic::catch_unwind(check).unwrap_or(1)
}
