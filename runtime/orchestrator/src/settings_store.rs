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
    "DLSSPreset",
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

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

// printf "%.9g" as the UCRT writes it: nine significant digits round-trip every float, ties
// round to even, trailing zeros are dropped and the exponent has at least two digits.
fn decimal(value: f32) -> String {
    let value = f64::from(value);
    if value.is_nan() {
        let bits = value.to_bits();
        return if bits >> 63 == 0 {
            "nan".into()
        } else if bits & ((1 << 51) - 1) == 0 {
            "-nan(ind)".into()
        } else {
            "-nan".into()
        };
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf" } else { "inf" }.into();
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.into();
    }
    let scientific = format!("{value:.8e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    fn trim(text: &str) -> &str {
        if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.')
        } else {
            text
        }
    }
    if !(-4..9).contains(&exponent) {
        let sign = if exponent < 0 { '-' } else { '+' };
        format!("{}e{sign}{:02}", trim(mantissa), exponent.unsigned_abs())
    } else {
        let fixed = format!("{:.*}", (8 - exponent) as usize, value);
        trim(&fixed).into()
    }
}

fn text(value: Value) -> Vec<u16> {
    match value {
        Value::Integer(value) => wide(&value.to_string()),
        Value::Decimal(value) => wide(&decimal(value)),
    }
}

unsafe extern "C" {
    // The CRT parser the store has always used, so locale, hex, inf and nan spellings match.
    fn wcstof(text: *const u16, end: *mut *mut u16) -> f32;
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

fn read(file: &File, defaults: &Settings) -> Settings {
    let value = |key, fallback| file.integer(key, fallback);
    let number = |key, fallback| file.decimal(key, fallback);
    // The shared post-pass keys fall back to rsf_sharpening_defaults, not to the host defaults,
    // because existing host aggregate initializers leave those fields zero.
    Settings {
        struct_size: size_of::<Settings>() as u32,
        abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
        sr_enabled: u32::from(value("SR", defaults.sr_enabled) != 0),
        sr_backend: value("SRBackend", defaults.sr_backend),
        dlss_preset: value("DLSSPreset", defaults.dlss_preset),
        fg_enabled: u32::from(value("FG", defaults.fg_enabled) != 0),
        fg_backend: value("FGBackend", defaults.fg_backend),
        generated_frames: value("FGGeneratedFrames", defaults.generated_frames),
        latency: value("Latency", defaults.latency),
        frame_limit_us: value("FrameLimitUs", defaults.frame_limit_us),
        status_overlay: u32::from(value("StatusOverlay", defaults.status_overlay) != 0),
        sr_quality: value("SRQuality", defaults.sr_quality),
        sharpening_mode: value("SharpeningMode", 0),
        sharpening_sharpness: number("Sharpness", 0.4),
        sharpening_mas: value("MAS", 0),
        sharpening_motion_sharpness: number("MotionSharpness", 0.2),
        sharpening_motion_threshold: number("MotionThreshold", 0.0),
        sharpening_motion_scale_limit: number("MotionScaleLimit", 10.0),
        sharpening_contrast_enabled: value("ContrastEnabled", 0),
        sharpening_contrast: number("Contrast", 0.0),
        sharpening_depth_scale: number("DADepthScale", 35.0),
        sharpening_depth_bias: number("DADepthBias", 0.001),
        sharpening_clamp_output: value("DAClampOutput", 0),
        sharpening_debug: value("SharpeningDebug", 0),
    }
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

/// Reads the owned keys, falling back to `defaults` for missing frontend values.
///
/// # Safety
/// `ini` and `section` are terminated UTF-16 strings, `defaults` and `output` are valid for
/// their full rsf_render_settings size.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_settings_store_read(
    ini: PCWSTR,
    section: PCWSTR,
    defaults: *const Settings,
    output: *mut Settings,
) -> i32 {
    let Some(file) = File::new(ini, section) else {
        return INVALID_ARGUMENT;
    };
    if defaults.is_null() || output.is_null() {
        return INVALID_ARGUMENT;
    }
    let value = read(&file, unsafe { &*defaults });
    unsafe { output.write(value) };
    OK
}

/// # Safety
/// `ini` and `section` are terminated UTF-16 strings; `request` is a full rsf_render_settings.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_settings_store_write(
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
unsafe extern "C" fn rsf_settings_store_read_layout(ini: PCWSTR, section: PCWSTR) -> u32 {
    File::new(ini, section).map_or(0, |file| file.integer(OVERLAY_LAYOUT, 0))
}

/// # Safety
/// `ini` and `section` are terminated UTF-16 strings.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_settings_store_write_layout(
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
unsafe extern "C" fn rsf_settings_store_write_sharpening(
    ini: PCWSTR,
    section: PCWSTR,
    options: *const Sharpening,
) -> i32 {
    let Some(file) = File::new(ini, section) else {
        return INVALID_ARGUMENT;
    };
    let Some(options) = (unsafe { options.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if (options.struct_size as usize) < size_of::<Sharpening>()
        || options.abi_version != RSF_SHARPENING_ABI_VERSION
    {
        return INVALID_ARGUMENT;
    }
    write_sharpening(&file, options, &mut |file, key, value| {
        file.write(key, value)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_matches_general_nine_digit_format() {
        let cases: [(f32, &str); 16] = [
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
        ];
        for (value, expected) in cases {
            assert_eq!(decimal(value), expected, "{value:e}");
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
        let read = read(&file, &empty);
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
        let mut request = read(&file, &defaults);
        assert_eq!(request.sr_enabled, 0);
        assert_eq!(request.sr_backend, 7);
        assert_eq!(request.sharpening_sharpness, 0.4);
        assert_eq!(request.sharpening_depth_scale, 35.0);
        request.sr_enabled = 1;
        request.generated_frames = 3;
        request.sharpening_contrast = 1.1;
        assert_eq!(write(&file, &request), OK);
        let contents = ini.contents();
        assert!(contents.contains("Foreign=kept"));
        assert!(contents.contains("OverlayLayout=5"));
        assert!(!contents.contains("sr=0"));
        assert!(contents.contains("Contrast=1.10000002"));
        let saved = read(&file, &defaults);
        assert_eq!(saved.sr_enabled, 1);
        assert_eq!(saved.generated_frames, 3);
        assert_eq!(saved.sharpening_contrast, 1.1);
        assert_eq!(file.integer(OVERLAY_LAYOUT, 0), 5);
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
}
