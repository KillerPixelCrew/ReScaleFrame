// SPDX-License-Identifier: MIT
//! Anomaly's use of the Runtime's one shared overlay (`rsf_get_overlay_api`).
//!
//! This module owns what the Runtime must not know about Anomaly: the binding and validation of the overlay table the
//! carrier-loaded Runtime exports, the translation of this adapter's SR, FG and option choices into the generic
//! service setup, the callbacks the Runtime calls back into, the start state, the engine-chain identity and the
//! per-panel diagnostics and backbuffer capture. The hooks and COM calls live in the adapter. Window, key and
//! INI reads are the Win32 calls in `win32` below. The service never loads a second Runtime.

use crate::fg_stage::{self, Frontend};
use crate::native_hooks::HookOps;
use crate::sys::*;
/// Rust-only records of the adapter options and chain identity.
pub(crate) mod records;
use crate::sr_stage::{OverlayChoices, STAGE};
use crate::sys::rsf_anomaly_sr_settings;
use crate::{overlay_hooks, overlay_native};
pub(crate) use records::*;
use rsf_game_sys::rsf_game_log_fn;
use rsf_game_sys::rsf_overlay_sr_report;
use std::ffi::{CStr, CString, OsString, c_char, c_int, c_void};
use std::fmt;
use std::os::windows::ffi::OsStringExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard, OnceLock, PoisonError};
use windows_sys::core::w;

use rsf_game_sys::{RSF_ERROR_ABI_MISMATCH as ABI_MISMATCH, RSF_OK as OK};
// The C++ service formatted log lines into char[768].
const LOG_CAPACITY: usize = 767;

type Status = u32;
const STARTED: Status = RSF_ANOMALY_OVERLAY_STARTED as Status;
const ALREADY_STARTED: Status = RSF_ANOMALY_OVERLAY_ALREADY_STARTED as Status;
const API_MISSING: Status = RSF_ANOMALY_OVERLAY_API_MISSING as Status;
const API_REFUSED: Status = RSF_ANOMALY_OVERLAY_API_REFUSED as Status;
const CONFIGURE_REFUSED: Status = RSF_ANOMALY_OVERLAY_CONFIGURE_REFUSED as Status;
const HOOKS_REFUSED: Status = RSF_ANOMALY_OVERLAY_HOOKS_REFUSED as Status;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A refused start or binding: the status and a NUL-terminated reason that outlives the process's use of it.
#[derive(Clone, Copy, Debug)]
struct Refusal {
    status: Status,
    reason: *const c_char,
}

impl Refusal {
    const fn new(status: Status, reason: &'static CStr) -> Self {
        Self {
            status,
            reason: reason.as_ptr(),
        }
    }

    fn text(&self) -> String {
        text(self.reason)
    }
}

fn text(reason: *const c_char) -> String {
    if reason.is_null() {
        "(none)".into()
    } else {
        unsafe { CStr::from_ptr(reason) }
            .to_string_lossy()
            .into_owned()
    }
}

#[derive(Clone, Copy)]
struct Log {
    log: rsf_game_log_fn,
    user: *mut c_void,
}

impl Log {
    fn say(&self, message: fmt::Arguments) {
        let Some(log) = self.log else { return };
        let mut line = message.to_string().replace('\0', " ");
        if line.len() > LOG_CAPACITY {
            let mut end = LOG_CAPACITY;
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            line.truncate(end);
        }
        if let Ok(line) = CString::new(line) {
            // The adapter log callback is borrowed for the process lifetime.
            unsafe { log(self.user, line.as_ptr()) };
        }
    }
}

// ---- adapter options ---------------------------------------------------------------------------------------------

/// One option the adapter registered. The label and both callbacks live for the process.
#[derive(Clone, Copy)]
struct AdapterOption {
    label: *const c_char,
    toggle: bool,
    restart: bool,
    minimum: i32,
    maximum: i32,
    step: i32,
    divisor: u32,
    get: unsafe extern "C" fn() -> i32,
    set: unsafe extern "C" fn(i32) -> bool,
}

// SAFETY: the label is an immutable or stage-owned static buffer, and get/set are plain functions.
unsafe impl Send for AdapterOption {}

impl AdapterOption {
    /// The registry skips options without both callbacks, as the C++ service did at every use.
    fn new(option: &rsf_anomaly_overlay_option) -> Option<Self> {
        Some(Self {
            label: option.label,
            toggle: option.toggle != 0,
            restart: option.restart != 0,
            minimum: option.minimum,
            maximum: option.maximum,
            step: option.step,
            divisor: option.divisor,
            get: option.get?,
            set: option.set?,
        })
    }

    fn apply(&self, value: i32) -> bool {
        (self.minimum..=self.maximum).contains(&value) && unsafe { (self.set)(value) }
    }

    fn describe(&self) -> rsf_overlay_service_option {
        rsf_overlay_service_option {
            label_utf8: self.label,
            kind: if self.toggle {
                RSF_OVERLAY_SERVICE_OPTION_TOGGLE
            } else {
                RSF_OVERLAY_SERVICE_OPTION_NUMBER
            },
            value: unsafe { (self.get)() },
            minimum: self.minimum,
            maximum: self.maximum,
            step: self.step,
            divisor: self.divisor,
            restart: self.restart.into(),
        }
    }
}

/// Built at cold start, before the overlay starts. The panel sees the first `RSF_OVERLAY_SERVICE_OPTION_LIMIT`.
static REGISTRY: Mutex<Vec<AdapterOption>> = Mutex::new(Vec::new());

