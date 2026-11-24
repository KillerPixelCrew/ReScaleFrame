// SPDX-License-Identifier: MIT
//! The runtime DLL's shared overlay service. Graphics and callbacks stay on the caller's
//! Present thread. Published settings and the last sampled frame can be read by any thread.
use crate::d3d11_d3d12::borrow;
use crate::d3d11_present::{
    rsf_d3d11_present_backend, rsf_d3d11_present_backend_choices, rsf_d3d11_present_has_owner,
    rsf_d3d11_present_request, rsf_d3d11_present_session, rsf_d3d11_present_switch_result,
};
use crate::dlss_pipeline::rsf_core_dlss_pipeline_apply_sharpening;
use crate::frame_limit::rsf_core_overlay_refresh_mhz;
use crate::graphics_d3d11::rsf_call_native_log;
use crate::native_fg::{rsf_native_fg_options_set, rsf_native_fg_status_get};
#[cfg(not(test))]
use crate::overlay_host::native::rsf_overlay_host_present;
use crate::overlay_input::{rsf_overlay_input_set_visible, rsf_overlay_input_visible};
use crate::overlay_input_route::rsf_overlay_input_counters_get;
#[cfg(not(test))]
use crate::overlay_wrapped12::native::{
    rsf_overlay_d3d12_frame_for_window, rsf_overlay_d3d12_stop,
};
use crate::sharpening_policy;
use crate::streamline_sr::preset_valid;
use crate::sync::lock;
use crate::wide::copy_nul as copy_wide;
#[cfg(not(test))]
use crate::{
    overlay_input::rsf_overlay_input_poll_toggle,
    sys::rsf_observer_present_event,
    sys::{RSF_OBSERVER_ABI_VERSION, rsf_observer_options},
};
#[cfg(not(test))]
use windows::Win32::Graphics::Dxgi::DXGI_PRESENT_TEST;
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;
use windows_sys::Win32::{
    System::{SystemInformation::GetTickCount64, Threading::GetCurrentProcessId},
    UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
};

use crate::d3d11_present::rsf_d3d11_present_provider;

unsafe extern "C" {
    #[cfg(not(test))]
    fn rsf_observer_install_guarded(options: *const rsf_observer_options) -> i32;
}

use crate::{
    fg_status,
    overlay_settings::{
        apply_settings_intent, fill_latency_stats, fill_sharpening_stats, settings_changed, sr_name,
    },
    present_rates::Rates,
    settings_store::{
        rsf_settings_store_read, rsf_settings_store_read_layout, rsf_settings_store_write,
        rsf_settings_store_write_layout, set_sharpening, sharpening_of,
    },
    sys::*,
    sys::{
        RSF_FG_BACKEND_ALL, RSF_FG_BACKEND_AUTO, RSF_FG_FIXED, RSF_FG_OFF,
        RSF_FG_STAT_TOTAL_PRESENTED, RSF_NATIVE_FG_REASON_NONE, RSF_REFLEX_BOOST, RSF_REFLEX_OFF,
        RSF_RENDER_FRONTEND_ABI_VERSION, RSF_SR_FSR2, RSF_SR_XESS, rsf_fg_status as ProviderStatus,
        rsf_native_fg_options, rsf_native_fg_status as FgStatus, rsf_render_settings as Settings,
        rsf_sharpening_options as Sharpening,
    },
};
use rsf_game_sys::ffi_util::has_capacity;
use rsf_game_sys::{RSF_FRAME_SOURCE_RENDERED, RSF_SR_AUTO, rsf_game_log_fn};
use std::{
    ffi::{CStr, CString, c_char, c_void},
    ops::RangeInclusive,
    ptr::{null, null_mut},
    sync::{
        LazyLock, Mutex, MutexGuard, OnceLock,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};

const OK: i32 = 0;
const INVALID_ARGUMENT: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const NOT_READY: i32 = -3;
const BUSY: i32 = -4;

const COLD: u8 = 0;
const STARTED: u8 = 1;
const FAILED: u8 = 2;
const RESET_PENDING: u8 = 3;
const AUTO_FG: u32 = 1 << RSF_FG_BACKEND_AUTO;
const AUTO_SR: u32 = 1 << 7; // RSF_SR_AUTO in sr_session.h
const KNOWN_SR: u32 = (1 << RSF_SR_DLSS)
    | (1 << RSF_SR_FSR2)
    | (1 << RSF_SR_FSR3)
    | (1 << RSF_SR_FSR4)
    | (1 << RSF_SR_XESS)
    | (1 << RSF_SR_SPATIAL)
    | (1 << RSF_SR_AUTO);
const KNOWN_LATENCY: u32 = (1 << RSF_REFLEX_OFF) | (1 << RSF_REFLEX_ON) | (1 << RSF_REFLEX_BOOST);
const OPTION_LIMIT: usize = RSF_OVERLAY_SERVICE_OPTION_LIMIT as usize;
const APPLY_LOG_LIMIT: u32 = 200;
type Status = rsf_render_status;

#[cfg(not(test))]
use crate::overlay_host::native::{rsf_overlay_host_start, rsf_overlay_host_stop as host_stop};

#[cfg(not(test))]
unsafe fn host_start(chain: *mut c_void, log: rsf_game_log_fn, user: *mut c_void) -> i32 {
    i32::from(unsafe { rsf_overlay_host_start(chain, log, user) } != 0)
}

#[cfg(not(test))]
unsafe fn host_present(
    chain: *mut c_void,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
) -> i32 {
    i32::from(unsafe { rsf_overlay_host_present(chain, stats, intent) } != 0)
}

// Unit fixtures exercise admission and reset independently of native graphics.
#[cfg(test)]
unsafe fn host_start(_: *mut c_void, _: rsf_game_log_fn, _: *mut c_void) -> i32 {
    0
}
#[cfg(test)]
unsafe fn host_present(
    _: *mut c_void,
    _: *const rsf_overlay_stats,
    _: *mut rsf_overlay_intent,
) -> i32 {
    0
}
#[cfg(test)]
fn host_stop() {}

static CLAIMED: AtomicBool = AtomicBool::new(false);
static CONFIG: OnceLock<Config> = OnceLock::new();
static LIFECYCLE: AtomicU8 = AtomicU8::new(COLD);
static PANEL: Mutex<()> = Mutex::new(());
static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::default()));
static APPLY_ACTIVE: AtomicBool = AtomicBool::new(false);
// Keep refusal accounting independent of the settings lock, which covers INI I/O.
static APPLY_DIAGNOSTICS: Mutex<ApplyDiagnostics> = Mutex::new(ApplyDiagnostics {
    attempts: 0,
    refused: 0,
    last_result: OK,
});

fn state() -> MutexGuard<'static, State> {
    lock(&STATE)
}

fn apply_diagnostics() -> MutexGuard<'static, ApplyDiagnostics> {
    lock(&APPLY_DIAGNOSTICS)
}

