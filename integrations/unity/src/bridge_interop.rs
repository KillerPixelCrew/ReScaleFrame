// SPDX-License-Identifier: MIT
//! The Unity routes each borrow their own Runtime submission handle through callback retirement.

use crate::sys::RSF_UNITY_FRAMES_SLOTS;
use rsf_runtime_submission::SubmissionHandle;

pub(crate) static D3D11: SubmissionHandle = SubmissionHandle::new(RSF_UNITY_FRAMES_SLOTS as usize);
pub(crate) static VULKAN: SubmissionHandle = SubmissionHandle::new(RSF_UNITY_FRAMES_SLOTS as usize);
