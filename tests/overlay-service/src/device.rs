// SPDX-License-Identifier: MIT
use super::{Checks, Ini, OK, Query};
use crate::abi::*;
use std::ffi::{CStr, CString, c_char, c_void};
use std::{thread, time::Duration};

type Handle = *mut c_void;
type Foreign = unsafe extern "C" fn(Handle, Handle);

#[repr(C)]
struct Leaves {
    clear: unsafe extern "C" fn(Handle) -> u32,
    submit: unsafe extern "C" fn(Handle) -> u32,
    resize: unsafe extern "C" fn(Handle, u32, u32) -> u32,
    foreign: unsafe extern "C" fn(Handle, Foreign, Handle) -> u32,
    hide_cursor: unsafe extern "C" fn() -> i32,
}

const _: () = assert!(size_of::<Leaves>() == size_of::<Handle>() * 5);
const SR_NOTE: &CStr = c"FSR4 building shaders (3s); DLSS keeps running";
const NOT_READY: i32 = -3;

struct Adapter {
    window: Handle,
    windows: u32,
    reports: u32,
    option_reads: u32,
    label_reads: u32,
    label_order: bool,
    option_value: i32,
    option_label: CString,
}

unsafe extern "C" fn log(_: Handle, message: *const c_char) {
    println!(
        "  log: {}",
        unsafe { CStr::from_ptr(message) }.to_string_lossy()
    );
}

unsafe extern "C" fn accept(user: Handle, window: Handle) -> i32 {
    let adapter = unsafe { &mut *user.cast::<Adapter>() };
    adapter.windows += 1;
    i32::from(window == adapter.window)
}

unsafe extern "C" fn report(user: Handle, out: *mut rsf_overlay_sr_report) -> i32 {
    let adapter = unsafe { &mut *user.cast::<Adapter>() };
    adapter.reports += 1;
    let out = unsafe { &mut *out };
    out.effective_backend = 1;
    out.effective_quality = 2;
    out.render_width = 320;
    out.render_height = 180;
    out.frames_evaluated = adapter.reports;
    out.inputs =
        RSF_OVERLAY_SR_INPUT_COLOR | RSF_OVERLAY_SR_INPUT_DEPTH | RSF_OVERLAY_SR_INPUT_MOTION;
    for (target, &byte) in out.reason.iter_mut().zip(SR_NOTE.to_bytes_with_nul()) {
        *target = byte as c_char;
    }
    1
}

unsafe extern "C" fn options_read(user: Handle, values: *mut i32, count: u32) {
    let adapter = unsafe { &mut *user.cast::<Adapter>() };
    adapter.option_reads += 1;
    if count != 0 {
        unsafe { values.write(adapter.option_value) };
    }
    adapter.option_label = CString::new(format!("Fixture pack: frame {}", adapter.option_reads))
        .expect("fixture label contains no NUL");
}

unsafe extern "C" fn option_changed(user: Handle, index: u32, value: i32) -> i32 {
    if index != 0 || !(0..=1).contains(&value) {
        return 0;
    }
    unsafe { (*user.cast::<Adapter>()).option_value = value };
    1
}

unsafe extern "C" fn option_label(user: Handle, index: u32) -> *const c_char {
    let adapter = unsafe { &mut *user.cast::<Adapter>() };
    adapter.label_reads += 1;
    adapter.label_order &= index == 0 && adapter.label_reads == adapter.option_reads;
    adapter.option_label.as_ptr()
}

fn capture_header() -> rsf_overlay_capture {
    rsf_overlay_capture {
        struct_size: size_of::<rsf_overlay_capture>() as u32,
        ..Default::default()
    }
}

struct ForeignCheck<'a> {
    api: &'a rsf_overlay_api,
    checks: &'a mut Checks,
}

unsafe extern "C" fn test_foreign(user: Handle, chain: Handle) {
    let test = unsafe { &mut *user.cast::<ForeignCheck<'_>>() };
    let mut drawn = 7;
    test.checks.check(
        unsafe { test.api.present.unwrap()(chain, &mut drawn) } == NOT_READY && drawn == 0,
        "window filter refuses another chain",
    );
    let mut occurrence = capture_header();
    test.checks.check(
        unsafe { test.api.capture.unwrap()(&mut occurrence) } == OK && occurrence.started == 0,
        "refused chain did not start the panel",
    );
}

struct Device<'a> {
    leaf: &'a Leaves,
    gpu: Handle,
    chain: Handle,
    api: &'a rsf_overlay_api,
}

