// SPDX-License-Identifier: MIT
use crate::engine_guards::{Image, Read};
use crate::engine_slots::Exchange;
use crate::fg_stage::rsf_anomaly_fg_set_overlay_sink;
use crate::fg_stage::rsf_anomaly_fg_stage_input_received;
use crate::input::rsf_anomaly_input_install;
use crate::input::rsf_anomaly_input_received;
use crate::native_hooks::HookOps;
use crate::native_profile::PROFILES;
use crate::overlay::{rsf_anomaly_overlay_present, rsf_anomaly_overlay_start};
use crate::overlay_hooks::rsf_anomaly_overlay_set_input_observer;
use crate::sys::rsf_anomaly_fg_swap_chain;
use crate::sys::{
    RSF_ANOMALY_OVERLAY_ALREADY_STARTED, RSF_ANOMALY_OVERLAY_STARTED, rsf_anomaly_overlay_options,
};
use rsf_game_sys::rsf_game_log_fn;
use std::ffi::{c_char, c_void};
use std::ptr::{null, null_mut};

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_bootstrap_install_input(
    image: *mut c_void,
    build: usize,
    replacement: *mut c_void,
    exchange: Exchange,
    copy: Read,
    reason: Option<&mut *const c_char>,
) -> bool {
    let Some(profile) = PROFILES.get(build) else {
        if let Some(reason) = reason {
            *reason = c"unknown CInput build".as_ptr();
        }
        return false;
    };
    let image = Image::new(image.cast(), profile.image_size, copy);
    {
        rsf_anomaly_input_install(
            &image,
            build,
            profile.device_frame_rva,
            replacement,
            exchange,
            reason,
        )
    }
}

unsafe extern "C" fn input_received(kind: u8) {
    rsf_anomaly_input_received(kind, rsf_anomaly_fg_stage_input_received);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_bootstrap_publish_input_observer() {
    rsf_anomaly_overlay_set_input_observer(Some(input_received));
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_bootstrap_start_overlay(
    runtime: *mut c_void,
    configuration: *const u16,
    log: rsf_game_log_fn,
    hooks: &HookOps,
) -> bool {
    let options = rsf_anomaly_overlay_options {
        runtime,
        configuration,
        log,
        log_user: null_mut(),
        api_override: null(),
        diagnostics_override: None,
        capture_override: None,
        install_hooks: 1,
    };
    let status = unsafe { rsf_anomaly_overlay_start(&options, Some(hooks), null_mut()) };
    status == RSF_ANOMALY_OVERLAY_STARTED as u32
        || status == RSF_ANOMALY_OVERLAY_ALREADY_STARTED as u32
}

unsafe extern "C" fn overlay_present(user: *mut c_void, chain: *mut rsf_anomaly_fg_swap_chain) {
    rsf_anomaly_overlay_present(user, chain.cast());
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_bootstrap_set_overlay_sink() {
    rsf_anomaly_fg_set_overlay_sink(Some(overlay_present), null_mut());
}
