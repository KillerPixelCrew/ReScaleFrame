// SPDX-License-Identifier: MIT
//! DLSS SR behind the D3D11 and shared-host facade (`rescaleframe/dlss.h`). The session owns the
//! Streamline SDK when no other owner exists, or borrows the D3D12 Streamline host's session and
//! CPU frame tokens. The record layouts are checked against the vendor headers in the
//! `dlss_sr_layout` test (`tests/dlss_sr_layout_facts.cpp`).
use rsf_game_sys::backend_result::{
    ABI_MISMATCH, FEATURE_FAILED, INIT_FAILED, INVALID_ARGUMENT, LOAD_FAILED, MISSING_ENTRY_POINT,
    NOT_READY, NOT_SUPPORTED, OK,
};
pub(super) mod exports;

use crate::com_ref;
use crate::diagnostic_output::Output;
use crate::sdk_module::{SdkModule, wide_path};
use crate::sr_session::has_capacity;
use crate::streamline_log;
use crate::streamline_sdk::{
    self as sdk, DEFAULT_PREFERENCE_FLAGS, DlssOptimal, DlssOptions, Entries, FEATURE_COMMON,
    FEATURE_DLSS, LOG_ERROR, LOG_LEVEL_DEFAULT, LOG_WARN, PREFERENCE_FRAME_TAGGING,
    PREFERENCE_MANUAL_HOOKING, PREFERENCES, Preferences, RENDER_API_D3D11,
    RESULT_DRIVER_OUT_OF_DATE, RESULT_NO_SUPPORTED_ADAPTER, RESULT_OK, RESULT_OS_OUT_OF_DATE,
    SDK_VERSION, TagRecords, viewport,
};
use crate::streamline_sr::{Constants, Options, TagBatch, dimensions, dlss_mode};
use crate::sys::{RSF_DLSS_ABI_VERSION, rsf_dlss_frame as Frame};
use crate::sys::{rsf_dlss_log_fn as Log, rsf_dlss_setup as Setup};
use crate::sys::{rsf_dlss_plan as Plan, rsf_dlss_support as Support};
use rsf_streamline_vk::sdk_api::{self, GetOptimalSettings, SetOptions};
use std::borrow::Cow;
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::{NonNull, addr_of, addr_of_mut, null, null_mut};
use std::sync::Mutex;
use windows::core::{IUnknown, Interface};

type Handle = *mut c_void;
const OPTIMAL: u32 = 1;
const OPTIONS: u32 = 2;
const LEGACY_SR_LOG: u32 = 0;


fn say(text: &str) {
    if let Ok(text) = CString::new(text) {
        unsafe { streamline_log::emit(LEGACY_SR_LOG, true, text.as_ptr()) };
    }
}

fn set_log(log: Log, user: Handle) {
    if log.is_some() {
        streamline_log::set(LEGACY_SR_LOG, 0, log, user, false);
    } else {
        streamline_log::clear(LEGACY_SR_LOG, 0);
    }
}

// Streamline's own log messages go to the same sink rather than to a console nobody sees. NGX's
// per-resource info messages grew the research log by hundreds of MB per session, so only
// actionable vendor diagnostics are kept.
unsafe extern "C" fn streamline_message(kind: u32, message: *const c_char) {
    if kind != LOG_ERROR && kind != LOG_WARN {
        return;
    }
    let text = if message.is_null() {
        Cow::Borrowed("")
    } else {
        unsafe { CStr::from_ptr(message) }.to_string_lossy()
    };
    say(&format!(
        "streamline {}: {text}",
        if kind == LOG_ERROR {
            "error"
        } else {
            "warning"
        }
    ));
}

