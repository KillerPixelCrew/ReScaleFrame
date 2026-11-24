// SPDX-License-Identifier: MIT
//! The ASI and explicit attach worker share renderer setup and plugin session admission.
use crate::sys::{
    RSF_RENDER_HOST_ABI_VERSION, RSF_RENDER_HOST_OWNS_PRESENTATION, RSF_UI_MODE_BACKBUFFER_HUDLESS,
    rsf_rdr2_fingerprint_fn, rsf_render_host_setup,
};
use crate::{OK, asi, engine_settings, layout, script_input};
use rsf_game_sys::rsf_game_host_services;
use rsf_game_sys::rsf_plugin_session_options;
use rsf_game_sys::session::carrier_log::CarrierLog;
use rsf_game_sys::session::{Probe, host_services, with_session_options};
use rsf_game_sys::win32::{module_path, typed_export};
use rsf_game_sys::windows_path::leaf_is;
use std::ffi::{CStr, c_char, c_void};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::{CloseHandle, HMODULE};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use windows_sys::Win32::System::LibraryLoader::{
    DisableThreadLibraryCalls, GetModuleHandleW, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LoadLibraryExW,
};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::{CreateThread, GetCurrentProcessId};
use windows_sys::Win32::System::WindowsProgramming::{
    GetPrivateProfileIntW, GetPrivateProfileStringW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowThreadProcessId};
use windows_sys::core::{PCWSTR, w};

type Prepare = unsafe extern "C" fn(*const rsf_plugin_session_options, *mut *mut c_void) -> i32;
type Start = unsafe extern "C" fn(*mut c_void) -> i32;
type Create =
    unsafe extern "C" fn(*const rsf_render_host_setup, *mut rsf_game_host_services) -> i32;
type Fingerprint = unsafe extern "C" fn(*const u16, u32, *mut u8) -> u32;

fn register_startup_options(runtime: HMODULE, scope: *mut c_void, path: *const u16) {
    use crate::sys::*;
    type Register = unsafe extern "C" fn(*mut c_void, *const rsf_overlay_saved_setup) -> i32;
    let Some(register) =
        (unsafe { typed_export::<Register>(runtime, c"rsf_overlay_saved_options_register") })
    else {
        log("RDR2 overlay startup controls unavailable in this Runtime.");
        return;
    };
    let toggle = |key: &CStr, label: &CStr, default_value| rsf_overlay_saved_option {
        label_utf8: label.as_ptr(),
        key_utf8: key.as_ptr(),
        default_value,
        kind: RSF_OVERLAY_SAVED_TOGGLE,
        restart: 1,
        ..Default::default()
    };
    #[cfg_attr(not(feature = "research"), expect(unused_mut, clippy::useless_vec))]
    let mut rows = vec![
        toggle(c"Enable", c"Enable renderer integration", 1),
        toggle(c"ForceD3D12", c"Force Direct3D 12", 1),
    ];
    // The remaining keys are read only by research builds.
    #[cfg(feature = "research")]
    {
        let text = |key: &CStr, label: &CStr, max_text_length| rsf_overlay_saved_option {
            label_utf8: label.as_ptr(),
            key_utf8: key.as_ptr(),
            max_text_length,
            kind: RSF_OVERLAY_SAVED_TEXT,
            restart: 1,
            ..Default::default()
        };
        rows.extend([
            toggle(c"NativeCapture", c"Native GPU capture", 0),
            toggle(c"CameraProbe", c"Camera probe", 0),
            text(
                c"CameraProbeDirectory",
                c"Camera and native capture directory",
                195,
            ),
            toggle(c"CameraReference", c"Camera reference capture", 0),
            text(c"CameraReferenceLibrary", c"Camera reference library", 259),
            toggle(c"FrameProbe", c"Frame probe", 0),
            text(c"FrameProbeLibrary", c"Frame probe library", 259),
            text(c"FrameProbePrefix", c"Frame probe prefix", 259),
            toggle(c"Census", c"Vendor census", 0),
            text(c"CensusDirectory", c"Census directory", 259),
            rsf_overlay_saved_option {
                label_utf8: c"Census flags".as_ptr(),
                key_utf8: c"CensusFlags".as_ptr(),
                kind: RSF_OVERLAY_SAVED_NUMBER,
                restart: 1,
                maximum: 127,
                step: 1,
                divisor: 1,
                ..Default::default()
            },
            toggle(c"TraceD3D12", c"Trace Direct3D 12", 0),
            toggle(c"NativeSettingsProbe", c"Native settings probe", 0),
            text(
                c"NativeSettingsProbePath",
                c"Native settings probe path",
                RSF_OVERLAY_SAVED_TEXT_LIMIT,
            ),
            toggle(c"DumpModule", c"Dump the executable module", 0),
            text(c"DumpDirectory", c"Module dump directory", 259),
        ]);
    }
    let setup = rsf_overlay_saved_setup {
        struct_size: size_of::<rsf_overlay_saved_setup>() as u32,
        abi_version: RSF_OVERLAY_SAVED_OPTIONS_ABI_VERSION,
        configuration_path: path,
        configuration_section: w!("RDR2"),
        options: rows.as_ptr(),
        option_count: rows.len() as u32,
        ..Default::default()
    };
    let result = unsafe { register(scope, &setup) };
    if result != OK {
        log(format!(
            "RDR2 overlay startup controls refused, result={result}."
        ));
    }
}

