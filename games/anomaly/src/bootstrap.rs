// SPDX-License-Identifier: MIT
//! Anomaly's synchronous cold entry. The thin carrier calls `rsf_anomaly_start` with the absolute adapter INI before
//! genuine DirectInput creation. This module selects the exact executable build, borrows the Runtime DLL the carrier
//! already loaded and starts Observe through the Runtime's stable plugin-session C ABI. This module owns the step
//! order, refusals, retry state and adapter log. The private leaves in bootstrap.cpp call the native engine code.

use crate::executable::{identity_name, module_path, running_build};
use crate::{AMD64, BuildIdentity};
use rsf_game_sys::{RSF_GAME_ABI_VERSION, rsf_game_host_services, rsf_game_log_fn, rsf_game_probe};
use std::ffi::{CStr, OsString, c_char, c_void};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::{ptr, slice};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, GetFileAttributesW, INVALID_FILE_ATTRIBUTES,
};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleHandleExW,
};
use windows_sys::Win32::System::Performance::QueryPerformanceCounter;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::core::w;

// rsf_anomaly_start status values. The carrier logs them; they are unchanged from the C++ entry.
const READY: u32 = 0;
const CONFIGURATION_REFUSED: u32 = 1;
const BUILD_REFUSED: u32 = 2;
const RUNTIME_MISSING: u32 = 3;
const RUNTIME_REFUSED: u32 = 4;
const PREPARE_REFUSED: u32 = 5;
const START_REFUSED: u32 = 6;
const MODE_REFUSED: u32 = 7;
const FAULT: u32 = 8;
const OVERLAY_REFUSED: u32 = 9;

const RSF_OK: i32 = 0;
const SESSION_ABI_VERSION: u32 = 1;

/// The C++ owners this entry orders, supplied by the export in bootstrap.cpp. Every leaf is noexcept, runs under the
/// entry lock and takes a build index this module selected from `builds`.
#[repr(C)]
pub(crate) struct Leaves {
    builds: *const BuildIdentity,
    build_count: usize,
    label: unsafe extern "C" fn(usize) -> *const c_char,
    /// Returns whether SR is selected; configures it only when the flag is set.
    sr: unsafe extern "C" fn(*const u16, bool, rsf_game_log_fn) -> bool,
    motion: unsafe extern "C" fn(*const u16, rsf_game_log_fn),
    /// Returns whether FG is selected; configures it and installs its bridge only when the flag is set.
    fg: unsafe extern "C" fn(*const u16, bool, HMODULE, usize, rsf_game_log_fn) -> bool,
    fg_installed: unsafe extern "C" fn() -> bool,
    /// 0 installed, 1 DirectInput observation refused, 2 engine input hook refused.
    input: unsafe extern "C" fn(usize, *mut *const c_char) -> u32,
    overlay: unsafe extern "C" fn(HMODULE, *const u16, rsf_game_log_fn) -> bool,
    overlay_sink: unsafe extern "C" fn(),
}

/// `rsf_plugin_session_options` from runtime/orchestrator/include/rescaleframe/plugin_session.h.
#[repr(C)]
struct SessionOptions {
    struct_size: u32,
    abi_version: u32,
    plugin_path: *const u16,
    probe: *const rsf_game_probe,
    services: rsf_game_host_services,
}
const _: () = assert!(size_of::<SessionOptions>() == 128);

type PrepareFn = unsafe extern "C" fn(*const SessionOptions, *mut *mut c_void) -> i32;
type ControlFn = unsafe extern "C" fn(*mut c_void) -> i32;

/// The carrier-loaded Runtime, borrowed without a reference.
struct Runtime {
    module: HMODULE,
    prepare: Option<PrepareFn>,
    start: Option<ControlFn>,
    quiesce: Option<ControlFn>,
    stop: Option<ControlFn>,
}