pub(crate) unsafe extern "C" fn rsf_anomaly_overlay_option_add(
    option: *const rsf_anomaly_overlay_option,
) {
    if let Some(option) = unsafe { option.as_ref() }.and_then(AdapterOption::new) {
        lock(&REGISTRY).push(option);
    }
}

fn registered_options() -> Vec<AdapterOption> {
    lock(&REGISTRY)
        .iter()
        .take(RSF_OVERLAY_SERVICE_OPTION_LIMIT as usize)
        .copied()
        .collect()
}

// ---- the Runtime's overlay table ---------------------------------------------------------------------------------

type DumpFn = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *const rsf_named_frame_dump,
    *mut rsf_named_frame_dump_report,
) -> i32;
type DiagnosticsFn = unsafe extern "C" fn(*mut rsf_overlay_diagnostics) -> i32;

/// A validated table: every entry present, the size and version of this adapter's header.
#[derive(Clone, Copy)]
struct Api {
    // The admitted table itself is returned only by the test bind entry.
    #[cfg_attr(not(test), allow(dead_code))]
    table: rsf_overlay_api,
    configure: unsafe extern "C" fn(*const rsf_overlay_service_setup) -> i32,
    present: unsafe extern "C" fn(*mut c_void, *mut u32) -> i32,
    notify: unsafe extern "C" fn(*mut c_void, u32) -> i32,
    visible: unsafe extern "C" fn() -> u32,
    set_visible: unsafe extern "C" fn(u32) -> i32,
    toggle: unsafe extern "C" fn() -> i32,
    capture: unsafe extern "C" fn(*mut rsf_overlay_capture) -> i32,
    settings_get: unsafe extern "C" fn(*mut rsf_render_settings) -> i32,
}

impl Api {
    fn new(table: &rsf_overlay_api) -> Option<Self> {
        table.status?;
        table.settings_apply?;
        Some(Self {
            table: *table,
            configure: table.configure?,
            present: table.present?,
            notify: table.notify?,
            visible: table.visible?,
            set_visible: table.set_visible?,
            toggle: table.toggle?,
            capture: table.capture?,
            settings_get: table.settings_get?,
        })
    }
}

fn validate(result: i32, table: &rsf_overlay_api) -> Result<Api, Refusal> {
    if result == ABI_MISMATCH {
        return Err(Refusal::new(
            API_REFUSED,
            c"the Runtime's overlay API version does not match",
        ));
    }
    if result != OK {
        return Err(Refusal::new(
            API_REFUSED,
            c"the Runtime refused the overlay API query",
        ));
    }
    if (table.struct_size as usize) < size_of::<rsf_overlay_api>()
        || table.abi_version != RSF_OVERLAY_API_VERSION
    {
        return Err(Refusal::new(
            API_REFUSED,
            c"the Runtime's overlay API table is short or of another version",
        ));
    }
    Api::new(table).ok_or(Refusal::new(
        API_REFUSED,
        c"the Runtime's overlay API table has a missing entry",
    ))
}

/// Queries and validates a table. A refusal never writes the caller's table.
fn bind(query: rsf_get_overlay_api_fn) -> Result<Api, Refusal> {
    let Some(query) = query else {
        return Err(Refusal::new(
            API_MISSING,
            c"the Runtime does not export rsf_get_overlay_api",
        ));
    };
    let mut table = rsf_overlay_api {
        struct_size: size_of::<rsf_overlay_api>() as u32,
        abi_version: RSF_OVERLAY_API_VERSION,
        ..Default::default()
    };
    let result = unsafe { query(&mut table) };
    validate(result, &table)
}

#[cfg(test)]
unsafe fn rsf_anomaly_overlay_bind(
    query: rsf_get_overlay_api_fn,
    out: *mut rsf_overlay_api,
    reason: *mut *const c_char,
) -> u32 {
    let result = if out.is_null() {
        Err(Refusal::new(
            API_MISSING,
            c"the Runtime does not export rsf_get_overlay_api",
        ))
    } else {
        bind(query)
    };
    match result {
        Ok(api) => {
            unsafe { out.write(api.table) };
            STARTED
        }
        Err(refusal) => {
            if let Some(reason) = unsafe { reason.as_mut() } {
                *reason = refusal.reason;
            }
            refusal.status
        }
    }
}

// ---- native leaves -----------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Leaves {
    chain: unsafe extern "C" fn(*mut c_void, *mut rsf_anomaly_overlay_chain) -> u32,
    capture_backbuffer: unsafe extern "C" fn(
        *mut c_void,
        rsf_input_dump_named_texture_fn,
        *const rsf_named_frame_dump,
        *mut rsf_named_frame_dump_report,
    ) -> i32,
}

type InstallHook = fn(&HookOps, &mut *const c_char) -> bool;

struct Platform {
    leaves: Leaves,
    install_input: InstallHook,
    install_present: InstallHook,
}

const NATIVE: Platform = Platform {
    leaves: Leaves {
        chain: overlay_native::describe_chain,
        capture_backbuffer: overlay_native::capture_backbuffer,
    },
    install_input: overlay_hooks::install_input,
    install_present: |hooks, reason| {
        overlay_hooks::install_present(
            rsf_anomaly_overlay_present,
            rsf_anomaly_overlay_resize,
            hooks,
            reason,
        )
    },
};

#[derive(Clone, Copy)]
struct StageControls {
    sr_choices: fn(*mut c_void) -> OverlayChoices,
    sr_report: fn(*mut rsf_overlay_sr_report) -> bool,
    sr_request: fn(&rsf_anomaly_sr_settings),
    fg_options: fn() -> Option<Frontend>,
    fg_foreign_chain: fn(*mut c_void) -> bool,
}

