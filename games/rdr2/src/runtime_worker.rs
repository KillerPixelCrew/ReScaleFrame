// SPDX-License-Identifier: MIT
//! The ASI and explicit attach worker share renderer setup and plugin session admission.
use crate::runtime_worker_abi::{
    RSF_RENDER_HOST_ABI_VERSION, RSF_RENDER_HOST_OWNS_PRESENTATION, RSF_UI_MODE_BACKBUFFER_HUDLESS,
    rsf_rdr2_fingerprint_fn, rsf_rdr2_runtime_leaves, rsf_render_host_setup,
};
use crate::{OK, asi, engine_settings, layout, script_input};
use rsf_game_sys::plugin_session_abi::{
    RSF_PLUGIN_SESSION_ABI_VERSION, rsf_plugin_session_options,
};
use rsf_game_sys::{RSF_GAME_ABI_VERSION, rsf_game_host_services, rsf_game_probe};
use std::ffi::{CStr, OsString, c_char, c_void};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::sync::{Mutex, OnceLock, PoisonError};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleW, GetProcAddress, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LoadLibraryExW,
};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::System::WindowsProgramming::{
    GetPrivateProfileIntW, GetPrivateProfileStringW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowThreadProcessId};
use windows_sys::core::{PCSTR, PCWSTR, s, w};

type Prepare = unsafe extern "C" fn(*const rsf_plugin_session_options, *mut *mut c_void) -> i32;
type Start = unsafe extern "C" fn(*mut c_void) -> i32;
type Create =
    unsafe extern "C" fn(*const rsf_render_host_setup, *mut rsf_game_host_services) -> i32;
type Fingerprint = unsafe extern "C" fn(*const u16, *mut c_char) -> bool;

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static LOG_WRITE: Mutex<()> = Mutex::new(());

fn set_log_path(path: &[u16]) {
    let path = path.strip_suffix(&[0]).unwrap_or(path);
    let _ = LOG_PATH.set(PathBuf::from(OsString::from_wide(path)));
}

