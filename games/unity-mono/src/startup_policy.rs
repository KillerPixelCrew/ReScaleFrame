// SPDX-License-Identifier: MIT
use crate::runtime_config::{runtime_root, terminated, utf8};
use std::time::Duration;

pub(crate) const NO_MONO: u32 = 1;
const SR_NOT_READY: u32 = 10;
pub(crate) const MONO_DEADLINE: Duration = Duration::from_secs(90);
const MONO_POLL: Duration = Duration::from_millis(100);
const MONO_SETTLE: Duration = Duration::from_secs(2);
const SR_RETRY: Duration = Duration::from_millis(500);

pub(crate) struct FgPaths {
    pub(crate) runtime: Vec<u8>,
    pub(crate) fsr3: Vec<u8>,
    pub(crate) fsr4: Vec<u8>,
    pub(crate) xess: Vec<u8>,
    pub(crate) streamline: Vec<u8>,
}

impl FgPaths {
    pub(crate) fn read(ini: &[u16], mut path_setting: impl FnMut(&str) -> Vec<u16>) -> Self {
        let mut read = |key| terminated(utf8(&path_setting(key)));
        Self {
            runtime: terminated(utf8(&runtime_root(ini))),
            fsr3: read("FSR3"),
            fsr4: read("FSR4"),
            xess: read("XeSS"),
            streamline: read("Streamline"),
        }
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
    fn auto_has_a_runtime_root_with_streamline_as_the_only_provider_path() {
        let ini: Vec<u16> = r"C:\Game\ReScaleFrame.Unity.ini".encode_utf16().collect();
        let paths = FgPaths::read(&ini, |key| match key {
            "Streamline" => r"C:\Vendor\Streamline".encode_utf16().collect(),
            _ => Vec::new(),
        });
        assert_eq!(paths.runtime, b"C:\\Game\\ReScaleFrame\0");
        assert_eq!(paths.streamline, b"C:\\Vendor\\Streamline\0");
        assert_eq!(paths.fsr3, [0]);
        assert_eq!(paths.fsr4, [0]);
        assert_eq!(paths.xess, [0]);
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