#[derive(Clone, Copy)]
struct ApplyDiagnostics {
    attempts: u32,
    refused: u32,
    last_result: i32,
}

fn record_apply(result: i32) -> u32 {
    let mut diagnostics = apply_diagnostics();
    diagnostics.attempts = diagnostics.attempts.wrapping_add(1);
    diagnostics.refused = diagnostics.refused.wrapping_add(u32::from(result != OK));
    diagnostics.last_result = result;
    diagnostics.attempts
}

// Keep persistence, forwarding and notification in order. A callback's reentrant
// update receives BUSY immediately, even if it came through a different thread.
struct ApplyAdmission;

impl ApplyAdmission {
    fn try_enter() -> Option<Self> {
        APPLY_ACTIVE
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| Self)
    }
}

impl Drop for ApplyAdmission {
    fn drop(&mut self) {
        APPLY_ACTIVE.store(false, Ordering::Release);
    }
}

const fn abi_size<T>() -> u32 {
    size_of::<T>() as u32
}

fn has(mask: u32, id: u32) -> bool {
    id < u32::BITS && mask & (1 << id) != 0
}

struct Callbacks {
    user: *mut c_void,
    log: rsf_game_log_fn,
    accept: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> i32>,
    report: Option<unsafe extern "C" fn(*mut c_void, *mut rsf_overlay_sr_report) -> i32>,
    changed: Option<unsafe extern "C" fn(*mut c_void, *const Settings)>,
    options_read: Option<unsafe extern "C" fn(*mut c_void, *mut i32, u32)>,
    option_changed: Option<unsafe extern "C" fn(*mut c_void, u32, i32) -> i32>,
    option_label: Option<unsafe extern "C" fn(*mut c_void, u32) -> *const c_char>,
}

// The adapter keeps the opaque user pointer valid for process lifetime. Rust only forwards it.
// Each callback is invoked on the thread allowed by the C contract.
unsafe impl Send for Callbacks {}
unsafe impl Sync for Callbacks {}

impl Callbacks {
    fn say(&self, message: std::fmt::Arguments<'_>) {
        if self.log.is_some() {
            let message = message.to_string();
            // Match the native log's 256-byte buffer, including the terminator.
            let mut bytes = [0u8; 256];
            let length = message.len().min(bytes.len() - 1);
            bytes[..length].copy_from_slice(&message.as_bytes()[..length]);
            unsafe { rsf_call_native_log(self.log, self.user, bytes.as_ptr().cast()) };
        }
    }
}

struct OptionSpec {
    label: CString,
    kind: u32,
    range: RangeInclusive<i32>,
    step: i32,
    divisor: u32,
    restart: u32,
    initial: i32,
}

struct Config {
    ini: Vec<u16>,
    section: Vec<u16>,
    identity: CString,
    defaults: Settings,
    sr_choices: u32,
    quality_choices: u32,
    fg_choices: u32,
    latency_choices: u32,
    max_generated: u32,
    preset_available: u32,
    deferred: bool,
    callbacks: Callbacks,
    options: Vec<OptionSpec>,
}

// The setup contract supplies terminated Windows paths. Keep their original code units.
impl Config {
    unsafe fn read_setup(setup: &rsf_overlay_service_setup) -> Result<(Self, Sharpening), i32> {
        if setup.option_count > RSF_OVERLAY_SERVICE_OPTION_LIMIT
            || (setup.option_count != 0
                && (setup.options.is_null() || setup.option_changed.is_none()))
        {
            return Err(INVALID_ARGUMENT);
        }
        let mut options = Vec::with_capacity(setup.option_count as usize);
        for index in 0..setup.option_count as usize {
            let entry = unsafe { &*setup.options.add(index) };
            if entry.label_utf8.is_null()
                || unsafe { *entry.label_utf8 } == 0
                || entry.kind > RSF_OVERLAY_SERVICE_OPTION_NUMBER
                || entry.maximum < entry.minimum
            {
                return Err(INVALID_ARGUMENT);
            }
            let toggle = entry.kind == RSF_OVERLAY_SERVICE_OPTION_TOGGLE;
            options.push(OptionSpec {
                label: unsafe { CStr::from_ptr(entry.label_utf8) }.to_owned(),
                kind: if toggle {
                    RSF_OVERLAY_HOST_OPTION_TOGGLE
                } else {
                    RSF_OVERLAY_HOST_OPTION_NUMBER
                },
                range: if toggle {
                    0..=1
                } else {
                    entry.minimum..=entry.maximum
                },
                step: entry.step.max(1),
                divisor: entry.divisor.max(1),
                restart: entry.restart,
                initial: entry.value,
            });
        }
        if setup.configuration_path.is_null()
            || unsafe { *setup.configuration_path } == 0
            || setup.configuration_section.is_null()
            || unsafe { *setup.configuration_section } == 0
            || setup.sr_backend_choices & !(KNOWN_SR | AUTO_SR) != 0
            || (setup.sr_backend_choices != 0 && setup.sr_quality_choices == 0)
            || setup.sr_quality_choices & !RSF_OVERLAY_SR_QUALITY_EXTENDED_MASK != 0
            || setup.fg_backend_choices & !(RSF_FG_BACKEND_ALL | AUTO_FG) != 0
            || !has(setup.latency_choices, RSF_REFLEX_OFF)
            || setup.latency_choices & !KNOWN_LATENCY != 0
            || !(1..=15).contains(&setup.max_generated_frames)
            || setup.defaults.struct_size < abi_size::<Settings>()
        {
            return Err(INVALID_ARGUMENT);
        }
        if setup.defaults.abi_version != RSF_RENDER_FRONTEND_ABI_VERSION {
            return Err(ABI_MISMATCH);
        }
        let mut defaults = setup.defaults;
        let sharpening = sharpening_policy::defaults();
        let embedded = sharpening_of(&defaults, &sharpening);
        if sharpening_policy::fields_are_zero(&embedded) {
            set_sharpening(&mut defaults, &sharpening);
        }
        defaults.struct_size = abi_size::<Settings>();
        let config = Self {
            ini: unsafe { copy_wide(setup.configuration_path) },
            section: unsafe { copy_wide(setup.configuration_section) },
            identity: if setup.identity_utf8.is_null() {
                c"adapter".to_owned()
            } else {
                unsafe { CStr::from_ptr(setup.identity_utf8) }.to_owned()
            },
            defaults,
            sr_choices: setup.sr_backend_choices,
            quality_choices: setup.sr_quality_choices,
            fg_choices: setup.fg_backend_choices,
            latency_choices: setup.latency_choices,
            max_generated: setup.max_generated_frames,
            preset_available: u32::from(setup.dlss_preset_available != 0),
            deferred: false,
            callbacks: Callbacks {
                user: setup.user,
                log: setup.log,
                accept: setup.accept_window,
                report: setup.sr_report,
                changed: setup.settings_changed,
                options_read: setup.options_read,
                option_changed: setup.option_changed,
                option_label: setup.option_label,
            },
            options,
        };
        if !config.valid(&defaults) {
            return Err(INVALID_ARGUMENT);
        }
        Ok((config, sharpening))
    }

