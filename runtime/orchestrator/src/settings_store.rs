// SPDX-License-Identifier: MIT
//! Shared persistence for rsf_render_settings in an adapter-supplied INI section.
//!
//! The key names and text formats belong to the render frontend; the file and section always
//! come from the adapter. The frontend ABI exposes FG as an enable flag, so the store
//! also tracks the original mode when reading or rewriting saved preferences.
use crate::sharpening_policy;
use crate::sys::{
    RSF_RENDER_FRONTEND_ABI_VERSION, RSF_SHARPENING_ABI_VERSION, rsf_render_settings as Settings,
    rsf_sharpening_options as Sharpening,
};
use std::ffi::{CStr, c_char};
use std::ptr::null;
use std::sync::Mutex;
use windows_sys::Win32::System::WindowsProgramming::{
    GetPrivateProfileIntW, GetPrivateProfileSectionW, GetPrivateProfileStringW,
    WritePrivateProfileSectionW, WritePrivateProfileStringW,
};
use windows_sys::core::PCWSTR;

const OK: i32 = 0;
const INVALID_ARGUMENT: i32 = -1;
const NOT_READY: i32 = -3;

// A section rewrite reads existing keys before publishing. Keep partial updates and their
// rollback in the same lock so they cannot race a rewrite from another caller of this store.
static PROFILE_IO: Mutex<()> = Mutex::new(());

// The section buffer the store has always read. A larger section is refused rather than truncated.
const SECTION_CAPACITY: usize = 32768;
// Decimal values longer than this were never produced by the store and read back as a fallback.
const NUMBER_CAPACITY: usize = 80;

// The overlay's panel layout word (rsf_overlay_stats::ui_preferences). Opaque here: only the
// overlay interprets it. A separate key rather than a field of rsf_render_settings, which other
// ABIs embed; a settings write keeps it because it is not an owned key.
const OVERLAY_LAYOUT: &str = "OverlayLayout";
const FG: &str = "FG";
const DLSS_PRESET: &str = "DLSSPreset";

#[derive(Clone, Copy)]
enum Value {
    Integer(u32),
    Decimal(f32),
}

impl From<u32> for Value {
    fn from(value: u32) -> Self {
        Self::Integer(value)
    }
}

impl From<f32> for Value {
    fn from(value: f32) -> Self {
        Self::Decimal(value)
    }
}

