// SPDX-License-Identifier: MIT
use super::{Platform, Session};
use crate::motion::stage::rsf_anomaly_motion_stage_report;
use crate::native_geometry::coverage::rsf_anomaly_geometry_report;
use crate::native_geometry::coverage::rsf_anomaly_geometry_stats_read;
use crate::native_geometry::rsf_anomaly_geometry_active;
use crate::native_geometry_abi::Stats;
use crate::sr_stage::rsf_anomaly_sr_stage_report;
use std::ffi::{CString, c_char, c_void};

struct Sink<'a> {
    session: Session,
    platform: &'a Platform,
}

unsafe extern "C" fn forward(user: *mut c_void, text: *const c_char) {
    // Each stage reports synchronously. The observer still holds the
    // invocation lease, and the C++ log leaf contains foreign exceptions.
    let sink = unsafe { &*user.cast::<Sink<'_>>() };
    unsafe { (sink.platform.log.unwrap())(sink.session.log, sink.session.user, text) };
}

pub(super) fn stages(session: Session, platform: &Platform) {
    if session.log.is_none() {
        return;
    }
    let mut sink = Sink { session, platform };
    let user = std::ptr::from_mut(&mut sink).cast();
    if rsf_anomaly_geometry_active() {
        let mut stats = Stats::default();
        unsafe { rsf_anomaly_geometry_stats_read(&mut stats) };
        if let Ok(text) = CString::new(format!(
            "Anomaly geometry identity registered={} retired={} draws_identified={} draws_unidentified={} faults={}; no history consumer, motion not established",
            stats.models_registered,
            stats.models_retired,
            stats.draws_identified,
            stats.draws_unidentified,
            stats.capture_faults,
        )) {
            session.emit(platform, &text);
        }
        rsf_anomaly_geometry_report(Some(forward), user);
    }
    unsafe {
        rsf_anomaly_sr_stage_report(Some(forward), user);
        rsf_anomaly_motion_stage_report(Some(forward), user);
    }
    crate::fg_stage::report(Some(forward), user);
}
