// SPDX-License-Identifier: MIT

/// A failure may release the consumer only after an actual producer signal.
pub(super) fn can_signal_done(ready: u64, done: u64, last_done: u64) -> bool {
    ready != 0 && done > ready && done > last_done
}

#[cfg(test)]
mod tests {
    use super::can_signal_done;

    #[test]
    fn failed_handoff_requires_producer_readiness_and_a_new_done_value() {
        assert!(!can_signal_done(0, 2, 0));
        assert!(!can_signal_done(2, 2, 0));
        assert!(!can_signal_done(2, 1, 0));
        assert!(!can_signal_done(2, 3, 3));
        assert!(can_signal_done(2, 4, 3));
    }
}