// One mapping supplies the read fallbacks, write values and owned-key filtering.
macro_rules! fields {
    (
        frontend { $( $key:expr => $field:ident : $kind:ident ),+ $(,)? }
        sharpening { $( $sharp_key:expr => $sharp_field:ident / $embedded:ident : $sharp_kind:ident ),+ $(,)? }
    ) => {
        const KEYS: &[&str] = &[$($key,)+ $($sharp_key,)+];

        fn frontend_values(s: &Settings) -> [(&'static str, Value); 10] {
            [$(($key, Value::from(s.$field))),+]
        }

        fn sharpening_values(o: &Sharpening) -> [(&'static str, Value); 12] {
            [$(($sharp_key, Value::from(o.$sharp_field))),+]
        }

        fn read_frontend(file: &File, s: &mut Settings) {
            $(s.$field = file.$kind($key, s.$field);)+
        }

        fn read_sharpening_values(file: &File, o: &mut Sharpening) {
            $(o.$sharp_field = file.$sharp_kind($sharp_key, o.$sharp_field);)+
        }

        pub(crate) fn sharpening_of(s: &Settings, base: &Sharpening) -> Sharpening {
            Sharpening {
                $($sharp_field: s.$embedded,)+
                ..*base
            }
        }
        pub(crate) fn set_sharpening(s: &mut Settings, o: &Sharpening) {
            $(s.$embedded = o.$sharp_field;)+
        }
    };
}

fields! {
    frontend {
        "SR" => sr_enabled: integer,
        "SRBackend" => sr_backend: integer,
        DLSS_PRESET => dlss_preset: integer,
        FG => fg_enabled: integer,
        "FGBackend" => fg_backend: integer,
        "FGGeneratedFrames" => generated_frames: integer,
        "Latency" => latency: integer,
        "FrameLimitUs" => frame_limit_us: integer,
        "StatusOverlay" => status_overlay: integer,
        "SRQuality" => sr_quality: integer,
    }
    sharpening {
        "SharpeningMode" => mode / sharpening_mode: integer,
        "Sharpness" => sharpness / sharpening_sharpness: decimal,
        "MAS" => mas / sharpening_mas: integer,
        "MotionSharpness" => motion_sharpness / sharpening_motion_sharpness: decimal,
        "MotionThreshold" => motion_threshold / sharpening_motion_threshold: decimal,
        "MotionScaleLimit" => motion_scale_limit / sharpening_motion_scale_limit: decimal,
        "ContrastEnabled" => contrast_enabled / sharpening_contrast_enabled: integer,
        "Contrast" => contrast / sharpening_contrast: decimal,
        "DADepthScale" => depth_scale / sharpening_depth_scale: decimal,
        "DADepthBias" => depth_bias / sharpening_depth_bias: decimal,
        "DAClampOutput" => clamp_output / sharpening_clamp_output: integer,
        "SharpeningDebug" => debug / sharpening_debug: integer,
    }
}

pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe extern "C" {
    // The CRT parser the store has always used, so locale, hex, inf and nan spellings match.
    fn wcstof(text: *const u16, end: *mut *mut u16) -> f32;
}

// MSVC's inline bounded swprintf forwards to _vswprintf_c. Use the bounded
// variadic entry point from the same CRT so locale, rounding and spellings agree.
#[cfg(target_env = "msvc")]
#[link(name = "legacy_stdio_definitions", kind = "static")]
unsafe extern "C" {
    #[link_name = "_swprintf_c"]
    fn format_decimal(text: *mut u16, capacity: usize, format: *const u16, ...) -> i32;
}

#[cfg(target_env = "gnu")]
unsafe extern "C" {
    #[link_name = "__mingw_swprintf"]
    fn format_decimal(text: *mut u16, capacity: usize, format: *const u16, ...) -> i32;
}

// Terminated UTF-16 text. "%.9g" of a float needs at most 16 characters.
fn decimal(value: f32) -> Vec<u16> {
    let mut text = [0u16; 32];
    let format = [b'%' as u16, b'.' as u16, b'9' as u16, b'g' as u16, 0];
    let length = unsafe {
        format_decimal(
            text.as_mut_ptr(),
            text.len(),
            format.as_ptr(),
            f64::from(value),
        )
    };
    // Unreachable for a float; an empty value would read back as the key's fallback.
    usize::try_from(length)
        .ok()
        .and_then(|length| text.get(..=length))
        .map_or_else(|| vec![0], <[u16]>::to_vec)
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

    fn write_integer(&self, key: &str, value: u32) -> i32 {
        let key = wide(key);
        let value = text(Value::Integer(value));
        let _io = PROFILE_IO.lock().unwrap();
        if self.write(&key, value.as_ptr()) {
            OK
        } else {
            NOT_READY
        }
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

pub(crate) fn read_saved_text(ini: &[u16], section: &[u16], key: &[u16]) -> Option<Vec<u16>> {
    let file = File::new(ini.as_ptr(), section.as_ptr())?;
    let _io = PROFILE_IO.lock().unwrap();
    let mut value = file.previous(key)?;
    value.pop();
    Some(value)
}

pub(crate) fn read_saved_number(ini: &[u16], section: &[u16], key: &[u16], fallback: i32) -> i32 {
    let _io = PROFILE_IO.lock().unwrap();
    unsafe { GetPrivateProfileIntW(section.as_ptr(), key.as_ptr(), fallback, ini.as_ptr()) }
}

pub(crate) fn write_saved_text(
    ini: &[u16],
    section: &[u16],
    key: &[u16],
    value: Option<&[u16]>,
) -> i32 {
    let Some(file) = File::new(ini.as_ptr(), section.as_ptr()) else {
        return INVALID_ARGUMENT;
    };
    // Surround the value so a read preserves whitespace and literal outer quotes.
    let quoted: Option<Vec<u16>> = value.map(|value| {
        let mut units = Vec::with_capacity(value.len() + 3);
        let quote = |unit: &u16| matches!(*unit, 9 | 32 | 34 | 39);
        let preserve = value.first().is_some_and(quote) || value.last().is_some_and(quote);
        if preserve {
            units.push(u16::from(b'"'));
        }
        units.extend_from_slice(value);
        if preserve {
            units.push(u16::from(b'"'));
        }
        units.push(0);
        units
    });
    let _io = PROFILE_IO.lock().unwrap();
    if file.write(
        key,
        quoted
            .as_ref()
            .map_or(std::ptr::null(), |value| value.as_ptr()),
    ) {
        OK
    } else {
        NOT_READY
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
    let _io = PROFILE_IO.lock().unwrap();
    let mut settings = Settings {
        struct_size: size_of::<Settings>() as u32,
        abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
        ..*defaults
    };
    let mut options = *sharpening;
    read_frontend(file, &mut settings);
    read_sharpening_values(file, &mut options);
    set_sharpening(&mut settings, &options);

    let fg_mode = settings.fg_enabled;
    settings.fg_enabled = u32::from(fg_mode != 0);
    settings.sr_enabled = u32::from(settings.sr_enabled != 0);
    settings.status_overlay = u32::from(settings.status_overlay != 0);
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
    let _io = PROFILE_IO.lock().unwrap();
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
    let mut stored = *request;
    // A Boolean enable leaves an existing Auto or Dynamic request intact. Explicit disable
    // and legacy callers writing a mode directly retain their original behavior.
    if stored.fg_enabled == 1 {
        let existing_mode = file.integer(FG, 1);
        if matches!(existing_mode, 2 | 3) {
            stored.fg_enabled = existing_mode;
        }
    }
    let sharpening = sharpening_of(&stored, &sharpening_policy::defaults());
    for (key, value) in frontend_values(&stored)
        .into_iter()
        .chain(sharpening_values(&sharpening))
    {
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
    let _io = PROFILE_IO.lock().unwrap();
    let values = sharpening_values(options);
    let keys = values.map(|(key, _)| wide(key));
    let previous: Vec<Option<Vec<u16>>> = keys.iter().map(|key| file.previous(key)).collect();
    for (index, (key, (_, value))) in keys.iter().zip(values).enumerate() {
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
    file.write_integer(OVERLAY_LAYOUT, value)
}

/// # Safety
/// `ini` and `section` are terminated UTF-16 strings.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_settings_store_read_fg_debug(
    ini: PCWSTR,
    section: PCWSTR,
    fallback: u32,
) -> u32 {
    File::new(ini, section).map_or(fallback, |file| file.integer("FGDebug", fallback))
}

/// # Safety
/// `ini` and `section` are terminated UTF-16 strings.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_settings_store_write_fg_debug(
    ini: PCWSTR,
    section: PCWSTR,
    value: u32,
) -> i32 {
    let Some(file) = File::new(ini, section) else {
        return INVALID_ARGUMENT;
    };
    file.write_integer("FGDebug", u32::from(value != 0))
}

/// Persists only the shared post-pass keys for hosts that own their base SR settings.
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
    if !sharpening_policy::valid(options) {
        return INVALID_ARGUMENT;
    }
    write_sharpening(&file, options, &mut |file, key, value| {
        file.write(key, value)
    })
}

/// Reads just the shared post-pass keys and applies the same policy used for live settings.
///
/// # Safety
/// Non-null pointers are terminated UTF-16 strings.
pub(crate) unsafe fn read_sharpening(ini: PCWSTR, section: PCWSTR) -> Sharpening {
    let mut options = sharpening_policy::defaults();
    if let Some(file) = File::new(ini, section) {
        let _io = PROFILE_IO.lock().unwrap();
        read_sharpening_values(&file, &mut options);
    }
    sharpening_policy::sanitize(options)
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_settings_store_read_sharpening(
    ini: PCWSTR,
    section: PCWSTR,
    output: *mut Sharpening,
) {
    if !output.is_null() {
        unsafe { output.write(read_sharpening(ini, section)) };
    }
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
    file.write_integer(DLSS_PRESET, preset)
}

/// # Safety
/// All non-null strings must be terminated and readable throughout the call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_adapter_setting_read_bool(
    ini: PCWSTR,
    section: PCWSTR,
    key: PCWSTR,
    fallback: u32,
) -> i32 {
    let Some(file) = File::new(ini, section).filter(|_| !key.is_null() && fallback <= 1) else {
        return INVALID_ARGUMENT;
    };
    let _io = PROFILE_IO.lock().unwrap_or_else(|lock| lock.into_inner());
    let stored = unsafe { GetPrivateProfileIntW(file.section, key, fallback as i32, file.ini) };
    i32::from(stored != 0)
}

/// # Safety
/// All non-null strings must be terminated and readable throughout the call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_adapter_setting_write_bool(
    ini: PCWSTR,
    section: PCWSTR,
    key: PCWSTR,
    value: u32,
) -> i32 {
    let Some(file) = File::new(ini, section).filter(|_| !key.is_null() && value <= 1) else {
        return INVALID_ARGUMENT;
    };
    let _io = PROFILE_IO.lock().unwrap_or_else(|lock| lock.into_inner());
    let text = [u16::from(b'0') + value as u16, 0];
    if unsafe { WritePrivateProfileStringW(file.section, key, text.as_ptr(), file.ini) } == 0 {
        NOT_READY
    } else {
        OK
    }
}

/// Reads a numeric adapter setting from its own INI section.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_settings_store_read_integer(
    ini: PCWSTR,
    section: PCWSTR,
    key: *const c_char,
    fallback: u32,
) -> u32 {
    let Some(file) = File::new(ini, section) else {
        return fallback;
    };
    if key.is_null() {
        return fallback;
    }
    unsafe { CStr::from_ptr(key) }
        .to_str()
        .map_or(fallback, |key| file.integer(key, fallback))
}

/// Saves a numeric adapter setting using the shared profile write lock.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_settings_store_write_integer(
    ini: PCWSTR,
    section: PCWSTR,
    key: *const c_char,
    value: u32,
) -> i32 {
    let Some(file) = File::new(ini, section) else {
        return INVALID_ARGUMENT;
    };
    if key.is_null() {
        return INVALID_ARGUMENT;
    }
    unsafe { CStr::from_ptr(key) }
        .to_str()
        .map_or(INVALID_ARGUMENT, |key| file.write_integer(key, value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::temporary_ini::TemporaryIni;

    #[test]
    fn saved_unicode_text_round_trips_through_an_existing_ascii_profile() {
        let ini = TemporaryIni::new("settings-store", "saved-unicode");
        let section = wide("Rendering");
        let key = wide("CaptureDirectory");
        let unrelated = wide("Unrelated");
        assert_eq!(
            write_saved_text(ini.wide(), &section, &unrelated, Some(&[u16::from(b'7')])),
            OK
        );
        let path: Vec<_> = r"C:\試験\🧪".encode_utf16().collect();
        let encoded = rsf_game_sys::profile_text::encode(&path);
        assert_eq!(
            write_saved_text(ini.wide(), &section, &key, Some(&encoded)),
            OK
        );
        let value = read_saved_text(ini.wide(), &section, &key).expect("stored path");
        assert_eq!(rsf_game_sys::profile_text::decode(value), path);
        assert_eq!(read_saved_number(ini.wide(), &section, &unrelated, 0), 7);
    }

    #[test]
    fn decimal_matches_general_nine_digit_format() {
        // Smoke cases for the CRT entry; the native fixture compares it with swprintf at scale.
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
        let ini = TemporaryIni::new("settings-store", "rollback");
        let section = wide("Rendering");
        let file = File::new(ini.wide().as_ptr(), section.as_ptr()).unwrap();
        assert!(file.write(&wide("EngineQuality"), wide("untouched").as_ptr()));
        assert!(file.write(&wide("Sharpness"), wide("0.25").as_ptr()));
        assert!(file.write(&wide("MAS"), wide("0").as_ptr()));
        let before = ini.contents().unwrap().expect("settings file exists");
        for failing in 0..12 {
            let mut calls = 0;
            let result = write_sharpening(&file, &sharpening(), &mut |file, key, value| {
                calls += 1;
                calls <= failing && file.write(key, value)
            });
            assert_eq!(result, NOT_READY);
            assert_eq!(
                ini.contents().unwrap().expect("settings file exists"),
                before,
                "failure at write {failing}"
            );
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
        assert!(
            ini.contents()
                .unwrap()
                .expect("settings file exists")
                .contains("EngineQuality=untouched")
        );
    }

    #[test]
    fn settings_write_replaces_owned_keys_and_keeps_others() {
        let ini = TemporaryIni::new("settings-store", "section");
        let section = wide("Adapter");
        let file = File::new(ini.wide().as_ptr(), section.as_ptr()).unwrap();
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
        let contents = ini.contents().unwrap().expect("settings file exists");
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
        let ini = TemporaryIni::new("settings-store", "fg-mode");
        let section = wide("Adapter");
        let file = File::new(ini.wide().as_ptr(), section.as_ptr()).unwrap();
        let defaults: Settings = unsafe { std::mem::zeroed() };
        for (stored, mode) in [("3", 3), ("2", 2), ("1", 1), ("0", 0)] {
            assert!(file.write(&wide("FG"), wide(stored).as_ptr()));
            let (settings, fg_mode) = read_with_fg_mode(&file, &defaults, &sharpening());
            assert_eq!(fg_mode, mode);
            assert_eq!(settings.fg_enabled, u32::from(mode != 0));
            let updated = Settings {
                status_overlay: 1,
                ..settings
            };
            assert_eq!(write(&file, &updated), OK);
            let (saved, saved_mode) = read_with_fg_mode(&file, &defaults, &sharpening());
            assert_eq!(saved_mode, mode);
            assert_eq!(saved.status_overlay, 1);
        }
    }

    #[test]
    fn preset_round_trip_keeps_fallback_and_other_keys() {
        let ini = TemporaryIni::new("settings-store", "preset");
        let section = wide("Rendering");
        let file = File::new(ini.wide().as_ptr(), section.as_ptr()).unwrap();
        let read_preset = || unsafe {
            rsf_settings_store_read_dlss_preset(ini.wide().as_ptr(), section.as_ptr(), 4)
        };
        assert_eq!(read_preset(), 4);
        // Callback hosts saved the key as DlssPreset; profile keys match case-insensitively.
        assert!(file.write(&wide("DlssPreset"), wide("2").as_ptr()));
        assert_eq!(read_preset(), 2);
        assert!(file.write(&wide("Sharpness"), wide("0.25").as_ptr()));
        assert_eq!(
            unsafe { write_dlss_preset(ini.wide().as_ptr(), section.as_ptr(), 11) },
            OK
        );
        assert_eq!(file.integer("DlssPreset", 0), 11);
        assert_eq!(read_preset(), 11);
        assert_eq!(file.decimal("Sharpness", 0.0), 0.25);
        assert_eq!(
            ini.contents()
                .unwrap()
                .expect("settings file exists")
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
        let ini = TemporaryIni::new("settings-store", "decimal");
        let section = wide("S");
        let file = File::new(ini.wide().as_ptr(), section.as_ptr()).unwrap();
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
        let ini = TemporaryIni::new("settings-store", "short");
        let section = wide("Rendering");
        // Only the four-byte prefix exists; nothing past it may be read.
        let short = [4u32];
        let options = short.as_ptr().cast::<Sharpening>();
        let result = unsafe {
            rsf_settings_store_write_sharpening(ini.wide().as_ptr(), section.as_ptr(), options)
        };
        assert_eq!(result, INVALID_ARGUMENT);
        let defaults: Settings = unsafe { std::mem::zeroed() };
        let mut output: Settings = unsafe { std::mem::zeroed() };
        let result = unsafe {
            rsf_settings_store_read(
                ini.wide().as_ptr(),
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
            rsf_settings_store_write_sharpening(ini.wide().as_ptr(), section.as_ptr(), &wrong)
        };
        assert_eq!(result, INVALID_ARGUMENT);
        assert_eq!(ini.contents().unwrap(), None);
    }
}
