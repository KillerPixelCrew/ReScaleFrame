// SPDX-License-Identifier: MIT
use super::capture::Schedule;
use super::*;
use crate::overlay::rsf_anomaly_overlay_option_add;
use crate::overlay_abi::rsf_anomaly_overlay_option as OptionEntry;
use std::cell::UnsafeCell;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use windows_sys::Win32::System::WindowsProgramming::{
    GetPrivateProfileIntW, WritePrivateProfileStringW,
};
use windows_sys::core::w;

#[derive(Default)]
pub(super) struct Settings {
    ini: Vec<u16>,
    pub shader_dir: PathBuf,
    pub capture: Schedule,
    registered: bool,
}

impl Settings {
    fn read(&self, key: *const u16, fallback: i32) -> u32 {
        if self.ini.is_empty() {
            return fallback as u32;
        }
        unsafe { GetPrivateProfileIntW(w!("Anomaly"), key, fallback, self.ini.as_ptr()) as u32 }
    }

    fn save(&self, key: *const u16, value: i32) {
        if !self.ini.is_empty() {
            let value: Vec<u16> = value.to_string().encode_utf16().chain([0]).collect();
            unsafe {
                WritePrivateProfileStringW(w!("Anomaly"), key, value.as_ptr(), self.ini.as_ptr())
            };
        }
    }
}

impl Stage {
    pub(super) fn configure(&self, ini: *const u16, log: Log) {
        let _lifecycle = lock(&self.lifecycle);
        if self.active.load(Acquire) {
            return;
        }
        let mut settings = lock(&self.settings);
        if !ini.is_null() {
            let mut len = 0;
            while unsafe { ini.add(len).read() } != 0 {
                len += 1;
            }
            let units = unsafe { slice::from_raw_parts(ini, len) };
            let path = PathBuf::from(OsString::from_wide(units));
            settings.shader_dir = path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join("ReScaleFrame")
                .join("patches");
            settings.ini = units.iter().copied().chain([0]).collect();
        }
        settings.capture = Schedule {
            enabled: settings.read(w!("MotionCapture"), 0) != 0,
            start: settings.read(w!("MotionCaptureStart"), 600),
            count: settings.read(w!("MotionCaptureCount"), 3).min(16),
            interval: settings.read(w!("MotionCaptureInterval"), 300).max(1),
        };
        self.enabled
            .store(settings.read(w!("Motion"), 0) == 1, Relaxed);
        self.foliage
            .store(settings.read(w!("MotionFoliage"), 1) != 0, Relaxed);
        self.invalidate
            .store(settings.read(w!("MotionInvalidate"), 1) != 0, Relaxed);
        *lock(&self.logger) = log;
        if !settings.registered {
            register();
            settings.registered = true;
        }
    }
}

struct Label(UnsafeCell<[u8; 96]>);
// The existing overlay contract calls get and copies its label on the Present
// owner thread. Startup registration and overlay stop bound that access.
unsafe impl Sync for Label {}
static PACK_LABEL: Label = Label(UnsafeCell::new({
    let text = b"Motion shader pack: not loaded";
    let mut label = [0; 96];
    let mut i = 0;
    while i < text.len() {
        label[i] = text[i];
        i += 1;
    }
    label
}));

type Get = unsafe extern "C" fn() -> i32;
type Set = unsafe extern "C" fn(i32);

fn entry(label: *const c_char, bound: i32, restart: bool, get: Get, set: Set) {
    let option = OptionEntry {
        label,
        toggle: (bound == 1).into(),
        restart: restart.into(),
        minimum: 0,
        maximum: bound,
        step: 1,
        divisor: 1,
        get: Some(get),
        set: Some(set),
    };
    unsafe { rsf_anomaly_overlay_option_add(&option) };
}

fn register() {
    entry(PACK_LABEL.0.get().cast(), 1, false, pack_get, pack_set);
    entry(
        c"Reload motion shader pack".as_ptr(),
        1,
        false,
        zero,
        reload,
    );
    entry(c"Dump intercepted shaders".as_ptr(), 1, false, zero, dump);
    entry(
        c"Tree and grass motion".as_ptr(),
        1,
        false,
        foliage_get,
        foliage_set,
    );
    entry(
        c"Invalidate motion under stock draws".as_ptr(),
        1,
        false,
        invalidate_get,
        invalidate_set,
    );
    entry(
        c"Motion target capture".as_ptr(),
        1,
        true,
        capture_get,
        capture_set,
    );
    entry(
        c"Motion capture start (main frames)".as_ptr(),
        i32::MAX,
        true,
        start_get,
        start_set,
    );
    entry(
        c"Motion capture count".as_ptr(),
        16,
        true,
        count_get,
        count_set,
    );
    entry(
        c"Motion capture interval (main frames)".as_ptr(),
        i32::MAX,
        true,
        interval_get,
        interval_set,
    );
}