    fn accepts_fg_backend(&self, backend: u32) -> bool {
        if self.fg_choices == 0 {
            backend == RSF_FG_BACKEND_AUTO
        } else {
            has(self.fg_choices, backend)
        }
    }

    fn valid(&self, value: &Settings) -> bool {
        let sr_valid = if self.sr_choices == 0 {
            value.sr_enabled == 0
                && value.sr_backend == self.defaults.sr_backend
                && value.sr_quality == self.defaults.sr_quality
        } else {
            has(self.sr_choices, value.sr_backend) && has(self.quality_choices, value.sr_quality)
        };
        value.sr_enabled <= 1
            && value.fg_enabled <= 1
            && value.status_overlay <= 1
            && sr_valid
            && (value.fg_enabled == 0 || self.fg_choices != 0)
            && self.accepts_fg_backend(value.fg_backend)
            && has(self.latency_choices, value.latency)
            && (1..=self.max_generated).contains(&value.generated_frames)
            && preset_valid(value.dlss_preset)
            && sharpening_policy::valid(&sharpening_of(value, &sharpening_policy::defaults()))
    }

    fn sanitize(&self, mut value: Settings) -> Settings {
        let defaults = &self.defaults;
        if value.sr_enabled > 1 || self.sr_choices == 0 {
            value.sr_enabled = defaults.sr_enabled;
        }
        if !has(self.sr_choices, value.sr_backend) {
            value.sr_backend = defaults.sr_backend;
        }
        if self.sr_choices == 0 || !has(self.quality_choices, value.sr_quality) {
            value.sr_quality = defaults.sr_quality;
        }
        if value.fg_enabled > 1 || (value.fg_enabled != 0 && self.fg_choices == 0) {
            value.fg_enabled = defaults.fg_enabled;
        }
        if !self.accepts_fg_backend(value.fg_backend) {
            value.fg_backend = defaults.fg_backend;
        }
        if value.status_overlay > 1 {
            value.status_overlay = defaults.status_overlay;
        }
        if !has(self.latency_choices, value.latency) {
            value.latency = defaults.latency;
        }
        if !(1..=self.max_generated).contains(&value.generated_frames) {
            value.generated_frames = defaults.generated_frames;
        }
        if !preset_valid(value.dlss_preset) {
            value.dlss_preset = defaults.dlss_preset;
        }
        let options =
            sharpening_policy::sanitize(sharpening_of(&value, &sharpening_policy::defaults()));
        set_sharpening(&mut value, &options);
        value
    }
}

struct Sample {
    sr: Option<rsf_overlay_sr_report>,
    fg: FgStatus,
    fg_owner: bool,
    fg_backend: u32,
    bridge_choices: u32,
    switch_result: i32,
}

impl Default for Sample {
    fn default() -> Self {
        Self {
            sr: None,
            // This C snapshot contains only scalar fields. Before Present they are unavailable.
            fg: unsafe { std::mem::zeroed() },
            fg_owner: false,
            fg_backend: 0,
            bridge_choices: 0,
            switch_result: OK,
        }
    }
}

struct State {
    settings: Settings,
    sampled: bool,
    sample: Sample,
    chain_size: [u32; 2],
    presents: u64,
    presents_drawn: u64,
    request_result: i32,
    panel_apply_result: i32,
    sr_change_id: u32,
    sr_change_status: u32,
    options: [i32; OPTION_LIMIT],
    ui_preferences: u32,
    rates: Rates,
    refresh_tick: u64,
    refresh_mhz: u32,
}

impl Default for State {
    fn default() -> Self {
        Self {
            // Settings are inaccessible until CONFIG is published.
            settings: unsafe { std::mem::zeroed() },
            sampled: false,
            sample: Sample::default(),
            chain_size: [0; 2],
            presents: 0,
            presents_drawn: 0,
            request_result: OK,
            panel_apply_result: OK,
            sr_change_id: 0,
            sr_change_status: 0,
            options: [0; OPTION_LIMIT],
            ui_preferences: 0,
            rates: Rates::default(),
            refresh_tick: 0,
            refresh_mhz: 0,
        }
    }
}

impl State {
    fn fg_choices(&self, config: &Config) -> u32 {
        config.fg_choices & (self.sample.bridge_choices | AUTO_FG)
    }

    fn description(&self, config: &Config, idle: bool) -> Option<&CStr> {
        if !self.sampled {
            return Some(c"Waiting for the first Present");
        }
        let sr_text = self.sample.sr.as_ref().and_then(|report| {
            // The copy always has a terminator, even when the callback fills every byte.
            (report.reason[0] != 0).then(|| unsafe { CStr::from_ptr(report.reason.as_ptr()) })
        });
        let sr_active = self
            .sample
            .sr
            .is_some_and(|report| report.effective_backend != RSF_SR_NONE);
        if self.settings.sr_enabled != 0 && !sr_active {
            return Some(sr_text.unwrap_or(if self.sample.sr.is_some() {
                c"SR requested; the adapter reports no SR output"
            } else {
                c"SR is not connected by the adapter"
            }));
        }
        if self.settings.fg_enabled != 0 {
            let fg = &self.sample.fg;
            if !self.sample.fg_owner {
                return Some(c"FG presentation owner unavailable");
            }
            if self.request_result != OK {
                return Some(c"FG provider request refused");
            }
            if self.sample.switch_result != OK {
                return Some(c"FG provider switch failed; previous provider retained");
            }
            if fg.last_result != OK {
                return Some(c"FG backend error");
            }
            if let Some(description) = crate::fg_reasons::description(fg.reason) {
                return Some(description);
            }
            if fg.vendor.active == 0 {
                return Some(c"FG requested; the provider reports inactive");
            }
            if fg.source_provenance == RSF_FRAME_SOURCE_RENDERED {
                return Some(c"FG active; input timing unavailable, latency Off");
            }
        }
        if config.sr_choices == 0 {
            return Some(c"Super resolution is not available in this adapter");
        }
        if !idle {
            return None;
        }
        if self.settings.sr_enabled != 0 && sr_active && sr_text.is_some() {
            return sr_text;
        }
        Some(if self.settings.fg_enabled != 0 {
            c"Active"
        } else {
            c"FG disabled by setting"
        })
    }
}

fn copy_reason(dest: &mut [c_char], reason: &CStr) {
    for (out, byte) in dest.iter_mut().zip(reason.to_bytes_with_nul()) {
        *out = *byte as c_char;
    }
}

