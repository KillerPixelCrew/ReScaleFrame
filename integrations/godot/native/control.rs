// SPDX-License-Identifier: MIT
//! The Godot runtime control and the D3D12 session: the requested settings and their
//! reconstruction generation, the `rsfg_configure` refusal rules, the engine module's services,
//! the D3D12 FG install and the status record assembled from the Runtime's facts. While the
//! Runtime's Vulkan host is booted, configuration, settings and status go to it after the same
//! refusal rules.
use crate::runtime::vulkan_booted;
use crate::sys::*;
use crate::{checked, checked_output, d3d12, window};
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_game_sys::{
    RSF_FG_BACKEND_AUTO, RSF_FG_OFF, RSF_FG_STAT_TOTAL_PRESENTED, RSF_REFLEX_OFF,
    RSF_RENDER_FRONTEND_ABI_VERSION, RSF_RENDER_SETTINGS_REFUSED_SHARPENING, RSF_SR_AUTO,
    RSF_SR_CHOICES_ALL, RSF_SR_SPATIAL, RSF_UI_MODE_BACKBUFFER_HUDLESS, rsf_render_settings,
    rsf_render_settings_check,
};
use rsf_game_sys::{
    backend_result, presentation_result::PresentationResult, rsf_game_log_fn,
    vendor_paths::VendorPaths,
};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard};

pub(crate) type Settings = rsf_godot_settings;
pub(crate) type Status = rsf_godot_status;

const DLSS_ENGINE_CUSTOM: u32 = 0;

/// The settings the D3D12 session and the GDExtension defaults start from, as the Runtime Vulkan
/// host's do; the sharpening request is empty.
pub(crate) fn default_settings() -> Settings {
    Settings {
        struct_size: size_of::<Settings>() as u32,
        abi_version: RSF_GODOT_SETTINGS_ABI_VERSION,
        sr_backend: RSF_SR_AUTO,
        quality: 1,
        dlss_preset: 0,
        fg_backend: RSF_FG_BACKEND_AUTO,
        fg_mode: RSF_FG_OFF,
        generated_frames: 1,
        latency_mode: RSF_REFLEX_OFF,
        frame_limit_us: 0,
        gameplay: 1,
        ..Default::default()
    }
}

/// The settings whose change retires the D3D12 SR context.
fn reconstruction_key(settings: &Settings) -> (u32, u32, u32) {
    (settings.sr_backend, settings.quality, settings.dlss_preset)
}

pub(crate) fn status_header() -> Status {
    Status {
        struct_size: size_of::<Status>() as u32,
        abi_version: RSF_GODOT_ABI_VERSION,
        ..Default::default()
    }
}

/// The requested sharpening, or the Runtime defaults when none was requested.
pub(crate) fn sharpening(settings: &Settings) -> rsf_sharpening_options {
    if settings.sharpening.struct_size != 0 {
        settings.sharpening
    } else {
        unsafe { rsf_sharpening_core_defaults() }
    }
}

/// The refusal rules of `rsfg_configure`, applied before either backend sees the settings: the
/// Runtime's settings check (no SR backend is SR off), its sharpening check for a request that
/// carries one, and Godot's own gameplay flag.
fn settings_valid(value: &Settings) -> bool {
    let sharpening = &value.sharpening;
    let request = rsf_render_settings {
        struct_size: size_of::<rsf_render_settings>() as u32,
        abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
        sr_enabled: u32::from(value.sr_backend != 0),
        sr_backend: if value.sr_backend == 0 {
            RSF_SR_AUTO
        } else {
            value.sr_backend
        },
        sr_quality: value.quality,
        dlss_preset: value.dlss_preset,
        fg_enabled: value.fg_mode,
        fg_backend: value.fg_backend,
        generated_frames: value.generated_frames,
        latency: value.latency_mode,
        frame_limit_us: value.frame_limit_us,
        ..Default::default()
    };
    let refused = unsafe { rsf_render_settings_check(&request, RSF_SR_CHOICES_ALL) };
    refused & !RSF_RENDER_SETTINGS_REFUSED_SHARPENING == 0
        && (sharpening.struct_size == 0
            || (sharpening.struct_size as usize >= size_of::<rsf_sharpening_options>()
                && unsafe { rsf_sharpening_core_valid(*sharpening) } != 0))
        && value.gameplay <= 1
}

/// Owner-held UTF-8 paths. Each boot attempt leaks one set so a Runtime that kept the previous
/// pointers never sees them freed.
pub(crate) struct SessionPaths {
    pub(crate) runtime: CString,
    pub(crate) vendors: VendorPaths,
    pub(crate) version: CString,
    pub(crate) project: CString,
}