const STAGES: StageControls = StageControls {
    sr_choices: |runtime| STAGE.choices(runtime),
    sr_report: |report| STAGE.overlay_report(report.cast()),
    sr_request: |settings| STAGE.request(settings),
    fg_options: fg_stage::overlay_options,
    fg_foreign_chain: fg_stage::foreign_chain,
};

impl StageControls {
    fn request_sr(&self, settings: &rsf_render_settings) {
        (self.sr_request)(&rsf_anomaly_sr_settings {
            enabled: settings.sr_enabled != 0,
            backend: settings.sr_backend,
            quality: settings.sr_quality,
            preset: settings.dlss_preset,
        });
    }
}

/// The window and key reads the Present path needs. Production uses `win32`.
#[derive(Clone, Copy)]
struct Desktop {
    engine_window: fn(*mut c_void) -> bool,
    insert_held: fn(*mut c_void) -> bool,
}

const DESKTOP: Desktop = Desktop {
    engine_window: win32::engine_window,
    insert_held: win32::insert_held,
};

// ---- configuration translation -----------------------------------------------------------------------------------

/// What this adapter can apply: FG when the presentation bridge was installed at cold start (FG=1 or FGBridge=1), with
/// every provider the Runtime knows (it masks them with the bridge's own device-dependent choices); SR only with
/// SRStage=5, as the backends whose vendor runtime is installed and the qualities Native to Ultra performance. Zero SR
/// choices tell the Runtime that SR is unavailable, so the panel offers none, a request to enable it is refused and
/// status says so. Requested and effective SR status come from `sr_report`. `table` and `configuration` must outlive
/// the configure call; the Runtime copies both.
fn make_setup(
    controls: StageControls,
    options: &rsf_anomaly_overlay_options,
    configuration: *const u16,
    adapter_options: &[AdapterOption],
    table: &mut Vec<rsf_overlay_service_option>,
) -> rsf_overlay_service_setup {
    let sr = (controls.sr_choices)(options.runtime);
    let fg = (controls.fg_options)();
    let mut setup = rsf_overlay_service_setup {
        struct_size: size_of::<rsf_overlay_service_setup>() as u32,
        abi_version: RSF_OVERLAY_API_VERSION,
        identity_utf8: c"Anomaly".as_ptr(),
        configuration_path: configuration,
        configuration_section: w!("Anomaly"),
        defaults: rsf_render_settings {
            struct_size: size_of::<rsf_render_settings>() as u32,
            abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
            sr_enabled: 0,
            sr_backend: RSF_SR_NONE,
            fg_enabled: 0,
            fg_backend: RSF_FG_BACKEND_AUTO,
            generated_frames: 1,
            latency: RSF_REFLEX_OFF,
            status_overlay: 0,
            sr_quality: 0,
            ..Default::default()
        },
        sr_backend_choices: sr.backends,
        sr_quality_choices: if sr.backends != 0 { sr.qualities } else { 0 },
        log: options.log,
        accept_window: Some(accept_window),
        sr_report: (sr.backends != 0).then_some(sr_report as _),
        settings_changed: Some(settings_changed),
        user: options.log_user,
        // Initial labels are copied; option_label refreshes live status after options_read.
        options_read: Some(options_read),
        option_changed: Some(option_changed),
        option_label: Some(option_label),
        ..Default::default()
    };
    if sr.backends != 0 {
        setup.defaults.sr_enabled = sr.defaults.enabled.into();
        setup.defaults.sr_backend = sr.defaults.backend;
        setup.defaults.sr_quality = sr.defaults.quality;
        setup.defaults.dlss_preset = sr.defaults.preset;
    }
    if let Some(fg) = fg {
        setup.defaults.fg_enabled = (fg.enabled != 0).into();
        setup.defaults.fg_backend = fg.backend;
        setup.defaults.generated_frames = fg.generated;
        setup.defaults.latency = fg.latency;
        setup.defaults.frame_limit_us = fg.frame_limit_us;
        setup.fg_backend_choices = RSF_FG_BACKEND_ALL | (1 << RSF_FG_BACKEND_AUTO);
        setup.latency_choices =
            (1 << RSF_REFLEX_OFF) | (1 << RSF_REFLEX_ON) | (1 << RSF_REFLEX_BOOST);
        setup.max_generated_frames = 3;
    } else {
        setup.fg_backend_choices = 0;
        setup.latency_choices = 1 << RSF_REFLEX_OFF;
        setup.max_generated_frames = 1;
    }
    setup.dlss_preset_available = (sr.backends != 0 && sr.preset).into();
    table.clear();
    table.extend(adapter_options.iter().map(AdapterOption::describe));
    setup.option_count = table.len() as u32;
    setup.options = if table.is_empty() {
        ptr::null()
    } else {
        table.as_ptr()
    };
    setup
}

// ---- diagnostic capture ------------------------------------------------------------------------------------------

/// What a run needs in order to judge the panel without desktop capture.
struct Capture {
    frames: u32,
    stride: u32,
    cycles: u32,
    directory: CString,
    dump: DumpFn,
}