fn forward_fg(value: &Settings) -> i32 {
    let mut current: FgStatus = unsafe { std::mem::zeroed() };
    current.struct_size = abi_size::<FgStatus>();
    unsafe { rsf_native_fg_status_get(&mut current) };
    let options = rsf_native_fg_options {
        struct_size: abi_size::<rsf_native_fg_options>(),
        mode: if value.fg_enabled != 0 {
            RSF_FG_FIXED
        } else {
            RSF_FG_OFF
        },
        generated_frames: value.generated_frames,
        reflex_mode: value.latency,
        frame_limit_us: value.frame_limit_us,
        ..current.requested
    };
    unsafe { rsf_native_fg_options_set(&options) };
    if value.fg_enabled == 0
        || value.fg_backend == RSF_FG_BACKEND_AUTO
        || rsf_d3d11_present_has_owner() == 0
        || rsf_d3d11_present_backend() == value.fg_backend
    {
        return OK;
    }
    rsf_d3d11_present_request(value.fg_backend)
}

unsafe extern "C" fn configure(setup: *const rsf_overlay_service_setup) -> i32 {
    unsafe { configure_inner(setup, false) }
}

#[cfg(not(test))]
pub(super) unsafe extern "C" fn rsf_overlay_configure_bound(
    setup: *const rsf_overlay_service_setup,
) -> i32 {
    unsafe { configure_inner(setup, true) }
}

unsafe fn configure_inner(setup: *const rsf_overlay_service_setup, deferred: bool) -> i32 {
    if !unsafe { has_capacity(setup) } {
        return INVALID_ARGUMENT;
    }
    let setup = unsafe { &*setup };
    if setup.abi_version != RSF_OVERLAY_API_VERSION {
        return ABI_MISMATCH;
    }
    let (mut config, sharpening) = match unsafe { Config::read_setup(setup) } {
        Ok(config) => config,
        Err(error) => return error,
    };
    if CLAIMED.swap(true, Ordering::AcqRel) {
        return BUSY;
    }
    config.deferred = deferred;

    let mut stored = config.defaults;
    let ini = config.ini.as_ptr();
    let section = config.section.as_ptr();
    unsafe { rsf_settings_store_read(ini, section, &config.defaults, &sharpening, &mut stored) };
    let settings = config.sanitize(stored);
    let layout = unsafe { rsf_settings_store_read_layout(ini, section) };
    if !config.valid(&stored) {
        config.callbacks.say(format_args!(
            "RSF overlay: saved settings this adapter cannot apply were replaced by its defaults."
        ));
    }
    let requested = if deferred {
        OK
    } else {
        rsf_core_dlss_pipeline_apply_sharpening(&settings);
        forward_fg(&settings)
    };
    {
        let mut state = state();
        state.settings = settings;
        state.ui_preferences = layout;
        state.request_result = requested;
        for (value, spec) in state.options.iter_mut().zip(&config.options) {
            *value = spec.initial;
        }
    }
    // The one-time claim guarantees a single publisher. Callers can read only after publication.
    let config = CONFIG.get_or_init(|| config);
    config.callbacks.say(format_args!(
        "RSF overlay service configured for {}; Insert opens the shared panel.",
        config.identity.to_string_lossy()
    ));
    OK
}

fn applied_result(requested: i32, fallback: i32) -> i32 {
    if requested != OK { requested } else { fallback }
}

fn log_apply(config: Option<&Config>, request: Option<&Settings>, origin: &str, result: i32) {
    let attempt = record_apply(result);
    if let Some(config) = config.filter(|_| attempt <= APPLY_LOG_LIMIT) {
        if let Some(value) = request {
            let outcome = if result == OK {
                "applied and saved"
            } else {
                "refused"
            };
            config.callbacks.say(format_args!(
                "RSF overlay: settings_apply #{attempt} from {origin}: SR={} SRBackend={} SRQuality={} FG={} FGBackend={} Generated={} Latency={} StatusOverlay={} -> {outcome} ({result}).",
                value.sr_enabled, value.sr_backend, value.sr_quality, value.fg_enabled,
                value.fg_backend, value.generated_frames, value.latency, value.status_overlay,
            ));
        } else {
            config.callbacks.say(format_args!(
                "RSF overlay: settings_apply #{attempt} from {origin}: short request refused ({result})."
            ));
        }
    }
}

fn apply(config: &Config, request: impl FnOnce(Settings) -> Settings, origin: &str) -> i32 {
    let (applied, result, requested) = {
        let Some(_admission) = ApplyAdmission::try_enter() else {
            // Record overlaps without invoking the adapter again inside its callback.
            record_apply(BUSY);
            return BUSY;
        };
        let mut state = state();
        let mut value = request(state.settings);
        value.struct_size = abi_size::<Settings>();
        let result = if config.valid(&value) {
            // Share this lock with layout writes: both update the same INI section.
            unsafe {
                rsf_settings_store_write(config.ini.as_ptr(), config.section.as_ptr(), &value)
            }
        } else {
            INVALID_ARGUMENT
        };
        if result == OK {
            state.settings = value;
        }
        drop(state);
        let requested = if result == OK {
            let requested = if config.deferred {
                OK
            } else {
                rsf_core_dlss_pipeline_apply_sharpening(&value);
                forward_fg(&value)
            };
            self::state().request_result = requested;
            if let Some(changed) = config.callbacks.changed {
                unsafe { changed(config.callbacks.user, &value) };
            }
            requested
        } else {
            OK
        };
        (value, result, requested)
    };
    if requested != OK {
        config.callbacks.say(format_args!(
            "RSF overlay: FG provider {} request result={requested}.",
            applied.fg_backend
        ));
    }
    log_apply(Some(config), Some(&applied), origin, result);
    result
}

impl Sample {
    // The provider and session are borrowed on the Present owner until its drained boundary.
    fn read(config: &Config, bound_status: Option<FgStatusGet>) -> Self {
        let sr = config
            .callbacks
            .report
            .filter(|_| config.sr_choices != 0)
            .and_then(|report| {
                let mut out = rsf_overlay_sr_report {
                    struct_size: abi_size::<rsf_overlay_sr_report>(),
                    ..Default::default()
                };
                let reported = unsafe { report(config.callbacks.user, &mut out) } != 0;
                let last = out.reason.len() - 1;
                out.reason[last] = 0;
                reported.then_some(out)
            });
        let mut fg: FgStatus = unsafe { std::mem::zeroed() };
        fg.struct_size = abi_size::<FgStatus>();
        unsafe { bound_status.unwrap_or(rsf_native_fg_status_get)(&mut fg) };
        let fg_owner = rsf_d3d11_present_has_owner() != 0;
        let mut fg_backend = 0;
        if fg_owner {
            let provider = unsafe { rsf_d3d11_present_provider().as_ref() };
            let session = rsf_d3d11_present_session();
            if let Some(status) = provider
                .and_then(|provider| provider.status)
                .filter(|_| bound_status.is_none() && !session.is_null())
            {
                let mut fresh: ProviderStatus = unsafe { std::mem::zeroed() };
                fresh.struct_size = abi_size::<ProviderStatus>();
                let queried = unsafe { status(session, &mut fresh) };
                if queried == OK {
                    let eligible = fg.requested.mode != RSF_FG_OFF
                        && fg.reason == RSF_NATIVE_FG_REASON_NONE
                        && fg.last_result == OK;
                    fg.vendor = fg_status::completed_activity(fresh, Some(&fg.vendor), eligible);
                } else {
                    fg_status::deactivate(&mut fg.vendor);
                    fg.last_result = queried;
                }
            }
            fg_backend = rsf_d3d11_present_backend();
        }
        Self {
            sr,
            fg,
            fg_owner,
            fg_backend,
            bridge_choices: rsf_d3d11_present_backend_choices(),
            switch_result: rsf_d3d11_present_switch_result(),
        }
    }
}

