// SPDX-License-Identifier: MIT
#![cfg(all(windows, not(test)))]

mod contract;
mod device;

use rsf_game_sys::*;
use rsf_game_sys::{RSF_ERROR_INVALID_ARGUMENT, RSF_OK};
use std::ffi::{CStr, c_void};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use windows_sys::Win32::System::WindowsProgramming::{
    GetPrivateProfileIntW, WritePrivateProfileStringW,
};

// Cast macros in game_api.h cannot be emitted as Rust constants by bindgen.
type Query = unsafe extern "C" fn(*mut rsf_overlay_api) -> i32;

struct Ini {
    path: PathBuf,
    wide: Vec<u16>,
    section: Vec<u16>,
}

impl Ini {
    fn new(prefix: &str, section: &str) -> Self {
        let path = std::env::temp_dir().join(format!("{prefix}_{}.ini", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let wide = rsf_test_support::wide_os(&path);
        Self {
            path,
            wide,
            section: ffi_util::wide(section),
        }
    }

    fn read(&self, key: &str) -> u32 {
        let key = ffi_util::wide(key);
        unsafe {
            GetPrivateProfileIntW(
                self.section.as_ptr(),
                key.as_ptr(),
                0xdead,
                self.wide.as_ptr(),
            ) as u32
        }
    }

    fn write(&self, key: &str, value: &str) -> bool {
        let (key, value) = (ffi_util::wide(key), ffi_util::wide(value));
        unsafe {
            WritePrivateProfileStringW(
                self.section.as_ptr(),
                key.as_ptr(),
                value.as_ptr(),
                self.wide.as_ptr(),
            ) != 0
        }
    }
}

impl Drop for Ini {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[derive(Default)]
struct Checks {
    failures: i32,
}

impl Checks {
    fn check(&mut self, condition: bool, message: &str) {
        if !condition {
            eprintln!("FAIL: {message}");
            self.failures += 1;
        }
    }
}

static CHANGED: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn settings_changed(_: *mut c_void, _: *const rsf_render_settings) {
    CHANGED.fetch_add(1, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_overlay_no_sr(query: Query) -> i32 {
    let mut checks = Checks::default();
    let ini = Ini::new("rsf_overlay_no_sr", "Anomaly");
    // A previous build saved SR on; this adapter cannot honour it.
    checks.check(
        [
            ("SR", "1"),
            ("SRBackend", "4"),
            ("SRQuality", "3"),
            ("FGBackend", "1"),
            ("StatusOverlay", "1"),
        ]
        .into_iter()
        .all(|(key, value)| ini.write(key, value)),
        "seed INI",
    );
    let mut api = rsf_overlay_api {
        struct_size: size_of::<rsf_overlay_api>() as u32,
        abi_version: RSF_OVERLAY_API_VERSION,
        ..Default::default()
    };
    checks.check(unsafe { query(&mut api) } == RSF_OK, "table");
    let (Some(configure), Some(get), Some(apply), Some(status_get)) = (
        api.configure,
        api.settings_get,
        api.settings_apply,
        api.status,
    ) else {
        checks.check(false, "required entries present");
        return 1;
    };
    let defaults = rsf_render_settings {
        struct_size: size_of::<rsf_render_settings>() as u32,
        abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
        sr_backend: 1,
        generated_frames: 1,
        fg_backend: RSF_FG_BACKEND_AUTO,
        latency: RSF_REFLEX_OFF,
        ..Default::default()
    };
    let setup = rsf_overlay_service_setup {
        struct_size: size_of::<rsf_overlay_service_setup>() as u32,
        abi_version: RSF_OVERLAY_API_VERSION,
        identity_utf8: c"no-sr fixture".as_ptr(),
        configuration_path: ini.wide.as_ptr(),
        configuration_section: ini.section.as_ptr(),
        defaults,
        latency_choices: 1 << RSF_REFLEX_OFF,
        max_generated_frames: 1,
        settings_changed: Some(settings_changed),
        ..Default::default()
    };
    let mut invalid = setup;
    invalid.defaults.sr_enabled = 1;
    checks.check(
        unsafe { configure(&invalid) } == RSF_ERROR_INVALID_ARGUMENT,
        "defaults may not enable SR when it is unavailable",
    );
    checks.check(
        unsafe { configure(&setup) } == RSF_OK,
        "configure with no SR choices",
    );

    let mut settings = rsf_render_settings {
        struct_size: size_of::<rsf_render_settings>() as u32,
        abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
        ..Default::default()
    };
    checks.check(unsafe { get(&mut settings) } == RSF_OK, "settings");
    checks.check(
        settings.sr_enabled == 0 && settings.sr_backend == 1 && settings.sr_quality == 0,
        "saved SR keys replaced by the defaults",
    );
    checks.check(
        settings.status_overlay == 1,
        "unrelated saved settings survive",
    );
    checks.check(
        settings.fg_backend == RSF_FG_BACKEND_AUTO && ini.read("FGBackend") == 1,
        "an adapter without FG normalizes its saved backend to Auto in memory",
    );
    checks.check(ini.read("SR") == 1, "loading does not rewrite the file");

    let invalid = [
        (
            rsf_render_settings {
                sr_enabled: 1,
                ..settings
            },
            "enabling SR refused",
        ),
        (
            rsf_render_settings {
                sr_backend: 3,
                ..settings
            },
            "SR backend change refused",
        ),
        (
            rsf_render_settings {
                sr_quality: 2,
                ..settings
            },
            "SR quality change refused",
        ),
    ];
    for (request, message) in &invalid {
        checks.check(
            unsafe { apply(request) } == RSF_ERROR_INVALID_ARGUMENT,
            message,
        );
    }
    checks.check(
        CHANGED.load(Ordering::Relaxed) == 0 && ini.read("SR") == 1 && ini.read("SRBackend") == 4,
        "refusals changed nothing",
    );
    let request = rsf_render_settings {
        status_overlay: 0,
        ..settings
    };
    checks.check(
        unsafe { apply(&request) } == RSF_OK && CHANGED.load(Ordering::Relaxed) == 1,
        "settings that do not involve SR still apply",
    );
    checks.check(
        ini.read("SR") == 0 && ini.read("SRBackend") == 1 && ini.read("StatusOverlay") == 0,
        "and persist SR as Off",
    );
    checks.check(
        ini.read("FGBackend") == RSF_FG_BACKEND_AUTO,
        "the next accepted write persists the normalized FG backend",
    );

    let mut status = rsf_render_status {
        struct_size: size_of::<rsf_render_status>() as u32,
        abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
        ..Default::default()
    };
    checks.check(unsafe { status_get(&mut status) } == RSF_OK, "status");
    checks.check(
        status.sr_enabled == 0
            && status.sr_requested_backend == 0
            && status.sr_effective_backend == 0
            && status.sr_backend_choices == 0,
        "status reports SR off, no backend, no choices",
    );
    // Status owns a fixed, zero-initialized array and Runtime writes a C string into it.
    let reason = unsafe { CStr::from_ptr(status.reason.as_ptr()) };
    checks.check(
        reason == c"Waiting for the first Present",
        "status reason before the first Present",
    );
    if checks.failures == 0 {
        println!("PASS: overlay DLL API contract for an adapter without super resolution");
        0
    } else {
        1
    }
}
