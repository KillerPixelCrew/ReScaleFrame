// SPDX-License-Identifier: MIT
//! Runtime setup behind the game plugin's rsf_unity_* exports (runtime_host.cpp): the [UnitySR]
//! keys and defaults, the start gates and their result codes, the wait for Unity's Mono runtime,
//! and the rsf_fg12_setup (through generation.rs) and rsf_callback_host_setup descriptors. The
//! descriptors reach the Runtime the loader already loaded through runtime_host.cpp's typed
//! leaves; nothing here loads a Runtime or references its exports directly.
use crate::generation::{self, RSF_DLSS_ENGINE_UNITY, Sources};
use crate::runtime_abi::{
    RSF_CALLBACK_HOST_ABI_VERSION, RSF_FG_BACKEND_ALL, RSF_FG_BACKEND_AUTO, RSF_FG_BACKEND_DLSS,
    RSF_FG_BACKEND_FSR3, RSF_FG_BACKEND_FSR4, RSF_FG_BACKEND_XESS, RSF_FG_FIXED, RSF_REFLEX_OFF,
    RSF_SR_AUTO, RSF_SR_DLSS, RSF_SR_FSR2, RSF_SR_FSR3, RSF_SR_FSR4, RSF_SR_SPATIAL, RSF_SR_XESS,
    rsf_backend_result, rsf_callback_host_setup, rsf_fg12_setup, rsf_native_fg_options,
    rsf_unity_runtime_platform,
};
use crate::runtime_config::{
    Ini, PATH_CAPACITY, file_name, path_setting, preferences_path, profile_int, setting,
    terminated, utf8,
};
use rsf_game_sys::rsf_game_probe;
use std::ffi::{CStr, OsString, c_char, c_int, c_void};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::null_mut;
use std::sync::{Mutex, PoisonError};
use std::thread::sleep;
use std::time::{Duration, Instant};
use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowThreadProcessId};
use windows_sys::core::{PCWSTR, w};

/// A cast macro bindgen does not evaluate; runtime_host.cpp asserts it.
const RSF_BACKEND_OK: rsf_backend_result = 0;

/// Result codes of the exported entry points, part of their public C ABI.
mod code {
    pub(super) const OK: u32 = 0;
    pub(super) const NO_CONFIGURATION: u32 = 1;
    pub(super) const NO_PATHS: u32 = 3;
    pub(super) const FG_FRAMES_REFUSED: u32 = 4;
    pub(super) const FG_REFUSED: u32 = 5;
    pub(super) const NO_EXECUTABLE: u32 = 5;
    pub(super) const NO_HASH: u32 = 6;
    /// rsf_callback_host_start's "not ready yet", retried by rsf_unity_start.
    pub(super) const SR_NOT_READY: u32 = 10;
    pub(super) const SR_FAILED: u32 = 11;
    /// rsf_unity_start without Unity's Mono runtime, or after an internal failure.
    pub(super) const NO_MONO: u32 = 1;
    /// rsf_callback_host_stop's own refusal.
    pub(super) const STOP_REFUSED: u32 = 1;
}

/// The SR providers the Unity callback host offers.
const SR_CHOICES: u32 = (1 << RSF_SR_DLSS)
    | (1 << RSF_SR_FSR2)
    | (1 << RSF_SR_FSR3)
    | (1 << RSF_SR_FSR4)
    | (1 << RSF_SR_XESS)
    | (1 << RSF_SR_SPATIAL);
const MAX_GENERATED_FRAMES: u32 = 15;
const AMD64: u16 = 0x8664;
const MONO: PCWSTR = w!("mono-2.0-bdwgc.dll");
const MONO_DEADLINE: Duration = Duration::from_millis(90_000);
const MONO_POLL: Duration = Duration::from_millis(100);
const MONO_SETTLE: Duration = Duration::from_millis(2000);
const SR_RETRY: Duration = Duration::from_millis(500);

