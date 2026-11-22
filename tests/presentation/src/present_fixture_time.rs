// SPDX-License-Identifier: MIT
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

pub(super) fn stamp() -> u64 {
    let mut count = 0;
    let _ = unsafe { QueryPerformanceCounter(&mut count) };
    count as u64
}

pub(super) fn frequency() -> u64 {
    let mut frequency = 0;
    let _ = unsafe { QueryPerformanceFrequency(&mut frequency) };
    frequency as u64
}

pub(super) fn milliseconds(since: u64, frequency: u64) -> f64 {
    stamp().saturating_sub(since) as f64 * 1000.0 / frequency as f64
}