/// The first entry this route needs that the interposer does not export. The shared host already
/// owns initialization, the device, tokens and allocation.
fn missing_entry(entries: &Entries, owned: bool) -> Option<&'static CStr> {
    let shared = [
        (
            c"slIsFeatureSupported",
            entries.is_feature_supported.is_some(),
        ),
        (
            c"slGetFeatureFunction",
            entries.get_feature_function.is_some(),
        ),
        (
            c"slGetFeatureRequirements",
            entries.get_feature_requirements.is_some(),
        ),
        (c"slSetConstants", entries.set_constants.is_some()),
        (c"slSetTagForFrame", entries.set_tag_for_frame.is_some()),
        (c"slEvaluateFeature", entries.evaluate_feature.is_some()),
        (c"slFreeResources", entries.free_resources.is_some()),
    ];
    let owned_only = [
        (c"slInit", entries.init.is_some()),
        (c"slShutdown", entries.shutdown.is_some()),
        (c"slSetD3DDevice", entries.set_d3d_device.is_some()),
        (c"slGetNewFrameToken", entries.get_new_frame_token.is_some()),
        (c"slAllocateResources", entries.allocate_resources.is_some()),
    ];
    let count = if owned { owned_only.len() } else { 0 };
    shared
        .into_iter()
        .chain(owned_only.into_iter().take(count))
        .find(|(_, present)| !present)
        .map(|(name, _)| name)
}

/// The entries the route needs, or MISSING_ENTRY_POINT after naming the first one absent.
fn require_entries(entries: Entries, owned: bool) -> Result<Entries, i32> {
    match missing_entry(&entries, owned) {
        Some(name) => {
            say(&format!(
                "entry point {} is missing from the interposer",
                name.to_string_lossy()
            ));
            Err(MISSING_ENTRY_POINT)
        }
        None => Ok(entries),
    }
}

/// Looks up one feature function: FEATURE_FAILED without slGetFeatureFunction, MISSING_ENTRY_POINT
/// when Streamline refuses. The address is written either way.
unsafe fn lookup(entries: &Entries, feature: u32, name: &CStr, out: &mut Handle) -> i32 {
    let Some(function) = entries.get_feature_function else {
        return FEATURE_FAILED;
    };
    if unsafe { function(feature, name.as_ptr(), out) } == RESULT_OK {
        OK
    } else {
        MISSING_ENTRY_POINT
    }
}

/// The DLSS feature functions, with the bits of those resolved.
#[derive(Clone, Copy, Default)]
struct Features {
    optimal: Option<GetOptimalSettings<DlssOptions, DlssOptimal>>,
    options: Option<SetOptions<DlssOptions>>,
}

impl Features {
    fn bits(&self) -> u32 {
        let optimal = if self.optimal.is_some() { OPTIMAL } else { 0 };
        let options = if self.options.is_some() { OPTIONS } else { 0 };
        optimal | options
    }
}

unsafe fn resolve_features(
    strict: bool,
    mut lookup: impl FnMut(&CStr, &mut Handle) -> i32,
) -> Result<Features, i32> {
    let mut optimal = null_mut();
    let mut options = null_mut();
    for (name, address) in [
        (c"slDLSSGetOptimalSettings", &mut optimal),
        (c"slDLSSSetOptions", &mut options),
    ] {
        let status = lookup(name, address);
        // Only the owned-device probe uses addresses returned alongside a vendor error.
        if status == FEATURE_FAILED || (strict && status != OK) {
            return Err(status);
        }
    }
    Ok(Features {
        optimal: unsafe { sdk_api::function(optimal) },
        options: unsafe { sdk_api::function(options) },
    })
}

#[cfg(test)]
#[test]
fn shared_features_refuse_a_vendor_error_even_with_an_address() {
    use std::ptr::without_provenance_mut;

    let lookup = |name: &CStr, address: &mut Handle| {
        if name == c"slDLSSGetOptimalSettings" {
            *address = without_provenance_mut(1);
            MISSING_ENTRY_POINT
        } else {
            *address = without_provenance_mut(2);
            OK
        }
    };
    assert_eq!(
        unsafe { resolve_features(true, lookup) }.err(),
        Some(MISSING_ENTRY_POINT)
    );
    let features = unsafe { resolve_features(false, lookup) }.ok().unwrap();
    assert_eq!(features.bits(), OPTIMAL | OPTIONS);
}

enum Route {
    Owned(Option<IUnknown>),
    Shared {
        host: NonNull<c_void>,
        device: IUnknown,
    },
}

impl Route {
    fn host(&self) -> Handle {
        match self {
            Self::Owned(_) => null_mut(),
            Self::Shared { host, .. } => host.as_ptr(),
        }
    }

