// SPDX-License-Identifier: MIT
use crate::runtime_config::{runtime_root, terminated, utf8};
use rsf_game_sys::vendor_paths::VendorPaths;
use std::ffi::CStr;
use std::time::Duration;

pub(crate) const NO_MONO: u32 = 1;
const SR_NOT_READY: u32 = 10;
pub(crate) const MONO_DEADLINE: Duration = Duration::from_secs(90);
const MONO_POLL: Duration = Duration::from_millis(100);
const MONO_SETTLE: Duration = Duration::from_secs(2);
const SR_RETRY: Duration = Duration::from_millis(500);

/// The ReScaleFrame directory beside the INI and its vendor runtime folders.
pub(crate) struct Paths {
    pub(crate) runtime: Vec<u8>,
    pub(crate) vendors: VendorPaths,
}

impl Paths {
    pub(crate) fn read(ini: &[u16]) -> Self {
        let runtime = terminated(utf8(&runtime_root(ini)));
        let vendors = VendorPaths::new(CStr::from_bytes_with_nul(&runtime).unwrap_or_default());
        Self { runtime, vendors }
    }
}

pub(crate) fn wait_for_sr<T: Copy + Ord>(
    deadline: T,
    now: impl Fn() -> T,
    pause: impl Fn(Duration),
    available: impl Fn() -> bool,
    mut attempt: impl FnMut() -> u32,
) -> u32 {
    while now() < deadline && !available() {
        pause(MONO_POLL);
    }
    if !available() {
        return NO_MONO;
    }
    pause(MONO_SETTLE);
    loop {
        let result = attempt();
        if result != SR_NOT_READY || now() >= deadline {
            return result;
        }
        pause(SR_RETRY);
        if now() >= deadline {
            return result;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_folders_sit_in_the_runtime_root_beside_the_ini() {
        let ini: Vec<u16> = r"C:\Game\ReScaleFrame.Unity.ini".encode_utf16().collect();
        let paths = Paths::read(&ini);
        assert_eq!(paths.runtime, b"C:\\Game\\ReScaleFrame\0");
        let vendors = paths.vendors;
        assert_eq!(
            vendors.streamline.as_c_str(),
            c"C:\\Game\\ReScaleFrame\\streamline"
        );
        assert_eq!(
            vendors.fidelityfx.as_c_str(),
            c"C:\\Game\\ReScaleFrame\\fidelityfx"
        );
        assert_eq!(vendors.xess.as_c_str(), c"C:\\Game\\ReScaleFrame\\xess");
    }

    #[test]
    fn mono_arriving_before_deadline_gets_one_attempt_after_settling() {
        use std::cell::Cell;

        let elapsed = Cell::new(Duration::ZERO);
        let attempts = Cell::new(0);
        let result = wait_for_sr(
            MONO_DEADLINE,
            || elapsed.get(),
            |duration| elapsed.set(elapsed.get() + duration),
            || elapsed.get() >= MONO_DEADLINE - MONO_POLL,
            || {
                attempts.set(attempts.get() + 1);
                SR_NOT_READY
            },
        );
        assert_eq!(result, SR_NOT_READY);
        assert_eq!(attempts.get(), 1);
    }
}