fn register_saved_options(runtime: *mut c_void, ini: &[u16], log: &Log) {
    use crate::sys::*;
    if runtime.is_null() {
        return;
    }
    type Register = unsafe extern "C" fn(*mut c_void, *const rsf_overlay_saved_setup) -> i32;
    let Some(register) = (unsafe {
        rsf_game_sys::win32::typed_export::<Register>(
            runtime,
            c"rsf_overlay_saved_options_register",
        )
    }) else {
        log.say(format_args!(
            "Anomaly overlay: saved startup controls are unavailable in this Runtime."
        ));
        return;
    };
    let rows = [
        rsf_overlay_saved_option {
            label_utf8: c"Enable overlay at startup".as_ptr(),
            key_utf8: c"Overlay".as_ptr(),
            kind: RSF_OVERLAY_SAVED_TOGGLE,
            restart: 1,
            ..Default::default()
        },
        rsf_overlay_saved_option {
            label_utf8: c"Overlay capture directory".as_ptr(),
            key_utf8: c"OverlayCaptureDir".as_ptr(),
            kind: RSF_OVERLAY_SAVED_TEXT,
            restart: 1,
            max_text_length: RSF_OVERLAY_SAVED_TEXT_LIMIT,
            ..Default::default()
        },
    ];
    let setup = rsf_overlay_saved_setup {
        struct_size: size_of::<rsf_overlay_saved_setup>() as u32,
        abi_version: RSF_OVERLAY_SAVED_OPTIONS_ABI_VERSION,
        configuration_path: ini.as_ptr(),
        configuration_section: w!("Anomaly"),
        options: rows.as_ptr(),
        option_count: rows.len() as u32,
        ..Default::default()
    };
    // The Runtime copies both rows. Restart options retain no adapter callback or user pointer.
    let result = unsafe { register(ptr::null_mut(), &setup) };
    if result != OK {
        log.say(format_args!(
            "Anomaly overlay: saved startup controls refused ({result})."
        ));
    }
}

/// Always resolves the diagnostics (they cost nothing); the PNG capture is opt-in.
fn configure_capture(
    options: &rsf_anomaly_overlay_options,
    ini: &[u16],
    log: &Log,
) -> (Option<DiagnosticsFn>, Option<Capture>) {
    let mut diagnostics = options.diagnostics_override;
    let mut dump = options.capture_override;
    if !options.runtime.is_null() {
        if diagnostics.is_none() {
            diagnostics = unsafe {
                crate::runtime_module::export::<DiagnosticsFn>(
                    options.runtime,
                    RSF_OVERLAY_DIAGNOSTICS_ENTRY_POINT,
                )
            };
        }
        if dump.is_none() {
            dump = unsafe {
                crate::runtime_module::export::<DumpFn>(
                    options.runtime,
                    RSF_INPUT_DUMP_NAMED_TEXTURE_ENTRY_POINT,
                )
            };
        }
    }
    if diagnostics.is_none() {
        log.say(format_args!("Anomaly overlay: the Runtime has no rsf_get_overlay_diagnostics; panel logs omit input and settings counters."));
    }
    if win32::ini_int(ini, w!("OverlayCapture"), 0) == 0 {
        return (diagnostics, None);
    }
    let Some(dump) = dump else {
        log.say(format_args!("Anomaly overlay: OverlayCapture=1 but the Runtime has no rsf_input_dump_named_texture; no PNGs will be written."));
        return (diagnostics, None);
    };
    let bounded = |key, fallback, low: i32, high: i32| {
        win32::ini_int(ini, key, fallback).clamp(low, high) as u32
    };
    let frames = bounded(w!("OverlayCaptureFrames"), 3, 1, 16);
    let stride = bounded(w!("OverlayCaptureStride"), 20, 1, 600);
    let cycles = bounded(w!("OverlayCaptureCycles"), 2, 1, 8);
    let custom = win32::ini_text(ini, w!("OverlayCaptureDir"));
    let directory = if custom.is_empty() {
        let configuration = PathBuf::from(OsString::from_wide(&ini[..ini.len() - 1]));
        configuration
            .parent()
            .map(PathBuf::from)
            .unwrap_or_default()
            .join("RSF-OverlayCapture")
            .join(crate::capture_clock::local_timestamp())
            .to_string_lossy()
            .into_owned()
    } else {
        String::from_utf16_lossy(&custom)
    };
    let directory = CString::new(directory.replace('\0', "")).unwrap_or_default();
    log.say(format_args!(
        "Anomaly overlay capture armed: {frames} frames every {stride} presents after each of the first {cycles} opens and closes -> {}",
        directory.to_string_lossy()
    ));
    (
        diagnostics,
        Some(Capture {
            frames,
            stride,
            cycles,
            directory,
            dump,
        }),
    )
}

// ---- the started service -----------------------------------------------------------------------------------------

struct Service {
    api: Api,
    leaves: Leaves,
    controls: StageControls,
    desktop: Desktop,
    log: Log,
    diagnostics: Option<DiagnosticsFn>,
    capture: Option<Capture>,
    key_poll: bool,
    options: Vec<AdapterOption>,
}

// SAFETY: the log user and option labels are process-lifetime values the adapter handed over for any-thread use.
unsafe impl Send for Service {}
unsafe impl Sync for Service {}

/// Bookkeeping for the panel. Present-owner thread.
#[derive(Default)]
struct PanelRun {
    visible: bool,
    opens: u32,
    closes: u32,
    frames: u64,
    drawn: u64,
    refused: u64,
    last_result: i32,
    summaries: u32,
    base: rsf_overlay_diagnostics,
    have_base: bool,
    // Capture bookkeeping for the phase (open or close) that began at the last visibility edge.
    phase_open: bool,
    phase_active: bool,
    phase_frame: u32,
    phase_taken: u32,
    phase_cycle: u32,
}

/// The chain and device the panel is attached to, compared only and never dereferenced, and the Insert edge.
#[derive(Default)]
struct Panel {
    chain: usize,
    device: usize,
    insert_down: bool,
    reset_pending: bool,
    run: PanelRun,
}

