// SPDX-License-Identifier: MIT
//! The native geometry fixture feeds its guarded copies into the Rust oracle.
use super::*;
use abi::{rsf_anomaly_plan_capture as Result, rsf_anomaly_plan_pose as Input};
use std::ffi::c_void;

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_plan_history_create() -> *mut c_void {
    Box::into_raw(Box::new(History::new())).cast()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_plan_history_destroy(owner: *mut c_void) {
    drop(unsafe { Box::from_raw(owner.cast::<History>()) });
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_plan_history_begin(
    owner: *mut c_void,
    frame: &Frame,
    token: &mut Token,
) -> u32 {
    let history = unsafe { &mut *owner.cast::<History>() };
    let (status, result) = history.begin(frame);
    *token = result;
    status
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_plan_history_capture(
    owner: *mut c_void,
    token: &Token,
    key: &Key,
    pose: &Input,
    out: &mut Result,
) {
    *out = Result {
        status: CAPTURE_INVALID,
        ..Result::default()
    };
    if pose.bone_count > 1024 || (pose.bone_count != 0 && pose.bones.is_null()) {
        return;
    }
    let bones = if pose.bone_count == 0 {
        Vec::new()
    } else {
        // The fixture lends initialized matrices for this call.
        unsafe { std::slice::from_raw_parts(pose.bones, pose.bone_count) }.to_vec()
    };
    let input = Pose {
        world: pose.world,
        palette_layout: pose.palette_layout,
        bones,
        domain: pose.domain,
        kind: pose.kind,
        ..Pose::default()
    };
    let history = unsafe { &mut *owner.cast::<History>() };
    let capture = history.capture(*token, *key, &input);
    out.status = capture.status();
    if let Some(previous) = capture.previous() {
        out.previous_world = previous.world;
        out.has_previous = true;
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_plan_history_retire(owner: *mut c_void, key: &Key) -> bool {
    unsafe { &mut *owner.cast::<History>() }.retire(*key)
}
