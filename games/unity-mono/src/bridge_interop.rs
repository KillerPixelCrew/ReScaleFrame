// SPDX-License-Identifier: MIT
//! The Unity routes each borrow their own Runtime submission handle through callback retirement.

use crate::sys::RSF_UNITY_FRAMES_SLOTS;
use rsf_runtime_submission::SubmissionHandle;
use std::ffi::c_void;

#[cfg(any(test, all(windows, not(feature = "mono-plugin"))))]
pub(crate) static D3D11: SubmissionHandle = SubmissionHandle::new(RSF_UNITY_FRAMES_SLOTS as usize);
pub(crate) static VULKAN: SubmissionHandle = SubmissionHandle::new(RSF_UNITY_FRAMES_SLOTS as usize);

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_vulkan_submission_ring() -> *const c_void {
    std::ptr::from_ref(&VULKAN).cast()
}
