// SPDX-License-Identifier: MIT
//! Loader entry points, UnitySR settings and the bounded wait for Mono. This module passes the
//! owned executable identity and adapter callbacks to the Runtime already loaded by the carrier.
use crate::generation::{self, RSF_DLSS_ENGINE_UNITY, Sources};
use crate::runtime_abi::{
    RSF_DLSS_PRESET_MASK, RSF_FG_BACKEND_ALL, RSF_FG_FIXED, RSF_REFLEX_OFF, RSF_SR_AUTO,
    RSF_SR_DLSS, RSF_SR_FSR2, RSF_SR_FSR3, RSF_SR_FSR4, RSF_SR_SPATIAL, RSF_SR_XESS,
    rsf_backend_result, rsf_fg_choice, rsf_fg12_setup, rsf_native_fg_options,
};
use crate::runtime_config::{
    Ini, PATH_CAPACITY, file_name, path_setting, preferences_path, profile_int, setting,
    terminated, utf8,
};
use crate::startup_policy::{self, MONO_DEADLINE, wait_for_sr};
use rsf_game_sys::{
    callback_host_abi::{
        RSF_CALLBACK_HOST_ABI_VERSION_V2, rsf_callback_host_setup, rsf_callback_host_setup_v2,
    },
    rsf_game_probe,
};
use std::ffi::{CStr, OsString, c_char, c_int, c_void};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::ptr::null_mut;
use std::sync::{Mutex, PoisonError};
use std::thread::sleep;
use std::time::Instant;
use windows::Win32::Graphics::Direct3D12::{D3D12GetDebugInterface, ID3D12Debug};
use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowThreadProcessId};
use windows_sys::core::{PCWSTR, w};

/// RSF_BACKEND_OK is a cast macro.
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
    pub(super) const SR_FAILED: u32 = 11;
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

/// Whether rsf_fg12_install accepted the generation setup; held across the install.
static FG_STARTED: Mutex<bool> = Mutex::new(false);
/// [UnitySR] Log, read by the generation start and written by [`log`].
static LOG_PATH: Mutex<Vec<u16>> = Mutex::new(Vec::new());
static DIAGNOSTICS_INI: Mutex<Vec<u16>> = Mutex::new(Vec::new());

#[path = "../../../runtime/orchestrator/src/overlay_saved_abi.rs"]
mod saved_abi;

const ENGINE_VERSION: &CStr = c"6000.3";
const PROJECT_ID: &CStr = c"57a42c7e-faf0-4bda-a9f9-892870948ac1";

// The carrier already loaded this Runtime. The plugin target links its import library.
unsafe extern "C" {
    fn rsf_fg_choice_start(path: *const u16, fallback: u32, choices: u32) -> u32;
    fn rsf_fg_choice_get() -> rsf_fg_choice;
    fn rsf_fg12_install(setup: *const rsf_fg12_setup) -> rsf_backend_result;
    fn rsf_fg12_capture_outputs(directory: *const c_char) -> rsf_backend_result;
    fn rsf_callback_host_start(setup: *const rsf_callback_host_setup) -> u32;
    fn rsf_callback_host_stop() -> u32;
    fn rsf_unity_set_engine_spatial(spatial: u32, quality: u32) -> c_int;
    fn rsf_unity_mono_drained_generation(generation: *mut u32) -> c_int;
    fn rsf_overlay_saved_options_register(
        scope: *mut c_void,
        setup: *const saved_abi::rsf_overlay_saved_setup,
    ) -> i32;
    fn rsf_overlay_saved_options_unregister(scope: *mut c_void) -> i32;
}

