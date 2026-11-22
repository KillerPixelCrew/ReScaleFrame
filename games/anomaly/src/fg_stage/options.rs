// SPDX-License-Identifier: MIT
use super::Frontend;
use crate::fg_stage_abi::*;
use crate::ini;
use std::ffi::{CString, OsString};
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::sync::Mutex;
use std::{ptr, slice};
use windows_sys::Win32::Foundation::{MAX_PATH, SYSTEMTIME};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::core::{PCWSTR, w};

#[derive(Clone, Copy)]
pub(super) struct CaptureSettings {
    pub enabled: bool,
    pub start: u32,
    pub count: u32,
    pub interval: u32,
}

impl CaptureSettings {
    fn new(enabled: bool, start: u32, count: u32, interval: u32) -> Self {
        Self {
            enabled,
            start,
            count: count.min(16),
            interval: interval.max(1),
        }
    }

    pub(super) unsafe fn read(path: PCWSTR) -> Self {
        let get = |key, fallback| unsafe { ini::read_int(path, key, fallback) as u32 };
        Self::new(
            get(w!("FGCapture"), 0) != 0,
            get(w!("FGCaptureStart"), 600),
            get(w!("FGCaptureCount"), 3),
            get(w!("FGCaptureInterval"), 300),
        )
    }
}

pub(super) struct Capture {
    pub directory: CString,
    pub settings: CaptureSettings,
    pub total: u64,
    captured: u32,
    last_frame: Option<u32>,
    generation: u64,
    in_flight: bool,
}

impl Capture {
    pub(super) fn new(directory: CString, settings: CaptureSettings) -> Self {
        Self {
            directory,
            settings,
            total: 0,
            captured: 0,
            last_frame: None,
            generation: 0,
            in_flight: false,
        }
    }

    pub(super) fn configure(&mut self, settings: CaptureSettings) {
        if settings.enabled != self.settings.enabled {
            self.captured = 0;
            self.last_frame = None;
            self.generation = self.generation.wrapping_add(1);
        }
        self.settings = settings;
    }

    pub(super) fn claim(&mut self, frame: u32) -> Option<(CString, u64)> {
        if self.in_flight
            || !self.settings.enabled
            || self.captured >= self.settings.count
            || frame < self.settings.start
            || self
                .last_frame
                .is_some_and(|last| frame.wrapping_sub(last) < self.settings.interval)
        {
            return None;
        }
        self.in_flight = true;
        Some((self.directory.clone(), self.generation))
    }

    pub(super) fn finish(&mut self, frame: u32, generation: u64, success: bool) {
        self.in_flight = false;
        if success {
            self.total += 1;
            if self.generation == generation {
                self.captured += 1;
                self.last_frame = Some(frame);
            }
        }
    }
}

pub(super) struct Options {
    pub bridge: bool,
    pub debug: bool,
    pub development: bool,
    pub scope_hold: u32,
    pub capture: Mutex<Capture>,
    pub streamline: CString,
    pub fidelityfx: CString,
    pub xess: CString,
    pub frontend: Frontend,
}