    fn device(&self) -> Option<&IUnknown> {
        match self {
            Self::Owned(device) => device.as_ref(),
            Self::Shared { device, .. } => Some(device),
        }
    }
}

/// What slInit borrows from the session through SDK shutdown.
struct InitStrings {
    plugin_paths: [*const u16; 1],
    feature: u32,
}

struct Session {
    entries: Entries,
    features: Features,
    route: Route,
    applied: [Option<Options>; 2],
    present_entries: Option<[Handle; 2]>,
    shutdown_failure: Option<i32>,
    _strings: Box<InitStrings>,
    _module: Option<SdkModule>,
    _directories: [Option<Vec<u16>>; 2],
}

impl Session {
    fn active(&self) -> bool {
        self.shutdown_failure.is_none()
    }

    fn features(&self) -> u32 {
        self.features.bits()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        streamline_log::clear(LEGACY_SR_LOG, 0);
    }
}

// Native calls and access to the borrowed graphics host are serialized here. The caller keeps the
// shared host alive until SR shutdown, and each COM device is retained until its route is retired.
unsafe impl Send for Session {}
static SESSION: Mutex<Option<Session>> = Mutex::new(None);

/// Resolves the interposer's entries and initializes Streamline for D3D11 DLSS SR.
fn start_owned(
    module: &SdkModule,
    setup: &Setup,
    strings: &mut InitStrings,
    plugins: *const u16,
    logs: *const u16,
) -> Result<Entries, i32> {
    set_log(setup.log, setup.log_user);
    let entries = match require_entries(unsafe { Entries::resolve(module.as_ptr()) }, true) {
        Ok(entries) => entries,
        Err(result) => {
            set_log(None, null_mut());
            return Err(result);
        }
    };
    strings.feature = FEATURE_DLSS;
    let has_plugins = !plugins.is_null();
    if has_plugins {
        strings.plugin_paths[0] = plugins;
    }
    let preferences = Preferences {
        base: PREFERENCES,
        show_console: 0,
        log_level: LOG_LEVEL_DEFAULT,
        plugin_paths: if has_plugins {
            strings.plugin_paths.as_mut_ptr()
        } else {
            null_mut()
        },
        num_plugin_paths: u32::from(has_plugins),
        logs_path: logs,
        log: Some(streamline_message),
        flags: DEFAULT_PREFERENCE_FLAGS | PREFERENCE_MANUAL_HOOKING | PREFERENCE_FRAME_TAGGING,
        features: &raw const strings.feature,
        num_features: 1,
        application_id: setup.application_id,
        engine: crate::streamline_policy::engine_type(setup.engine),
        engine_version: setup.engine_version_utf8,
        project: setup.project_id_utf8,
        render_api: RENDER_API_D3D11,
        ..Default::default()
    };
    let init = entries.init.expect("required entry");
    let result = unsafe { init(&preferences, SDK_VERSION) };
    if result != RESULT_OK {
        say(&format!("slInit failed with result {}", result as u32));
        set_log(None, null_mut());
        return Err(INIT_FAILED);
    }
    say("streamline loaded and initialised");
    Ok(entries)
}

