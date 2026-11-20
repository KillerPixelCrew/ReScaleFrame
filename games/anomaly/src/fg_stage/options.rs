// SPDX-License-Identifier: MIT
use crate::fg_stage_abi::*;
use std::ffi::{CString, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::sync::Mutex;
use std::{ptr, slice};
use windows_sys::Win32::Foundation::{MAX_PATH, SYSTEMTIME};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::System::WindowsProgramming::{
    GetPrivateProfileIntW, GetPrivateProfileStringW,
};
use windows_sys::core::{PCWSTR, w};

const SECTION: PCWSTR = w!("Anomaly");

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

    pub(super) fn read(ini: PCWSTR) -> Self {
        let get =
            |key, fallback| unsafe { GetPrivateProfileIntW(SECTION, key, fallback, ini) as u32 };
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
    pub frontend: rsf_anomaly_fg_overlay_link,
}

impl Options {
    pub(super) fn read(ini: PCWSTR) -> Self {
        let number =
            |key, fallback| unsafe { GetPrivateProfileIntW(SECTION, key, fallback, ini) as u32 };
        let enabled = number(w!("FG"), 0) != 0;
        let capture = CaptureSettings::read(ini);
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
        let ini_path = PathBuf::from(OsString::from_wide(unsafe { wide(ini) }));
        let base = ini_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""));
        let text = |key, fallback: PathBuf| {
            let mut buffer = [0u16; MAX_PATH as usize];
            let len = unsafe {
                GetPrivateProfileStringW(SECTION, key, w!(""), buffer.as_mut_ptr(), MAX_PATH, ini)
            };
            if len > 0 {
                String::from_utf16_lossy(&buffer[..len as usize])
            } else {
                String::from_utf16_lossy(&fallback.as_os_str().encode_wide().collect::<Vec<_>>())
            }
        };
        let vendors = base.join("ReScaleFrame");
        Self::admit(
            &native,
            text(
                w!("FGCaptureDir"),
                base.join("RSF-FGCapture").join(local_timestamp()),
            ),
            text(w!("SRStreamlineDir"), vendors.join("streamline")),
            text(w!("SRFidelityFXDir"), vendors.join("fidelityfx")),
            text(w!("SRXeSSDir"), vendors.join("xess")),
        )
    }

    pub(super) fn from_native(native: &rsf_anomaly_fg_config) -> Self {
        let path = |value| String::from_utf16_lossy(unsafe { wide(value) });
        Self::admit(
            native,
            path(native.capture_dir),
            path(native.streamline_dir),
            path(native.fidelityfx_dir),
            path(native.xess_dir),
        )
    }

    fn admit(
        native: &rsf_anomaly_fg_config,
        capture_dir: String,
        streamline: String,
        fidelityfx: String,
        xess: String,
    ) -> Self {
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
            streamline: c_string(plain_directory(streamline)),
            fidelityfx: c_string(plain_directory(fidelityfx)),
            xess: c_string(plain_directory(xess)),
            frontend: rsf_anomaly_fg_overlay_link {
                foreign_chain: Some(super::rsf_anomaly_fg_foreign_chain),
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

// Match sr_settings.h: Streamline appends '/' to these paths, which a verbatim Win32 path rejects.
fn plain_directory(path: String) -> String {
    let mut path = if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path
        .strip_prefix(r"\\?\")
        .or_else(|| path.strip_prefix(r"\??\"))
    {
        rest.to_owned()
    } else {
        path
    }
    .replace('/', "\\");
    let mut units = path.encode_utf16().count();
    while units > 3 && path.ends_with('\\') {
        path.pop();
        units -= 1;
    }
    path
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