struct Session {
    paths: Option<&'static SessionPaths>,
    log: rsf_game_log_fn,
    // The engine module's log user, an opaque pointer kept as an address.
    log_user: usize,
    settings: Settings,
    generation: u64,
    status: Status,
    services: Option<rsf_godot_engine_services>,
}

static BOOTED: AtomicBool = AtomicBool::new(false);
static BOOT_GATE: Mutex<()> = Mutex::new(());
// No Runtime or engine callback runs while this lock is held: the log, the FG install and the
// engine services can re-enter this module.
static SESSION: LazyLock<Mutex<Session>> = LazyLock::new(|| {
    Mutex::new(Session {
        paths: None,
        log: None,
        log_user: 0,
        settings: default_settings(),
        generation: 1,
        status: status_header(),
        services: None,
    })
});

fn session() -> MutexGuard<'static, Session> {
    lock_or_recover(&SESSION)
}

/// The D3D12 session is booted; it stays booted for the process.
pub(crate) fn booted() -> bool {
    BOOTED.load(Ordering::Acquire)
}

/// Any backend that accepts engine control.
fn active() -> bool {
    booted() || vulkan_booted()
}

pub(crate) fn snapshot() -> (Settings, u64) {
    let session = session();
    (session.settings, session.generation)
}

pub(crate) fn session_id() -> u64 {
    session().status.session_id
}

pub(crate) fn paths() -> Option<&'static SessionPaths> {
    session().paths
}

/// The Runtime's log callback, forwarding to the engine module's logger.
pub(crate) extern "C" fn log(_: *mut c_void, message: *const c_char) {
    let (log, user) = {
        let session = session();
        (session.log, session.log_user)
    };
    if let Some(log) = log {
        // SAFETY: the engine module's logger lives while its library is loaded; the message is
        // the caller's borrow.
        unsafe { log(user as *mut c_void, message) };
    }
}

pub(crate) fn log_text(message: String) {
    if let Ok(message) = CString::new(message) {
        log(null_mut(), message.as_ptr());
    }
}

unsafe extern "C" fn accept(_: *mut c_void, window: *mut c_void) -> i32 {
    window::accept(window).into()
}

/// One evaluated D3D12 SR pass. FSR1 is evaluated by Godot before the callback; the native
/// context only normalises its depth and motion, so a spatial request reports spatial.
pub(crate) fn sr_frame(
    requested: u32,
    backend: u32,
    context_result: i32,
    evaluated: i32,
    plan: (u32, u32),
    output: (u32, u32),
) {
    let ok = evaluated == backend_result::OK;
    let mut session = session();
    let status = &mut session.status;
    status.requested_sr = requested;
    status.active_sr = match (ok, requested) {
        (false, _) => 0,
        (true, RSF_SR_SPATIAL) => RSF_SR_SPATIAL,
        (true, _) => backend,
    };
    status.sr_result = if context_result != backend_result::OK {
        context_result
    } else {
        evaluated
    };
    (status.render_width, status.render_height) = plan;
    (status.output_width, status.output_height) = output;
    if ok {
        status.evaluated_frames = status.evaluated_frames.wrapping_add(1);
    } else {
        status.refused_frames = status.refused_frames.wrapping_add(1);
    }
}

/// Present found no SR pass for the frame it shows.
pub(crate) fn sr_missed() {
    session().status.active_sr = 0;
}

fn fg_options(settings: &Settings) -> rsf_native_fg_options {
    rsf_native_fg_options {
        struct_size: size_of::<rsf_native_fg_options>() as u32,
        mode: settings.fg_mode,
        generated_frames: settings.generated_frames,
        reflex_mode: settings.latency_mode,
        frame_limit_us: settings.frame_limit_us,
        debug: 0,
    }
}

/// # Safety
/// `services` is null or a readable `rsf_godot_engine_services` of its stated size.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_attach_engine(services: *const rsf_godot_engine_services) -> c_int {
    // SAFETY: the caller's guarantee.
    let Some(services) = (unsafe { checked(services, RSF_GODOT_ABI_VERSION) }) else {
        return 0;
    };
    if services.configure.is_none() || services.reset_history.is_none() {
        return 0;
    }
    session().services = Some(*services);
    1
}

/// Forwards to the engine module, which applies the request to its primary viewport and returns
/// to `rsfg_configure`.
///
/// # Safety
/// `settings` follows the checked record contract.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_configure_engine(settings: *const Settings, viewport_id: u64) -> c_int {
    // SAFETY: the caller's guarantee.
    unsafe { checked(settings, RSF_GODOT_SETTINGS_ABI_VERSION) }
        .map_or(0, |settings| configure_engine(settings, viewport_id))
}