static LOG: CarrierLog = CarrierLog::new();

pub(crate) fn log(message: impl AsRef<[u8]>) {
    LOG.write(unsafe { GetTickCount64() }, message);
}

unsafe extern "C" fn rsf_rdr2_runtime_log(_: *mut c_void, message: *const c_char) {
    unsafe { LOG.write_c_message(GetTickCount64(), message) };
}

pub(crate) use rsf_game_sys::ffi_util::wide_path as append_path;

fn profile_string(
    section: PCWSTR,
    key: PCWSTR,
    config: &[u16],
    capacity: usize,
) -> Option<Vec<u16>> {
    let read_capacity = rsf_game_sys::profile_text::read_capacity(capacity);
    let mut value = vec![0; read_capacity];
    let length = unsafe {
        GetPrivateProfileStringW(
            section,
            key,
            w!(""),
            value.as_mut_ptr(),
            read_capacity as u32,
            config.as_ptr(),
        )
    } as usize;
    if length == 0 || length >= read_capacity - 1 {
        return None;
    }
    value.truncate(length);
    let mut value = rsf_game_sys::profile_text::decode(value);
    if value.is_empty() || value.len() >= capacity - 1 {
        return None;
    }
    value.push(0);
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
            typed_export(runtime, c"rsf_plugin_session_prepare").ok_or_else(lifecycle_missing)?,
            typed_export(runtime, c"rsf_plugin_session_start").ok_or_else(lifecycle_missing)?,
            typed_export(runtime, c"rsf_plugin_session_quiesce").ok_or_else(lifecycle_missing)?,
            typed_export(runtime, c"rsf_plugin_session_stop").ok_or_else(lifecycle_missing)?,
        )
    };
    let executable = unsafe { module_path(null_mut()) }
        .filter(|path| leaf_is(path.units(), layout::EXECUTABLE_NAME))
        .ok_or(Refusal {
            stage: "executable admission",
            result: 1,
        })?;
    let mut bytes = [0u8; 32];
    let sharing = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    if unsafe {
        fingerprint(
            executable.terminated().as_ptr(),
            sharing,
            bytes.as_mut_ptr(),
        )
    } == 0
    {
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
        unsafe { typed_export::<Create>(runtime, c"rsf_render_host_create") }.ok_or(Refusal {
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
    register_startup_options(runtime, services.user, config.as_ptr());
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

pub(crate) fn asi_start_runtime(module: HMODULE, fingerprint: Fingerprint) -> u32 {
    let Some((root, config)) =
        unsafe { module_path(module) }.and_then(|path| asi::deployment(path.units()))
    else {
        return 1;
    };
    if unsafe { GetPrivateProfileIntW(w!("RDR2"), w!("Enable"), 1, config.as_ptr()) } == 0 {
        return 0;
    }
    let directory = append_path(&root, "ReScaleFrame\\");
    let directory = &directory[..directory.len() - 1];
    LOG.set_path(&append_path(directory, "rdr2.log"));
    log("RDR2 ASI entry started.");
    asi::report_startup();
    #[cfg(feature = "research")]
    {
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
            crate::diagnostics::start_diagnostics(
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
            #[cfg(feature = "research")]
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
    engine_settings::report();
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
    let Some(path) = (unsafe { module_path(module) }) else {
        return 1;
    };
    let path = path.units();
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
