// SPDX-License-Identifier: MIT
//! The production Stage's driver leaves (sr_stage/native.rs re-exports them as `sr_stage::leaves`) over the Rust
//! temporal owner. Under test the stage calls fakes, so these are compiled but unused.
#![cfg_attr(test, allow(dead_code))]
use super::*;
use crate::host_log::Log;
use std::sync::LazyLock;

struct StageDriver(Driver);

// The stage serializes RenderState access. Overlay updates and reports use
// Driver's published mutex and may run outside the stage render lock.
unsafe impl Send for StageDriver {}
unsafe impl Sync for StageDriver {}

static DRIVER: LazyLock<StageDriver> = LazyLock::new(|| StageDriver(new_driver()));
static SINK: Mutex<Log> = Mutex::new(Log {
    callback: None,
    user: 0,
});

unsafe extern "C" fn forward(_: Raw, message: *const c_char) {
    let sink = *crate::lock(&SINK);
    unsafe { rsf_game_sys::rsf_game_log_guarded(sink.callback, sink.user as Raw, message) };
}

pub(crate) unsafe fn configure(
    log: rsf_game_log_fn,
    user: Raw,
    config: *const rsf_anomaly_sr_driver_config,
) {
    if let Some(config) = unsafe { config.as_ref() } {
        *crate::lock(&SINK) = Log {
            callback: log,
            user: user as usize,
        };
        unsafe {
            rsf_anomaly_sr_configure(&DRIVER.0, config, log.map(|_| forward as _), null_mut())
        };
    }
}

pub(crate) unsafe fn bind() -> bool {
    let runtime = crate::runtime_module::rsf_anomaly_runtime_module();
    unsafe { rsf_anomaly_sr_bind_runtime(&DRIVER.0, runtime) }
}

pub(crate) unsafe fn enabled() -> bool {
    rsf_anomaly_sr_enabled(&DRIVER.0)
}

pub(crate) unsafe fn output_format() -> u32 {
    unsafe { rsf_anomaly_sr_output_format(&DRIVER.0) }
}

pub(crate) fn plan(ow: u32, oh: u32) -> Option<[u32; 2]> {
    rsf_anomaly_sr_plan(&DRIVER.0, ow, oh)
}

pub(crate) unsafe fn boundary(width: u32, height: u32) {
    #[allow(unused_unsafe)] // the test fake is safe
    let device = unsafe { crate::sr_stage::leaves::device() };
    unsafe { rsf_anomaly_sr_boundary(&DRIVER.0, device, width, height) };
}

pub(crate) unsafe fn evaluate(
    inputs: *mut PassInputs,
    camera: *mut rsf_camera_frame,
    backend: *mut u32,
) -> bool {
    #[allow(unused_unsafe)] // the test fake is safe
    let context = unsafe { crate::sr_stage::leaves::context() };
    let inputs = unsafe { &mut *inputs };
    let reset = ResetFacts {
        sequence: inputs.frame.reset_sequence,
        cut: inputs.frame.cut,
        ..Default::default()
    };
    unsafe { rsf_anomaly_sr_evaluate(&DRIVER.0, context, inputs, &reset, camera, backend) }
}

pub(crate) unsafe fn request_dump(prefix: *const c_char) -> bool {
    unsafe { rsf_anomaly_sr_request_dump(&DRIVER.0, prefix) }
}

pub(crate) unsafe fn fallback(reason: u8, drawn: bool) {
    rsf_anomaly_sr_fallback(&DRIVER.0, reason, drawn);
}

pub(crate) unsafe fn count_prepared(missing: bool) {
    rsf_anomaly_sr_count_prepared(&DRIVER.0, missing);
}

pub(crate) unsafe fn stop() {
    unsafe { rsf_anomaly_sr_stop(&DRIVER.0) };
}

pub(crate) unsafe fn report(
    exposure: bool,
    first_person: bool,
    log: rsf_game_log_fn,
    user: Raw,
) {
    rsf_anomaly_sr_report(&DRIVER.0, exposure, first_person, log, user);
}

pub(crate) unsafe fn overlay_report(report: Raw) {
    rsf_anomaly_sr_overlay_report(&DRIVER.0, unsafe { &mut *report.cast() });
}

pub(crate) unsafe fn request(settings: *const Settings) {
    rsf_anomaly_sr_request(&DRIVER.0, unsafe { &*settings });
}
