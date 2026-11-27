// SPDX-License-Identifier: MIT
pub(super) use rsf_test_support::{qpc as stamp, qpc_frequency as frequency};

pub(super) fn milliseconds(since: u64, frequency: u64) -> f64 {
    stamp().saturating_sub(since) as f64 * 1000.0 / frequency as f64
}