/// The engine module's configure service, also called by `ReScaleFrameRuntime.configure`.
pub(crate) fn configure_engine(settings: &Settings, viewport_id: u64) -> c_int {
    let configure = session().services.and_then(|services| services.configure);
    match configure {
        // SAFETY: the engine module's service lives while its library is loaded.
        Some(configure) if active() => unsafe { configure(settings, viewport_id) },
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsfg_reset_history() {
    let reset = session()
        .services
        .and_then(|services| services.reset_history);
    if let Some(reset) = reset
        && active()
    {
        // SAFETY: as in rsfg_configure_engine.
        unsafe { reset() };
    }
}

/// The D3D12 session, which binds the Runtime's D3D12 CPU service for the input window.
///
/// # Safety
/// `setup` is null or a readable `rsf_godot_setup` of its stated size whose strings are null or
/// NUL-terminated for the call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_boot(setup: *const rsf_godot_setup) -> c_int {
    // SAFETY: the caller's guarantee.
    let Some(setup) = (unsafe { checked(setup, RSF_GODOT_ABI_VERSION) }) else {
        return 0;
    };
    let result = boot(setup);
    if result != 0 {
        window::bind_cpu_service(false);
    }
    result
}

/// A booted session stays booted for the process.
fn boot(setup: &rsf_godot_setup) -> c_int {
    if setup.runtime_directory.is_null()
        || setup.engine_version.is_null()
        || setup.project_id.is_null()
    {
        return 0;
    }
    if booted() {
        return 1;
    }
    // FG installation can call into the host. Refuse an overlapping or reentrant boot.
    let Ok(_boot) = BOOT_GATE.try_lock() else {
        return c_int::from(booted());
    };
    if booted() {
        return 1;
    }
    d3d12::prepare_capture();
    // SAFETY: non-null NUL-terminated strings borrowed for the call.
    let (runtime, version, project) = unsafe {
        (
            CStr::from_ptr(setup.runtime_directory),
            CStr::from_ptr(setup.engine_version),
            CStr::from_ptr(setup.project_id),
        )
    };
    let paths: &'static SessionPaths = Box::leak(Box::new(SessionPaths {
        runtime: runtime.to_owned(),
        vendors: VendorPaths::new(runtime),
        version: version.to_owned(),
        project: project.to_owned(),
    }));
    {
        let mut session = session();
        session.paths = Some(paths);
        session.log = setup.log;
        session.log_user = setup.log_user as usize;
        session.status.session_id =
            unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
                .wrapping_add(1);
    }
    // The explicit-presentation FG host: Auto backend, unbounded generation, runtime switching,
    // HUD-less backbuffer UI, inverted finite depth in metres and Godot's custom engine identity.
    let vendors = &paths.vendors;
    let fg = rsf_fg12_setup {
        struct_size: size_of::<rsf_fg12_setup>() as u32,
        backend: RSF_FG_BACKEND_AUTO,
        max_generated_frames: u32::MAX,
        runtime_switching: 1,
        ui_mode: RSF_UI_MODE_BACKBUFFER_HUDLESS,
        options: fg_options(&default_settings()),
        depth_inverted: 1,
        depth_infinite: 0,
        units_to_meters: 1.0,
        accept_window: Some(accept),
        log: Some(log),
        runtime_directory_utf8: vendors.fidelityfx.as_ptr(),
        fsr3_directory_utf8: vendors.fidelityfx.as_ptr(),
        fsr4_directory_utf8: vendors.fidelityfx.as_ptr(),
        xess_directory_utf8: vendors.xess.as_ptr(),
        streamline_directory_utf8: vendors.streamline.as_ptr(),
        engine_type: DLSS_ENGINE_CUSTOM,
        engine_version_utf8: paths.version.as_ptr(),
        project_id_utf8: paths.project.as_ptr(),
        user: null_mut(),
    };
    let result = unsafe { rsf_fg12_install_explicit(&fg) };
    session().status.fg_result = result;
    let ready = result == backend_result::OK;
    BOOTED.store(ready, Ordering::Release);
    c_int::from(ready)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_create_swapchain(
    factory: *mut c_void,
    queue: *mut c_void,
    window: *mut c_void,
    description: *const c_void,
    out: *mut *mut c_void,
) -> i32 {
    if !booted() {
        return windows::Win32::Graphics::Dxgi::DXGI_ERROR_UNSUPPORTED.0;
    }
    unsafe { rsf_d3d11_present_create_for_hwnd(factory, queue, window, description, out) }
}

