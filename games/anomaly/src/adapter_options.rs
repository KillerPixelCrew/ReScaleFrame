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

fn save_capture(key: PCWSTR, value: i32) -> bool {
    let Some(path) = PATH.get() else {
        return false;
    };
    if !unsafe { ini::write_int(path.as_ptr(), key, value) } {
        return false;
    }
    unsafe { fg_stage::reload_capture(path.as_ptr()) };
    true
}

fn register_panel_option(
    label: &'static CStr,
    range: RangeInclusive<i32>,
    restart: bool,
    get: extern "C" fn() -> i32,
    set: extern "C" fn(i32) -> bool,
) {
    unsafe { register_option(label.as_ptr(), range, restart, get, set) };
}

/// The label must remain readable through the lifetime of the panel entry.
pub(crate) unsafe fn register_option(
    label: *const c_char,
    range: RangeInclusive<i32>,
    restart: bool,
    get: unsafe extern "C" fn() -> i32,
    set: unsafe extern "C" fn(i32) -> bool,
) {
    let option = rsf_anomaly_overlay_option {
        label,
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
    register_panel_option(
        c"Poll Insert as overlay fallback",
        0..=1,
        true,
        key_poll,
        set_key_poll,
    );
    register_panel_option(
        c"Capture overlay backbuffer",
        0..=1,
        true,
        overlay_capture,
        set_overlay_capture,
    );
    register_panel_option(
        c"Overlay capture frame count",
        1..=16,
        true,
        overlay_frames,
        set_overlay_frames,
    );
    register_panel_option(
        c"Overlay capture interval (presents)",
        1..=600,
        true,
        overlay_stride,
        set_overlay_stride,
    );
    register_panel_option(
        c"Overlay capture cycles",
        1..=8,
        true,
        overlay_cycles,
        set_overlay_cycles,
    );
}

extern "C" fn sr_stage() -> i32 {
    read(w!("SRStage"), 0)
}

extern "C" fn set_sr_stage(value: i32) -> bool {
    write(w!("SRStage"), value)
}

extern "C" fn bridge() -> i32 {
    read(w!("FGBridge"), 1)
}

extern "C" fn set_bridge(value: i32) -> bool {
    write(w!("FGBridge"), value)
}

extern "C" fn sr_capture() -> i32 {
    read(w!("SRStageCapture"), 0)
}

extern "C" fn set_sr_capture(value: i32) -> bool {
    (value == 0 || write(w!("SRStageCaptureCount"), 0)) && write(w!("SRStageCapture"), value)
}

extern "C" fn scope_hold() -> i32 {
    fg_stage::scope_hold(read(w!("FGScopeHold"), 4) as u32) as i32
}

extern "C" fn set_scope_hold(value: i32) -> bool {
    if !(1..=64).contains(&value) || !write(w!("FGScopeHold"), value) {
        return false;
    }
    fg_stage::set_scope_hold(value as u32);
    true
}

extern "C" fn debug() -> i32 {
    i32::from(read(w!("FGDebug"), 0) != 0)
}

extern "C" fn set_debug(value: i32) -> bool {
    write(w!("FGDebug"), i32::from(value != 0))
}

extern "C" fn development() -> i32 {
    i32::from(read(w!("FGDevelopment"), 0) != 0)
}

extern "C" fn set_development(value: i32) -> bool {
    write(w!("FGDevelopment"), i32::from(value != 0))
}

extern "C" fn fg_capture() -> i32 {
    i32::from(fg_stage::CaptureSettings::decode(read).enabled)
}

extern "C" fn set_fg_capture(value: i32) -> bool {
    save_capture(w!("FGCapture"), i32::from(value != 0))
}

extern "C" fn capture_start() -> i32 {
    fg_stage::CaptureSettings::decode(read).start as i32
}

extern "C" fn set_capture_start(value: i32) -> bool {
    save_capture(w!("FGCaptureStart"), value)
}

extern "C" fn capture_count() -> i32 {
    fg_stage::CaptureSettings::decode(read).count as i32
}

extern "C" fn set_capture_count(value: i32) -> bool {
    save_capture(w!("FGCaptureCount"), value)
}

extern "C" fn capture_interval() -> i32 {
    fg_stage::CaptureSettings::decode(read).interval as i32
}

extern "C" fn set_capture_interval(value: i32) -> bool {
    save_capture(w!("FGCaptureInterval"), value)
}

extern "C" fn key_poll() -> i32 {
    i32::from(read(w!("OverlayKeyPoll"), 1) != 0)
}

extern "C" fn set_key_poll(value: i32) -> bool {
    write(w!("OverlayKeyPoll"), value)
}

extern "C" fn overlay_capture() -> i32 {
    i32::from(read(w!("OverlayCapture"), 0) != 0)
}

extern "C" fn set_overlay_capture(value: i32) -> bool {
    write(w!("OverlayCapture"), value)
}

extern "C" fn overlay_frames() -> i32 {
    capture_number(w!("OverlayCaptureFrames"), 3, 1, 16)
}

extern "C" fn set_overlay_frames(value: i32) -> bool {
    write(w!("OverlayCaptureFrames"), value)
}

extern "C" fn overlay_stride() -> i32 {
    capture_number(w!("OverlayCaptureStride"), 20, 1, 600)
}

extern "C" fn set_overlay_stride(value: i32) -> bool {
    write(w!("OverlayCaptureStride"), value)
}

extern "C" fn overlay_cycles() -> i32 {
    capture_number(w!("OverlayCaptureCycles"), 2, 1, 8)
}

extern "C" fn set_overlay_cycles(value: i32) -> bool {
    write(w!("OverlayCaptureCycles"), value)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_persist_int(key: *const c_char, value: i32) -> bool {
    let key = unsafe { ini::widen(key) };
    write(key.as_ptr(), value)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_persist_text(key: *const c_char, value: *const c_char) -> bool {
    PATH.get().is_some_and(|path| {
        let key = unsafe { ini::widen(key) };
        let value = unsafe { ini::widen(value) };
        unsafe { ini::write_text(path.as_ptr(), key.as_ptr(), value.as_ptr()) }
    })
}
