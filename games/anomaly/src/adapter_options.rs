// SPDX-License-Identifier: MIT
use crate::{fg_stage, ini, overlay, overlay_abi::rsf_anomaly_overlay_option};
use std::ffi::{CStr, c_char};
use std::ops::RangeInclusive;
use std::sync::OnceLock;
use windows_sys::core::{PCWSTR, w};

// The bootstrap supplies a validated, NUL-terminated path once per process.
static PATH: OnceLock<Vec<u16>> = OnceLock::new();
const PANEL_MAX: i32 = i32::MAX;

fn read(key: PCWSTR, fallback: i32) -> i32 {
    PATH.get().map_or(fallback, |path| unsafe {
        ini::read_int(path.as_ptr(), key, fallback)
    })
}

fn write(key: PCWSTR, value: i32) -> bool {
    PATH.get()
        .is_some_and(|path| unsafe { ini::write_int(path.as_ptr(), key, value) })
}

fn capture_number(key: PCWSTR, fallback: i32, low: u32, high: u32) -> i32 {
    (read(key, fallback) as u32).clamp(low, high) as i32
}

fn save_capture(key: PCWSTR, value: i32) {
    if let Some(path) = PATH.get()
        && unsafe { ini::write_int(path.as_ptr(), key, value) }
    {
        unsafe { fg_stage::reload_capture(path.as_ptr()) };
    }
}

fn register_panel_option(
    label: &'static CStr,
    range: RangeInclusive<i32>,
    restart: bool,
    get: extern "C" fn() -> i32,
    set: extern "C" fn(i32),
) {
    let option = rsf_anomaly_overlay_option {
        label: label.as_ptr(),
        toggle: (range == (0..=1)).into(),
        restart: restart.into(),
        minimum: *range.start(),
        maximum: *range.end(),
        step: 1,
        divisor: 1,
        get: Some(get),
        set: Some(set),
    };
    unsafe { overlay::rsf_anomaly_overlay_option_add(&option) };
}

pub(crate) fn register(path: &[u16]) {
    if path.last() != Some(&0) {
        return;
    }
    PATH.get_or_init(|| path.to_vec());

    register_panel_option(
        c"SR stage (0 off .. 5 temporal)",
        0..=5,
        true,
        sr_stage,
        set_sr_stage,
    );
    register_panel_option(c"Frame generation bridge", 0..=1, true, bridge, set_bridge);
    register_panel_option(
        c"F10 frame capture",
        0..=1,
        true,
        sr_capture,
        set_sr_capture,
    );
    register_panel_option(
        c"Frame generation scope hold (frames)",
        1..=64,
        false,
        scope_hold,
        set_scope_hold,
    );
    register_panel_option(
        c"Frame generation debug logging",
        0..=1,
        true,
        debug,
        set_debug,
    );
    register_panel_option(
        c"Frame generation development runtime",
        0..=1,
        true,
        development,
        set_development,
    );
    register_panel_option(
        c"Capture frame generation inputs",
        0..=1,
        false,
        fg_capture,
        set_fg_capture,
    );
    register_panel_option(
        c"FG capture start frame",
        0..=PANEL_MAX,
        false,
        capture_start,
        set_capture_start,
    );
    register_panel_option(
        c"FG capture frame count",
        0..=16,
        false,
        capture_count,
        set_capture_count,
    );
    register_panel_option(
        c"FG capture interval (frames)",
        1..=PANEL_MAX,
        false,
        capture_interval,
        set_capture_interval,
    );
}

extern "C" fn sr_stage() -> i32 {
    read(w!("SRStage"), 0)
}

extern "C" fn set_sr_stage(value: i32) {
    write(w!("SRStage"), value);
}

extern "C" fn bridge() -> i32 {
    read(w!("FGBridge"), 1)
}

extern "C" fn set_bridge(value: i32) {
    write(w!("FGBridge"), value);
}

extern "C" fn sr_capture() -> i32 {
    read(w!("SRStageCapture"), 0)
}

extern "C" fn set_sr_capture(value: i32) {
    write(w!("SRStageCapture"), value);
    if value != 0 {
        write(w!("SRStageCaptureCount"), 0);
    }
}

extern "C" fn scope_hold() -> i32 {
    fg_stage::scope_hold(read(w!("FGScopeHold"), 4) as u32) as i32
}

extern "C" fn set_scope_hold(value: i32) {
    if (1..=64).contains(&value) && write(w!("FGScopeHold"), value) {
        fg_stage::set_scope_hold(value as u32);
    }
}

extern "C" fn debug() -> i32 {
    i32::from(read(w!("FGDebug"), 0) != 0)
}

extern "C" fn set_debug(value: i32) {
    write(w!("FGDebug"), i32::from(value != 0));
}

extern "C" fn development() -> i32 {
    i32::from(read(w!("FGDevelopment"), 0) != 0)
}

extern "C" fn set_development(value: i32) {
    write(w!("FGDevelopment"), i32::from(value != 0));
}

extern "C" fn fg_capture() -> i32 {
    i32::from(read(w!("FGCapture"), 0) != 0)
}

extern "C" fn set_fg_capture(value: i32) {
    save_capture(w!("FGCapture"), i32::from(value != 0));
}

extern "C" fn capture_start() -> i32 {
    capture_number(w!("FGCaptureStart"), 600, 0, PANEL_MAX as u32)
}

extern "C" fn set_capture_start(value: i32) {
    save_capture(w!("FGCaptureStart"), value);
}

extern "C" fn capture_count() -> i32 {
    capture_number(w!("FGCaptureCount"), 3, 0, 16)
}

extern "C" fn set_capture_count(value: i32) {
    save_capture(w!("FGCaptureCount"), value);
}

extern "C" fn capture_interval() -> i32 {
    capture_number(w!("FGCaptureInterval"), 300, 1, PANEL_MAX as u32)
}

extern "C" fn set_capture_interval(value: i32) {
    save_capture(w!("FGCaptureInterval"), value);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_persist_int(key: *const c_char, value: i32) {
    let key = unsafe { ini::widen(key) };
    write(key.as_ptr(), value);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_persist_text(key: *const c_char, value: *const c_char) {
    if let Some(path) = PATH.get() {
        let key = unsafe { ini::widen(key) };
        let value = unsafe { ini::widen(value) };
        unsafe { ini::write_text(path.as_ptr(), key.as_ptr(), value.as_ptr()) };
    }
}
