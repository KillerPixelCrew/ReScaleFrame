// SPDX-License-Identifier: MIT
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering::Relaxed};

pub(crate) struct Diagnostics {
    present_error: AtomicI32,
    worker_timeout_epoch: AtomicU64,
    verbose: AtomicBool,
}

impl Diagnostics {
    pub(crate) const fn new() -> Self {
        Self {
            present_error: AtomicI32::new(0),
            worker_timeout_epoch: AtomicU64::new(0),
            verbose: AtomicBool::new(false),
        }
    }

    pub(crate) fn present_error(&self, result: i32) {
        self.present_error.store(result, Relaxed);
    }

    pub(crate) fn take_present_error(&self) -> i32 {
        self.present_error.swap(0, Relaxed)
    }

    pub(crate) fn worker_timeout_epoch(&self) -> u64 {
        self.worker_timeout_epoch.load(Relaxed)
    }

    pub(crate) fn set_verbose(&self, verbose: bool) {
        self.verbose.store(verbose, Relaxed);
    }

    pub(crate) fn message(&self, message: &[u8], info: bool) -> bool {
        let contains = |text: &[u8]| message.windows(text.len()).any(|part| part == text);
        if contains(b"Worker thread '") && contains(b"' timed out") {
            self.worker_timeout_epoch.fetch_add(1, Relaxed);
        }
        !info || self.verbose.load(Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_timeouts_count_even_when_info_logging_is_hidden() {
        let diagnostics = Diagnostics::new();
        assert!(!diagnostics.message(b"Worker thread 'x' timed out", true));
        assert_eq!(diagnostics.worker_timeout_epoch(), 1);
        assert!(diagnostics.message(b"Unrelated warning", false));
        diagnostics.set_verbose(true);
        assert!(diagnostics.message(b"Worker thread 'x' started", true));
        assert_eq!(diagnostics.worker_timeout_epoch(), 1);
        diagnostics.present_error(-4);
        assert_eq!(diagnostics.take_present_error(), -4);
        assert_eq!(diagnostics.take_present_error(), 0);
    }
}