/// # Safety
/// `settings` is null or a readable `rsf_godot_settings` of its stated size.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_configure(settings: *const Settings) -> c_int {
    // SAFETY: the caller's guarantee.
    let Some(settings) = (unsafe { checked(settings, RSF_GODOT_SETTINGS_ABI_VERSION) }) else {
        return 0;
    };
    if !settings_valid(settings) {
        return 0;
    }
    if vulkan_booted() {
        return unsafe { rsf_vkhost_configure(settings) };
    }
    {
        let mut session = session();
        if reconstruction_key(&session.settings) != reconstruction_key(settings) {
            session.generation = session.generation.wrapping_add(1);
        }
        session.settings = *settings;
    }
    unsafe { rsf_fg12_options(&fg_options(settings)) };
    1
}

/// # Safety
/// `output` follows the checked output record contract.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsfg_settings(output: *mut Settings) -> c_int {
    // SAFETY: the caller's guarantee; a matching header covers writable storage.
    let Some(output) = (unsafe { checked_output(output, RSF_GODOT_SETTINGS_ABI_VERSION) }) else {
        return 0;
    };
    if vulkan_booted() {
        return unsafe { rsf_vkhost_settings_get(output) };
    }
    unsafe { output.write(session().settings) };
    1
}

/// # Safety
/// `output` follows the checked output record contract.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsfg_status(output: *mut Status) -> c_int {
    // SAFETY: the caller's guarantee; checked_output established the record.
    let Some(output) = (unsafe { checked_output(output, RSF_GODOT_ABI_VERSION) }) else {
        return 0;
    };
    if vulkan_booted() {
        let mut value = status_header();
        let result = unsafe { rsf_vkhost_status_get(&mut value) };
        if result != 0 {
            let mut inputs = rsf_native_cpu_counters {
                struct_size: size_of::<rsf_native_cpu_counters>() as u32,
                ..Default::default()
            };
            unsafe { rsf_native_cpu_counters_get(&mut inputs) };
            apply_inputs(&mut value, &inputs, inputs.derived_frame_ends);
            window::apply_facts(&mut value);
        }
        unsafe { output.write(value) };
        return result;
    }
    if !booted() {
        return 0;
    }
    let mut value = session().status;
    let mut fg = rsf_native_fg_status {
        struct_size: size_of::<rsf_native_fg_status>() as u32,
        ..Default::default()
    };
    unsafe { rsf_fg12_status(&mut fg) };
    value.fg_backend = unsafe { rsf_d3d11_present_backend() };
    value.fg_available = fg.available;
    value.fg_active = fg.vendor.active;
    // The presentation provider's switch failure outranks the FG session's last result.
    value.fg_result = PresentationResult {
        session: fg.last_result,
        provider_switch: unsafe { rsf_d3d11_present_switch_result() },
    }
    .aggregate();
    value.reason = fg.reason;
    value.max_generated_frames = fg.vendor.max_generated_frames;
    value.generated_frames = fg.vendor.effective_generated_frames;
    value.tagged_frames = fg.tagged_frames;
    value.total_presented = fg.vendor.total_presented;
    value.generated_presented = fg.vendor.generated_presented;
    value.presentation_statistics_valid =
        u32::from(fg.vendor.valid_statistics & RSF_FG_STAT_TOTAL_PRESENTED != 0);
    value.presentation_statistics_flags = fg.vendor.valid_statistics;
    value.source_frame_id = fg.source_frame_id;
    value.provider_generation = unsafe { rsf_d3d11_present_generation() };
    value.generated_callbacks = fg.vendor.generated_callbacks;
    window::apply_facts(&mut value);
    // Availability, mode and pacing come from the latency owner.
    let mut latency = rsf_native_latency_status {
        struct_size: size_of::<rsf_native_latency_status>() as u32,
        version: 1,
        ..Default::default()
    };
    if unsafe { rsf_native_latency_status_get(&mut latency) } != 0 {
        apply_inputs(&mut value, &latency.events, latency.derived_boundaries);
        value.sleep_calls = latency.paced_frames;
        value.marker_calls = latency.marker_calls;
        value.pcl_available = latency.pcl_available;
        value.latency_report_available = latency.report_available;
        value.latency_owner = latency.owner;
        value.latency_available = latency.available;
        value.latency_mode = latency.effective_mode;
    }
    unsafe { output.write(value) };
    1
}

fn apply_inputs(status: &mut Status, inputs: &rsf_native_cpu_counters, boundaries: u64) {
    status.keyboard_inputs = inputs.input_events[0];
    status.mouse_inputs = inputs.input_events[1];
    status.controller_inputs = inputs.input_events[2];
    status.pcl_pings = inputs.pings;
    status.input_substitutes = inputs.substitutes;
    status.order_violations = inputs.order_violations;
    status.derived_boundaries = boundaries;
}