pub(crate) fn log(message: impl AsRef<[u8]>) {
    let Some(path) = LOG_PATH.get() else { return };
    let _writer = LOG_WRITE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let tick = unsafe { GetTickCount64() };
        let _ = write!(file, "[{tick}] ");
        let _ = file.write_all(message.as_ref());
        let _ = file.write_all(b"\n");
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_runtime_log(_: *mut c_void, message: *const c_char) {
    if !message.is_null() {
        log(unsafe { CStr::from_ptr(message) }.to_bytes());
    }
}

fn module_path(module: HMODULE) -> Option<Vec<u16>> {
    let mut path = vec![0; 32768];
    let length =
        unsafe { GetModuleFileNameW(module, path.as_mut_ptr(), path.len() as u32) } as usize;
    if length == 0 || length >= path.len() {
        return None;
    }
    path.truncate(length);
    Some(path)
}

pub(crate) fn append_path(parent: &[u16], suffix: &str) -> Vec<u16> {
    parent
        .iter()
        .copied()
        .chain(suffix.encode_utf16())
        .chain([0])
        .collect()
}

fn profile_string(
    section: PCWSTR,
    key: PCWSTR,
    config: &[u16],
    capacity: usize,
) -> Option<Vec<u16>> {
    let mut value = vec![0; capacity];
    let length = unsafe {
        GetPrivateProfileStringW(
            section,
            key,
            w!(""),
            value.as_mut_ptr(),
            capacity as u32,
            config.as_ptr(),
        )
    } as usize;
    if length == 0 || length >= capacity - 1 {
        return None;
    }
    value.truncate(length + 1);
    Some(value)
}

unsafe extern "C" fn accept_window(_: *mut c_void, window: *mut c_void) -> i32 {
    let mut process = 0;
    let mut class = [0u16; 64];
    let owned = unsafe {
        GetWindowThreadProcessId(window, &mut process) != 0 && process == GetCurrentProcessId()
    };
    if !owned {
        return 0;
    }
    let length = unsafe { GetClassNameW(window, class.as_mut_ptr(), class.len() as i32) };
    i32::from(
        length > 0
            && class[..length as usize]
                .iter()
                .copied()
                .eq("sgaWindow".encode_utf16()),
    )
}

fn setup(configuration: PCWSTR, directory: PCWSTR) -> rsf_render_host_setup {
    rsf_render_host_setup {
        struct_size: size_of::<rsf_render_host_setup>() as u32,
        abi_version: RSF_RENDER_HOST_ABI_VERSION,
        configuration_path: configuration,
        runtime_directory: directory,
        configuration_section: w!("RDR2"),
        presentation_owner: RSF_RENDER_HOST_OWNS_PRESENTATION,
        sr_backend_choices: (1 << 1) | (1 << 3) | (1 << 4) | (1 << 5),
        engine_type: 0,
        application_id: 0x60e8ab7,
        engine_version_utf8: c"RAGE 1.0.1491.50".as_ptr(),
        project_id_utf8: c"6fd496e2-e0fd-48dc-9e99-0c37e87d30ca".as_ptr(),
        depth_inverted: 1,
        units_to_meters: 1.0,
        ui_mode: RSF_UI_MODE_BACKBUFFER_HUDLESS,
        accept_window: Some(accept_window),
        user: null_mut(),
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_fill_runtime_setup(
    config: PCWSTR,
    directory: PCWSTR,
    out: *mut rsf_render_host_setup,
) {
    if !out.is_null() {
        unsafe { out.write(setup(config, directory)) };
    }
}

pub(crate) unsafe fn export<F: Copy>(runtime: HMODULE, name: PCSTR) -> Option<F> {
    const { assert!(size_of::<F>() == size_of::<usize>()) };
    let function = unsafe { GetProcAddress(runtime, name) }?;
    Some(unsafe { std::mem::transmute_copy(&function) })
}

fn run_session(
    runtime: HMODULE,
    config: &[u16],
    directory: &[u16],
    plugin: &[u16],
    fingerprint: Fingerprint,
    asi: Option<(HMODULE, &rsf_rdr2_runtime_leaves)>,
) -> u32 {
    let (Some(prepare), Some(start)) = (
        unsafe { export::<Prepare>(runtime, s!("rsf_plugin_session_prepare")) },
        unsafe { export::<Start>(runtime, s!("rsf_plugin_session_start")) },
    ) else {
        if asi.is_some() {
            log("Runtime plugin lifecycle exports are unavailable.");
        }
        return 1;
    };
    let Some(mut executable) = module_path(null_mut()).filter(|path| asi::is_game(path)) else {
        return 1;
    };
    executable.push(0);
    let mut digest = [0 as c_char; 65];
    if !unsafe { fingerprint(executable.as_ptr(), digest.as_mut_ptr()) } {
        if asi.is_some() {
            log("Executable fingerprint could not be read.");
        }
        return 1;
    }
    let probe = rsf_game_probe {
        struct_size: size_of::<rsf_game_probe>() as u32,
        pe_machine: layout::AMD64,
        reserved: 0,
        executable_name_utf8: layout::EXECUTABLE_NAME.as_ptr(),
        sha256_hex: digest.as_ptr(),
    };
    // Unused callbacks are null; the Runtime copies the fields it retains.
    let mut services: rsf_game_host_services = unsafe { std::mem::zeroed() };
    services.struct_size = size_of::<rsf_game_host_services>() as u32;
    services.abi_version = RSF_GAME_ABI_VERSION;
    services.session_id = unsafe { GetTickCount64() }.wrapping_add(1);
    services.log = Some(rsf_rdr2_runtime_log);
    let mut options = rsf_plugin_session_options {
        struct_size: size_of::<rsf_plugin_session_options>() as u32,
        abi_version: RSF_PLUGIN_SESSION_ABI_VERSION,
        plugin_path: plugin.as_ptr(),
        probe: &probe,
        services,
    };
    let host_setup = setup(config.as_ptr(), directory.as_ptr());
    let create = unsafe { export::<Create>(runtime, s!("rsf_render_host_create")) };
    let Some(create) = create else {
        if asi.is_some() {
            log("RDR2 SR runtime host initialization refused.");
        }
        return 1;
    };
    let created = unsafe { create(&host_setup, &mut options.services) };
    if let Some((_, asi)) = asi {
        if created != OK {
            log("RDR2 SR runtime host initialization refused.");
            return 1;
        }
        if unsafe {
            GetPrivateProfileIntW(w!("RDR2"), w!("NativeSettingsProbe"), 0, config.as_ptr())
        } != 0
        {
            let path = profile_string(w!("RDR2"), w!("NativeSettingsProbePath"), config, 32768)
                .unwrap_or_else(|| vec![0]);
            if !asi
                .native_settings
                .is_some_and(|native| unsafe { native(path.as_ptr()) })
            {
                log("Native settings probe refused.");
            }
        }
    }
    let mut session = null_mut();
    let prepared = if created == OK {
        unsafe { prepare(&options, &mut session) }
    } else {
        created
    };
    let prefix = if asi.is_some() {
        "RDR2"
    } else {
        "RDR2 research"
    };
    let hash = unsafe { CStr::from_ptr(digest.as_ptr()) }.to_string_lossy();
    log(format!("{prefix} prepare result={prepared} sha256={hash}"));
    if prepared != OK {
        return if asi.is_some() { prepared as u32 } else { 1 };
    }
    let started = unsafe { start(session) };
    log(format!("{prefix} start result={started}"));
    if let Some((module, _)) = asi {
        if started == OK && !script_input::start(module, runtime, config) {
            log(
                "RDR2 ScriptHook input guard unavailable; shared overlay still uses its window input hook.",
            );
        }
        unsafe {
            engine_settings::rsf_rdr2_report_engine_settings(Some(rsf_rdr2_runtime_log), null_mut())
        };
        return started as u32;
    }
    u32::from(started != OK)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_asi_start_runtime(
    module: HMODULE,
    leaves: *const rsf_rdr2_runtime_leaves,
) -> u32 {
    let Some(leaves) = (unsafe { leaves.as_ref() }) else {
        return 1;
    };
    let Some(fingerprint) = leaves.fingerprint else {
        return 1;
    };
    let Some((root, config)) = module_path(module).and_then(|path| asi::deployment(&path)) else {
        return 1;
    };
    if unsafe { GetPrivateProfileIntW(w!("RDR2"), w!("Enable"), 1, config.as_ptr()) } == 0 {
        return 0;
    }
    let directory = append_path(&root, "ReScaleFrame\\");
    let directory = &directory[..directory.len() - 1];
    set_log_path(&append_path(directory, "rdr2.log"));
    log("RDR2 ASI entry started.");
    unsafe { asi::rsf_rdr2_asi_report_startup(Some(rsf_rdr2_runtime_log), null_mut()) };
    if let Some(diagnostics) = leaves.diagnostics {
        let modules_path = append_path(directory, "rdr2-modules.log");
        let dump_directory =
            if unsafe { GetPrivateProfileIntW(w!("RDR2"), w!("DumpModule"), 0, config.as_ptr()) }
                != 0
            {
                Some(
                    profile_string(w!("RDR2"), w!("DumpDirectory"), &config, 260)
                        .unwrap_or_else(|| append_path(directory, "dumps")),
                )
            } else {
                None
            };
        unsafe {
            diagnostics(
                modules_path.as_ptr(),
                dump_directory.as_ref().map_or(null(), |path| path.as_ptr()),
            )
        };
    }
    let runtime_path = append_path(directory, "ReScaleFrame.Runtime.dll");
    let runtime = unsafe {
        LoadLibraryExW(
            runtime_path.as_ptr(),
            null_mut(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        )
    };
    if runtime.is_null() {
        log("Runtime DLL load refused.");
        return 1;
    }
    let plugin = append_path(directory, "ReScaleFrame.Game.RDR2.dll");
    run_session(
        runtime,
        &config,
        &append_path(directory, ""),
        &plugin,
        fingerprint,
        Some((module, leaves)),
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_attach_start_runtime(
    module: HMODULE,
    fingerprint: rsf_rdr2_fingerprint_fn,
) -> u32 {
    let Some(fingerprint) = fingerprint else {
        return 1;
    };
    let Some(path) = module_path(module) else {
        return 1;
    };
    let Some(separator) = path.iter().rposition(|&unit| unit == u16::from(b'\\')) else {
        return 1;
    };
    let attach_path = append_path(&path[..=separator], "attach.ini");
    let value = |key| profile_string(w!("Attach"), key, &attach_path, 32768);
    let (Some(config), Some(plugin), Some(directory), Some(output)) = (
        value(w!("Configuration")),
        value(w!("Plugin")),
        value(w!("RuntimeDirectory")),
        value(w!("Log")),
    ) else {
        return 1;
    };
    set_log_path(&output);
    log("RDR2 research attachment started; borrowing the already loaded runtime.");
    let runtime = unsafe { GetModuleHandleW(w!("ReScaleFrame.Runtime.dll")) };
    if runtime.is_null() {
        log("Attachment refused: no existing runtime.");
        return 1;
    }
    run_session(runtime, &config, &directory, &plugin, fingerprint, None)
}