impl Service {
    fn say(&self, message: fmt::Arguments) {
        self.log.say(message);
    }

    fn capture_bits(&self) -> u32 {
        let mut capture = rsf_overlay_capture {
            struct_size: size_of::<rsf_overlay_capture>() as u32,
            ..Default::default()
        };
        if unsafe { (self.api.capture)(&mut capture) } == OK {
            capture.captured
        } else {
            0
        }
    }

    fn read_diagnostics(&self, out: &mut rsf_overlay_diagnostics) -> bool {
        *out = rsf_overlay_diagnostics {
            struct_size: size_of::<rsf_overlay_diagnostics>() as u32,
            ..Default::default()
        };
        self.diagnostics
            .is_some_and(|diagnostics| unsafe { diagnostics(out) } == OK)
    }

    fn settings_changed(&self, applied: &rsf_render_settings) {
        self.say(format_args!(
            "Anomaly overlay settings applied: SR={} SRBackend={} SRQuality={} DLSSPreset={} FG={} StatusOverlay={} Latency={}",
            applied.sr_enabled,
            applied.sr_backend,
            applied.sr_quality,
            applied.dlss_preset,
            applied.fg_enabled,
            applied.status_overlay,
            applied.latency
        ));
        // The SR stage applies them at its next frame boundary (drained: after the frame's SR work, before Present).
        self.controls.request_sr(applied);
    }

    fn summary(&self, run: &PanelRun, when: &str) {
        let mut now = rsf_overlay_diagnostics::default();
        if !self.read_diagnostics(&mut now) || !run.have_base {
            self.say(format_args!(
                "Anomaly overlay panel #{} {when}: present drawn {} of {} visible frames ({} refused, last result {}); input and settings counters unavailable.",
                run.opens, run.drawn, run.frames, run.refused, run.last_result
            ));
            return;
        }
        let b = &run.base;
        self.say(format_args!(
            "Anomaly overlay panel #{} {when}: present drawn {} of {} visible frames ({} refused, last result {}), target {}x{}; \
             pointer events received {} consumed {} (button downs {}, wheel {}), raw mouse reports {}, keys consumed {}; \
             input frames sampled {} with a button held {}, last pointer ({},{}), cursor freed {}; \
             settings_apply attempts {} refused {} last result {}.",
            run.opens,
            run.drawn,
            run.frames,
            run.refused,
            run.last_result,
            now.chain_width,
            now.chain_height,
            now.pointer_received.wrapping_sub(b.pointer_received),
            now.pointer_consumed.wrapping_sub(b.pointer_consumed),
            now.button_downs_consumed.wrapping_sub(b.button_downs_consumed),
            now.wheel_consumed.wrapping_sub(b.wheel_consumed),
            now.raw_mouse_reports.wrapping_sub(b.raw_mouse_reports),
            now.keys_consumed.wrapping_sub(b.keys_consumed),
            now.frames_visible.wrapping_sub(b.frames_visible),
            now.frames_with_button.wrapping_sub(b.frames_with_button),
            now.last_pointer_x,
            now.last_pointer_y,
            now.cursor_freed,
            now.apply_attempts.wrapping_sub(b.apply_attempts),
            now.apply_refused.wrapping_sub(b.apply_refused),
            now.last_apply_result
        ));
    }

    /// Writes the presented backbuffer, after the panel drew into it, through the Runtime's frame dump service.
    fn capture_backbuffer(
        &self,
        capture: &Capture,
        chain: *mut c_void,
        phase: &str,
        cycle: u32,
        index: u32,
    ) {
        let name = format!("{phase}{cycle:02}_f{index:02}");
        let name_z = CString::new(name.as_str()).unwrap_or_default();
        let request = rsf_named_frame_dump {
            struct_size: size_of::<rsf_named_frame_dump>() as u32,
            abi_version: RSF_NAMED_FRAME_DUMP_ABI_VERSION,
            directory_utf8: capture.directory.as_ptr(),
            name: name_z.as_ptr(),
            view: 0, // the backbuffer is display-referred: channels written through
            scale: 1.0,
            log: None,
            log_user: ptr::null_mut(),
        };
        let mut report = rsf_named_frame_dump_report {
            struct_size: size_of::<rsf_named_frame_dump_report>() as u32,
            ..Default::default()
        };
        let result = unsafe {
            (self.leaves.capture_backbuffer)(chain, Some(capture.dump), &request, &mut report)
        };
        self.say(format_args!(
            "Anomaly overlay capture {name}: result {result} ({}x{} format {}) -> {}",
            report.width,
            report.height,
            report.dxgi_format,
            capture.directory.to_string_lossy()
        ));
    }