fn dimension(reported: u32, chain: u32) -> u32 {
    if reported != 0 { reported } else { chain }
}

impl State {
    fn record_sample(&mut self, sample: Sample, size: [u32; 2], now: u64) {
        self.presents = self.presents.wrapping_add(1);
        self.sampled = true;
        self.chain_size = size;
        self.rates.sample(
            self.presents,
            sample.fg_backend,
            sample.fg.vendor.total_presented,
            sample.fg.available != 0
                && sample.fg.vendor.valid_statistics & RSF_FG_STAT_TOTAL_PRESENTED != 0,
            now,
        );
        self.sample = sample;
    }

    fn panel_stats(&self, config: &Config, reason: &mut [c_char; 160]) -> rsf_overlay_stats {
        let settings = &self.settings;
        let sample = &self.sample;
        let sr = sample.sr.unwrap_or_default();
        let backend = if config.sr_choices == 0 {
            RSF_SR_NONE
        } else {
            settings.sr_backend
        };
        let sr_active = sr.effective_backend != RSF_SR_NONE;
        let vendor = &sample.fg.vendor;
        let fg_enabled = settings.fg_enabled != 0;
        if let Some(text) = self.description(config, false) {
            copy_reason(reason, text);
        }
        let mut stats = rsf_overlay_stats {
            struct_size: abi_size::<rsf_overlay_stats>(),
            backend_loaded: 1,
            backend_supported: u32::from(sr_active),
            backend_name: sr_name(if sr_active {
                sr.effective_backend
            } else {
                backend
            })
            .as_ptr(),
            render_width: sr.render_width,
            render_height: sr.render_height,
            output_width: dimension(sr.output_width, self.chain_size[0]),
            output_height: dimension(sr.output_height, self.chain_size[1]),
            frames_presented: self.presents as u32,
            frames_evaluated: sr.frames_evaluated,
            frames_refused: sr.frames_refused,
            last_result: sr.last_result,
            have_scene_color: u32::from(sr.inputs & RSF_OVERLAY_SR_INPUT_COLOR != 0),
            have_depth: u32::from(sr.inputs & RSF_OVERLAY_SR_INPUT_DEPTH != 0),
            have_motion: u32::from(sr.inputs & RSF_OVERLAY_SR_INPUT_MOTION != 0),
            have_exposure: u32::from(sr.inputs & RSF_OVERLAY_SR_INPUT_EXPOSURE != 0),
            motion_decoded: u32::from(sr.inputs & RSF_OVERLAY_SR_INPUT_MOTION != 0 && sr_active),
            jitter_active: u32::from(sr.inputs & RSF_OVERLAY_SR_INPUT_JITTER != 0),
            jitter_pixels: sr.jitter_pixels,
            quality: if sr_active {
                sr.effective_quality
            } else {
                settings.sr_quality
            },
            enabled: settings.sr_enabled,
            backend,
            requested_backend: backend,
            last_switch_result: self.panel_apply_result,
            sr_backend_choices: config.sr_choices,
            sr_quality_choices: if config.sr_choices != 0 {
                config.quality_choices
            } else {
                0
            },
            sr_change_id: self.sr_change_id,
            sr_change_status: self.sr_change_status,
            dlss_preset: settings.dlss_preset,
            dlss_preset_available: config.preset_available,
            sr_quality_locked: u32::from(sr.quality_locked != 0),
            application_presented_frames: self.presents,
            show_performance_hud: settings.status_overlay,
            fg_available: sample.fg.available,
            fg_backend: if !fg_enabled && vendor.configured_mode == RSF_FG_OFF {
                0
            } else {
                sample.fg_backend
            },
            fg_requested_backend: if fg_enabled { settings.fg_backend } else { 0 },
            fg_backend_choices: self.fg_choices(config)
                | 1
                | if sample.fg_owner {
                    RSF_OVERLAY_FG_RUNTIME_SWITCH
                } else {
                    0
                },
            fg_requested_mode: if fg_enabled { RSF_FG_FIXED } else { RSF_FG_OFF },
            fg_requested_generated: settings.generated_frames,
            fg_effective_mode: vendor.effective_mode,
            fg_effective_generated: vendor.effective_generated_frames,
            fg_active: vendor.active,
            fg_max_generated: vendor.max_generated_frames,
            fg_mfg_unlock_status: vendor.mfg_unlock_status,
            reflex_available: vendor.low_latency_available,
            latency_pacing_owner: vendor.pacing_owner,
            latency_paced_frames: vendor.latency_paced_frames,
            reflex_requested_mode: settings.latency,
            reflex_effective_mode: vendor.effective_reflex,
            fg_reason: sample.fg.reason,
            fg_last_result: sample.fg.last_result,
            fg_total_presented: vendor.total_presented,
            fg_present_count_valid: u32::from(
                sample.fg.available != 0
                    && vendor.valid_statistics & RSF_FG_STAT_TOTAL_PRESENTED != 0,
            ),
            frame_limit_us: settings.frame_limit_us,
            fg_selection_result: applied_result(self.request_result, sample.switch_result),
            refusal_reason: if reason[0] != 0 {
                reason.as_ptr()
            } else {
                null()
            },
            sharpening_available: u32::from(config.sr_choices != 0),
            ui_preferences: self.ui_preferences,
            ..Default::default()
        };
        fill_sharpening_stats(
            &mut stats,
            &sharpening_of(settings, &sharpening_policy::defaults()),
        );
        stats
    }

