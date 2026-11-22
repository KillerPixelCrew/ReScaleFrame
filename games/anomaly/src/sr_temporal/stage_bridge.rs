// SPDX-License-Identifier: MIT
//! Bind the production Stage directly to the Rust temporal owner.
use super::*;
use crate::host_log::Log;
use crate::sr_stage_abi::rsf_anomaly_sr_stage_platform as Platform;
use std::sync::{LazyLock, OnceLock};

struct StageDriver(Driver);

// The stage serializes RenderState access. Overlay updates and reports use
// Driver's published mutex and may run outside the stage render lock.
unsafe impl Send for StageDriver {}
unsafe impl Sync for StageDriver {}

static DRIVER: LazyLock<StageDriver> = LazyLock::new(|| StageDriver(new_driver()));
static PLATFORM: OnceLock<Platform> = OnceLock::new();
static SINK: Mutex<Log> = Mutex::new(Log {
    callback: None,
    user: 0,
});

unsafe extern "C" fn forward(_: Raw, message: *const c_char) {
    let sink = *SINK.lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(call) = PLATFORM.get().and_then(|p| p.guarded_log) {
        unsafe { call(sink.callback, sink.user as Raw, message) };
    }
}

unsafe extern "C" fn configure(
    log: rsf_game_log_fn,
    user: Raw,
    config: *const rsf_anomaly_sr_driver_config,
) {
    if let Some(config) = unsafe { config.as_ref() } {
        *SINK.lock().unwrap_or_else(|poison| poison.into_inner()) = Log {
            callback: log,
            user: user as usize,
        };
        unsafe {
            rsf_anomaly_sr_configure(&DRIVER.0, config, log.map(|_| forward as _), null_mut())
        };
    }
}

unsafe extern "C" fn bind() -> bool {
    let runtime = crate::runtime_module::rsf_anomaly_runtime_module();
    unsafe { rsf_anomaly_sr_bind_runtime(&DRIVER.0, runtime) }
}

unsafe extern "C" fn enabled() -> bool {
    rsf_anomaly_sr_enabled(&DRIVER.0)
}

unsafe extern "C" fn output_format() -> u32 {
    unsafe { rsf_anomaly_sr_output_format(&DRIVER.0) }
}

unsafe extern "C" fn plan(ow: u32, oh: u32, rw: *mut u32, rh: *mut u32) -> bool {
    unsafe { rsf_anomaly_sr_plan(&DRIVER.0, ow, oh, rw, rh) }
}

unsafe extern "C" fn boundary(width: u32, height: u32) {
    let device = PLATFORM
        .get()
        .and_then(|p| p.device)
        .map_or(null_mut(), |get| unsafe { get() });
    unsafe { rsf_anomaly_sr_boundary(&DRIVER.0, device, width, height) };
}

unsafe extern "C" fn evaluate(
    inputs: *mut PassInputs,
    camera: *mut rsf_camera_frame,
    backend: *mut u32,
) -> bool {
    let context = PLATFORM
        .get()
        .and_then(|p| p.context)
        .map_or(null_mut(), |get| unsafe { get() });
    let inputs = unsafe { &mut *inputs };
    let reset = ResetFacts {
        sequence: inputs.frame.reset_sequence,
        cut: inputs.frame.cut,
        ..Default::default()
    };
    unsafe { rsf_anomaly_sr_evaluate(&DRIVER.0, context, inputs, &reset, camera, backend) }
}

unsafe extern "C" fn request_dump(prefix: *const c_char) -> bool {
    unsafe { rsf_anomaly_sr_request_dump(&DRIVER.0, prefix) }
}

unsafe extern "C" fn fallback(reason: u8, drawn: bool) {
    rsf_anomaly_sr_fallback(&DRIVER.0, reason, drawn);
}

unsafe extern "C" fn count_prepared(missing: bool) {
    rsf_anomaly_sr_count_prepared(&DRIVER.0, missing);
}

unsafe extern "C" fn stop() {
    unsafe { rsf_anomaly_sr_stop(&DRIVER.0) };
}

unsafe extern "C" fn report(exposure: bool, first_person: bool, log: rsf_game_log_fn, user: Raw) {
    rsf_anomaly_sr_report(&DRIVER.0, exposure, first_person, log, user);
}

unsafe extern "C" fn overlay_report(report: Raw) {
    rsf_anomaly_sr_overlay_report(&DRIVER.0, unsafe { &mut *report.cast() });
}

unsafe extern "C" fn request(settings: *const Settings) {
    rsf_anomaly_sr_request(&DRIVER.0, unsafe { &*settings });
}

pub(crate) fn production(leaves: Platform) -> &'static Platform {
    PLATFORM.get_or_init(|| Platform {
        configured: Some(configure),
        driver_bind: Some(bind),
        driver_enabled: Some(enabled),
        driver_output_format: Some(output_format),
        driver_planned: Some(plan),
        driver_boundary: Some(boundary),
        driver_evaluate: Some(evaluate),
        driver_request_dump: Some(request_dump),
        driver_fallback: Some(fallback),
        driver_count_prepared: Some(count_prepared),
        driver_stop: Some(stop),
        driver_report: Some(report),
        driver_overlay_report: Some(overlay_report),
        driver_request: Some(request),
        ..leaves
    })
}