/// Whether rsf_fg12_install accepted the generation setup; held across the install.
static FG_STARTED: Mutex<bool> = Mutex::new(false);
/// [UnitySR] Log, read by the generation start and written by [`log`].
static LOG_PATH: Mutex<Vec<u16>> = Mutex::new(Vec::new());

/// runtime_host.cpp's leaves, all present.
struct Leaves {
    engine_version: *const c_char,
    project_id: *const c_char,
    fg_backend_key: unsafe extern "C" fn(u32) -> *const c_char,
    fg_choice_start: unsafe extern "C" fn(*const u16, u32, u32) -> u32,
    fg_choice_choices: unsafe extern "C" fn() -> u32,
    fg_install: unsafe extern "C" fn(*const rsf_fg12_setup) -> rsf_backend_result,
    fg_capture_outputs: unsafe extern "C" fn(*const c_char) -> rsf_backend_result,
    graphics_debug: unsafe extern "C" fn(),
    dlss_preset_valid: unsafe extern "C" fn(u32) -> u32,
    sha256_file: unsafe extern "C" fn(*const u16, *mut c_char) -> u32,
    callback_host_start: unsafe extern "C" fn(*const rsf_callback_host_setup) -> u32,
    callback_host_stop: unsafe extern "C" fn() -> u32,
    set_engine_spatial: unsafe extern "C" fn(u32, u32) -> c_int,
}

impl Leaves {
    /// The table is runtime_host.cpp's constant; an incomplete one is a build defect.
    unsafe fn adopt(platform: *const rsf_unity_runtime_platform) -> Option<Self> {
        if platform.is_null() {
            return None;
        }
        let p = unsafe { *platform };
        if p.engine_version_utf8.is_null() || p.project_id_utf8.is_null() {
            return None;
        }
        Some(Self {
            engine_version: p.engine_version_utf8,
            project_id: p.project_id_utf8,
            fg_backend_key: p.fg_backend_key?,
            fg_choice_start: p.fg_choice_start?,
            fg_choice_choices: p.fg_choice_choices?,
            fg_install: p.fg_install?,
            fg_capture_outputs: p.fg_capture_outputs?,
            graphics_debug: p.graphics_debug?,
            dlss_preset_valid: p.dlss_preset_valid?,
            sha256_file: p.sha256_file?,
            callback_host_start: p.callback_host_start?,
            callback_host_stop: p.callback_host_stop?,
            set_engine_spatial: p.set_engine_spatial?,
        })
    }
}

/// Runs an entry point body, mapping a missing table or a panic to the export's failure code,
/// as the C++ mapped exceptions.
fn guarded(
    platform: *const rsf_unity_runtime_platform,
    failure: u32,
    body: impl FnOnce(&Leaves) -> u32,
) -> u32 {
    let Some(leaves) = (unsafe { Leaves::adopt(platform) }) else {
        return failure;
    };
    catch_unwind(AssertUnwindSafe(|| body(&leaves))).unwrap_or(failure)
}

/// The generation log: each message appended with a newline to [UnitySR] Log.
unsafe extern "C" fn log(_: *mut c_void, message: *const c_char) {
    if message.is_null() {
        return;
    }
    let message = unsafe { CStr::from_ptr(message) }.to_bytes();
    let path = LOG_PATH.lock().unwrap_or_else(PoisonError::into_inner);
    if path.is_empty() {
        return;
    }
    if let Ok(mut file) = OpenOptions::new()
        .append(true)
        .create(true)
        .open(OsString::from_wide(&path))
    {
        let mut line = Vec::with_capacity(message.len() + 1);
        line.extend_from_slice(message);
        line.push(b'\n');
        let _ = file.write_all(&line);
    }
}

fn unity_window_class(name: &[u16]) -> bool {
    name.iter().copied().eq("UnityWndClass".encode_utf16())
}

