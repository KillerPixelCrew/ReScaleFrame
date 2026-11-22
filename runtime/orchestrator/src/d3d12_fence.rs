// SPDX-License-Identifier: MIT
use std::ffi::c_void;
use std::time::Instant;
use windows_sys::Win32::{
    Foundation::{E_FAIL, S_OK, WAIT_OBJECT_0},
    System::Threading::{INFINITE, WaitForSingleObject},
};

const DEVICE_REMOVED: i32 = 0x887a_0005_u32 as i32;
const DEVICE_HUNG: i32 = 0x887a_0006_u32 as i32;
type Handle = *mut c_void;

#[derive(PartialEq, Eq)]
pub(super) enum FenceState {
    Pending,
    Complete,
    DeviceRemoved,
}

impl FenceState {
    pub(super) fn check(done: u64, value: u64) -> Self {
        if done == u64::MAX {
            Self::DeviceRemoved
        } else if done >= value {
            Self::Complete
        } else {
            Self::Pending
        }
    }
}

fn completion_result(done: u64, value: u64) -> Option<i32> {
    match FenceState::check(done, value) {
        FenceState::Pending => None,
        FenceState::Complete => Some(S_OK),
        FenceState::DeviceRemoved => Some(DEVICE_REMOVED),
    }
}

pub(super) fn wait_for_value(
    value: u64,
    timeout_ms: u32,
    mut completed: impl FnMut() -> u64,
    arm: impl FnOnce() -> i32,
    mut wait: impl FnMut(u32) -> bool,
) -> i32 {
    if let Some(result) = completion_result(completed(), value) {
        return result;
    }
    if arm() < 0 {
        return E_FAIL;
    }
    let start = Instant::now();
    let mut remaining = timeout_ms;
    loop {
        let woke = wait(remaining);
        if let Some(result) = completion_result(completed(), value) {
            return result;
        }
        if !woke {
            return DEVICE_HUNG;
        }
        if timeout_ms != INFINITE {
            let elapsed = start.elapsed().as_millis();
            if elapsed >= u128::from(timeout_ms) {
                return DEVICE_HUNG;
            }
            remaining = timeout_ms - elapsed as u32;
        }
        // A shared auto-reset event can still be signaled by an earlier wait.
        // The requested fence remains armed, so keep waiting within the same budget.
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_d3d12_wait_fence(
    fence: Handle,
    value: u64,
    event: Handle,
    timeout_ms: u32,
    completed: unsafe extern "C" fn(Handle) -> u64,
    arm: unsafe extern "C" fn(Handle, u64, Handle) -> i32,
) -> i32 {
    if value == 0 {
        return S_OK;
    }
    if fence.is_null() || event.is_null() {
        return E_FAIL;
    }
    wait_for_value(
        value,
        timeout_ms,
        || unsafe { completed(fence) },
        || unsafe { arm(fence, value, event) },
        |remaining| unsafe { WaitForSingleObject(event, remaining) == WAIT_OBJECT_0 },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_wake_does_not_complete_the_requested_fence() {
        let mut values = [0, 3, 7].into_iter();
        let mut waits = 0;
        let result = wait_for_value(
            7,
            INFINITE,
            || values.next().unwrap(),
            || S_OK,
            |_| {
                waits += 1;
                true
            },
        );
        assert_eq!(result, S_OK);
        assert_eq!(waits, 2);
    }

    #[test]
    fn removal_and_timeout_are_distinct_from_completion() {
        let mut values = [0, u64::MAX].into_iter();
        assert_eq!(
            wait_for_value(7, 1, || values.next().unwrap(), || S_OK, |_| true),
            DEVICE_REMOVED
        );
        assert_eq!(wait_for_value(7, 0, || 3, || S_OK, |_| true), DEVICE_HUNG);
    }
}