    fn status(&self, config: &Config, foreground: u32) -> Status {
        let wanted = &self.settings;
        let sample = &self.sample;
        let sr = sample.sr.unwrap_or_default();
        let fg = &sample.fg;
        let vendor = &fg.vendor;
        let mut value = Status {
            struct_size: abi_size::<Status>(),
            abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
            sr_enabled: wanted.sr_enabled,
            sr_requested_backend: if config.sr_choices == 0 {
                RSF_SR_NONE
            } else {
                wanted.sr_backend
            },
            sr_effective_backend: sr.effective_backend,
            render_width: sr.render_width,
            render_height: sr.render_height,
            output_width: dimension(sr.output_width, self.chain_size[0]),
            output_height: dimension(sr.output_height, self.chain_size[1]),
            fg_requested: wanted.fg_enabled,
            fg_requested_backend: if wanted.fg_enabled != 0 {
                wanted.fg_backend
            } else {
                0
            },
            fg_effective_backend: if sample.fg_owner && vendor.configured_mode != RSF_FG_OFF {
                sample.fg_backend
            } else {
                0
            },
            fg_active: if sample.fg_owner { vendor.active } else { 0 },
            requested_generated: wanted.generated_frames,
            effective_generated: if sample.fg_owner {
                vendor.effective_generated_frames
            } else {
                0
            },
            max_generated: vendor.max_generated_frames.min(config.max_generated),
            foreground,
            latency_requested: wanted.latency,
            latency_effective: if sample.fg_owner && vendor.low_latency_available != 0 {
                vendor.effective_reflex
            } else {
                RSF_REFLEX_OFF
            },
            fg_reason: fg.reason,
            sr_result: sr.last_result,
            fg_result: applied_result(self.request_result, fg.last_result),
            fg_vendor_result: vendor.vendor_status,
            output_fps_valid: u32::from(self.rates.output_valid),
            status_overlay: wanted.status_overlay,
            sr_backend_choices: config.sr_choices,
            fg_backend_choices: self.fg_choices(config),
            base_fps: self.rates.base_fps,
            output_fps: self.rates.output_fps,
            source_frame: fg.source_frame_id,
            tagged_frames: fg.tagged_frames,
            application_presents: self.presents,
            output_presents: vendor.total_presented,
            ..Default::default()
        };
        if let Some(text) = self.description(config, true) {
            copy_reason(&mut value.reason, text);
        }
        value
    }
}

fn sample_latency(stats: &mut rsf_overlay_stats) {
    fill_latency_stats(stats);
    stats.sample_qpc = crate::now_qpc();
    stats.qpc_frequency = crate::qpc_frequency();
}

fn read_options(config: &Config) -> [rsf_overlay_host_option; OPTION_LIMIT] {
    let mut host = [rsf_overlay_host_option::default(); OPTION_LIMIT];
    if config.options.is_empty() {
        return host;
    }
    let mut values = state().options;
    if let Some(read) = config.callbacks.options_read {
        unsafe {
            read(
                config.callbacks.user,
                values.as_mut_ptr(),
                config.options.len() as u32,
            )
        };
    }
    state().options = values;
    for (index, option) in config.options.iter().enumerate() {
        let live_label = config
            .callbacks
            .option_label
            .map_or(null(), |label| unsafe {
                label(config.callbacks.user, index as u32)
            });
        host[index] = rsf_overlay_host_option {
            label: if !live_label.is_null() && unsafe { *live_label } != 0 {
                live_label
            } else {
                option.label.as_ptr()
            },
            kind: option.kind,
            value: values[index],
            minimum: *option.range.start(),
            maximum: *option.range.end(),
            step: option.step,
            divisor: option.divisor,
            restart: option.restart,
            reserved: 0,
        };
    }
    host
}

fn apply_intent(config: &Config, intent: &rsf_overlay_intent) {
    if intent.host_option_changed != 0
        && let Some(option) = config.options.get(intent.host_option_index as usize)
    {
        let wanted = intent
            .host_option_value
            .clamp(*option.range.start(), *option.range.end());
        let accepted = config
            .callbacks
            .option_changed
            .is_some_and(|changed| unsafe {
                changed(config.callbacks.user, intent.host_option_index, wanted) != 0
            });
        if accepted {
            state().options[intent.host_option_index as usize] = wanted;
        }
        let outcome = if accepted {
            "applied"
        } else {
            "refused by the adapter"
        };
        config.callbacks.say(format_args!(
            "RSF overlay: option '{}' = {wanted} {outcome}.",
            option.label.to_string_lossy()
        ));
    }
    if intent.ui_preferences_changed != 0 {
        let saved = {
            let mut state = state();
            (intent.ui_preferences != state.ui_preferences).then(|| {
                let result = unsafe {
                    rsf_settings_store_write_layout(
                        config.ini.as_ptr(),
                        config.section.as_ptr(),
                        intent.ui_preferences,
                    )
                };
                state.ui_preferences = intent.ui_preferences;
                result == OK
            })
        };
        if let Some(saved) = saved {
            let outcome = if saved {
                "saved"
            } else {
                "applied for this session; the save failed"
            };
            config.callbacks.say(format_args!(
                "RSF overlay: panel layout 0x{:x} {outcome}.",
                intent.ui_preferences
            ));
        }
    }
    if settings_changed(intent) {
        let result = apply(
            config,
            |mut current| {
                apply_settings_intent(&mut current, intent);
                current
            },
            "panel",
        );
        {
            let mut state = state();
            state.panel_apply_result = result;
            if intent.sr_change_id != 0
                && (intent.quality_changed != 0 || intent.enabled_changed != 0)
            {
                state.sr_change_id = intent.sr_change_id;
                state.sr_change_status = if result == OK {
                    RSF_OVERLAY_SR_CHANGE_APPLIED
                } else {
                    RSF_OVERLAY_SR_CHANGE_REFUSED
                };
            }
        }
        if result != OK {
            config.callbacks.say(format_args!(
                "RSF overlay: panel request refused ({result}); saved settings retained."
            ));
        }
    }
}

// The adapter calls present and notify on the same graphics owner. Serialize panel
// effects without borrowing the settings/state lock across caller callbacks.
unsafe extern "C" fn present(chain: *mut c_void, drawn: *mut u32) -> i32 {
    unsafe {
        present_inner(chain, null_mut(), drawn, None, |stats, intent| {
            host_present(chain, stats, intent)
        })
    }
}

type FgStatusGet = unsafe extern "C" fn(*mut FgStatus) -> i32;

#[cfg(not(test))]
unsafe extern "C" fn bound_status(out: *mut FgStatus) -> i32 {
    unsafe { crate::fg12::rsf_core_fg12_status(out) }
}

#[cfg(not(test))]
pub(super) unsafe extern "C" fn rsf_overlay_present_bound(
    device: *mut c_void,
    queue: *mut c_void,
    chain: *mut c_void,
    backbuffer: *mut c_void,
    input_window: *mut c_void,
    log: rsf_game_log_fn,
    user: *mut c_void,
) -> i32 {
    unsafe {
        present_inner(
            chain,
            input_window,
            null_mut(),
            Some(bound_status),
            |stats, intent| {
                rsf_overlay_d3d12_frame_for_window(
                    device,
                    queue,
                    backbuffer,
                    input_window,
                    stats,
                    intent,
                    log,
                    user,
                )
            },
        )
    }
}