impl Runtime {
    fn borrow() -> Option<Self> {
        use crate::runtime_module::{export, rsf_anomaly_runtime_module};
        let module = rsf_anomaly_runtime_module();
        if module.is_null() {
            return None;
        }
        // These exports have the C signatures declared in plugin_session.h.
        let prepare = unsafe { export(module, c"rsf_plugin_session_prepare".to_bytes_with_nul()) };
        let start = unsafe { export(module, c"rsf_plugin_session_start".to_bytes_with_nul()) };
        let quiesce = unsafe { export(module, c"rsf_plugin_session_quiesce".to_bytes_with_nul()) };
        let stop = unsafe { export(module, c"rsf_plugin_session_stop".to_bytes_with_nul()) };
        Some(Self {
            module,
            prepare,
            start,
            quiesce,
            stop,
        })
    }
}

/// One accepted call: the NUL-terminated INI, the selected build and the requested modes.
struct Request<'a> {
    ini: &'a [u16],
    build: usize,
    identity: &'a BuildIdentity,
    observe: bool,
    overlay: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Observation {
    Unprepared,
    RetainedAfterRefusal,
    Prepared,
    Active,
}

struct Entry {
    started: bool,            // every requested mode is complete
    options_registered: bool, // restart options are in the overlay list once
    stages_configured: bool,  // Observe stages were configured and FG/input installed once
    observation: Observation,
    session: *mut c_void, // the Runtime's session, including one retained after refused prepare
}

// SAFETY: the session handle is only used while ENTRY is locked.
unsafe impl Send for Entry {}

impl Entry {
    const fn new() -> Self {
        Self {
            started: false,
            options_registered: false,
            stages_configured: false,
            observation: Observation::Unprepared,
            session: ptr::null_mut(),
        }
    }
}

static ENTRY: Mutex<Entry> = Mutex::new(Entry::new());
static LOG: Mutex<Option<PathBuf>> = Mutex::new(None);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn write_log(message: &[u8]) {
    let path = lock(&LOG);
    let Some(path) = path.as_deref() else { return };
    let mut line = Vec::with_capacity(message.len() + 32);
    let _ = write!(line, "[{}:{}] ", unsafe { GetCurrentProcessId() }, unsafe {
        GetTickCount64()
    });
    line.extend_from_slice(message);
    line.push(b'\n');
    if let Ok(mut file) = OpenOptions::new().append(true).create(true).open(path) {
        let _ = file.write_all(&line);
    }
}

/// The adapter log, handed to the Runtime session and the C++ stages.
unsafe extern "C" fn adapter_log(_: *mut c_void, message: *const c_char) {
    if !message.is_null() {
        write_log(unsafe { CStr::from_ptr(message) }.to_bytes());
    }
}

const LOG_FN: rsf_game_log_fn = Some(adapter_log);

fn log_result(operation: &str, result: i32) {
    write_log(format!("Anomaly {operation} result={result}").as_bytes());
}

fn observe_selected(ini: &[u16]) -> bool {
    let mut mode = [0u16; 64];
    let length = unsafe { crate::ini::read_text(ini.as_ptr(), w!("Mode"), w!(""), &mut mode) };
    mode[..length.min(mode.len())]
        .iter()
        .copied()
        .eq("Observe".encode_utf16())
}

