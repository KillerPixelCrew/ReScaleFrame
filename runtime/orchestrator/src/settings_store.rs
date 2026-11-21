// SPDX-License-Identifier: MIT
//! Shared persistence for rsf_render_settings in an adapter-supplied INI section.
//!
//! The key names and text formats belong to the render frontend; the file and section always
//! come from the adapter. Range checks and sanitization stay with the public
//! sharpening_settings.h helpers that the C++ facade applies around these calls.
use crate::render_abi::{
    RSF_RENDER_FRONTEND_ABI_VERSION, RSF_SHARPENING_ABI_VERSION, rsf_render_settings as Settings,
    rsf_sharpening_options as Sharpening,
};
use std::ptr::null;
use windows_sys::Win32::System::WindowsProgramming::{
    GetPrivateProfileIntW, GetPrivateProfileSectionW, GetPrivateProfileStringW,
    WritePrivateProfileSectionW, WritePrivateProfileStringW,
};
use windows_sys::core::PCWSTR;

const OK: i32 = 0;
const INVALID_ARGUMENT: i32 = -1;
const NOT_READY: i32 = -3;

// The section buffer the store has always read. A larger section is refused rather than truncated.
const SECTION_CAPACITY: usize = 32768;
// Decimal values longer than this were never produced by the store and read back as a fallback.
const NUMBER_CAPACITY: usize = 80;

// The overlay's panel layout word (rsf_overlay_stats::ui_preferences). Opaque here: only the
// overlay interprets it. A separate key rather than a field of rsf_render_settings, which other
// ABIs embed; a settings write keeps it because it is not an owned key.
const OVERLAY_LAYOUT: &str = "OverlayLayout";
const DLSS_PRESET: &str = "DLSSPreset";

#[derive(Clone, Copy)]
enum Value {
    Integer(u32),
    Decimal(f32),
}

// Owned keys in file order. The last twelve are the shared post-pass keys that callback hosts
// persist on their own.
const KEYS: [&str; 22] = [
    "SR",
    "SRBackend",
    DLSS_PRESET,
    "FG",
    "FGBackend",
    "FGGeneratedFrames",
    "Latency",
    "FrameLimitUs",
    "StatusOverlay",
    "SRQuality",
    "SharpeningMode",
    "Sharpness",
    "MAS",
    "MotionSharpness",
    "MotionThreshold",
    "MotionScaleLimit",
    "ContrastEnabled",
    "Contrast",
    "DADepthScale",
    "DADepthBias",
    "DAClampOutput",
    "SharpeningDebug",
];
const SHARPENING_START: usize = 10;

fn settings_values(s: &Settings) -> [Value; 22] {
    use Value::{Decimal as D, Integer as I};
    [
        I(s.sr_enabled),
        I(s.sr_backend),
        I(s.dlss_preset),
        I(s.fg_enabled),
        I(s.fg_backend),
        I(s.generated_frames),
        I(s.latency),
        I(s.frame_limit_us),
        I(s.status_overlay),
        I(s.sr_quality),
        I(s.sharpening_mode),
        D(s.sharpening_sharpness),
        I(s.sharpening_mas),
        D(s.sharpening_motion_sharpness),
        D(s.sharpening_motion_threshold),
        D(s.sharpening_motion_scale_limit),
        I(s.sharpening_contrast_enabled),
        D(s.sharpening_contrast),
        D(s.sharpening_depth_scale),
        D(s.sharpening_depth_bias),
        I(s.sharpening_clamp_output),
        I(s.sharpening_debug),
    ]
}

fn sharpening_values(o: &Sharpening) -> [Value; 12] {
    use Value::{Decimal as D, Integer as I};
    [
        I(o.mode),
        D(o.sharpness),
        I(o.mas),
        D(o.motion_sharpness),
        D(o.motion_threshold),
        D(o.motion_scale_limit),
        I(o.contrast_enabled),
        D(o.contrast),
        D(o.depth_scale),
        D(o.depth_bias),
        I(o.clamp_output),
        I(o.debug),
    ]
}