impl Options {
    pub(super) fn read(ini: PCWSTR) -> Self {
        let number = |key, fallback| unsafe { ini::read_int(ini, key, fallback) as u32 };
        let enabled = number(w!("FG"), 0) != 0;
        let capture = unsafe { CaptureSettings::read(ini) };
        let native = rsf_anomaly_fg_config {
            // The panel can enable generation live only with a bridge installed at startup.
            bridge: enabled || number(w!("FGBridge"), 1) != 0,
            enabled,
            backend: number(w!("FGBackend"), RSF_FG_BACKEND_AUTO as i32),
            generated: number(w!("FGGeneratedFrames"), 1),
            latency: number(w!("Latency"), RSF_REFLEX_ON as i32),
            frame_limit_us: number(w!("FrameLimitUs"), 0),
            scope_hold: number(w!("FGScopeHold"), 4),
            debug: number(w!("FGDebug"), 0) != 0,
            development: number(w!("FGDevelopment"), 0) != 0,
            capture: capture.enabled,
            capture_start: capture.start,
            capture_count: capture.count,
            capture_interval: capture.interval,
            ..Default::default()
        };
        let ini_units = unsafe { wide(ini) };
        let ini_path = PathBuf::from(OsString::from_wide(ini_units));
        let base = ini_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""));
        let text = |key| {
            let mut buffer = [0u16; MAX_PATH as usize];
            let len = unsafe { ini::read_text(ini, key, w!(""), &mut buffer) };
            buffer[..len].to_vec()
        };
        let capture = text(w!("FGCaptureDir"));
        let capture_dir = if capture.is_empty() {
            base.join("RSF-FGCapture")
                .join(local_timestamp())
                .to_string_lossy()
                .into_owned()
        } else {
            String::from_utf16_lossy(&capture)
        };
        let vendors = crate::sr_stage::vendor_directories(ini_units, |key| {
            let mut buffer = [0u16; MAX_PATH as usize];
            let length = unsafe {
                ini::rsf_anomaly_sr_ini_text(
                    ini.cast(),
                    key.as_ptr(),
                    c"".as_ptr(),
                    buffer.as_mut_ptr(),
                    MAX_PATH,
                )
            };
            buffer[..length as usize].to_vec()
        });
        Self::admit(&native, capture_dir, vendors)
    }

    pub(super) fn from_native(native: &rsf_anomaly_fg_config) -> Self {
        let vendors = [
            native.streamline_dir,
            native.fidelityfx_dir,
            native.xess_dir,
        ]
        .map(|path| crate::sr_stage::plain_directory(unsafe { wide(path) }));
        Self::admit(
            native,
            String::from_utf16_lossy(unsafe { wide(native.capture_dir) }),
            vendors,
        )
    }

    fn admit(native: &rsf_anomaly_fg_config, capture_dir: String, vendors: [Vec<u16>; 3]) -> Self {
        let [streamline, fidelityfx, xess] =
            vendors.map(|dir| c_string(String::from_utf16_lossy(&dir)));
        let backend = native.backend;
        let known = RSF_FG_BACKEND_ALL.checked_shr(backend).unwrap_or(0) & 1 != 0;
        Self {
            bridge: native.bridge,
            debug: native.debug,
            development: native.development,
            scope_hold: native.scope_hold.clamp(1, 64),
            capture: Mutex::new(Capture::new(
                // Diagnostic output may use the original Win32 extended-length path.
                c_string(capture_dir),
                CaptureSettings::new(
                    native.capture,
                    native.capture_start,
                    native.capture_count,
                    native.capture_interval,
                ),
            )),
            streamline,
            fidelityfx,
            xess,
            frontend: Frontend {
                enabled: u32::from(native.enabled),
                backend: if known { backend } else { RSF_FG_BACKEND_AUTO },
                generated: native.generated.clamp(1, 3),
                latency: native.latency.min(RSF_REFLEX_BOOST),
                frame_limit_us: native.frame_limit_us.min(1_000_000),
            },
        }
    }
}

unsafe fn wide<'a>(value: PCWSTR) -> &'a [u16] {
    if value.is_null() {
        return &[];
    }
    let mut size = 0;
    // The carrier and fixture provide NUL-terminated Win32 strings.
    while unsafe { value.add(size).read() } != 0 {
        size += 1;
    }
    unsafe { slice::from_raw_parts(value, size) }
}

fn c_string(text: String) -> CString {
    CString::new(text).unwrap_or_default()
}

fn local_timestamp() -> String {
    let mut time = SYSTEMTIME::default();
    unsafe { GetLocalTime(ptr::from_mut(&mut time)) };
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute, time.wSecond
    )
}

pub(super) fn backend_name(backend: u32) -> &'static str {
    match backend {
        RSF_FG_BACKEND_DLSS => "DLSS-G",
        RSF_FG_BACKEND_FSR3 => "FSR3",
        RSF_FG_BACKEND_FSR4 => "FSR4",
        RSF_FG_BACKEND_XESS => "XeSS",
        RSF_FG_BACKEND_AUTO => "Auto",
        _ => "Off",
    }
}