unsafe extern "C" fn pack_get() -> i32 {
    let stage = STAGE.get().unwrap();
    let status = stage.pack_status.load(Relaxed);
    let refused = if status == 1 {
        lock(&stage.loader)
            .pack
            .as_ref()
            .map_or(0, |pack| pack.counters(stage.platform).refused)
    } else {
        0
    };
    let label = match status {
        1 if refused != 0 => format!("Motion shader pack: loaded ({refused} refusals)"),
        1 => "Motion shader pack: loaded".into(),
        2 => "Motion shader pack: reload refused; previous pack active".into(),
        _ => "Motion shader pack: not loaded".into(),
    };
    unsafe {
        let buffer = &mut *PACK_LABEL.0.get();
        buffer[..label.len()].copy_from_slice(label.as_bytes());
        buffer[label.len()] = 0;
    }
    stage.enabled.load(Relaxed).into()
}

unsafe extern "C" fn pack_set(value: i32) {
    let stage = STAGE.get().unwrap();
    stage.enabled.store(value != 0, Relaxed);
    lock(&stage.settings).save(w!("Motion"), (value != 0).into());
}
unsafe extern "C" fn zero() -> i32 {
    0
}
unsafe extern "C" fn reload(value: i32) {
    if value != 0 {
        STAGE.get().unwrap().reload.store(true, Relaxed);
    }
}
unsafe extern "C" fn dump(value: i32) {
    if value != 0 {
        STAGE.get().unwrap().dump_shaders.store(true, Relaxed);
    }
}
unsafe extern "C" fn foliage_get() -> i32 {
    STAGE.get().unwrap().foliage.load(Relaxed).into()
}
unsafe extern "C" fn foliage_set(value: i32) {
    let stage = STAGE.get().unwrap();
    let enabled = value != 0;
    stage.foliage.store(enabled, Relaxed);
    if stage.active.load(Acquire) {
        unsafe { (stage.platform.foliage_identity)(enabled) };
    }
    lock(&stage.settings).save(w!("MotionFoliage"), enabled.into());
}
unsafe extern "C" fn invalidate_get() -> i32 {
    STAGE.get().unwrap().invalidate.load(Relaxed).into()
}
unsafe extern "C" fn invalidate_set(value: i32) {
    let stage = STAGE.get().unwrap();
    stage.invalidate.store(value != 0, Relaxed);
    lock(&stage.settings).save(w!("MotionInvalidate"), (value != 0).into());
}

// These controls persist a new schedule for the next startup. The current
// capture owns the Runtime dump service for the whole observation session.
fn read(key: *const u16, fallback: i32) -> i32 {
    lock(&STAGE.get().unwrap().settings)
        .read(key, fallback)
        .min(i32::MAX as u32) as i32
}
fn save(key: *const u16, value: i32) {
    lock(&STAGE.get().unwrap().settings).save(key, value);
}
unsafe extern "C" fn capture_get() -> i32 {
    (read(w!("MotionCapture"), 0) != 0).into()
}
unsafe extern "C" fn capture_set(value: i32) {
    save(w!("MotionCapture"), (value != 0).into());
}
unsafe extern "C" fn start_get() -> i32 {
    read(w!("MotionCaptureStart"), 600)
}
unsafe extern "C" fn start_set(value: i32) {
    save(w!("MotionCaptureStart"), value.max(0));
}
unsafe extern "C" fn count_get() -> i32 {
    read(w!("MotionCaptureCount"), 3).min(16)
}
unsafe extern "C" fn count_set(value: i32) {
    save(w!("MotionCaptureCount"), value.clamp(0, 16));
}
unsafe extern "C" fn interval_get() -> i32 {
    read(w!("MotionCaptureInterval"), 300).max(1)
}
unsafe extern "C" fn interval_set(value: i32) {
    save(w!("MotionCaptureInterval"), value.max(1));
}