/// Generation presents only on this process's Unity player window.
unsafe extern "C" fn accept_window(_: *mut c_void, window: *mut c_void) -> c_int {
    let mut process = 0u32;
    if unsafe { GetWindowThreadProcessId(window, &mut process) } == 0
        || process != unsafe { GetCurrentProcessId() }
    {
        return 0;
    }
    let mut name = [0u16; 64];
    let length = unsafe { GetClassNameW(window, name.as_mut_ptr(), name.len() as i32) };
    c_int::from(length > 0 && unity_window_class(&name[..length as usize]))
}

/// The UTF-8 [UnitySR] directory of a generation backend; empty for an unknown id or an unset key.
fn backend_directory(ini: &Ini, leaves: &Leaves, backend: u32) -> Vec<u8> {
    let key = unsafe { (leaves.fg_backend_key)(backend) };
    if key.is_null() {
        return Vec::new();
    }
    let key: Vec<u16> = unsafe { CStr::from_ptr(key) }
        .to_bytes()
        .iter()
        .map(|&byte| u16::from(byte))
        .chain([0])
        .collect();
    utf8(&path_setting(ini, key.as_ptr()))
}

/// Backend for the next start: the saved preference, else [UnitySR] FrameGeneration, offering
/// every implemented provider. 0 is Off.
fn fg_choice_start(ini: &Ini, leaves: &Leaves) -> u32 {
    let preferences = terminated(preferences_path(ini.chars()));
    let fallback = setting(ini, w!("FrameGeneration"), 0);
    unsafe { (leaves.fg_choice_start)(preferences.as_ptr(), fallback, RSF_FG_BACKEND_ALL) }
}

fn fg_start(ini: *const u16, leaves: &Leaves) -> u32 {
    let mut started = FG_STARTED.lock().unwrap_or_else(PoisonError::into_inner);
    let ini = match unsafe { Ini::new(ini) } {
        Some(ini) if !*started => ini,
        _ => {
            return if *started {
                code::OK
            } else {
                code::NO_CONFIGURATION
            };
        }
    };
    let backend = fg_choice_start(&ini, leaves);
    let runtime_backend = if backend != 0 && backend != RSF_FG_BACKEND_AUTO {
        backend
    } else {
        RSF_FG_BACKEND_FSR3
    };
    let runtime = backend_directory(&ini, leaves, runtime_backend);
    if runtime.is_empty() {
        return code::NO_PATHS;
    }
    *LOG_PATH.lock().unwrap_or_else(PoisonError::into_inner) = path_setting(&ini, w!("Log"));
    if setting(&ini, w!("GraphicsDebug"), 0) != 0 {
        unsafe { (leaves.graphics_debug)() };
    }
    let generated_frames = setting(&ini, w!("GeneratedFrames"), 1);
    if generated_frames == 0 || generated_frames > MAX_GENERATED_FRAMES {
        return code::FG_FRAMES_REFUSED;
    }
    let limit = setting(&ini, w!("FrameLimitFPS"), 0);
    let frame_limit_us = if limit != 0 { 1_000_000 / limit } else { 0 };
    let runtime = terminated(runtime);
    let fsr3 = terminated(backend_directory(&ini, leaves, RSF_FG_BACKEND_FSR3));
    let fsr4 = terminated(backend_directory(&ini, leaves, RSF_FG_BACKEND_FSR4));
    let xess = terminated(backend_directory(&ini, leaves, RSF_FG_BACKEND_XESS));
    let streamline = terminated(backend_directory(&ini, leaves, RSF_FG_BACKEND_DLSS));
    let options = rsf_native_fg_options {
        struct_size: size_of::<rsf_native_fg_options>() as u32,
        mode: RSF_FG_FIXED,
        generated_frames,
        reflex_mode: RSF_REFLEX_OFF,
        frame_limit_us,
        debug: 0,
    };
    let sources = Sources {
        runtime: runtime.as_ptr().cast(),
        fsr3: fsr3.as_ptr().cast(),
        fsr4: fsr4.as_ptr().cast(),
        xess: xess.as_ptr().cast(),
        streamline: streamline.as_ptr().cast(),
        engine_version: leaves.engine_version,
        project_id: leaves.project_id,
    };
    let setup: rsf_fg12_setup = generation::setup(backend, options, &sources, accept_window, log);
    *started = unsafe { (leaves.fg_install)(&setup) } == RSF_BACKEND_OK;
    let capture = utf8(&path_setting(&ini, w!("FGCaptureDirectory")));
    if *started && !capture.is_empty() {
        let capture = terminated(capture);
        let _ = unsafe { (leaves.fg_capture_outputs)(capture.as_ptr().cast()) };
    }
    let message = if *started {
        c"Unity generation installed before graphics creation."
    } else {
        c"Unity generation installation refused."
    };
    unsafe { log(null_mut(), message.as_ptr()) };
    if *started { code::OK } else { code::FG_REFUSED }
}