unsafe extern "C" fn capture_directory_changed(
    _: *mut c_void,
    index: u32,
    _: *const u16,
    length: u32,
) -> i32 {
    if index != 1 {
        return -1;
    }
    let ini = {
        let path = DIAGNOSTICS_INI
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if path.is_empty() {
            return -3;
        }
        unsafe { Ini::new(path.as_ptr()) }.expect("registered INI")
    };
    // Runtime persisted the value before invoking this callback. Resolve it the
    // same way as startup, including relative paths and clearing the directory.
    let path = path_setting(&ini, w!("FGCaptureDirectory"));
    if length != 0 && path.is_empty() {
        return -1;
    }
    let capture = terminated(utf8(&path));
    unsafe { rsf_fg12_capture_outputs(capture.as_ptr().cast()) }
}

fn register_diagnostics(ini: &Ini) {
    use saved_abi::*;
    let mut path = DIAGNOSTICS_INI
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let rows = [
        rsf_overlay_saved_option {
            label_utf8: c"Graphics debug layer".as_ptr(),
            key_utf8: c"GraphicsDebug".as_ptr(),
            kind: RSF_OVERLAY_SAVED_TOGGLE,
            restart: 1,
            ..Default::default()
        },
        rsf_overlay_saved_option {
            label_utf8: c"Frame generation capture directory".as_ptr(),
            key_utf8: c"FGCaptureDirectory".as_ptr(),
            kind: RSF_OVERLAY_SAVED_TEXT,
            max_text_length: RSF_OVERLAY_SAVED_TEXT_LIMIT,
            ..Default::default()
        },
    ];
    let setup = rsf_overlay_saved_setup {
        struct_size: size_of::<rsf_overlay_saved_setup>() as u32,
        abi_version: RSF_OVERLAY_SAVED_OPTIONS_ABI_VERSION,
        configuration_path: ini.as_ptr(),
        configuration_section: w!("UnitySR"),
        options: rows.as_ptr(),
        option_count: rows.len() as u32,
        apply: Some(capture_directory_changed),
        ..Default::default()
    };
    if unsafe { rsf_overlay_saved_options_register(null_mut(), &setup) } == 0 {
        *path = terminated(ini.chars().to_vec());
    } else {
        unsafe {
            log(
                null_mut(),
                c"Unity graphics diagnostic controls could not be registered.".as_ptr(),
            )
        };
    }
}