unsafe fn present_inner(
    chain: *mut c_void,
    input_window: *mut c_void,
    drawn: *mut u32,
    bound_status: Option<FgStatusGet>,
    draw: impl FnOnce(*const rsf_overlay_stats, *mut rsf_overlay_intent) -> i32,
) -> i32 {
    if !drawn.is_null() {
        unsafe { drawn.write(0) };
    }
    if chain.is_null() {
        return INVALID_ARGUMENT;
    }
    let Some(config) = CONFIG.get() else {
        return NOT_READY;
    };
    let Ok(_panel) = PANEL.try_lock() else {
        return BUSY;
    };
    let (chain_window, width, height) = unsafe { borrow::<IDXGISwapChain>(&chain).GetDesc() }
        .map_or((null_mut(), 0, 0), |desc| {
            (
                desc.OutputWindow.0,
                desc.BufferDesc.Width,
                desc.BufferDesc.Height,
            )
        });
    let window = if input_window.is_null() {
        chain_window
    } else {
        input_window
    };
    if config
        .callbacks
        .accept
        .is_some_and(|accept| unsafe { accept(config.callbacks.user, window) } == 0)
    {
        return NOT_READY;
    }
    let lifecycle = LIFECYCLE.load(Ordering::Acquire);
    if matches!(lifecycle, FAILED | RESET_PENDING) {
        return NOT_READY;
    }
    if lifecycle != STARTED && !config.deferred {
        match unsafe { host_start(chain, config.callbacks.log, config.callbacks.user) } {
            0 => {
                LIFECYCLE.store(FAILED, Ordering::Release);
                config.callbacks.say(format_args!(
                    "RSF overlay: the shared panel did not start for {}; see the preceding overlay lines.",
                    config.identity.to_string_lossy()
                ));
                return NOT_READY;
            }
            1 => LIFECYCLE.store(STARTED, Ordering::Release),
            _ => return NOT_READY,
        }
    }

    let sample = Sample::read(config, bound_status);
    let now = unsafe { GetTickCount64() };
    let mut reason = [0; 160];
    let mut stats = {
        let mut state = state();
        state.record_sample(sample, [width, height], now);
        let mut stats = state.panel_stats(config, &mut reason);
        if config.deferred {
            stats.requested_backend = state.settings.sr_backend;
            if state.settings.fg_enabled != 0 {
                stats.fg_requested_backend = state.settings.fg_backend;
            }
        } else {
            stats.fg_backend_choices &= !AUTO_FG;
        }
        stats
    };
    let options = read_options(config);
    let saved = crate::overlay_saved::Frame::new(null_mut());
    saved.attach(&mut stats);
    if !config.options.is_empty() {
        stats.host_option_count = config.options.len() as u32;
        stats.host_options = options.as_ptr();
    }
    sample_latency(&mut stats);
    if visible() != 0 {
        let now = unsafe { GetTickCount64() };
        let refresh_due = {
            let mut state = state();
            let due = state.refresh_tick == 0 || now.wrapping_sub(state.refresh_tick) >= 1000;
            if due {
                state.refresh_tick = now.max(1);
            }
            due
        };
        let refresh = if refresh_due {
            rsf_core_overlay_refresh_mhz(window)
        } else {
            0
        };
        let mut state = state();
        if refresh != 0 {
            state.refresh_mhz = refresh;
        }
        stats.display_refresh_mhz = state.refresh_mhz;
    }
    let mut intent = rsf_overlay_intent {
        struct_size: abi_size::<rsf_overlay_intent>(),
        ..Default::default()
    };
    let result = draw(&stats, &mut intent);
    if config.deferred && result >= 0 {
        LIFECYCLE.store(STARTED, Ordering::Release);
    }
    match result {
        0 => return OK,
        1 => {}
        _ => return NOT_READY,
    }
    if !drawn.is_null() {
        unsafe { drawn.write(1) };
    }
    {
        let mut state = state();
        state.presents_drawn = state.presents_drawn.wrapping_add(1);
    }
    apply_intent(config, &intent);
    saved.apply(&intent);
    OK
}

unsafe extern "C" fn notify(_chain: *mut c_void, reason: u32) -> i32 {
    notify_inner(reason, false, || {
        host_stop();
        OK
    })
}

#[cfg(not(test))]
pub(super) extern "C" fn rsf_overlay_notify_bound(reason: u32) -> i32 {
    notify_inner(reason, true, || {
        rsf_overlay_d3d12_stop();
        OK
    })
}

fn notify_inner(reason: u32, deferred: bool, stop: impl FnOnce() -> i32) -> i32 {
    if reason != RSF_OVERLAY_NOTIFY_RESIZE && reason != RSF_OVERLAY_NOTIFY_DEVICE_RESET {
        return INVALID_ARGUMENT;
    }
    let Some(config) = CONFIG.get() else {
        return NOT_READY;
    };
    if config.deferred != deferred {
        return INVALID_ARGUMENT;
    }
    if reason == RSF_OVERLAY_NOTIFY_RESIZE {
        state().chain_size = [0; 2];
        return OK;
    }
    let Ok(_panel) = PANEL.try_lock() else {
        return BUSY;
    };
    if deferred && LIFECYCLE.load(Ordering::Acquire) == COLD {
        // A cold panel has nothing to reset, but a deferred host still stops its renderer.
        return if stop() == OK { OK } else { NOT_READY };
    }
    let previous = match reset_panel(&LIFECYCLE, stop) {
        Ok(previous) => previous,
        Err(result) => return result,
    };
    if previous == STARTED {
        config.callbacks.say(format_args!(
            "RSF overlay: released for a chain/device reset; it restarts at the next accepted Present."
        ));
    }
    OK
}

// Called with the panel admission held. A refused stop must not make the
// renderer look cold or let another Present start it over the old one.
fn reset_panel(lifecycle: &AtomicU8, stop: impl FnOnce() -> i32) -> Result<u8, i32> {
    let previous = lifecycle.load(Ordering::Acquire);
    if previous == COLD {
        return Ok(COLD);
    }
    lifecycle.store(RESET_PENDING, Ordering::Release);
    if stop() != OK {
        return Err(NOT_READY);
    }
    lifecycle.store(COLD, Ordering::Release);
    Ok(previous)
}

fn started() -> bool {
    LIFECYCLE.load(Ordering::Acquire) == STARTED
}

extern "C" fn visible() -> u32 {
    if started() {
        rsf_overlay_input_visible()
    } else {
        0
    }
}

extern "C" fn set_visible(requested: u32) -> i32 {
    if !started() {
        return NOT_READY;
    }
    rsf_overlay_input_set_visible(u32::from(requested != 0));
    if (visible() != 0) == (requested != 0) {
        OK
    } else {
        BUSY
    }
}

