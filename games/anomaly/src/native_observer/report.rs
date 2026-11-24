// SPDX-License-Identifier: MIT
use super::Session;
use crate::motion::stage::rsf_anomaly_motion_stage_report;
use crate::native_geometry::coverage::rsf_anomaly_geometry_report;
use crate::native_geometry::coverage::rsf_anomaly_geometry_stats_read;
use crate::native_geometry::rsf_anomaly_geometry_active;
use crate::native_geometry_abi::Stats;
use crate::sr_stage::rsf_anomaly_sr_stage_report;
use std::ffi::{c_char, c_void};

unsafe extern "C" fn forward(user: *mut c_void, text: *const c_char) {
    // Each stage reports synchronously. The observer still holds the
    // invocation lease, and the guarded log leaf contains foreign exceptions.
    let session = unsafe { &*user.cast::<Session>() };
    unsafe { rsf_game_sys::rsf_game_log_guarded(session.log, session.user, text) };
}

pub(super) fn stages(mut session: Session) {
    if session.log.is_none() {
        return;
    }
    let user = std::ptr::from_mut(&mut session).cast();
    if rsf_anomaly_geometry_active() {
        let mut stats = Stats::default();
        unsafe { rsf_anomaly_geometry_stats_read(&mut stats) };
        session.emit(&format!(
            "Anomaly geometry identity registered={} retired={} draws_identified={} draws_unidentified={} faults={}; no history consumer, motion not established",
            stats.models_registered,
            stats.models_retired,
            stats.draws_identified,
            stats.draws_unidentified,
            stats.capture_faults,
        ));
        rsf_anomaly_geometry_report(Some(forward), user);
    }
    unsafe {
        rsf_anomaly_sr_stage_report(Some(forward), user);
        rsf_anomaly_motion_stage_report(Some(forward), user);
    }
    crate::fg_stage::report(Some(forward), user);
}
