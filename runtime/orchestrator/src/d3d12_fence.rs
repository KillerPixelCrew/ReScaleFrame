// SPDX-License-Identifier: MIT
use std::ffi::c_void;
use std::time::Instant;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D12::ID3D12Fence;
use windows::core::Interface;
use windows_sys::Win32::{
    Foundation::{E_FAIL, GetLastError, S_OK, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{INFINITE, WaitForSingleObject},
};

const DEVICE_REMOVED: i32 = 0x887a_0005_u32 as i32;
const fn win32_hresult(code: u32) -> i32 {
    if code as i32 <= 0 {
        code as i32
    } else {
        ((code & 0xffff) | 0x8007_0000) as i32
    }
}

const TIMED_OUT: i32 = win32_hresult(WAIT_TIMEOUT);
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

fn wait_for_value(
    value: u64,
    timeout_ms: u32,
    mut completed: impl FnMut() -> u64,
    arm: impl FnOnce() -> i32,
    mut wait: impl FnMut(u32) -> i32,
) -> i32 {
    if let Some(result) = completion_result(completed(), value) {
        return result;
    }
    let armed = arm();
    if armed < 0 {
        return armed;
    }
    let start = Instant::now();
    let mut remaining = timeout_ms;
    loop {
        let waited = wait(remaining);
        if waited < 0 && waited != TIMED_OUT {
            return waited;
        }
        if let Some(result) = completion_result(completed(), value) {
            return result;
        }
        if waited == TIMED_OUT {
            return TIMED_OUT;
        }
        if timeout_ms != INFINITE {
            let elapsed = start.elapsed().as_millis();
            if elapsed >= u128::from(timeout_ms) {
                return TIMED_OUT;
            }
            remaining = timeout_ms - elapsed as u32;
        }
        // A shared auto-reset event can still be signaled by an earlier wait.
        // The requested fence remains armed, so keep waiting within the same budget.
    }
}

/// Waits until `fence` (a live ID3D12Fence) reaches `value`, using `event` for the wait.
pub(crate) unsafe fn wait_fence(fence: Handle, value: u64, event: Handle, timeout_ms: u32) -> i32 {
    if value == 0 {
        return S_OK;
    }
    let Some(fence) = (unsafe { ID3D12Fence::from_raw_borrowed(&fence) }) else {
        return E_FAIL;
    };
    if event.is_null() {
        return E_FAIL;
    }
    wait_for_value(
        value,
        timeout_ms,
        || unsafe { fence.GetCompletedValue() },
        || {
            unsafe { fence.SetEventOnCompletion(value, HANDLE(event)) }
                .map_or_else(|error| error.code().0, |()| S_OK)
        },
        |remaining| unsafe { wait_event(event, remaining) },
    )
}
// One wait on a fence event, as the HRESULT `wait_for_value` expects from its wait step. `event`
// is a live event handle for the duration of the call.
unsafe fn wait_event(event: Handle, remaining: u32) -> i32 {
    match unsafe { WaitForSingleObject(event, remaining) } {
        WAIT_OBJECT_0 => S_OK,
        WAIT_TIMEOUT => TIMED_OUT,
        WAIT_FAILED => {
            let error = unsafe { GetLastError() };
            if error == 0 {
                E_FAIL
            } else {
                win32_hresult(error)
            }
        }
        _ => E_FAIL,
    }
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
                S_OK
            },
        );
        assert_eq!(result, S_OK);
        assert_eq!(waits, 2);
    }

    #[test]
    fn removal_and_timeout_are_distinct_from_completion() {
        let mut values = [0, u64::MAX].into_iter();
        assert_eq!(
            wait_for_value(7, 1, || values.next().unwrap(), || S_OK, |_| S_OK),
            DEVICE_REMOVED
        );
        assert_eq!(wait_for_value(7, 0, || 3, || S_OK, |_| S_OK), TIMED_OUT);
        let invalid_handle = win32_hresult(6);
        assert_eq!(
            wait_for_value(7, 1, || 3, || S_OK, |_| invalid_handle),
            invalid_handle
        );
        assert_eq!(
            wait_for_value(7, 1, || 3, || invalid_handle, |_| unreachable!()),
            invalid_handle
        );
    }
}
