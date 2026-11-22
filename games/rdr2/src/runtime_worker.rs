// SPDX-License-Identifier: MIT
//! The ASI and explicit attach worker share renderer setup and plugin session admission.
use crate::runtime_worker_abi::{
    RSF_RENDER_HOST_ABI_VERSION, RSF_RENDER_HOST_OWNS_PRESENTATION, RSF_UI_MODE_BACKBUFFER_HUDLESS,
    rsf_rdr2_fingerprint_fn, rsf_rdr2_runtime_leaves, rsf_render_host_setup,
};
use crate::{OK, asi, engine_settings, layout, script_input};
use rsf_game_sys::plugin_session_abi::rsf_plugin_session_options;
use rsf_game_sys::rsf_game_host_services;
use rsf_game_sys::session::carrier_log::CarrierLog;
use rsf_game_sys::session::{Probe, host_services, with_session_options};
use std::ffi::{CStr, c_char, c_void};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::{CloseHandle, HMODULE};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use windows_sys::Win32::System::LibraryLoader::{
    DisableThreadLibraryCalls, GetModuleFileNameW, GetModuleHandleW, GetProcAddress,
    LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LoadLibraryExW,
};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::{CreateThread, GetCurrentProcessId};
use windows_sys::Win32::System::WindowsProgramming::{
    GetPrivateProfileIntW, GetPrivateProfileStringW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowThreadProcessId};
use windows_sys::core::{PCSTR, PCWSTR, s, w};

type Prepare = unsafe extern "C" fn(*const rsf_plugin_session_options, *mut *mut c_void) -> i32;
type Start = unsafe extern "C" fn(*mut c_void) -> i32;
type Create =
    unsafe extern "C" fn(*const rsf_render_host_setup, *mut rsf_game_host_services) -> i32;
type Fingerprint = unsafe extern "C" fn(*const u16, u32, *mut u8) -> u32;

static LOG: CarrierLog = CarrierLog::new();

pub(crate) fn log(message: impl AsRef<[u8]>) {
    LOG.write(unsafe { GetTickCount64() }, message);
}