/// The shared post-pass fields of `s` over `base`, which supplies the header and ABI.
pub(crate) fn sharpening_of(s: &Settings, base: &Sharpening) -> Sharpening {
    Sharpening {
        mode: s.sharpening_mode,
        sharpness: s.sharpening_sharpness,
        mas: s.sharpening_mas,
        motion_sharpness: s.sharpening_motion_sharpness,
        motion_threshold: s.sharpening_motion_threshold,
        motion_scale_limit: s.sharpening_motion_scale_limit,
        contrast_enabled: s.sharpening_contrast_enabled,
        contrast: s.sharpening_contrast,
        depth_scale: s.sharpening_depth_scale,
        depth_bias: s.sharpening_depth_bias,
        clamp_output: s.sharpening_clamp_output,
        debug: s.sharpening_debug,
        ..*base
    }
}

pub(crate) fn set_sharpening(s: &mut Settings, o: &Sharpening) {
    s.sharpening_mode = o.mode;
    s.sharpening_sharpness = o.sharpness;
    s.sharpening_mas = o.mas;
    s.sharpening_motion_sharpness = o.motion_sharpness;
    s.sharpening_motion_threshold = o.motion_threshold;
    s.sharpening_motion_scale_limit = o.motion_scale_limit;
    s.sharpening_contrast_enabled = o.contrast_enabled;
    s.sharpening_contrast = o.contrast;
    s.sharpening_depth_scale = o.depth_scale;
    s.sharpening_depth_bias = o.depth_bias;
    s.sharpening_clamp_output = o.clamp_output;
    s.sharpening_debug = o.debug;
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe extern "C" {
    // The CRT's "%.9g" through a C++ leaf, so locale, rounding and inf/nan spellings are the CRT's.
    fn rsf_settings_store_format_decimal(value: f32, text: *mut u16, capacity: usize) -> i32;
    // The CRT parser the store has always used, so locale, hex, inf and nan spellings match.
    fn wcstof(text: *const u16, end: *mut *mut u16) -> f32;
}

// Terminated UTF-16 text. "%.9g" of a float needs at most 16 characters.
fn decimal(value: f32) -> Vec<u16> {
    let mut text = [0u16; 32];
    let length = unsafe { rsf_settings_store_format_decimal(value, text.as_mut_ptr(), text.len()) };
    // Unreachable for a float; an empty value would read back as the key's fallback.
    usize::try_from(length).map_or_else(|_| vec![0], |length| text[..=length].to_vec())
}

fn text(value: Value) -> Vec<u16> {
    match value {
        Value::Integer(value) => wide(&value.to_string()),
        Value::Decimal(value) => decimal(value),
    }
}

struct File {
    ini: PCWSTR,
    section: PCWSTR,
}

impl File {
    fn new(ini: PCWSTR, section: PCWSTR) -> Option<Self> {
        (!ini.is_null() && !section.is_null()).then_some(Self { ini, section })
    }

    fn integer(&self, key: &str, fallback: u32) -> u32 {
        let key = wide(key);
        unsafe {
            GetPrivateProfileIntW(self.section, key.as_ptr(), fallback as i32, self.ini) as u32
        }
    }

    // A value that does not parse completely, including an empty or missing one, is the fallback.
    fn decimal(&self, key: &str, fallback: f32) -> f32 {
        let key = wide(key);
        let mut buffer = [0u16; NUMBER_CAPACITY];
        let empty = [0u16];
        unsafe {
            GetPrivateProfileStringW(
                self.section,
                key.as_ptr(),
                empty.as_ptr(),
                buffer.as_mut_ptr(),
                NUMBER_CAPACITY as u32,
                self.ini,
            )
        };
        let mut end = std::ptr::null_mut();
        let parsed = unsafe { wcstof(buffer.as_ptr(), &mut end) };
        if !std::ptr::eq(end, buffer.as_ptr()) && unsafe { *end } == 0 {
            parsed
        } else {
            fallback
        }
    }

    fn write(&self, key: &[u16], value: PCWSTR) -> bool {
        unsafe { WritePrivateProfileStringW(self.section, key.as_ptr(), value, self.ini) != 0 }
    }

    // The stored value, or None when the key is absent. Two distinct defaults tell an absent key
    // from a stored one without parsing the section.
    fn previous(&self, key: &[u16]) -> Option<Vec<u16>> {
        let mut capacity = NUMBER_CAPACITY;
        loop {
            let read = |default: &[u16]| {
                let mut buffer = vec![0u16; capacity];
                let length = unsafe {
                    GetPrivateProfileStringW(
                        self.section,
                        key.as_ptr(),
                        default.as_ptr(),
                        buffer.as_mut_ptr(),
                        capacity as u32,
                        self.ini,
                    )
                } as usize;
                buffer.truncate(length);
                buffer
            };
            let first = read(&[1, u16::from(b'a'), 0]);
            if first.len() + 1 >= capacity && capacity < SECTION_CAPACITY {
                capacity *= 4;
                continue;
            }
            let second = read(&[1, u16::from(b'b'), 0]);
            if first != second {
                return None;
            }
            let mut value = first;
            value.push(0);
            return Some(value);
        }
    }
}

// The shared post-pass keys fall back to `sharpening` (the facade passes rsf_sharpening_defaults),
// not to the host defaults, because existing host aggregate initializers leave those fields zero.
fn read(file: &File, defaults: &Settings, sharpening: &Sharpening) -> Settings {
    read_with_fg_mode(file, defaults, sharpening).0
}

// Also returns the stored FG value before it is reduced to `fg_enabled`. Native FG owners take it
// as their rsf_fg_mode, so a saved 2 (Auto) or 3 (Dynamic) still reaches them.
fn read_with_fg_mode(file: &File, defaults: &Settings, sharpening: &Sharpening) -> (Settings, u32) {
    let value = |key, fallback| file.integer(key, fallback);
    let number = |key, fallback| file.decimal(key, fallback);
    let fg_mode = value("FG", defaults.fg_enabled);
    let settings = Settings {
        struct_size: size_of::<Settings>() as u32,
        abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
        sr_enabled: u32::from(value("SR", defaults.sr_enabled) != 0),
        sr_backend: value("SRBackend", defaults.sr_backend),
        dlss_preset: value(DLSS_PRESET, defaults.dlss_preset),
        fg_enabled: u32::from(fg_mode != 0),
        fg_backend: value("FGBackend", defaults.fg_backend),
        generated_frames: value("FGGeneratedFrames", defaults.generated_frames),
        latency: value("Latency", defaults.latency),
        frame_limit_us: value("FrameLimitUs", defaults.frame_limit_us),
        status_overlay: u32::from(value("StatusOverlay", defaults.status_overlay) != 0),
        sr_quality: value("SRQuality", defaults.sr_quality),
        sharpening_mode: value("SharpeningMode", sharpening.mode),
        sharpening_sharpness: number("Sharpness", sharpening.sharpness),
        sharpening_mas: value("MAS", sharpening.mas),
        sharpening_motion_sharpness: number("MotionSharpness", sharpening.motion_sharpness),
        sharpening_motion_threshold: number("MotionThreshold", sharpening.motion_threshold),
        sharpening_motion_scale_limit: number("MotionScaleLimit", sharpening.motion_scale_limit),
        sharpening_contrast_enabled: value("ContrastEnabled", sharpening.contrast_enabled),
        sharpening_contrast: number("Contrast", sharpening.contrast),
        sharpening_depth_scale: number("DADepthScale", sharpening.depth_scale),
        sharpening_depth_bias: number("DADepthBias", sharpening.depth_bias),
        sharpening_clamp_output: value("DAClampOutput", sharpening.clamp_output),
        sharpening_debug: value("SharpeningDebug", sharpening.debug),
    };
    (settings, fg_mode)
}

/// One read of the owned keys, as `rsf_settings_store_read`, plus the stored FG value.
///
/// # Safety
/// `ini` and `section` are terminated UTF-16 strings; `sharpening` is a full options structure.
pub(crate) unsafe fn read_settings_and_fg_mode(
    ini: PCWSTR,
    section: PCWSTR,
    defaults: &Settings,
    sharpening: &Sharpening,
) -> Option<(Settings, u32)> {
    let file = File::new(ini, section)?;
    let sharpening = unsafe { sharpening_options(sharpening) }?;
    Some(read_with_fg_mode(&file, defaults, sharpening))
}

fn owned(entry: &[u16]) -> bool {
    KEYS.iter().any(|key| {
        let key = key.as_bytes();
        entry.len() > key.len()
            && entry[key.len()] == u16::from(b'=')
            && entry
                .iter()
                .zip(key)
                .all(|(&c, &k)| u8::try_from(c).is_ok_and(|c| c.eq_ignore_ascii_case(&k)))
    })
}

// Rewrites the owned keys in one section write and keeps every other entry of the section, so a
// failed write leaves the previous section intact.
fn write(file: &File, request: &Settings) -> i32 {
    let mut buffer = vec![0u16; SECTION_CAPACITY];
    let length = unsafe {
        GetPrivateProfileSectionW(
            file.section,
            buffer.as_mut_ptr(),
            SECTION_CAPACITY as u32,
            file.ini,
        )
    } as usize;
    if length >= SECTION_CAPACITY - 2 {
        return INVALID_ARGUMENT;
    }
    let mut updated = Vec::with_capacity(length + 1024);
    for entry in buffer[..length].split(|&c| c == 0) {
        if !entry.is_empty() && !owned(entry) {
            updated.extend_from_slice(entry);
            updated.push(0);
        }
    }
    for (key, value) in KEYS.iter().zip(settings_values(request)) {
        updated.extend(key.encode_utf16());
        updated.push(u16::from(b'='));
        updated.extend(text(value));
    }
    updated.push(0);
    if unsafe { WritePrivateProfileSectionW(file.section, updated.as_ptr(), file.ini) } != 0 {
        OK
    } else {
        NOT_READY
    }
}

// Writes each shared post-pass key in place. When one write fails, the keys already written are
// restored to their previous values, or removed when they were absent, before reporting failure.
fn write_sharpening(
    file: &File,
    options: &Sharpening,
    store: &mut dyn FnMut(&File, &[u16], PCWSTR) -> bool,
) -> i32 {
    let keys: Vec<Vec<u16>> = KEYS[SHARPENING_START..]
        .iter()
        .map(|key| wide(key))
        .collect();
    let previous: Vec<Option<Vec<u16>>> = keys.iter().map(|key| file.previous(key)).collect();
    for (index, (key, value)) in keys.iter().zip(sharpening_values(options)).enumerate() {
        let value = text(value);
        if !store(file, key, value.as_ptr()) {
            for (key, old) in keys[..=index].iter().zip(&previous) {
                let old = old.as_ref().map_or(null(), |old| old.as_ptr());
                let _ = file.write(key, old);
            }
            return NOT_READY;
        }
    }
    OK
}

// A full rsf_sharpening_options of this ABI, or None. Only the leading struct_size is read until
// it shows the whole structure is present.
unsafe fn sharpening_options<'a>(options: *const Sharpening) -> Option<&'a Sharpening> {
    if options.is_null() {
        return None;
    }
    let size = unsafe { options.cast::<u32>().read() } as usize;
    if size < size_of::<Sharpening>() {
        return None;
    }
    let options = unsafe { &*options };
    (options.abi_version == RSF_SHARPENING_ABI_VERSION).then_some(options)
}

/// Reads the owned keys. Missing frontend values take `defaults`, missing post-pass values take
/// `sharpening`.
///
/// # Safety
/// `ini` and `section` are terminated UTF-16 strings, `defaults` and `output` are valid for
/// their full rsf_render_settings size, and `sharpening` is readable for its `struct_size`.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_settings_store_read(
    ini: PCWSTR,
    section: PCWSTR,
    defaults: *const Settings,
    sharpening: *const Sharpening,
    output: *mut Settings,
) -> i32 {
    let Some(file) = File::new(ini, section) else {
        return INVALID_ARGUMENT;
    };
    let Some(sharpening) = (unsafe { sharpening_options(sharpening) }) else {
        return INVALID_ARGUMENT;
    };
    if defaults.is_null() || output.is_null() {
        return INVALID_ARGUMENT;
    }
    let value = read(&file, unsafe { &*defaults }, sharpening);
    unsafe { output.write(value) };
    OK
}