unsafe fn rsf_core_dlss_streamline_load(setup: *const Setup) -> i32 {
    if !unsafe { has_capacity(setup) } {
        return INVALID_ARGUMENT;
    }
    let setup = unsafe { &*setup };
    if setup.interposer_path_utf8.is_null() {
        return INVALID_ARGUMENT;
    }
    if setup.abi_version != RSF_DLSS_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let Ok(mut state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    if let Some(session) = state.as_ref() {
        return session.shutdown_failure.unwrap_or(OK);
    }
    if sdk::interposer_loaded() {
        if let Some(log) = setup.log
            && let Some(_output) = Output::try_enter()
        {
            unsafe {
                log(setup.log_user, c"D3D11 DLSS cannot initialize over an existing Streamline owner; use the shared D3D12 host".as_ptr());
            }
        }
        return INIT_FAILED;
    }
    let Some(path) = (unsafe { wide_path(setup.interposer_path_utf8) }) else {
        return INVALID_ARGUMENT;
    };
    let log = crate::input_dump::Log::new(setup.log, setup.log_user);
    let Some(module) = SdkModule::adopt_system(unsafe {
        sdk::load_interposer(path.as_ptr(), setup.require_signature != 0, &log)
    }) else {
        return LOAD_FAILED;
    };
    let directories = unsafe {
        [
            wide_path(setup.plugin_directory_utf8),
            wide_path(setup.log_directory_utf8),
        ]
    };
    let [plugins, logs] = directories
        .each_ref()
        .map(|directory| directory.as_deref().map_or(null(), <[u16]>::as_ptr));
    let mut strings = Box::new(InitStrings {
        plugin_paths: [null()],
        feature: 0,
    });
    let entries = match start_owned(&module, setup, &mut strings, plugins, logs) {
        Ok(entries) => entries,
        Err(result) => return result,
    };
    *state = Some(Session {
        entries,
        features: Features::default(),
        route: Route::Owned(None),
        applied: [None; 2],
        present_entries: None,
        shutdown_failure: None,
        _strings: strings,
        _module: Some(module),
        _directories: directories,
    });
    OK
}

unsafe fn rsf_core_dlss_streamline_share_host(host: Handle, log: Log, user: Handle) -> i32 {
    let Some(host) = NonNull::new(host) else {
        return NOT_READY;
    };
    let Ok(mut state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    if state.is_some() {
        return NOT_READY;
    }
    // DLSS SR borrows the host's interposer, entry table and device.
    let Some(shared) = (unsafe { crate::streamline_host::shared(host.as_ptr().cast()) }) else {
        return NOT_READY;
    };
    if shared.module.is_null() || shared.native_device.is_null() {
        return NOT_READY;
    }
    let entries = match require_entries(shared.entries, false) {
        Ok(entries) => entries,
        Err(result) => {
            streamline_log::clear(LEGACY_SR_LOG, 0);
            return result;
        }
    };
    let features = match unsafe {
        resolve_features(true, |name, address| {
            lookup(&entries, FEATURE_DLSS, name, address)
        })
    } {
        Ok(features) if features.bits() == OPTIMAL | OPTIONS => features,
        result => {
            streamline_log::clear(LEGACY_SR_LOG, 0);
            return match result {
                Err(FEATURE_FAILED) => INIT_FAILED,
                Err(result) => result,
                Ok(_) => MISSING_ENTRY_POINT,
            };
        }
    };
    set_log(log, user);
    say("DLSS SR shares the D3D12 FG host and CPU frame token");
    *state = Some(Session {
        entries,
        features,
        route: Route::Shared {
            host,
            device: unsafe { com_ref::retain(shared.native_device) },
        },
        applied: [None; 2],
        present_entries: None,
        shutdown_failure: None,
        _strings: Box::new(InitStrings {
            plugin_paths: [null()],
            feature: 0,
        }),
        _module: None,
        _directories: [None, None],
    });
    OK
}

unsafe fn rsf_core_dlss_streamline_set_device(device: Handle) -> i32 {
    let Ok(mut state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    if state
        .as_ref()
        .is_some_and(|session| !session.route.host().is_null())
    {
        return OK;
    }
    if device.is_null() {
        return INVALID_ARGUMENT;
    }
    let Some(session) = state.as_mut().filter(|session| session.active()) else {
        return NOT_READY;
    };
    let Some(set_device) = session.entries.set_d3d_device else {
        return FEATURE_FAILED;
    };
    let result = unsafe { set_device(device) };
    if result != RESULT_OK {
        say(&format!(
            "slSetD3DDevice failed with result {}",
            result as u32
        ));
        return FEATURE_FAILED;
    }
    let entries = session.entries;
    let features = match unsafe {
        resolve_features(false, |name, address| {
            lookup(&entries, FEATURE_DLSS, name, address)
        })
    } {
        Ok(features) => features,
        Err(result) => return result,
    };
    session.route = Route::Owned(Some(unsafe { com_ref::retain(device) }));
    session.features = features;
    session.applied = [None; 2];
    if features.bits() == OPTIMAL | OPTIONS {
        OK
    } else {
        say("DLSS feature functions are not available");
        MISSING_ENTRY_POINT
    }
}

unsafe fn rsf_core_dlss_streamline_support(out: *mut Support) -> i32 {
    if !unsafe { has_capacity(out) } {
        return INVALID_ARGUMENT;
    }
    let Ok(state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    let Some(session) = state.as_ref().filter(|session| session.active()) else {
        return NOT_READY;
    };
    let Some(device) = session.route.device() else {
        return NOT_READY;
    };
    let luid = if session.route.host().is_null() {
        unsafe { crate::streamline_sr::d3d11_luid(device.as_raw()) }
    } else {
        Some(unsafe { crate::streamline_sr::d3d12_luid(device.as_raw()) })
    };
    let Some(mut luid) = luid else {
        return NOT_READY;
    };
    let entries = &session.entries;
    let Some(supported) = entries.is_feature_supported else {
        return FEATURE_FAILED;
    };
    let adapter = sdk::adapter(&mut luid);
    let result = unsafe { supported(FEATURE_DLSS, &adapter) } as u32;
    let supported = result == 0;
    // The caller may have initialized only struct_size. Publish the
    // capability flags before the optional driver requirements query.
    let size = unsafe { addr_of!((*out).struct_size).read() };
    unsafe {
        out.write(Support {
            struct_size: size,
            supported: supported.into(),
            driver_out_of_date: (result == RESULT_DRIVER_OUT_OF_DATE).into(),
            os_out_of_date: (result == RESULT_OS_OUT_OF_DATE).into(),
            no_supported_adapter: (result == RESULT_NO_SUPPORTED_ADAPTER).into(),
            required_driver_major: 0,
            required_driver_minor: 0,
            detected_driver_major: 0,
            detected_driver_minor: 0,
        });
    }
    let Some(requirements) = entries.get_feature_requirements else {
        return FEATURE_FAILED;
    };
    let mut record = sdk::requirements();
    let (required, detected) = if unsafe { requirements(FEATURE_DLSS, &mut record) } == RESULT_OK {
        (record.driver_required, record.driver_detected)
    } else {
        Default::default()
    };
    unsafe {
        addr_of_mut!((*out).required_driver_major).write(required.major);
        addr_of_mut!((*out).required_driver_minor).write(required.minor);
        addr_of_mut!((*out).detected_driver_major).write(detected.major);
        addr_of_mut!((*out).detected_driver_minor).write(detected.minor);
    }
    say(&format!(
        "DLSS support on this adapter: {} (streamline result {result}, driver {}.{}, requires {}.{})",
        if supported { "yes" } else { "no" },
        detected.major,
        detected.minor,
        required.major,
        required.minor,
    ));
    if supported { OK } else { NOT_SUPPORTED }
}

unsafe fn rsf_core_dlss_streamline_plan(plan: *mut Plan) -> i32 {
    if !unsafe { has_capacity(plan) } {
        return INVALID_ARGUMENT;
    }
    // The caller only needs to initialize the inputs.
    let (width, height, quality) = unsafe {
        (
            addr_of!((*plan).output_width).read(),
            addr_of!((*plan).output_height).read(),
            addr_of!((*plan).quality).read(),
        )
    };
    if width == 0 || height == 0 {
        return INVALID_ARGUMENT;
    }
    let Ok(state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    let Some(session) = state
        .as_ref()
        .filter(|session| session.active() && session.features() & OPTIMAL != 0)
    else {
        return NOT_READY;
    };
    let Some(optimal) = session.features.optimal else {
        return FEATURE_FAILED;
    };
    let options = sdk::plan_options(&dimensions(quality, width, height));
    let mut sizes = sdk::dlss_optimal();
    let result = unsafe { optimal(&options, &mut sizes) };
    if result != RESULT_OK {
        say(&format!(
            "slDLSSGetOptimalSettings failed with result {}",
            result as u32
        ));
        return FEATURE_FAILED;
    }
    let (render_width, render_height) = (sizes.width, sizes.height);
    unsafe {
        addr_of_mut!((*plan).render_width).write(render_width);
        addr_of_mut!((*plan).render_height).write(render_height);
        addr_of_mut!((*plan).render_width_min).write(sizes.min_width);
        addr_of_mut!((*plan).render_height_min).write(sizes.min_height);
        addr_of_mut!((*plan).render_width_max).write(sizes.max_width);
        addr_of_mut!((*plan).render_height_max).write(sizes.max_height);
    }
    say(&format!(
        "DLSS wants {render_width}x{render_height} to produce {width}x{height}"
    ));
    OK
}

unsafe fn rsf_core_dlss_streamline_present(
    chain: Handle,
    interval: u32,
    flags: u32,
    begin: u32,
) -> i32 {
    if chain.is_null() || begin > 1 {
        return INVALID_ARGUMENT;
    }
    let Ok(mut state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    let Some(session) = state.as_mut().filter(|session| session.active()) else {
        return NOT_READY;
    };
    if !session.route.host().is_null() {
        return OK;
    }
    if session.route.device().is_none() {
        return NOT_READY;
    }
    let entries = match session.present_entries {
        Some(entries) => entries,
        None => {
            let mut entries = [null_mut(); 2];
            for (name, entry) in [c"slHookPresent", c"slHookAfterPresent"]
                .into_iter()
                .zip(&mut entries)
            {
                let result = unsafe { lookup(&session.entries, FEATURE_COMMON, name, entry) };
                if result != OK {
                    return result;
                }
            }
            if entries.iter().any(|entry| entry.is_null()) {
                return MISSING_ENTRY_POINT;
            }
            session.present_entries = Some(entries);
            entries
        }
    };
    if unsafe { sdk::hook_present(entries, chain, interval, flags, begin != 0) } {
        OK
    } else {
        FEATURE_FAILED
    }
}

/// The frame's camera and motion constants.
fn frame_constants(frame: &Frame) -> Constants {
    Constants {
        view_to_clip: frame.camera_view_to_clip,
        clip_to_view: frame.clip_to_camera_view,
        clip_to_previous: frame.clip_to_prev_clip,
        previous_to_clip: frame.prev_clip_to_clip,
        jitter: [frame.jitter_x, frame.jitter_y],
        motion_scale: [frame.motion_scale_x, frame.motion_scale_y],
        pinhole: [0.0; 2],
        position: frame.camera_position,
        up: frame.camera_up,
        right: frame.camera_right,
        forward: frame.camera_forward,
        near_plane: frame.near_plane,
        far_plane: frame.far_plane,
        fov: frame.vertical_fov,
        aspect: frame.aspect_ratio,
        invalid_motion: frame.motion_invalid_value,
        depth_inverted: u8::from(frame.depth_inverted != 0),
        camera_motion: u8::from(frame.camera_motion_included != 0),
        motion_3d: 0,
        reset: u8::from(frame.reset != 0),
    }
}

unsafe fn rsf_core_dlss_streamline_evaluate(
    context: Handle,
    frame: *const Frame,
    source: u64,
) -> i32 {
    let Ok(mut state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    let host = state
        .as_ref()
        .map_or(null_mut(), |session| session.route.host());
    let shared = !host.is_null();
    if shared != (source != 0) {
        return NOT_READY;
    }
    if context.is_null() || !unsafe { has_capacity(frame) } {
        return INVALID_ARGUMENT;
    }
    let frame = unsafe { &*frame };
    if frame.abi_version != RSF_DLSS_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if frame.viewport >= 2 {
        return INVALID_ARGUMENT;
    }
    if [frame.color_in, frame.color_out, frame.depth, frame.motion]
        .into_iter()
        .any(|input| input.is_null())
    {
        return INVALID_ARGUMENT;
    }
    let Some(session) = state
        .as_mut()
        .filter(|session| session.active() && session.features() & OPTIONS != 0)
    else {
        return NOT_READY;
    };
    let entries = session.entries;
    let token = if shared {
        unsafe { crate::streamline_host::rsf_core_slhost_token(host.cast(), source) }
    } else {
        match entries.get_new_frame_token {
            Some(token) => {
                // A zero index leaves frame counting to the SDK.
                let index = frame.frame_index;
                let index = if index == 0 { null() } else { &raw const index };
                let mut frame_token = null();
                if unsafe { token(&mut frame_token, index) } == RESULT_OK {
                    frame_token.cast_mut()
                } else {
                    null_mut()
                }
            }
            None => null_mut(),
        }
    };
    if token.is_null() {
        return if shared { NOT_READY } else { FEATURE_FAILED };
    }
    let slot = frame.viewport;
    let viewport = viewport(slot);
    let constants = sdk::constants(&frame_constants(frame));
    let submitted = if shared {
        unsafe { crate::streamline_host::common_set(host.cast(), source, slot, &constants) }
    } else {
        match entries.set_constants {
            Some(set) => (unsafe { set(&constants, token, &viewport) }) == RESULT_OK,
            None => return FEATURE_FAILED,
        }
    };
    if !submitted {
        say("slSetConstants failed");
        return FEATURE_FAILED;
    }

    if !crate::streamline_sr::preset_valid(frame.preset) {
        return INVALID_ARGUMENT;
    }
    let options = Options {
        mode: dlss_mode(frame.quality),
        width: frame.output_width,
        height: frame.output_height,
        hdr: (frame.color_encoded == 0).into(),
        auto_exposure: (frame.exposure.is_null() && frame.color_encoded == 0).into(),
        alpha: (frame.alpha != 0).into(),
        preset: frame.preset,
    };
    let applied = &mut session.applied[slot as usize];
    if *applied != Some(options) {
        let Some(set_options) = session.features.options else {
            *applied = None;
            return FEATURE_FAILED;
        };
        if unsafe { set_options(&viewport, &sdk::options(&options.payload())) } != RESULT_OK {
            say("slDLSSSetOptions failed");
            *applied = None;
            return FEATURE_FAILED;
        }
        *applied = Some(options);
    }
    let Some(set_tags) = entries.set_tag_for_frame else {
        return FEATURE_FAILED;
    };
    let mut tags = TagRecords::new();
    tags.fill(&TagBatch::d3d11(frame, shared));
    let (tags, count) = tags.tags();
    if unsafe { set_tags(token, &viewport, tags, count, context) } != RESULT_OK {
        say("slSetTagForFrame failed");
        return FEATURE_FAILED;
    }
    let Some(evaluate) = entries.evaluate_feature else {
        return FEATURE_FAILED;
    };
    let inputs = [&raw const viewport.base];
    let result = unsafe { evaluate(FEATURE_DLSS, token, inputs.as_ptr(), 1, context) };
    if result != RESULT_OK {
        say(&format!(
            "slEvaluateFeature failed with result {}",
            result as u32
        ));
        return FEATURE_FAILED;
    }
    OK
}

unsafe fn rsf_core_dlss_streamline_release(slot: u32) -> i32 {
    if slot >= 2 {
        return INVALID_ARGUMENT;
    }
    let Ok(mut state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    let Some(session) = state.as_mut().filter(|session| session.active()) else {
        return NOT_READY;
    };
    let host = session.route.host();
    if !host.is_null() && unsafe { crate::streamline_host::drain(host.cast()) } != OK {
        return FEATURE_FAILED;
    }
    session.applied[slot as usize] = None;
    let Some(free) = session.entries.free_resources else {
        return FEATURE_FAILED;
    };
    if unsafe { free(FEATURE_DLSS, &viewport(slot)) } == RESULT_OK {
        OK
    } else {
        FEATURE_FAILED
    }
}

unsafe fn rsf_core_dlss_streamline_shutdown() -> i32 {
    if Output::active_here() {
        return NOT_READY;
    }
    let Ok(mut state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    let Some(session) = state.as_mut() else {
        return NOT_READY;
    };
    if let Some(failure) = session.shutdown_failure {
        return failure;
    }
    if matches!(&session.route, Route::Owned(_)) {
        let stopped = match session.entries.shutdown {
            Some(shutdown) if unsafe { shutdown() } == RESULT_OK => OK,
            _ => FEATURE_FAILED,
        };
        if stopped != OK {
            // Shutdown can fail after partial teardown. Keep the dependencies
            // alive and close admission to every subsequent SDK operation.
            session.shutdown_failure = Some(stopped);
            return stopped;
        }
    }
    // The session releases the log sink, then the COM device, module and borrowed directories.
    let retired = state.take();
    say("streamline shut down");
    drop(retired);
    OK
}