unsafe extern "C" fn rsf_rdr2_runtime_log(_: *mut c_void, message: *const c_char) {
    unsafe { LOG.write_c_message(GetTickCount64(), message) };
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

struct Refusal {
    stage: &'static str,
    result: i32,
}

fn run_session(
    runtime: HMODULE,
    config: &[u16],
    directory: &[u16],
    plugin: &[u16],
    fingerprint: Fingerprint,
    after_create: impl FnOnce(),
) -> Result<(), Refusal> {
    let lifecycle_missing = || Refusal {
        stage: "Runtime lifecycle exports",
        result: 1,
    };
    let (prepare, start, quiesce, stop): (Prepare, Start, Start, Start) = unsafe {
        (
            export(runtime, s!("rsf_plugin_session_prepare")).ok_or_else(lifecycle_missing)?,
            export(runtime, s!("rsf_plugin_session_start")).ok_or_else(lifecycle_missing)?,
            export(runtime, s!("rsf_plugin_session_quiesce")).ok_or_else(lifecycle_missing)?,
            export(runtime, s!("rsf_plugin_session_stop")).ok_or_else(lifecycle_missing)?,
        )
    };
    let mut executable = module_path(null_mut())
        .filter(|path| asi::is_game(path))
        .ok_or(Refusal {
            stage: "executable admission",
            result: 1,
        })?;
    executable.push(0);
    let mut bytes = [0u8; 32];
    let sharing = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    if unsafe { fingerprint(executable.as_ptr(), sharing, bytes.as_mut_ptr()) } == 0 {
        return Err(Refusal {
            stage: "executable fingerprint",
            result: 1,
        });
    }
    let mut digest = [0u8; 65];
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for (pair, byte) in digest[..64].chunks_exact_mut(2).zip(bytes) {
        pair.copy_from_slice(&[HEX[(byte >> 4) as usize], HEX[(byte & 15) as usize]]);
    }
    let hash = CStr::from_bytes_with_nul(&digest).expect("hex digest");
    let probe = Probe {
        machine: layout::AMD64,
        executable: layout::EXECUTABLE_NAME,
        sha256: hash,
    };
    let mut services = host_services(unsafe { GetTickCount64() }, Some(rsf_rdr2_runtime_log));
    let host_setup = setup(config.as_ptr(), directory.as_ptr());
    let create =
        unsafe { export::<Create>(runtime, s!("rsf_render_host_create")) }.ok_or(Refusal {
            stage: "Runtime host export",
            result: 1,
        })?;
    let created = unsafe { create(&host_setup, &mut services) };
    if created != OK {
        return Err(Refusal {
            stage: "Runtime host initialization",
            result: created,
        });
    }
    after_create();

    let mut session = null_mut();
    let prepared = with_session_options(plugin, probe, &services, |options| unsafe {
        prepare(options, &mut session)
    });
    log(format!(
        "RDR2 prepare result={prepared} sha256={}",
        hash.to_string_lossy()
    ));
    let (stage, result) = if prepared == OK && !session.is_null() {
        ("start", unsafe { start(session) })
    } else {
        ("prepare", if prepared == OK { 1 } else { prepared })
    };
    if result == OK {
        log("RDR2 plugin session started.");
        return Ok(());
    }
    log(format!("RDR2 {stage} refused, result={result}."));
    if !session.is_null() {
        // Runtime keeps the module admitted while this owner exists. Retry here on the
        // startup worker so another prepare cannot overtake a partially retired plugin.
        log("RDR2 is retiring the refused plugin session.");
        while unsafe { quiesce(session) } != OK || unsafe { stop(session) } != OK {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    Err(Refusal { stage, result })
}

fn log_refusal(prefix: &str, refusal: Refusal) -> i32 {
    log(format!(
        "{prefix} refused at {}, result={}.",
        refusal.stage, refusal.result
    ));
    refusal.result
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_rdr2_asi_start_runtime(
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
    LOG.set_path(&append_path(directory, "rdr2.log"));
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
    let result = run_session(
        runtime,
        &config,
        &append_path(directory, ""),
        &plugin,
        fingerprint,
        || {
            if unsafe {
                GetPrivateProfileIntW(w!("RDR2"), w!("NativeSettingsProbe"), 0, config.as_ptr())
            } != 0
            {
                let path =
                    profile_string(w!("RDR2"), w!("NativeSettingsProbePath"), &config, 32768)
                        .unwrap_or_else(|| vec![0]);
                if !crate::native_settings::install(&path) {
                    log("Native settings probe refused.");
                }
            }
        },
    );
    if result.is_ok() && !script_input::start(module, runtime, &config) {
        log(
            "RDR2 ScriptHook input guard unavailable; shared overlay still uses its window input hook.",
        );
    }
    unsafe {
        engine_settings::rsf_rdr2_report_engine_settings(Some(rsf_rdr2_runtime_log), null_mut())
    };
    result.map_or_else(|refusal| log_refusal("RDR2 ASI", refusal) as u32, |()| 0)
}

static ATTACH_FINGERPRINT: OnceLock<Fingerprint> = OnceLock::new();

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_attach_start_runtime(
    module: HMODULE,
    fingerprint: rsf_rdr2_fingerprint_fn,
) -> u32 {
    let Some(fingerprint) = fingerprint else {
        return 1;
    };
    if ATTACH_FINGERPRINT.set(fingerprint).is_err() {
        return 1;
    }
    unsafe {
        DisableThreadLibraryCalls(module);
        let thread = CreateThread(null(), 0, Some(attach_worker), module, 0, null_mut());
        if thread.is_null() {
            return 1;
        }
        CloseHandle(thread);
    }
    0
}

unsafe extern "system" fn attach_worker(module: *mut c_void) -> u32 {
    let Some(&fingerprint) = ATTACH_FINGERPRINT.get() else {
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
    LOG.set_path(&output);
    log("RDR2 research attachment started; borrowing the already loaded runtime.");
    let runtime = unsafe { GetModuleHandleW(w!("ReScaleFrame.Runtime.dll")) };
    if runtime.is_null() {
        log("Attachment refused: no existing runtime.");
        return 1;
    }
    run_session(runtime, &config, &directory, &plugin, fingerprint, || {}).map_or_else(
        |refusal| {
            log_refusal("RDR2 research attachment", refusal);
            1
        },
        |()| 0,
    )
}