/// # Safety
/// `ini` and `section` are terminated UTF-16 strings; `request` is a full rsf_render_settings.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_settings_store_write(
    ini: PCWSTR,
    section: PCWSTR,
    request: *const Settings,
) -> i32 {
    match (File::new(ini, section), unsafe { request.as_ref() }) {
        (Some(file), Some(request)) => write(&file, request),
        _ => INVALID_ARGUMENT,
    }
}

/// # Safety
/// `ini` and `section` are terminated UTF-16 strings.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_settings_store_read_layout(
    ini: PCWSTR,
    section: PCWSTR,
) -> u32 {
    File::new(ini, section).map_or(0, |file| file.integer(OVERLAY_LAYOUT, 0))
}

/// # Safety
/// `ini` and `section` are terminated UTF-16 strings.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_settings_store_write_layout(
    ini: PCWSTR,
    section: PCWSTR,
    value: u32,
) -> i32 {
    let Some(file) = File::new(ini, section) else {
        return INVALID_ARGUMENT;
    };
    let value = wide(&value.to_string());
    if file.write(&wide(OVERLAY_LAYOUT), value.as_ptr()) {
        OK
    } else {
        NOT_READY
    }
}

/// Persists only the shared post-pass keys for hosts that own their base SR settings.
/// The caller has range-checked `options`.
///
/// # Safety
/// `ini` and `section` are terminated UTF-16 strings; `options` is readable for `struct_size`.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_settings_store_write_sharpening(
    ini: PCWSTR,
    section: PCWSTR,
    options: *const Sharpening,
) -> i32 {
    let Some(file) = File::new(ini, section) else {
        return INVALID_ARGUMENT;
    };
    let Some(options) = (unsafe { sharpening_options(options) }) else {
        return INVALID_ARGUMENT;
    };
    write_sharpening(&file, options, &mut |file, key, value| {
        file.write(key, value)
    })
}