impl Device<'_> {
    fn frame(&self) -> Option<u32> {
        let mut drawn = 2;
        unsafe {
            ((self.leaf.clear)(self.gpu) != 0
                && self.api.present.unwrap()(self.chain, &mut drawn) == OK
                && (self.leaf.submit)(self.gpu) != 0)
                .then_some(drawn)
        }
    }

    fn frames(&self, count: u32, expected: Option<u32>, checks: &mut Checks, message: &str) {
        for index in 0..count {
            let drawn = self.frame();
            if drawn.is_none() || expected.is_some_and(|expected| drawn != Some(expected)) {
                checks.check(
                    false,
                    &format!("{message}: frame {index}, drawn {drawn:?}, expected {expected:?}"),
                );
                break;
            }
        }
    }

    fn visible(&self, visible: u32) -> bool {
        thread::sleep(Duration::from_millis(300));
        unsafe {
            self.api.set_visible.unwrap()(visible) == OK && self.api.visible.unwrap()() == visible
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_overlay_device(
    query: Query,
    leaf: &Leaves,
    gpu: Handle,
    chain: Handle,
    window: Handle,
) -> i32 {
    let mut checks = Checks::default();
    let mut api = rsf_overlay_api {
        struct_size: size_of::<rsf_overlay_api>() as u32,
        abi_version: RSF_OVERLAY_API_VERSION,
        ..Default::default()
    };
    if unsafe { query(&mut api) } != OK {
        checks.check(false, "query");
        return 1;
    }
    let device = Device {
        leaf,
        gpu,
        chain,
        api: &api,
    };
    let ini = Ini::new("rsf_overlay_dll_device", "Overlay");
    let mut adapter = Adapter {
        window,
        windows: 0,
        reports: 0,
        option_reads: 0,
        label_reads: 0,
        label_order: true,
        option_value: 1,
        option_label: c"Fixture pack: loading".to_owned(),
    };
    let option = rsf_overlay_service_option {
        label_utf8: c"Fixture pack".as_ptr(),
        kind: RSF_OVERLAY_SERVICE_OPTION_TOGGLE,
        value: 1,
        maximum: 1,
        step: 1,
        divisor: 1,
        ..Default::default()
    };
    let setup = rsf_overlay_service_setup {
        struct_size: size_of::<rsf_overlay_service_setup>() as u32,
        abi_version: RSF_OVERLAY_API_VERSION,
        identity_utf8: c"D3D11 overlay fixture".as_ptr(),
        configuration_path: ini.wide.as_ptr(),
        configuration_section: ini.section.as_ptr(),
        defaults: rsf_render_settings {
            struct_size: size_of::<rsf_render_settings>() as u32,
            abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
            sr_enabled: 1,
            sr_backend: 1,
            fg_backend: RSF_FG_BACKEND_AUTO,
            generated_frames: 1,
            sr_quality: 2,
            ..Default::default()
        },
        sr_backend_choices: 1 << 1,
        sr_quality_choices: 0x1f,
        latency_choices: 1,
        max_generated_frames: 1,
        log: Some(log),
        accept_window: Some(accept),
        sr_report: Some(report),
        user: (&raw mut adapter).cast(),
        option_count: 1,
        options: &option,
        options_read: Some(options_read),
        option_changed: Some(option_changed),
        option_label: Some(option_label),
        ..Default::default()
    };
    checks.check(unsafe { api.configure.unwrap()(&setup) } == OK, "configure");
    let mut foreign = ForeignCheck {
        api: &api,
        checks: &mut checks,
    };
    let foreign_ready =
        unsafe { (leaf.foreign)(gpu, test_foreign, (&raw mut foreign).cast()) } != 0;
    checks.check(foreign_ready, "second chain for the window filter");

    device.frames(3, None, &mut checks, "closed frame presents");
    let get_capture = api.capture.unwrap();
    let mut capture = capture_header();
    checks.check(
        unsafe { get_capture(&mut capture) } == OK
            && capture.started != 0
            && capture.visible == 0
            && capture.captured == 0,
        "started closed and input passes through",
    );
    checks.check(device.visible(1), "open panel");
    checks.check(
        unsafe { get_capture(&mut capture) } == OK
            && capture.visible != 0
            && capture.captured == (RSF_OVERLAY_CAPTURE_MOUSE | RSF_OVERLAY_CAPTURE_KEYBOARD),
        "open panel captures mouse and keyboard",
    );
    device.frames(6, Some(1), &mut checks, "open panel draws every frame");

    let mut count = 0;
    let mut attempts = 0;
    while count >= 0 && attempts < 512 {
        count = unsafe { (leaf.hide_cursor)() };
        attempts += 1;
    }
    checks.check(
        count < 0,
        "game-style ShowCursor hide loop terminated while the panel is open",
    );
    println!("hide loop: count={count} after {attempts} calls");

    let get_status = api.status.unwrap();
    let mut status = rsf_render_status {
        struct_size: size_of::<rsf_render_status>() as u32,
        abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
        ..Default::default()
    };
    checks.check(
        unsafe { get_status(&mut status) } == OK,
        "status while open",
    );
    checks.check(
        status.sr_requested_backend == 1
            && status.sr_effective_backend == 1
            && status.render_width == 320
            && status.output_width == 640
            && status.output_height == 360
            && status.fg_requested == 0
            && status.fg_effective_backend == 0
            && status.application_presents >= 9,
        "status reports adapter SR facts and chain extent",
    );
    let reason = unsafe { CStr::from_ptr(status.reason.as_ptr()) }.to_string_lossy();
    println!(
        "status: presents={} reason={reason}",
        status.application_presents
    );
    checks.check(
        adapter.reports >= 9 && adapter.windows >= 10,
        "adapter callbacks consulted per Present",
    );
    checks.check(
        adapter.label_reads >= 9 && adapter.label_order,
        "live option labels follow current values",
    );
    checks.check(
        reason.contains(SR_NOTE.to_str().unwrap()),
        "status shows the adapter's note while SR is running",
    );

    checks.check(device.visible(0), "close panel");
    checks.check(
        unsafe { get_capture(&mut capture) } == OK
            && capture.started != 0
            && capture.visible == 0
            && capture.captured == 0,
        "closed panel releases input",
    );
    device.frames(
        4,
        Some(0),
        &mut checks,
        "closed panel without HUD draws nothing",
    );

    let notify = api.notify.unwrap();
    checks.check(
        unsafe { notify(chain, RSF_OVERLAY_NOTIFY_RESIZE) } == OK,
        "resize notification",
    );
    checks.check(
        unsafe { (leaf.resize)(gpu, 800, 450) } != 0,
        "ResizeBuffers",
    );
    checks.check(device.visible(1), "reopen after resize");
    device.frames(3, Some(1), &mut checks, "open panel draws after resize");
    checks.check(
        unsafe { get_status(&mut status) } == OK
            && status.output_width == 800
            && status.output_height == 450,
        "status follows resized chain",
    );

    checks.check(
        unsafe { notify(chain, RSF_OVERLAY_NOTIFY_DEVICE_RESET) } == OK,
        "device reset notification",
    );
    checks.check(
        unsafe { get_capture(&mut capture) } == OK
            && capture.started == 0
            && capture.captured == 0
            && unsafe { api.visible.unwrap()() } == 0,
        "reset released the panel",
    );
    device.frames(2, None, &mut checks, "frame after reset");
    checks.check(
        unsafe { get_capture(&mut capture) } == OK && capture.started != 0,
        "panel restarted on next Present",
    );
    checks.check(device.visible(1), "open after restart");
    device.frames(3, Some(1), &mut checks, "restarted panel draws");
    checks.check(device.visible(0), "close after restart");
    checks.check(
        unsafe { notify(chain, RSF_OVERLAY_NOTIFY_DEVICE_RESET) } == OK,
        "final release",
    );
    if checks.failures != 0 {
        return 1;
    }
    println!(
        "PASS: shared Rust overlay opened, drew, captured and released input through the Runtime DLL API"
    );
    0
}
