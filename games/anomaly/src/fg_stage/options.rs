// SPDX-License-Identifier: MIT
use super::Frontend;
use crate::fg_stage_abi::{
    RSF_FG_BACKEND_ALL, RSF_FG_BACKEND_AUTO, RSF_FG_BACKEND_DLSS, RSF_FG_BACKEND_FSR3,
    RSF_FG_BACKEND_FSR4, RSF_FG_BACKEND_XESS, RSF_REFLEX_BOOST, RSF_REFLEX_ON,
};
use crate::ini;
use std::ffi::{CString, OsString};
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::slice;
use std::sync::Mutex;
use windows_sys::Win32::Foundation::MAX_PATH;
use windows_sys::core::{PCWSTR, w};

#[derive(Clone, Copy)]
pub(crate) struct CaptureSettings {
    pub enabled: bool,
    pub start: u32,
    pub count: u32,
    pub interval: u32,
}

impl CaptureSettings {
    pub(super) fn new(enabled: bool, start: u32, count: u32, interval: u32) -> Self {
        Self {
            enabled,
            start: start.min(i32::MAX as u32),
            count: count.min(16),
            interval: interval.clamp(1, i32::MAX as u32),
        }
    }

    /// Uses the nonnegative ranges representable by the overlay's signed
    /// controls. Keep the unsigned INI interpretation before clamping.
    pub(crate) fn decode(mut read: impl FnMut(PCWSTR, i32) -> i32) -> Self {
        let mut get = |key, fallback| read(key, fallback) as u32;
        Self::new(
            get(w!("FGCapture"), 0) != 0,
            get(w!("FGCaptureStart"), 600),
            get(w!("FGCaptureCount"), 3),
            get(w!("FGCaptureInterval"), 300),
        )
    }

    pub(super) unsafe fn read(path: PCWSTR) -> Self {
        Self::decode(|key, fallback| unsafe { ini::read_int(path, key, fallback) })
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

pub(super) struct Settings {
    pub bridge: bool,
    pub debug: bool,
    pub development: bool,
    pub scope_hold: u32,
    pub capture: CaptureSettings,
    pub frontend: Frontend,
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
        let settings = Settings {
            // The panel can enable generation live only with a bridge installed at startup.
            bridge: enabled || number(w!("FGBridge"), 1) != 0,
            scope_hold: number(w!("FGScopeHold"), 4),
            debug: number(w!("FGDebug"), 0) != 0,
            development: number(w!("FGDevelopment"), 0) != 0,
            capture,
            frontend: Frontend {
                enabled: u32::from(enabled),
                backend: number(w!("FGBackend"), RSF_FG_BACKEND_AUTO as i32),
                generated: number(w!("FGGeneratedFrames"), 1),
                latency: number(w!("Latency"), RSF_REFLEX_ON as i32),
                frame_limit_us: number(w!("FrameLimitUs"), 0),
            },
        };
        let ini_units = unsafe { crate::wide::terminated(ini) }.map_or(&[][..], |(units, _)| units);
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
                .join(crate::capture_clock::local_timestamp())
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
        Self::admit(settings, capture_dir, vendors)
    }

    pub(super) fn admit(settings: Settings, capture_dir: String, vendors: [Vec<u16>; 3]) -> Self {
        let [streamline, fidelityfx, xess] =
            vendors.map(|dir| c_string(String::from_utf16_lossy(&dir)));
        let Frontend {
            enabled,
            backend,
            generated,
            latency,
            frame_limit_us,
        } = settings.frontend;
        let known = RSF_FG_BACKEND_ALL.checked_shr(backend).unwrap_or(0) & 1 != 0;
        Self {
            bridge: settings.bridge,
            debug: settings.debug,
            development: settings.development,
            scope_hold: settings.scope_hold.clamp(1, 64),
            // Diagnostic output may use the original Win32 extended-length path.
            capture: Mutex::new(Capture::new(c_string(capture_dir), settings.capture)),
            streamline,
            fidelityfx,
            xess,
            frontend: Frontend {
                enabled,
                backend: if known { backend } else { RSF_FG_BACKEND_AUTO },
                generated: generated.clamp(1, 3),
                latency: latency.min(RSF_REFLEX_BOOST),
                frame_limit_us: frame_limit_us.min(1_000_000),
            },
        }
    }
}

fn c_string(text: String) -> CString {
    CString::new(text).unwrap_or_default()
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

#[cfg(test)]
mod capture_settings_tests {
    use super::*;

    #[test]
    fn persisted_capture_settings_fit_the_panel_ranges() {
        let defaults = CaptureSettings::decode(|_, fallback| fallback);
        assert_eq!(
            (defaults.start, defaults.count, defaults.interval),
            (600, 3, 300)
        );

        let large = CaptureSettings::decode(|_, _| -1);
        assert!(large.enabled);
        assert_eq!(
            (large.start, large.count, large.interval),
            (i32::MAX as u32, 16, i32::MAX as u32)
        );
        let zero = CaptureSettings::decode(|_, _| 0);
        assert_eq!((zero.start, zero.count, zero.interval), (0, 0, 1));
    }
}