/// Reads only the DLSS preset key for hosts that own their other SR settings.
///
/// # Safety
/// `ini` and `section` are terminated UTF-16 strings.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_settings_store_read_dlss_preset(
    ini: PCWSTR,
    section: PCWSTR,
    fallback: u32,
) -> u32 {
    File::new(ini, section).map_or(fallback, |file| file.integer(DLSS_PRESET, fallback))
}

/// Persists only the DLSS preset key for hosts that own their other SR settings. The caller has
/// validated `preset`.
///
/// # Safety
/// `ini` and `section` are terminated UTF-16 strings.
#[unsafe(export_name = "rsf_settings_store_write_dlss_preset")]
pub(crate) unsafe extern "C" fn write_dlss_preset(
    ini: PCWSTR,
    section: PCWSTR,
    preset: u32,
) -> i32 {
    let Some(file) = File::new(ini, section) else {
        return INVALID_ARGUMENT;
    };
    let value = text(Value::Integer(preset));
    if file.write(&wide(DLSS_PRESET), value.as_ptr()) {
        OK
    } else {
        NOT_READY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_matches_general_nine_digit_format() {
        // Smoke cases for the CRT leaf; the native fixture compares it with swprintf at scale.
        let cases: [(f32, &str); 17] = [
            (0.4, "0.400000006"),
            (0.2, "0.200000003"),
            (0.001, "0.00100000005"),
            (0.0, "0"),
            (-0.0, "-0"),
            (10.0, "10"),
            (35.0, "35"),
            (600.0, "600"),
            (-0.17, "-0.170000002"),
            (1e-5, "9.99999975e-06"),
            (1e10, "1e+10"),
            (123456792.0, "123456792"),
            (1234567936.0, "1.23456794e+09"),
            (1.0 + 1.0 / 512.0, "1.00195312"),
            (f32::INFINITY, "inf"),
            (f32::NAN, "nan"),
            (-f32::NAN, "-nan(ind)"),
        ];
        for (value, expected) in cases {
            assert_eq!(decimal(value), wide(expected), "{value:e}");
        }
    }

    #[test]
    fn owned_keys_match_case_insensitively_at_equals() {
        let entry = |text: &str| text.encode_utf16().collect::<Vec<_>>();
        assert!(owned(&entry("sharpness=0.5")));
        assert!(owned(&entry("SR=1")));
        assert!(!owned(&entry("SRX=1")));
        assert!(!owned(&entry("OverlayLayout=3")));
        assert!(!owned(&entry("SR")));
    }

    struct Temporary(Vec<u16>);
    impl Temporary {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "rsf-settings-store-{}-{name}.ini",
                std::process::id()
            ));
            let _ = std::fs::remove_file(&path);
            Self(wide(path.to_str().unwrap()))
        }
        fn contents(&self) -> String {
            let path = String::from_utf16_lossy(&self.0[..self.0.len() - 1]);
            std::fs::read(path)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_default()
        }
    }
    impl Drop for Temporary {
        fn drop(&mut self) {
            let path = String::from_utf16_lossy(&self.0[..self.0.len() - 1]);
            let _ = std::fs::remove_file(path);
        }
    }

    fn sharpening() -> Sharpening {
        Sharpening {
            struct_size: size_of::<Sharpening>() as u32,
            abi_version: RSF_SHARPENING_ABI_VERSION,
            mode: 3,
            sharpness: 0.64,
            mas: 1,
            motion_sharpness: -0.1,
            motion_threshold: 2.0,
            motion_scale_limit: 17.0,
            contrast_enabled: 1,
            contrast: 1.3,
            depth_scale: 105.0,
            depth_bias: 0.0037,
            clamp_output: 1,
            debug: 1,
        }
    }

    #[test]
    fn failed_sharpening_write_restores_previous_keys() {
        let ini = Temporary::new("rollback");
        let section = wide("Rendering");
        let file = File::new(ini.0.as_ptr(), section.as_ptr()).unwrap();
        assert!(file.write(&wide("EngineQuality"), wide("untouched").as_ptr()));
        assert!(file.write(&wide("Sharpness"), wide("0.25").as_ptr()));
        assert!(file.write(&wide("MAS"), wide("0").as_ptr()));
        let before = ini.contents();
        for failing in 0..12 {
            let mut calls = 0;
            let result = write_sharpening(&file, &sharpening(), &mut |file, key, value| {
                calls += 1;
                calls <= failing && file.write(key, value)
            });
            assert_eq!(result, NOT_READY);
            assert_eq!(ini.contents(), before, "failure at write {failing}");
        }
        assert_eq!(
            write_sharpening(&file, &sharpening(), &mut |file, key, value| file
                .write(key, value)),
            OK
        );
        let empty = Settings {
            struct_size: size_of::<Settings>() as u32,
            abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
            ..unsafe { std::mem::zeroed() }
        };
        let read = read(&file, &empty, &sharpening());
        assert_eq!(read.sharpening_mode, 3);
        assert_eq!(read.sharpening_sharpness, 0.64);
        assert_eq!(read.sharpening_depth_bias, 0.0037);
        assert_eq!(read.sharpening_debug, 1);
        assert!(ini.contents().contains("EngineQuality=untouched"));
    }

    #[test]
    fn settings_write_replaces_owned_keys_and_keeps_others() {
        let ini = Temporary::new("section");
        let section = wide("Adapter");
        let file = File::new(ini.0.as_ptr(), section.as_ptr()).unwrap();
        assert!(file.write(&wide("Foreign"), wide("kept").as_ptr()));
        assert!(file.write(&wide("sr"), wide("0").as_ptr()));
        assert!(file.write(&wide(OVERLAY_LAYOUT), wide("5").as_ptr()));
        let mut defaults: Settings = unsafe { std::mem::zeroed() };
        defaults.sr_enabled = 1;
        defaults.sr_backend = 7;
        defaults.generated_frames = 1;
        // Missing post-pass keys take the caller's sharpening fallbacks.
        let fallback = Sharpening {
            sharpness: 0.55,
            depth_scale: 77.0,
            ..sharpening()
        };
        let mut request = read(&file, &defaults, &fallback);
        assert_eq!(request.sr_enabled, 0);
        assert_eq!(request.sr_backend, 7);
        assert_eq!(request.sharpening_mode, 3);
        assert_eq!(request.sharpening_sharpness, 0.55);
        assert_eq!(request.sharpening_depth_scale, 77.0);
        request.sr_enabled = 1;
        request.generated_frames = 3;
        request.sharpening_contrast = 1.1;
        assert_eq!(write(&file, &request), OK);
        let contents = ini.contents();
        assert!(contents.contains("Foreign=kept"));
        assert!(contents.contains("OverlayLayout=5"));
        assert!(!contents.contains("sr=0"));
        assert!(contents.contains("Contrast=1.10000002"));
        let saved = read(&file, &defaults, &fallback);
        assert_eq!(saved.sr_enabled, 1);
        assert_eq!(saved.generated_frames, 3);
        assert_eq!(saved.sharpening_contrast, 1.1);
        assert_eq!(file.integer(OVERLAY_LAYOUT, 0), 5);
    }

    #[test]
    fn fg_mode_keeps_the_stored_value_beside_the_boolean() {
        let ini = Temporary::new("fg-mode");
        let section = wide("Adapter");
        let file = File::new(ini.0.as_ptr(), section.as_ptr()).unwrap();
        let defaults: Settings = unsafe { std::mem::zeroed() };
        for (stored, mode) in [("3", 3), ("2", 2), ("1", 1), ("0", 0)] {
            assert!(file.write(&wide("FG"), wide(stored).as_ptr()));
            let (settings, fg_mode) = read_with_fg_mode(&file, &defaults, &sharpening());
            assert_eq!(fg_mode, mode);
            assert_eq!(settings.fg_enabled, u32::from(mode != 0));
        }
    }

    #[test]
    fn preset_round_trip_keeps_fallback_and_other_keys() {
        let ini = Temporary::new("preset");
        let section = wide("Rendering");
        let file = File::new(ini.0.as_ptr(), section.as_ptr()).unwrap();
        let read_preset =
            || unsafe { rsf_settings_store_read_dlss_preset(ini.0.as_ptr(), section.as_ptr(), 4) };
        assert_eq!(read_preset(), 4);
        // Callback hosts saved the key as DlssPreset; profile keys match case-insensitively.
        assert!(file.write(&wide("DlssPreset"), wide("2").as_ptr()));
        assert_eq!(read_preset(), 2);
        assert!(file.write(&wide("Sharpness"), wide("0.25").as_ptr()));
        assert_eq!(
            unsafe { write_dlss_preset(ini.0.as_ptr(), section.as_ptr(), 11) },
            OK
        );
        assert_eq!(file.integer("DlssPreset", 0), 11);
        assert_eq!(read_preset(), 11);
        assert_eq!(file.decimal("Sharpness", 0.0), 0.25);
        assert_eq!(
            ini.contents()
                .to_ascii_lowercase()
                .matches("dlsspreset")
                .count(),
            1
        );
        assert_eq!(
            unsafe { write_dlss_preset(null(), section.as_ptr(), 1) },
            INVALID_ARGUMENT
        );
    }

    #[test]
    fn unparsable_decimals_fall_back() {
        let ini = Temporary::new("decimal");
        let section = wide("S");
        let file = File::new(ini.0.as_ptr(), section.as_ptr()).unwrap();
        assert!(file.write(&wide("A"), wide("0.5x").as_ptr()));
        assert!(file.write(&wide("B"), wide("").as_ptr()));
        assert!(file.write(&wide("C"), wide("-8").as_ptr()));
        assert!(file.write(&wide("D"), wide("nan").as_ptr()));
        assert_eq!(file.decimal("A", 0.4), 0.4);
        assert_eq!(file.decimal("B", 0.4), 0.4);
        assert_eq!(file.decimal("C", 0.4), -8.0);
        assert!(file.decimal("D", 0.4).is_nan());
        assert_eq!(file.decimal("Missing", 0.001), 0.001);
    }

    #[test]
    fn short_sharpening_options_are_refused_from_their_prefix() {
        let ini = Temporary::new("short");
        let section = wide("Rendering");
        // Only the four-byte prefix exists; nothing past it may be read.
        let short = [4u32];
        let options = short.as_ptr().cast::<Sharpening>();
        let result = unsafe {
            rsf_settings_store_write_sharpening(ini.0.as_ptr(), section.as_ptr(), options)
        };
        assert_eq!(result, INVALID_ARGUMENT);
        let defaults: Settings = unsafe { std::mem::zeroed() };
        let mut output: Settings = unsafe { std::mem::zeroed() };
        let result = unsafe {
            rsf_settings_store_read(
                ini.0.as_ptr(),
                section.as_ptr(),
                &defaults,
                options,
                &mut output,
            )
        };
        assert_eq!(result, INVALID_ARGUMENT);
        let mut wrong = sharpening();
        wrong.abi_version += 1;
        let result = unsafe {
            rsf_settings_store_write_sharpening(ini.0.as_ptr(), section.as_ptr(), &wrong)
        };
        assert_eq!(result, INVALID_ARGUMENT);
        assert!(ini.contents().is_empty());
    }
}
