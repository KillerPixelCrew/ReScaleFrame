// SPDX-License-Identifier: MIT
//! The Unity routes each borrow their own Runtime submission handle through callback retirement.

use crate::sys::RSF_UNITY_FRAMES_SLOTS;
use rsf_runtime_submission::SubmissionHandle;
use std::ffi::c_void;
use windows::core::Interface;

pub(crate) static D3D11: SubmissionHandle = SubmissionHandle::new(RSF_UNITY_FRAMES_SLOTS as usize);
pub(crate) static VULKAN: SubmissionHandle = SubmissionHandle::new(RSF_UNITY_FRAMES_SLOTS as usize);

/// A COM interface the bridge's published graphics record or surface lease keeps live.
///
/// # Safety
/// `raw` must hold a live COM pointer of type `T` for as long as the result is used.
pub(crate) unsafe fn borrowed<T: Interface>(raw: &*mut c_void) -> &T {
    unsafe { T::from_raw_borrowed(raw) }.expect("live Unity bridge COM interface")
}