fn sr_start(ini: *const u16, leaves: &Leaves) -> u32 {
    let Some(ini) = (unsafe { Ini::new(ini) }).filter(|ini| !ini.chars().is_empty()) else {
        return code::NO_CONFIGURATION;
    };
    if unsafe { (leaves.fg_choice_choices)() } == 0 {
        fg_choice_start(&ini, leaves);
    }
    let preferences = terminated(preferences_path(ini.chars()));
    let plugin = path_setting(&ini, w!("Plugin"));
    let output = path_setting(&ini, w!("Log"));
    if plugin.is_empty() || output.is_empty() {
        return code::NO_PATHS;
    }
    let (plugin, output) = (terminated(plugin), terminated(output));
    let directory = |name| terminated(utf8(&path_setting(&ini, name)));
    let streamline = directory(w!("Streamline"));
    let fsr2 = directory(w!("FSR2"));
    let fsr3 = directory(w!("FSR3"));
    let fsr4 = directory(w!("FSR4"));
    let xess = directory(w!("XeSS"));
    let mut executable = vec![0u16; PATH_CAPACITY];
    let length =
        unsafe { GetModuleFileNameW(null_mut(), executable.as_mut_ptr(), PATH_CAPACITY as u32) }
            as usize;
    if length == 0 || length >= PATH_CAPACITY {
        return code::NO_EXECUTABLE;
    }
    executable.truncate(length);
    let name = terminated(utf8(file_name(&executable)));
    let executable = terminated(executable);
    let mut hash = [0 as c_char; 65];
    if unsafe { (leaves.sha256_file)(executable.as_ptr(), hash.as_mut_ptr()) } == 0 {
        return code::NO_HASH;
    }
    let probe = rsf_game_probe {
        struct_size: size_of::<rsf_game_probe>() as u32,
        pe_machine: AMD64,
        reserved: 0,
        executable_name_utf8: name.as_ptr().cast(),
        sha256_hex: hash.as_ptr(),
    };
    let backend = setting(&ini, w!("Backend"), RSF_SR_AUTO);
    let quality = setting(&ini, w!("Quality"), 1);
    // The overlay's saved preset wins over the ini's.
    let mut dlss_preset = profile_int(
        w!("Rendering"),
        w!("DlssPreset"),
        setting(&ini, w!("DlssPreset"), 0),
        preferences.as_ptr(),
    );
    if unsafe { (leaves.dlss_preset_valid)(dlss_preset) } == 0 {
        dlss_preset = 0;
    }
    let setup = rsf_callback_host_setup {
        struct_size: size_of::<rsf_callback_host_setup>() as u32,
        abi_version: RSF_CALLBACK_HOST_ABI_VERSION,
        plugin_path: plugin.as_ptr(),
        log_path: output.as_ptr(),
        preferences_path: preferences.as_ptr(),
        probe: &probe,
        backend,
        quality,
        dlss_preset,
        sr_backend_choices: SR_CHOICES,
        engine_type: RSF_DLSS_ENGINE_UNITY,
        engine_version_utf8: leaves.engine_version,
        project_id_utf8: leaves.project_id,
        streamline_directory_utf8: streamline.as_ptr().cast(),
        fsr2_directory_utf8: fsr2.as_ptr().cast(),
        fsr3_directory_utf8: fsr3.as_ptr().cast(),
        fsr4_directory_utf8: fsr4.as_ptr().cast(),
        xess_directory_utf8: xess.as_ptr().cast(),
        set_engine_spatial: Some(leaves.set_engine_spatial),
    };
    unsafe { (leaves.callback_host_start)(&setup) }
}