    fn track(
        &self,
        run: &mut PanelRun,
        chain: *mut c_void,
        target: [u32; 2],
        presented: i32,
        drawn: u32,
    ) {
        let visible = unsafe { (self.api.visible)() } != 0;
        let rising = visible && !run.visible;
        let falling = !visible && run.visible;
        if falling {
            self.summary(run, "closed");
            // The Runtime counts input only while the panel is open, so the next panel's counts start where this one
            // ended; events that arrive between the open request and its first presented frame still count.
            let mut now = rsf_overlay_diagnostics::default();
            if self.read_diagnostics(&mut now) {
                run.base = now;
            }
        }
        if rising {
            run.opens = run.opens.wrapping_add(1);
            run.frames = 0;
            run.drawn = 0;
            run.refused = 0;
            run.summaries = 0;
            run.last_result = 0;
            // The first panel's baseline is the zeroed counters of process start.
            run.have_base = self.diagnostics.is_some();
            let mut now = rsf_overlay_diagnostics::default();
            self.read_diagnostics(&mut now);
            self.say(format_args!(
                "Anomaly overlay panel #{} opened: target {}x{}, input layer installed {}, cursor freed {}.",
                run.opens, target[0], target[1], now.input_installed, now.cursor_freed
            ));
        }
        if falling {
            run.closes = run.closes.wrapping_add(1);
        }
        if rising || falling {
            run.phase_open = rising;
            run.phase_active = true;
            run.phase_frame = 0;
            run.phase_taken = 0;
            run.phase_cycle = if rising { run.opens } else { run.closes };
        }
        if visible {
            run.frames += 1;
            run.drawn += u64::from(drawn != 0);
            run.last_result = presented;
            if presented != OK || drawn == 0 {
                run.refused += 1;
            }
            // The first frames in full; after that a bounded progress line so a crash while open still leaves counters.
            if run.frames <= 3 {
                self.say(format_args!(
                    "Anomaly overlay panel #{} frame {}: api.present result {presented}, drawn {drawn}, target {}x{}.",
                    run.opens, run.frames, target[0], target[1]
                ));
            } else if run.summaries < 6 && (run.frames == 60 || run.frames.is_multiple_of(600)) {
                run.summaries += 1;
                self.summary(run, "progress");
            }
        }
        match &self.capture {
            Some(capture) if run.phase_active && run.phase_cycle <= capture.cycles => {
                if run.phase_taken < capture.frames
                    && run.phase_frame.is_multiple_of(capture.stride)
                {
                    let phase = if run.phase_open { "open" } else { "close" };
                    self.capture_backbuffer(
                        capture,
                        chain,
                        phase,
                        run.phase_cycle,
                        run.phase_taken,
                    );
                    run.phase_taken += 1;
                }
                run.phase_frame = run.phase_frame.wrapping_add(1);
                if run.phase_taken >= capture.frames {
                    run.phase_active = false;
                }
            }
            None if run.phase_active => run.phase_active = false,
            _ => {}
        }
        run.visible = visible;
    }

    fn present(&self, panel: &mut Panel, chain: *mut c_void, suspended: bool) {
        if suspended && !panel.reset_pending {
            return;
        }
        // With the FG bridge presenting, DXGI's Present also runs for the provider's physical or proxy chain (on the
        // provider's own thread for generated frames). The panel belongs to the engine-facing chain only.
        if (self.controls.fg_foreign_chain)(chain) {
            return;
        }
        let mut info = rsf_anomaly_overlay_chain::default();
        if unsafe { (self.leaves.chain)(chain, &mut info) } == 0
            || info.window.is_null()
            || !(self.desktop.engine_window)(info.window)
        {
            return;
        }
        if panel.reset_pending {
            if unsafe { (self.api.notify)(chain, RSF_OVERLAY_NOTIFY_DEVICE_RESET) } != OK {
                return;
            }
            panel.chain = 0;
            panel.device = 0;
            panel.insert_down = false;
            panel.reset_pending = false;
            panel.run.visible = false;
            panel.run.phase_active = false;
            panel.run.have_base = false;
        }
        if suspended {
            return;
        }
        let device = info.device as usize;
        // A different chain or device than the panel drew with: release it before it draws on stale objects.
        if (panel.chain != 0 && panel.chain != chain as usize)
            || (panel.device != 0 && panel.device != device)
        {
            if unsafe { (self.api.notify)(chain, RSF_OVERLAY_NOTIFY_DEVICE_RESET) } != OK {
                return;
            }
            self.say(format_args!("Anomaly overlay: the engine presented on a new swap chain or device; the panel restarts on it."));
        }
        panel.chain = chain as usize;
        panel.device = device;
        let mut drawn = 0;
        let presented = unsafe { (self.api.present)(chain, &mut drawn) };
        self.track(
            &mut panel.run,
            chain,
            [info.width, info.height],
            presented,
            drawn,
        );
        if self.key_poll {
            // Insert normally arrives as a window message. If DirectInput's focus handling swallows it, this edge
            // toggles instead; the Runtime's 250 ms debounce makes the two paths safe together.
            let down = (self.desktop.insert_held)(info.window);
            if down && !panel.insert_down {
                unsafe { (self.api.toggle)() };
            }
            panel.insert_down = down;
        }
    }
}

// ---- start -------------------------------------------------------------------------------------------------------

/// One overlay per process. A configured service whose hooks were refused stays refused: the Runtime accepts one
/// configuration and the guarded hook installs do not repeat.
struct Overlay {
    gate: Mutex<()>,
    service: OnceLock<Service>,
    started: AtomicBool,
    suspended: AtomicBool,
    desktop: Desktop,
    controls: StageControls,
}

impl Overlay {
    const fn new(desktop: Desktop, controls: StageControls) -> Self {
        Self {
            gate: Mutex::new(()),
            service: OnceLock::new(),
            started: AtomicBool::new(false),
            suspended: AtomicBool::new(false),
            desktop,
            controls,
        }
    }