unsafe fn enter(configuration: *const u16, leaves: &Leaves) -> u32 {
    let Some(ini) = (unsafe { crate::ini::path(configuration) }) else {
        return CONFIGURATION_REFUSED;
    };
    let attributes = unsafe { GetFileAttributesW(ini.as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES || attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        return CONFIGURATION_REFUSED;
    }
    let path = PathBuf::from(OsString::from_wide(&ini[..ini.len() - 1]));
    if !path.is_absolute() {
        return CONFIGURATION_REFUSED;
    }
    let mut entry = lock(&ENTRY);
    if entry.started {
        return READY;
    }
    let builds = unsafe { slice::from_raw_parts(leaves.builds, leaves.build_count) };
    let Ok(build) = running_build(builds) else {
        return BUILD_REFUSED;
    };
    *lock(&LOG) = path
        .parent()
        .map(|directory| directory.join("ReScaleFrame.Anomaly.log"));
    let identity = &builds[build];
    let label = unsafe { CStr::from_ptr((leaves.label)(build)) };
    write_log(
        format!(
            "Anomaly executable {}: {} build profile selected by exact SHA-256.",
            identity_name(identity).to_string_lossy(),
            label.to_string_lossy()
        )
        .as_bytes(),
    );
    let observe = observe_selected(ini);
    // Overlay=1 is independent of Mode: it wires only the shared overlay, never SR or FG.
    let overlay = unsafe { crate::ini::read_int(ini.as_ptr(), w!("Overlay"), 0) } != 0;
    if !observe && !overlay {
        write_log(b"Anomaly startup refused: only explicitly selected Observe mode or Overlay=1 is currently validated; SR/FG is not implemented.");
        return MODE_REFUSED;
    }
    let Some(runtime) = Runtime::borrow() else {
        write_log(b"Anomaly refused: carrier has not loaded the existing Runtime DLL.");
        return RUNTIME_MISSING;
    };
    let request = Request {
        ini,
        build,
        identity,
        observe,
        overlay,
    };
    orchestrate(
        &mut entry,
        leaves,
        &request,
        &runtime,
        crate::adapter_options::register,
    )
}

/// The cold-start order: restart options, SR, motion, the FG bridge and input facts before the engine creates its
/// device, then the Observe session, then the overlay. A retry repeats only the steps that have not completed.
fn orchestrate(
    entry: &mut Entry,
    leaves: &Leaves,
    request: &Request,
    runtime: &Runtime,
    register_options: fn(&[u16]),
) -> u32 {
    let ini = request.ini.as_ptr();
    if request.observe
        && entry.observation == Observation::RetainedAfterRefusal
        && !retire_partial_observation(entry, runtime)
    {
        return PREPARE_REFUSED;
    }
    if !entry.options_registered {
        register_options(request.ini);
        entry.options_registered = true;
    }
    if !entry.stages_configured {
        // SR Stage 1 rides on Observe: the observer's start installs it after the geometry hooks.
        if unsafe { (leaves.sr)(ini, request.observe, LOG_FN) } && !request.observe {
            write_log(b"Anomaly SR stage refused: SRStage/SRStageCapture require Mode=Observe; game left stock.");
        }
        // Observe records motion variants even when disabled, so the pack can turn on live later.
        if request.observe {
            unsafe { (leaves.motion)(ini, LOG_FN) };
        }
        // FG also rides on Observe. Its presentation bridge must intercept the engine's swap-chain creation, so it
        // installs here, before the device exists. A refusal leaves presentation stock.
        let fg =
            unsafe { (leaves.fg)(ini, request.observe, runtime.module, request.build, LOG_FN) };
        if fg && !request.observe {
            write_log(
                b"Anomaly FG refused: FG/FGBridge require Mode=Observe; presentation stays stock.",
            );
        }
        if unsafe { (leaves.fg_installed)() } {
            install_input(leaves, request.build);
        }
        entry.stages_configured = request.observe;
    }
    if request.observe && entry.observation != Observation::Active {
        let status = start_observation(entry, request, runtime);
        if status != READY {
            return status;
        }
    }
    if request.overlay {
        if !unsafe { (leaves.overlay)(runtime.module, ini, LOG_FN) } {
            return OVERLAY_REFUSED;
        }
        // With the bridge presenting, the engine's Present never reaches the DXGI hooks; the panel draws from the
        // bridge's before_present into the engine-facing back buffer instead.
        if unsafe { (leaves.fg_installed)() } {
            unsafe { (leaves.overlay_sink)() };
        }
    }
    entry.started = true;
    READY
}

fn install_input(leaves: &Leaves, build: usize) {
    let mut reason: *const c_char = ptr::null();
    let status = unsafe { (leaves.input)(build, &mut reason) };
    let fallback: &[u8] = match status {
        0 => b"Anomaly input facts: guarded CInput slot 0 installed; buffered keyboard/mouse reads observed.",
        1 => b"Anomaly DirectInput observation refused.",
        _ => b"Anomaly input hook refused.",
    };
    if status != 0 && !reason.is_null() {
        write_log(unsafe { CStr::from_ptr(reason) }.to_bytes());
    } else {
        write_log(fallback);
    }
}

fn retire_partial_observation(entry: &mut Entry, runtime: &Runtime) -> bool {
    let (Some(quiesce), Some(stop)) = (runtime.quiesce, runtime.stop) else {
        write_log(b"Anomaly refused: Runtime plugin-session cleanup exports missing.");
        return false;
    };
    let result = unsafe { quiesce(entry.session) };
    log_result("partial session quiesce", result);
    if result != RSF_OK {
        return false;
    }
    let result = unsafe { stop(entry.session) };
    log_result("partial session stop", result);
    if result != RSF_OK {
        return false;
    }
    entry.session = ptr::null_mut();
    entry.observation = Observation::Unprepared;
    true
}

fn start_observation(entry: &mut Entry, request: &Request, runtime: &Runtime) -> u32 {
    let (Some(prepare), Some(start)) = (runtime.prepare, runtime.start) else {
        write_log(b"Anomaly refused: required Runtime plugin-session exports missing.");
        return RUNTIME_REFUSED;
    };
    if entry.observation == Observation::Unprepared {
        let status = prepare_observation(entry, request, prepare);
        if status != READY {
            return status;
        }
    }
    let activated = unsafe { start(entry.session) };
    log_result("session start", activated);
    if activated != RSF_OK {
        return START_REFUSED;
    }
    entry.observation = Observation::Active;
    write_log(
        b"Anomaly Observe active. This is native-boundary instrumentation, not working SR or FG.",
    );
    READY
}

fn prepare_observation(entry: &mut Entry, request: &Request, prepare: PrepareFn) -> u32 {
    static ANCHOR: u8 = 0;
    let mut module: HMODULE = ptr::null_mut();
    let flags =
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
    if unsafe { GetModuleHandleExW(flags, ptr::addr_of!(ANCHOR).cast(), &mut module) } == 0 {
        return RUNTIME_REFUSED;
    }
    let Some(plugin) = module_path(module) else {
        return RUNTIME_REFUSED;
    };
    let mut counter = 0i64;
    if unsafe { QueryPerformanceCounter(&mut counter) } == 0 || counter <= 0 {
        return RUNTIME_REFUSED;
    }
    let probe = rsf_game_probe {
        struct_size: size_of::<rsf_game_probe>() as u32,
        pe_machine: AMD64,
        reserved: 0,
        executable_name_utf8: request.identity.executable,
        sha256_hex: request.identity.sha256,
    };
    let options = SessionOptions {
        struct_size: size_of::<SessionOptions>() as u32,
        abi_version: SESSION_ABI_VERSION,
        plugin_path: plugin.as_ptr(),
        probe: &probe,
        services: rsf_game_host_services {
            struct_size: size_of::<rsf_game_host_services>() as u32,
            abi_version: RSF_GAME_ABI_VERSION,
            session_id: counter as u64,
            log: LOG_FN,
            render_pass: None,
            user: ptr::null_mut(),
            render_config: None,
            cpu_event: None,
            scope_state: None,
            process_upscale: None,
            submit: None,
            present: None,
            process_latency: None,
            render_feedback: None,
        },
    };
    write_log(b"Anomaly Observe cold startup; borrowing existing Runtime, before DirectInput/device creation.");
    let prepared = unsafe { prepare(&options, &mut entry.session) };
    log_result("session prepare", prepared);
    if prepared != RSF_OK {
        if !entry.session.is_null() {
            entry.observation = Observation::RetainedAfterRefusal;
        }
        return PREPARE_REFUSED;
    }
    if entry.session.is_null() {
        write_log(b"Anomaly refused: Runtime returned no prepared session.");
        return PREPARE_REFUSED;
    }
    entry.observation = Observation::Prepared;
    READY
}

/// Private body of the `rsf_anomaly_start` export in bootstrap.cpp.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_cold_start(
    configuration: *const u16,
    leaves: *const Leaves,
) -> u32 {
    let Some(leaves) = (unsafe { leaves.as_ref() }) else {
        return FAULT;
    };
    catch_unwind(AssertUnwindSafe(|| unsafe { enter(configuration, leaves) })).unwrap_or_else(
        |_| {
            write_log(b"Anomaly startup refused after a contained panic.");
            FAULT
        },
    )
}

#[cfg(test)]
mod tests;