extern "C" fn toggle() -> i32 {
    set_visible(u32::from(visible() == 0))
}

unsafe extern "C" fn capture(out: *mut rsf_overlay_capture) -> i32 {
    if !unsafe { has_capacity(out) } {
        return INVALID_ARGUMENT;
    }
    let started = started();
    let open = if started {
        rsf_overlay_input_visible()
    } else {
        0
    };
    unsafe {
        out.write(rsf_overlay_capture {
            struct_size: abi_size::<rsf_overlay_capture>(),
            started: u32::from(started),
            visible: open,
            captured: if open != 0 {
                RSF_OVERLAY_CAPTURE_MOUSE | RSF_OVERLAY_CAPTURE_KEYBOARD
            } else {
                0
            },
        })
    };
    OK
}

unsafe extern "C" fn status(out: *mut Status) -> i32 {
    if !unsafe { has_capacity(out) } {
        return INVALID_ARGUMENT;
    }
    if unsafe { (*out).abi_version } != RSF_RENDER_FRONTEND_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let Some(config) = CONFIG.get() else {
        return NOT_READY;
    };
    let mut process = 0;
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut process) };
    let foreground = u32::from(process == unsafe { GetCurrentProcessId() });
    let snapshot = state().status(config, foreground);
    unsafe { out.write(snapshot) };
    OK
}

pub(crate) unsafe extern "C" fn settings_get(out: *mut Settings) -> i32 {
    if !unsafe { has_capacity(out) } {
        return INVALID_ARGUMENT;
    }
    if unsafe { (*out).abi_version } != RSF_RENDER_FRONTEND_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if CONFIG.get().is_none() {
        return NOT_READY;
    }
    let settings = state().settings;
    unsafe { out.write(settings) };
    OK
}

pub(crate) unsafe extern "C" fn settings_apply(request: *const Settings) -> i32 {
    if request.is_null() {
        return INVALID_ARGUMENT;
    }
    let config = CONFIG.get();
    if !unsafe { has_capacity(request) } {
        log_apply(config, None, "api", INVALID_ARGUMENT);
        return INVALID_ARGUMENT;
    }
    let value = unsafe { request.read() };
    if value.abi_version != RSF_RENDER_FRONTEND_ABI_VERSION {
        log_apply(config, Some(&value), "api", ABI_MISMATCH);
        return ABI_MISMATCH;
    }
    let Some(config) = config else {
        log_apply(None, Some(&value), "api", NOT_READY);
        return NOT_READY;
    };
    apply(config, |_| value, "api")
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_get_overlay_diagnostics(out: *mut rsf_overlay_diagnostics) -> i32 {
    if !unsafe { has_capacity(out) } {
        return INVALID_ARGUMENT;
    }
    let started = started();
    let open = if started {
        rsf_overlay_input_visible()
    } else {
        0
    };
    let apply = *apply_diagnostics();
    let mut value = {
        let state = state();
        rsf_overlay_diagnostics {
            struct_size: abi_size::<rsf_overlay_diagnostics>(),
            started: u32::from(started),
            visible: open,
            chain_width: state.chain_size[0],
            chain_height: state.chain_size[1],
            presents: state.presents,
            presents_drawn: state.presents_drawn,
            apply_attempts: apply.attempts,
            apply_refused: apply.refused,
            last_apply_result: apply.last_result,
            ..Default::default()
        }
    };
    let mut input = rsf_overlay_input_counters {
        struct_size: abi_size::<rsf_overlay_input_counters>(),
        ..Default::default()
    };
    if unsafe { rsf_overlay_input_counters_get(&mut input) } == OK {
        value.input_installed = input.installed;
        value.cursor_freed = input.cursor_freed;
        value.pointer_received = input.pointer_received;
        value.pointer_consumed = input.pointer_consumed;
        value.button_downs_consumed = input.button_downs_consumed;
        value.wheel_consumed = input.wheel_consumed;
        value.raw_mouse_reports = input.raw_mouse_reports;
        value.keys_consumed = input.keys_consumed;
        value.frames_visible = input.frames_visible;
        value.frames_with_button = input.frames_with_button;
        value.last_pointer_x = input.last_pointer_x;
        value.last_pointer_y = input.last_pointer_y;
    }
    unsafe { out.write(value) };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_get_overlay_api(api: *mut rsf_overlay_api) -> i32 {
    if !unsafe { has_capacity(api) } {
        return INVALID_ARGUMENT;
    }
    if unsafe { (*api).abi_version } != RSF_OVERLAY_API_VERSION {
        return ABI_MISMATCH;
    }
    unsafe {
        api.write(rsf_overlay_api {
            struct_size: abi_size::<rsf_overlay_api>(),
            abi_version: RSF_OVERLAY_API_VERSION,
            configure: Some(configure),
            present: Some(present),
            notify: Some(notify),
            visible: Some(visible),
            set_visible: Some(set_visible),
            toggle: Some(toggle),
            capture: Some(capture),
            status: Some(status),
            settings_get: Some(settings_get),
            settings_apply: Some(settings_apply),
        })
    };
    OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_runs_stop_again_until_the_panel_is_cold() {
        let lifecycle = AtomicU8::new(STARTED);
        assert_eq!(reset_panel(&lifecycle, || -1), Err(NOT_READY));
        assert_eq!(lifecycle.load(Ordering::Acquire), RESET_PENDING);
        assert_eq!(
            reset_panel(&lifecycle, || {
                assert_eq!(lifecycle.load(Ordering::Acquire), RESET_PENDING);
                0
            }),
            Ok(RESET_PENDING)
        );
        assert_eq!(lifecycle.load(Ordering::Acquire), COLD);
        assert_eq!(reset_panel(&lifecycle, || unreachable!()), Ok(COLD));
    }
}

#[cfg(not(test))]
unsafe extern "C" fn observed_present(_: *mut c_void, event: *const rsf_observer_present_event) {
    let Some(event) = (unsafe { event.as_ref() }) else {
        return;
    };
    if event.completed == 0 && event.flags & DXGI_PRESENT_TEST.0 == 0 {
        rsf_overlay_input_poll_toggle();
        unsafe { present(event.swapchain, null_mut()) };
    }
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_start_d3d11_overlay(setup: *const rsf_overlay_service_setup) -> i32 {
    let configured = unsafe { configure(setup) };
    if configured != OK {
        return configured;
    }
    let observer = rsf_observer_options {
        struct_size: abi_size::<rsf_observer_options>(),
        abi_version: RSF_OBSERVER_ABI_VERSION,
        format: u32::MAX,
        minimum_width: u32::MAX,
        capacity: 1,
        on_present_event: Some(observed_present),
        ..unsafe { std::mem::zeroed() }
    };
    // The observer's native registration can allocate and throw.
    if unsafe { rsf_observer_install_guarded(&observer) } == OK {
        OK
    } else {
        NOT_READY
    }
}