fn mono_loaded() -> bool {
    !unsafe { GetModuleHandleW(MONO) }.is_null()
}

/// Calls the guarded generation and SR entry points, so their failures map to their own codes
/// as the C++ exports' did.
fn start(ini: *const u16, platform: *const rsf_unity_runtime_platform) -> u32 {
    let Some(configuration) = (unsafe { Ini::new(ini) }) else {
        return code::OK;
    };
    if setting(&configuration, w!("AutoStart"), 1) == 0 {
        return code::OK;
    }
    // Generation installs before graphics creation; its result does not gate SR.
    let _ = unsafe { rsf_unity_mono_fg_start(ini, platform) };
    let deadline = Instant::now() + MONO_DEADLINE;
    while Instant::now() < deadline && !mono_loaded() {
        sleep(MONO_POLL);
    }
    if !mono_loaded() {
        return code::NO_MONO;
    }
    sleep(MONO_SETTLE);
    let mut result = code::SR_NOT_READY;
    while Instant::now() < deadline {
        result = unsafe { rsf_unity_mono_sr_start(ini, platform) };
        if result != code::SR_NOT_READY {
            break;
        }
        sleep(SR_RETRY);
    }
    result
}

/// rsf_unity_start: unless [UnitySR] AutoStart is 0, installs generation, waits up to 90 s from
/// entry for Unity's Mono runtime, then starts SR, retrying while the host is not ready yet.
///
/// # Safety
/// `ini` is null or a NUL-terminated UTF-16 path; `platform` is runtime_host.cpp's table.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_unity_mono_runtime_start(
    ini: *const u16,
    platform: *const rsf_unity_runtime_platform,
) -> u32 {
    guarded(platform, code::NO_MONO, |_| start(ini, platform))
}

/// rsf_unity_fg_start: installs the shared D3D12 generation once, before graphics creation.
///
/// # Safety
/// As [`rsf_unity_mono_runtime_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_unity_mono_fg_start(
    ini: *const u16,
    platform: *const rsf_unity_runtime_platform,
) -> u32 {
    guarded(platform, code::FG_REFUSED, |leaves| fg_start(ini, leaves))
}

/// rsf_unity_sr_start: starts the callback SR host for this executable.
///
/// # Safety
/// As [`rsf_unity_mono_runtime_start`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_unity_mono_sr_start(
    ini: *const u16,
    platform: *const rsf_unity_runtime_platform,
) -> u32 {
    guarded(platform, code::SR_FAILED, |leaves| sr_start(ini, leaves))
}

/// rsf_unity_sr_stop: the callback host's stop result.
///
/// # Safety
/// `platform` is runtime_host.cpp's table.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_unity_mono_sr_stop(
    platform: *const rsf_unity_runtime_platform,
) -> u32 {
    guarded(platform, code::STOP_REFUSED, |leaves| unsafe {
        (leaves.callback_host_stop)()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sr_choices_match_the_cpp_mask() {
        assert_eq!(SR_CHOICES, 0b111_1110);
    }

    #[test]
    fn only_the_unity_player_class_is_accepted() {
        let class: Vec<u16> = "UnityWndClass".encode_utf16().collect();
        assert!(unity_window_class(&class));
        assert!(!unity_window_class(&class[..class.len() - 1]));
        let other: Vec<u16> = "UnityWndClassX".encode_utf16().collect();
        assert!(!unity_window_class(&other));
    }
}
