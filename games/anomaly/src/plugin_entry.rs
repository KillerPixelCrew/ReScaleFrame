// SPDX-License-Identifier: MIT
use crate::native_observer::*;
use crate::native_observer_abi::rsf_anomaly_observer_platform as Platform;
use rsf_game_sys::*;

unsafe extern "C" {
    fn rsf_anomaly_observer_native() -> *const Platform;
}

fn platform() -> &'static Platform {
    // The linked adapter owns this immutable table.
    unsafe { &*rsf_anomaly_observer_native() }
}

unsafe extern "C" fn detect(probe: *const rsf_game_probe) -> u32 {
    unsafe { crate::rsf_anomaly_detect(probe) }
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
    api: *mut rsf_game_plugin_api,
) -> rsf_result {
    let hooks = rsf_game_hooks_api {
        prepare: Some(prepare),
        start: Some(start),
        quiesce: Some(quiesce),
        stop: Some(stop),
        status: Some(rsf_anomaly_observer_status),
    };
    unsafe { crate::rsf_anomaly_publish_api(version, api, Some(detect), &hooks) }
}