    fn start(
        &self,
        options: &rsf_anomaly_overlay_options,
        platform: &Platform,
        hooks: Option<&HookOps>,
        adapter_options: Vec<AdapterOption>,
    ) -> Result<Status, Refusal> {
        let _entry = lock(&self.gate);
        if self.started.load(Ordering::Acquire) {
            return Err(Refusal::new(
                ALREADY_STARTED,
                c"the overlay was already started",
            ));
        }
        if self.service.get().is_some() {
            return Err(Refusal::new(
                HOOKS_REFUSED,
                c"the overlay hooks were refused earlier in this process",
            ));
        }
        let Some(ini) = (unsafe { crate::ini::path(options.configuration) }) else {
            return Err(Refusal::new(
                CONFIGURE_REFUSED,
                c"no adapter configuration path",
            ));
        };
        let log = Log {
            log: options.log,
            user: options.log_user,
        };
        let api = match unsafe { options.api_override.as_ref() } {
            Some(table) => validate(OK, table),
            None => bind(unsafe {
                crate::runtime_module::export(options.runtime, RSF_OVERLAY_API_ENTRY_POINT)
            }),
        }
        .inspect_err(|refusal| {
            log.say(format_args!("Anomaly overlay refused: {}.", refusal.text()));
        })?;
        let leaves = platform.leaves;
        let mut table = Vec::new();
        let setup = make_setup(
            self.controls,
            options,
            ini.as_ptr(),
            &adapter_options,
            &mut table,
        );
        let configured = unsafe { (api.configure)(&setup) };
        if configured != OK {
            let refusal = Refusal::new(
                CONFIGURE_REFUSED,
                c"the Runtime rejected the Anomaly overlay configuration",
            );
            log.say(format_args!(
                "Anomaly overlay refused: {} (result={configured}).",
                refusal.text()
            ));
            return Err(refusal);
        }
        let key_poll = win32::ini_int(ini, w!("OverlayKeyPoll"), 1) != 0;
        let (diagnostics, capture) = configure_capture(options, ini, &log);
        let service = self.service.get_or_init(|| Service {
            api,
            leaves,
            controls: self.controls,
            desktop: self.desktop,
            log,
            diagnostics,
            capture,
            key_poll,
            options: adapter_options,
        });
        let mut saved = rsf_render_settings {
            struct_size: size_of::<rsf_render_settings>() as u32,
            abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
            ..Default::default()
        };
        if unsafe { (api.settings_get)(&mut saved) } == OK {
            log.say(format_args!(
                "Anomaly overlay configured: saved SR={} SRBackend={} SRQuality={} DLSSPreset={} FG={} StatusOverlay={}. \
                 SR backend choices={:#x}, quality choices={:#x}, FG backend choices={:#x}.",
                saved.sr_enabled,
                saved.sr_backend,
                saved.sr_quality,
                saved.dlss_preset,
                saved.fg_enabled,
                saved.status_overlay,
                setup.sr_backend_choices,
                setup.sr_quality_choices,
                setup.fg_backend_choices
            ));
            if setup.sr_backend_choices != 0 {
                // The persisted (and validated) choice is what SR starts with.
                service.controls.request_sr(&saved);
            }
        }
        if options.install_hooks != 0 {
            let hooks = hooks.ok_or(Refusal::new(HOOKS_REFUSED, c"no hook operations"))?;
            // Input first: a panel that cannot keep the game from acting on mouse and keyboard must not open.
            let mut reason = ptr::null();
            if !(platform.install_input)(hooks, &mut reason) {
                log.say(format_args!(
                    "Anomaly overlay refused: DirectInput gate unavailable ({}). The panel will not start.",
                    text(reason)
                ));
                return Err(Refusal {
                    status: HOOKS_REFUSED,
                    reason,
                });
            }
            let mut reason = ptr::null();
            if !(platform.install_present)(hooks, &mut reason) {
                log.say(format_args!(
                    "Anomaly overlay refused: Present hook unavailable ({}). The panel will not start.",
                    text(reason)
                ));
                return Err(Refusal {
                    status: HOOKS_REFUSED,
                    reason,
                });
            }
        }
        register_saved_options(options.runtime, ini, &log);
        self.started.store(true, Ordering::Release);
        log.say(format_args!("Anomaly overlay active: Insert opens the shared panel; mouse and keyboard are withheld from the game while it is open."));
        Ok(STARTED)
    }

    fn committed(&self) -> Option<&Service> {
        self.started
            .load(Ordering::Acquire)
            .then(|| self.service.get())
            .flatten()
    }

    fn running(&self) -> Option<&Service> {
        self.committed()
            .filter(|_| !self.suspended.load(Ordering::Acquire))
    }

    // Hide input on the caller's thread, but leave renderer retirement to Present.
    fn suspend(&self, panel: &Mutex<Panel>) -> bool {
        self.suspended.store(true, Ordering::Release);
        let Some(service) = self.committed() else {
            return true;
        };
        let Ok(mut panel) = panel.try_lock() else {
            return false;
        };
        if panel.chain != 0 {
            panel.reset_pending = true;
        }
        if unsafe { (service.api.visible)() } != 0 {
            unsafe { (service.api.set_visible)(0) };
        }
        unsafe { (service.api.visible)() == 0 }
    }
}

static OVERLAY: Overlay = Overlay::new(DESKTOP, STAGES);
static PANEL: LazyLock<Mutex<Panel>> = LazyLock::new(Mutex::default);

pub(crate) fn suspend() -> bool {
    OVERLAY.suspend(&PANEL)
}

pub(crate) fn resume() {
    OVERLAY.suspended.store(false, Ordering::Release);
}

