// SPDX-License-Identifier: MIT
use crate::native_observer::*;
use crate::sys::rsf_anomaly_observer_platform as Platform;
use rsf_game_sys::session::{Probe, publish_api};
use rsf_game_sys::*;

unsafe extern "C" {
    fn rsf_anomaly_observer_native() -> *const Platform;
}

fn platform() -> &'static Platform {
    // The linked adapter owns this immutable table.
    unsafe { &*rsf_anomaly_observer_native() }
}

const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");

unsafe extern "C" fn detect(probe: *const rsf_game_probe) -> rsf_detection {
    unsafe {
        Probe::detect(probe, |probe| {
            crate::native_profile::find(probe.executable.to_bytes(), probe.sha256.to_bytes())
                .is_some()
        })
    }
}

unsafe extern "C" fn prepare(args: *const rsf_game_prepare_args) -> rsf_result {
    unsafe { rsf_anomaly_observer_prepare(args, platform()) }
}

unsafe extern "C" fn start(args: *const rsf_game_start_args) -> rsf_result {
    unsafe { rsf_anomaly_observer_start(args, platform()) }
}

unsafe extern "C" fn quiesce(args: *const rsf_game_control_args) -> rsf_result {
    unsafe { rsf_anomaly_observer_quiesce(args, platform()) }
}

unsafe extern "C" fn stop(args: *const rsf_game_control_args) -> rsf_result {
    unsafe { rsf_anomaly_observer_stop(args, platform()) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_get_game_plugin_api(
    version: u32,
    out: *mut rsf_game_plugin_api,
) -> rsf_result {
    let info = rsf_game_info {
        id: c"anomaly".as_ptr(),
        name: c"STALKER Anomaly".as_ptr(),
        version: VERSION.as_ptr().cast(),
        rendering_ready: 0,
        status: c"Guarded native boundary observation; temporal SR/FG execution is not validated."
            .as_ptr(),
    };
    let hooks = rsf_game_hooks_api {
        prepare: Some(prepare),
        start: Some(start),
        quiesce: Some(quiesce),
        stop: Some(stop),
        status: Some(rsf_anomaly_observer_status),
    };
    unsafe { publish_api(version, out, info, detect, hooks) }
}