fn guarded(failure: u32, body: impl FnOnce() -> u32) -> u32 {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or(failure)
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

fn directory(ini: &Ini, key: PCWSTR) -> Vec<u8> {
    terminated(utf8(&path_setting(ini, key)))
}

/// Backend for the next start: the saved preference, else [UnitySR] FrameGeneration, offering
/// every implemented provider. 0 is Off.
fn fg_choice_start(ini: &Ini) -> u32 {
    let preferences = terminated(preferences_path(ini.chars()));
    let fallback = setting(ini, w!("FrameGeneration"), 0);
    unsafe { rsf_fg_choice_start(preferences.as_ptr(), fallback, RSF_FG_BACKEND_ALL) }
}

fn fg_start(ini: *const u16) -> u32 {
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
    let backend = fg_choice_start(&ini);
    *LOG_PATH.lock().unwrap_or_else(PoisonError::into_inner) = path_setting(&ini, w!("Log"));
    if setting(&ini, w!("GraphicsDebug"), 0) != 0 {
        let mut debug: Option<ID3D12Debug> = None;
        if unsafe { D3D12GetDebugInterface(&mut debug) }.is_ok()
            && let Some(debug) = debug
        {
            unsafe { debug.EnableDebugLayer() };
        }
    }
    let generated_frames = setting(&ini, w!("GeneratedFrames"), 1);
    if generated_frames == 0 || generated_frames > MAX_GENERATED_FRAMES {
        return code::FG_FRAMES_REFUSED;
    }
    let limit = setting(&ini, w!("FrameLimitFPS"), 0);
    let frame_limit_us = if limit != 0 { 1_000_000 / limit } else { 0 };
    let paths = startup_policy::FgPaths::read(ini.chars(), |key| {
        let key = terminated(key.encode_utf16().collect());
        path_setting(&ini, key.as_ptr())
    });
    let options = rsf_native_fg_options {
        struct_size: size_of::<rsf_native_fg_options>() as u32,
        mode: RSF_FG_FIXED,
        generated_frames,
        reflex_mode: RSF_REFLEX_OFF,
        frame_limit_us,
        debug: 0,
    };
    let sources = Sources {
        runtime: paths.runtime.as_ptr().cast(),
        fsr3: paths.fsr3.as_ptr().cast(),
        fsr4: paths.fsr4.as_ptr().cast(),
        xess: paths.xess.as_ptr().cast(),
        streamline: paths.streamline.as_ptr().cast(),
        engine_version: ENGINE_VERSION.as_ptr(),
        project_id: PROJECT_ID.as_ptr(),
    };
    let setup: rsf_fg12_setup = generation::setup(backend, options, &sources, accept_window, log);
    *started = unsafe { rsf_fg12_install(&setup) } == RSF_BACKEND_OK;
    let capture = utf8(&path_setting(&ini, w!("FGCaptureDirectory")));
    if *started && !capture.is_empty() {
        let capture = terminated(capture);
        let _ = unsafe { rsf_fg12_capture_outputs(capture.as_ptr().cast()) };
    }
    let message = if *started {
        c"Unity generation installed before graphics creation."
    } else {
        c"Unity generation installation refused."
    };
    unsafe { log(null_mut(), message.as_ptr()) };
    if *started { code::OK } else { code::FG_REFUSED }
}

struct Fingerprint {
    name: Vec<u8>,
    hash: [c_char; 65],
}

fn fingerprint() -> Result<Fingerprint, u32> {
    let mut executable = vec![0u16; PATH_CAPACITY];
    let length =
        unsafe { GetModuleFileNameW(null_mut(), executable.as_mut_ptr(), PATH_CAPACITY as u32) }
            as usize;
    if length == 0 || length >= PATH_CAPACITY {
        return Err(code::NO_EXECUTABLE);
    }
    executable.truncate(length);
    let name = terminated(utf8(file_name(&executable)));
    let executable = OsString::from_wide(&executable);
    let digest = rsf_windows::sha256_file(Path::new(&executable)).ok_or(code::NO_HASH)?;
    let hex = b"0123456789abcdef";
    let mut hash = [0; 65];
    for (byte, pair) in digest.into_iter().zip(hash.chunks_exact_mut(2)) {
        pair[0] = hex[usize::from(byte >> 4)] as c_char;
        pair[1] = hex[usize::from(byte & 15)] as c_char;
    }
    Ok(Fingerprint { name, hash })
}

fn sr_start(ini: *const u16, identity: &mut Option<Fingerprint>) -> u32 {
    let Some(ini) = (unsafe { Ini::new(ini) }).filter(|ini| !ini.chars().is_empty()) else {
        return code::NO_CONFIGURATION;
    };
    if unsafe { rsf_fg_choice_get().choices } == 0 {
        fg_choice_start(&ini);
    }
    let preferences = terminated(preferences_path(ini.chars()));
    let plugin = path_setting(&ini, w!("Plugin"));
    let output = path_setting(&ini, w!("Log"));
    if plugin.is_empty() || output.is_empty() {
        return code::NO_PATHS;
    }
    let (plugin, output) = (terminated(plugin), terminated(output));

    let streamline = directory(&ini, w!("Streamline"));
    let fsr2 = directory(&ini, w!("FSR2"));
    let fsr3 = directory(&ini, w!("FSR3"));
    let fsr4 = directory(&ini, w!("FSR4"));
    let xess = directory(&ini, w!("XeSS"));
    let identity = match identity {
        Some(identity) => identity,
        slot @ None => match fingerprint() {
            Ok(identity) => slot.insert(identity),
            Err(code) => return code,
        },
    };
    let probe = rsf_game_probe {
        struct_size: size_of::<rsf_game_probe>() as u32,
        pe_machine: AMD64,
        reserved: 0,
        executable_name_utf8: identity.name.as_ptr().cast(),
        sha256_hex: identity.hash.as_ptr(),
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
    if dlss_preset > 13 || RSF_DLSS_PRESET_MASK & (1 << dlss_preset) == 0 {
        dlss_preset = 0;
    }
    let setup = rsf_callback_host_setup_v2 {
        base: rsf_callback_host_setup {
            struct_size: size_of::<rsf_callback_host_setup_v2>() as u32,
            abi_version: RSF_CALLBACK_HOST_ABI_VERSION_V2,
            plugin_path: plugin.as_ptr(),
            log_path: output.as_ptr(),
            preferences_path: preferences.as_ptr(),
            probe: &probe,
            backend,
            quality,
            dlss_preset,
            sr_backend_choices: SR_CHOICES,
            engine_type: RSF_DLSS_ENGINE_UNITY,
            engine_version_utf8: ENGINE_VERSION.as_ptr(),
            project_id_utf8: PROJECT_ID.as_ptr(),
            streamline_directory_utf8: streamline.as_ptr().cast(),
            fsr2_directory_utf8: fsr2.as_ptr().cast(),
            fsr3_directory_utf8: fsr3.as_ptr().cast(),
            fsr4_directory_utf8: fsr4.as_ptr().cast(),
            xess_directory_utf8: xess.as_ptr().cast(),
            set_engine_spatial: Some(rsf_unity_set_engine_spatial),
        },
        request_policy: Some(crate::bridge_state::rsf_unity_mono_request_policy),
        drained_generation: Some(rsf_unity_mono_drained_generation),
        spatial_extent: Some(crate::bridge_state::rsf_unity_mono_spatial_extent),
        spatial_name_utf8: c"FSR1".as_ptr(),
    };
    let result = unsafe { rsf_callback_host_start((&raw const setup).cast()) };
    if result == code::OK {
        register_diagnostics(&ini);
    }
    result
}

fn mono_loaded() -> bool {
    !unsafe { GetModuleHandleW(MONO) }.is_null()
}

fn start(ini: *const u16) -> u32 {
    let Some(configuration) = (unsafe { Ini::new(ini) }) else {
        return code::OK;
    };
    if setting(&configuration, w!("AutoStart"), 1) == 0 {
        return code::OK;
    }
    // Generation installs before graphics creation; its result does not gate SR.
    let _ = unsafe { rsf_unity_fg_start(ini) };
    let mut identity = None;
    wait_for_sr(
        Instant::now() + MONO_DEADLINE,
        Instant::now,
        sleep,
        mono_loaded,
        || {
            catch_unwind(AssertUnwindSafe(|| sr_start(ini, &mut identity)))
                .unwrap_or(code::SR_FAILED)
        },
    )
}

/// Combined loader entry. The generation install precedes Mono and graphics startup.
#[unsafe(no_mangle)]
unsafe extern "system" fn rsf_unity_start(ini: *const u16) -> u32 {
    guarded(startup_policy::NO_MONO, || start(ini))
}

#[unsafe(no_mangle)]
unsafe extern "system" fn rsf_unity_fg_start(ini: *const u16) -> u32 {
    guarded(code::FG_REFUSED, || fg_start(ini))
}

#[unsafe(no_mangle)]
unsafe extern "system" fn rsf_unity_sr_start(ini: *const u16) -> u32 {
    guarded(code::SR_FAILED, || sr_start(ini, &mut None))
}

#[unsafe(no_mangle)]
extern "system" fn rsf_unity_sr_stop(_: *mut c_void) -> u32 {
    guarded(code::STOP_REFUSED, || {
        let previous = {
            let mut path = DIAGNOSTICS_INI
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !path.is_empty() && unsafe { rsf_overlay_saved_options_unregister(null_mut()) } != 0
            {
                return code::STOP_REFUSED;
            }
            std::mem::take(&mut *path)
        };
        let result = unsafe { rsf_callback_host_stop() };
        if result != code::OK
            && !previous.is_empty()
            && let Some(ini) = unsafe { Ini::new(previous.as_ptr()) }
        {
            register_diagnostics(&ini);
        }
        result
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