/// The configured service the Runtime's callbacks reach. The Runtime calls them only from present() and
/// settings_apply(), after configure succeeded.
fn service() -> Option<&'static Service> {
    OVERLAY
        .service
        .get()
        .filter(|_| !OVERLAY.suspended.load(Ordering::Acquire))
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_anomaly_overlay_start(
    options: *const rsf_anomaly_overlay_options,
    hooks: Option<&HookOps>,
    reason: *mut *const c_char,
) -> u32 {
    let result = catch_unwind(AssertUnwindSafe(|| match unsafe { options.as_ref() } {
        Some(options) => OVERLAY.start(options, &NATIVE, hooks, registered_options()),
        None => Err(Refusal::new(
            CONFIGURE_REFUSED,
            c"no adapter configuration path",
        )),
    }))
    .unwrap_or(Err(Refusal::new(
        CONFIGURE_REFUSED,
        c"contained Rust panic",
    )));
    match result {
        Ok(status) => status,
        Err(refusal) => {
            if let Some(reason) = unsafe { reason.as_mut() } {
                *reason = refusal.reason;
            }
            refusal.status
        }
    }
}

/// The capture bits (RSF_OVERLAY_CAPTURE_*) the input gate acts on; 0 before start or when the query fails.
pub(super) extern "C" fn rsf_anomaly_overlay_capture_bits(_: *mut c_void) -> u32 {
    OVERLAY.running().map_or(0, Service::capture_bits)
}

/// Present-owner thread, immediately before the real Present (never for DXGI_PRESENT_TEST).
#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_anomaly_overlay_present(_: *mut c_void, chain: *mut c_void) {
    if chain.is_null() {
        return;
    }
    if let Some(service) = OVERLAY.committed()
        && let Ok(mut panel) = PANEL.try_lock()
    {
        service.present(&mut panel, chain, OVERLAY.suspended.load(Ordering::Acquire));
    }
}

/// Before ResizeBuffers or ResizeTarget is forwarded.
extern "C" fn rsf_anomaly_overlay_resize(_: *mut c_void, chain: *mut c_void) {
    if let Ok(panel) = PANEL.try_lock()
        && panel.chain != 0
        && panel.chain == chain.addr()
        && let Some(service) = OVERLAY.running()
    {
        unsafe { (service.api.notify)(chain, RSF_OVERLAY_NOTIFY_RESIZE) };
    }
}

// ---- callbacks handed to the Runtime -----------------------------------------------------------------------------

unsafe extern "C" fn accept_window(_: *mut c_void, window: *mut c_void) -> c_int {
    c_int::from(win32::engine_window(window))
}

unsafe extern "C" fn sr_report(_: *mut c_void, report: *mut rsf_overlay_sr_report) -> c_int {
    match service() {
        Some(service) if !report.is_null() => (service.controls.sr_report)(report).into(),
        _ => 0,
    }
}

unsafe extern "C" fn settings_changed(_: *mut c_void, applied: *const rsf_render_settings) {
    if let (Some(service), Some(applied)) = (service(), unsafe { applied.as_ref() }) {
        service.settings_changed(applied);
    }
}

unsafe extern "C" fn options_read(_: *mut c_void, values: *mut i32, count: u32) {
    let Some(service) = service().filter(|_| !values.is_null()) else {
        return;
    };
    for (index, option) in service.options.iter().take(count as usize).enumerate() {
        unsafe { values.add(index).write((option.get)()) };
    }
}

unsafe extern "C" fn option_changed(_: *mut c_void, index: u32, value: i32) -> c_int {
    i32::from(
        service()
            .and_then(|service| service.options.get(index as usize))
            .is_some_and(|option| option.apply(value)),
    )
}

unsafe extern "C" fn option_label(_: *mut c_void, index: u32) -> *const c_char {
    service()
        .and_then(|service| service.options.get(index as usize))
        .map_or(ptr::null(), |option| option.label)
}

// ---- Win32 leaves ------------------------------------------------------------------------------------------------

pub(crate) use win32::engine_window;

mod win32 {
    use crate::ini;
    use std::ffi::c_void;
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_INSERT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId, IsWindow,
    };
    use windows_sys::core::{PCWSTR, w};

    // xrEngine Device_Initialize.cpp
    const ENGINE_WINDOW_CLASS: &str = "_XRAY_1.5";

    /// The engine's main window in this process.
    pub(crate) fn engine_window(window: *mut c_void) -> bool {
        if window.is_null() || unsafe { IsWindow(window) } == 0 {
            return false;
        }
        let mut process = 0;
        unsafe { GetWindowThreadProcessId(window, &mut process) };
        if process != unsafe { GetCurrentProcessId() } {
            return false;
        }
        let mut name = [0u16; 32];
        let length = unsafe { GetClassNameW(window, name.as_mut_ptr(), name.len() as i32) };
        length > 0
            && name[..length as usize]
                .iter()
                .copied()
                .eq(ENGINE_WINDOW_CLASS.encode_utf16())
    }

    /// Insert is held and `window` has the foreground.
    pub(super) fn insert_held(window: *mut c_void) -> bool {
        (unsafe { GetAsyncKeyState(i32::from(VK_INSERT)) } as u16 & 0x8000) != 0
            && unsafe { GetForegroundWindow() } == window
    }

    pub(super) fn ini_int(ini: &[u16], key: PCWSTR, fallback: i32) -> i32 {
        unsafe { ini::read_int(ini.as_ptr(), key, fallback) }
    }

    pub(super) fn ini_text(ini: &[u16], key: PCWSTR) -> Vec<u16> {
        use rsf_game_sys::profile_text;
        let mut text = vec![0u16; profile_text::read_capacity(profile_text::MAX_UNITS + 1)];
        let length = unsafe { ini::read_text(ini.as_ptr(), key, w!(""), &mut text) };
        text.truncate(length);
        profile_text::decode(text)
    }
}

#[cfg(feature = "overlay-device-fixture")]
mod fixture;
#[cfg(test)]
mod tests;
