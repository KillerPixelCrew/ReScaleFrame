// SPDX-License-Identifier: MIT
use crate::native_observer::*;
use crate::sys::rsf_anomaly_observer_platform as Platform;
use rsf_game_sys::*;
use std::ffi::CStr;

unsafe extern "C" {
    fn rsf_anomaly_observer_native() -> *const Platform;
}

fn platform() -> &'static Platform {
    // The linked adapter owns this immutable table.
    unsafe { &*rsf_anomaly_observer_native() }
}

const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");

unsafe extern "C" fn detect(probe: *const rsf_game_probe) -> rsf_detection {
    // Only the size field is readable from a short SDK structure.
    if probe.is_null() || unsafe { probe.cast::<u32>().read() } < size_of::<rsf_game_probe>() as u32
    {
        return RSF_GAME_UNKNOWN;
    }
    let probe = unsafe { &*probe };
    if probe.pe_machine != crate::AMD64
        || probe.executable_name_utf8.is_null()
        || probe.sha256_hex.is_null()
    {
        return RSF_GAME_UNKNOWN;
    }
    let executable = unsafe { CStr::from_ptr(probe.executable_name_utf8) };
    let sha256 = unsafe { CStr::from_ptr(probe.sha256_hex) };
    if crate::native_profile::find(executable.to_bytes(), sha256.to_bytes()).is_some() {
        RSF_GAME_RECOGNIZED
    } else {
        RSF_GAME_UNKNOWN
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
    if out.is_null()
        || unsafe { out.cast::<u32>().read() } < size_of::<rsf_game_plugin_api>() as u32
    {
        return RSF_ERROR_INVALID_ARGUMENT;
    }
    if version != RSF_GAME_ABI_VERSION {
        return RSF_ERROR_ABI_MISMATCH;
    }
    let api = rsf_game_plugin_api {
        struct_size: size_of::<rsf_game_plugin_api>() as u32,
        abi_version: RSF_GAME_ABI_VERSION,
        info: rsf_game_info {
            id: c"anomaly".as_ptr(),
            name: c"STALKER Anomaly".as_ptr(),
            version: VERSION.as_ptr().cast(),
            rendering_ready: 0,
            status:
                c"Guarded native boundary observation; temporal SR/FG execution is not validated."
                    .as_ptr(),
        },
        detect: Some(detect),
        hooks: rsf_game_hooks_api {
            prepare: Some(prepare),
            start: Some(start),
            quiesce: Some(quiesce),
            stop: Some(stop),
            status: Some(rsf_anomaly_observer_status),
        },
    };
    unsafe { out.write(api) };
    RSF_OK
}
