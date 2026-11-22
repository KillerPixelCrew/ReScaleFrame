// SPDX-License-Identifier: MIT
//! Typed Streamline calls behind the D3D11 and shared-host DLSS SR facade. The interposer exports
//! are plain C entry points taking record pointers, so Rust builds the records and calls them.
use super::*;
use crate::streamline_log::{
    rsf_core_streamline_log_clear, rsf_core_streamline_log_emit, rsf_core_streamline_log_set,
};
use crate::streamline_sr::native::{
    rsf_dlss_sr_d3d11_luid, rsf_dlss_sr_d3d12_luid, rsf_dlss_sr_host_device, rsf_dlss_sr_load_sdk,
};
use crate::streamline_sr::sdk::{
    self as records, ConstantsFn, EvaluateFn, FEATURE_COMMON, FEATURE_DLSS, FeatureFn, FreeFn,
    HookAfterPresentFn, HookPresentFn, InitFn, LOG_ERROR, LOG_WARN, OptimalFn, OptionsFn,
    PREFERENCES, RESULT_OK, RequirementsFn, SDK_VERSION, SetDeviceFn, ShutdownFn, SupportedFn,
    TagRecords, TagsFn, TokenFn,
};
use crate::streamline_sr::{Constants, Dimensions, Optimal, OptionPayload, TagBatch};
use crate::vulkan_streamline_camera::{rsf_slvk_sdk_header, rsf_slvk_sdk_preferences};
use std::borrow::Cow;
use std::mem::transmute_copy;

const LEGACY_SR_LOG: u32 = 0;

unsafe extern "C" {
    fn rsf_streamline_host_module(host: Handle) -> Handle;
    fn rsf_streamline_host_token(host: Handle, source: u64) -> Handle;
    fn rsf_streamline_host_drain(host: Handle) -> i32;
    fn rsf_streamline_common_set(
        host: Handle,
        source: u64,
        viewport: u32,
        constants: *const crate::vulkan_streamline_camera::rsf_slvk_sdk_constants,
    ) -> u32;
}

/// Everything Streamline exports that this integration uses, resolved by name from the interposer
/// rather than linked, so a missing entry point is a reported failure instead of a load-time abort.
struct Native {
    init: Option<InitFn>,
    shutdown: Option<ShutdownFn>,
    supported: Option<SupportedFn>,
    set_device: Option<SetDeviceFn>,
    feature_function: Option<FeatureFn>,
    requirements: Option<RequirementsFn>,
    token: Option<TokenFn>,
    constants: Option<ConstantsFn>,
    tags: Option<TagsFn>,
    evaluate: Option<EvaluateFn>,
    free: Option<FreeFn>,
    optimal: Option<OptimalFn>,
    options: Option<OptionsFn>,
    // The SDK borrows these through slInit. Rust owns the strings through shutdown.
    plugin_paths: [*const u16; 1],
    feature: u32,
}

impl Drop for Native {
    fn drop(&mut self) {
        rsf_core_streamline_log_clear(LEGACY_SR_LOG, 0);
    }
}

unsafe fn native<'a>(handle: Handle) -> &'a mut Native {
    unsafe { &mut *handle.cast::<Native>() }
}

unsafe fn typed<F: Copy>(address: Handle) -> Option<F> {
    (!address.is_null()).then(|| unsafe { transmute_copy::<Handle, F>(&address) })
}

unsafe extern "C" fn emit(log: Log, user: Handle, text: *const c_char) {
    if let Some(log) = log {
        unsafe { log(user, text) };
    }
}

fn say(text: &str) {
    if let Ok(text) = CString::new(text) {
        unsafe { rsf_core_streamline_log_emit(LEGACY_SR_LOG, 1, text.as_ptr(), emit) };
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

unsafe extern "C" fn allocate() -> Handle {
    Box::into_raw(Box::new(Native {
        init: None,
        shutdown: None,
        supported: None,
        set_device: None,
        feature_function: None,
        requirements: None,
        token: None,
        constants: None,
        tags: None,
        evaluate: None,
        free: None,
        optimal: None,
        options: None,
        plugin_paths: [null()],
        feature: 0,
    }))
    .cast()
}

unsafe extern "C" fn dispose(handle: Handle) {
    drop(unsafe { Box::from_raw(handle.cast::<Native>()) });
}

unsafe extern "C" fn set_log(log: Log, user: Handle) {
    if log.is_some() {
        rsf_core_streamline_log_set(LEGACY_SR_LOG, 0, log, user, 0);
    } else {
        rsf_core_streamline_log_clear(LEGACY_SR_LOG, 0);
    }
}

unsafe extern "C" fn missing_entry(name: *const c_char) {
    say(&format!(
        "entry point {} is missing from the interposer",
        unsafe { CStr::from_ptr(name) }.to_string_lossy()
    ));
}

unsafe extern "C" fn bind_entries(handle: Handle, addresses: &Entries) {
    let native = unsafe { native(handle) };
    // The order is ENTRY_NAMES. The shared route leaves the owned-only entries unresolved.
    let [
        supported,
        feature_function,
        requirements,
        constants,
        tags,
        evaluate,
        free,
        init,
        shutdown,
        set_device,
        token,
        _allocate,
    ] = *addresses;
    unsafe {
        native.supported = typed(supported);
        native.feature_function = typed(feature_function);
        native.requirements = typed(requirements);
        native.constants = typed(constants);
        native.tags = typed(tags);
        native.evaluate = typed(evaluate);
        native.free = typed(free);
        native.init = typed(init);
        native.shutdown = typed(shutdown);
        native.set_device = typed(set_device);
        native.token = typed(token);
    }
}

unsafe extern "C" fn init_owned(handle: Handle, input: &InitPreferences) -> i32 {
    let native = unsafe { native(handle) };
    native.feature = input.feature;
    let has_plugins = !input.plugins.is_null();
    if has_plugins {
        native.plugin_paths[0] = input.plugins;
    }
    let preferences = rsf_slvk_sdk_preferences {
        base: PREFERENCES,
        show_console: u8::from(input.show_console != 0),
        log_level: input.log_level,
        plugin_paths: if has_plugins {
            native.plugin_paths.as_mut_ptr()
        } else {
            null_mut()
        },
        num_plugin_paths: u32::from(has_plugins),
        logs_path: input.logs,
        log: Some(streamline_message),
        flags: records::DEFAULT_PREFERENCE_FLAGS | input.flags,
        features: &raw const native.feature,
        num_features: 1,
        application_id: input.application_id,
        engine: input.engine,
        engine_version: input.engine_version,
        project: input.project_id,
        render_api: input.render_api,
        ..Default::default()
    };
    let Some(init) = native.init else {
        return INIT_FAILED;
    };
    let result = unsafe { init(&preferences, SDK_VERSION) };
    if result != RESULT_OK {
        say(&format!("slInit failed with result {}", result as u32));
        return INIT_FAILED;
    }
    OK
}

unsafe extern "C" fn register_device(handle: Handle, device: Handle) -> i32 {
    let Some(set_device) = unsafe { native(handle) }.set_device else {
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
    OK
}

unsafe extern "C" fn lookup(
    handle: Handle,
    feature: u32,
    name: *const c_char,
    out: &mut Handle,
) -> i32 {
    let Some(function) = unsafe { native(handle) }.feature_function else {
        return FEATURE_FAILED;
    };
    if unsafe { function(feature, name, out) } == RESULT_OK {
        OK
    } else {
        MISSING_ENTRY_POINT
    }
}

unsafe extern "C" fn lookup_dlss_function(
    handle: Handle,
    name: *const c_char,
    out: &mut Handle,
) -> i32 {
    unsafe { lookup(handle, FEATURE_DLSS, name, out) }
}

unsafe extern "C" fn lookup_present(handle: Handle, name: *const c_char, out: &mut Handle) -> i32 {
    unsafe { lookup(handle, FEATURE_COMMON, name, out) }
}

unsafe extern "C" fn bind_dlss_functions(handle: Handle, optimal: Handle, options: Handle) {
    let native = unsafe { native(handle) };
    native.optimal = unsafe { typed(optimal) };
    native.options = unsafe { typed(options) };
}

unsafe extern "C" fn present(
    entries: &PresentEntries,
    swapchain: Handle,
    interval: u32,
    flags: u32,
    begin: u32,
) -> i32 {
    let [before, after] = *entries;
    let mut skip = false;
    let result = if begin != 0 {
        let before = unsafe { transmute_copy::<Handle, HookPresentFn>(&before) };
        unsafe { before(swapchain, interval, flags, &raw mut skip) }
    } else {
        let after = unsafe { transmute_copy::<Handle, HookAfterPresentFn>(&after) };
        unsafe { after(flags) }
    };
    if result >= 0 && !skip {
        OK
    } else {
        FEATURE_FAILED
    }
}

unsafe extern "C" fn query_support(handle: Handle, luid: u64, out: &mut u32) -> i32 {
    let Some(supported) = unsafe { native(handle) }.supported else {
        return FEATURE_FAILED;
    };
    let mut luid = luid;
    let adapter = records::adapter(&mut luid);
    *out = unsafe { supported(FEATURE_DLSS, &adapter) } as u32;
    OK
}

unsafe extern "C" fn requirements(handle: Handle, out: &mut planning::DriverVersions) -> i32 {
    let Some(requirements) = unsafe { native(handle) }.requirements else {
        return FEATURE_FAILED;
    };
    let mut record = records::requirements();
    if unsafe { requirements(FEATURE_DLSS, &mut record) } == RESULT_OK {
        *out = planning::DriverVersions {
            required_major: record.driver_required.major,
            required_minor: record.driver_required.minor,
            detected_major: record.driver_detected.major,
            detected_minor: record.driver_detected.minor,
        };
    }
    OK
}

unsafe extern "C" fn optimal(handle: Handle, dimensions: &Dimensions, out: &mut Optimal) -> i32 {
    let Some(optimal) = unsafe { native(handle) }.optimal else {
        return FEATURE_FAILED;
    };
    let options = records::plan_options(dimensions);
    let mut settings = records::dlss_optimal();
    let result = unsafe { optimal(&options, &mut settings) };
    if result != RESULT_OK {
        say(&format!(
            "slDLSSGetOptimalSettings failed with result {}",
            result as u32
        ));
        return FEATURE_FAILED;
    }
    *out = Optimal {
        width: settings.width,
        height: settings.height,
        width_min: settings.width_min,
        height_min: settings.height_min,
        width_max: settings.width_max,
        height_max: settings.height_max,
    };
    OK
}

unsafe extern "C" fn owned_token(handle: Handle, frame_index: *const u32) -> Handle {
    let Some(token) = unsafe { native(handle) }.token else {
        return null_mut();
    };
    let mut frame = null_mut();
    if unsafe { token(&mut frame, frame_index) } == RESULT_OK {
        frame
    } else {
        null_mut()
    }
}

fn constants_result(submitted: bool) -> i32 {
    if !submitted {
        say("slSetConstants failed");
        return FEATURE_FAILED;
    }
    OK
}

unsafe extern "C" fn owned_constants(
    handle: Handle,
    token: Handle,
    slot: u32,
    input: &Constants,
) -> i32 {
    let Some(constants) = unsafe { native(handle) }.constants else {
        return FEATURE_FAILED;
    };
    let record = records::constants(input);
    let viewport = records::viewport(slot);
    constants_result(unsafe { constants(&record, token, &viewport) } == RESULT_OK)
}

unsafe extern "C" fn host_constants(
    host: Handle,
    source: u64,
    viewport: u32,
    input: &Constants,
) -> i32 {
    let record = records::constants(input);
    constants_result(unsafe { rsf_streamline_common_set(host, source, viewport, &record) } != 0)
}

unsafe extern "C" fn set_options(handle: Handle, slot: u32, input: &OptionPayload) -> i32 {
    let Some(options) = unsafe { native(handle) }.options else {
        return FEATURE_FAILED;
    };
    let record = records::options(input);
    let viewport = records::viewport(slot);
    if unsafe { options(&viewport, &record) } != RESULT_OK {
        say("slDLSSSetOptions failed");
        return FEATURE_FAILED;
    }
    OK
}

unsafe extern "C" fn set_tags(
    handle: Handle,
    context: Handle,
    token: Handle,
    slot: u32,
    batch: &TagBatch,
) -> i32 {
    let Some(tags) = unsafe { native(handle) }.tags else {
        return FEATURE_FAILED;
    };
    let mut tag_records = TagRecords::new();
    tag_records.fill(batch);
    let (entries, count) = tag_records.tags();
    let viewport = records::viewport(slot);
    if unsafe { tags(token, &viewport, entries, count, context) } != RESULT_OK {
        say("slSetTagForFrame failed");
        return FEATURE_FAILED;
    }
    OK
}

unsafe extern "C" fn evaluate(handle: Handle, context: Handle, token: Handle, slot: u32) -> i32 {
    let Some(evaluate) = unsafe { native(handle) }.evaluate else {
        return FEATURE_FAILED;
    };
    let viewport = records::viewport(slot);
    let inputs: [*const rsf_slvk_sdk_header; 1] = [&raw const viewport.base];
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

unsafe extern "C" fn drain(host: Handle) -> u32 {
    u32::from(unsafe { rsf_streamline_host_drain(host) } == OK)
}

unsafe extern "C" fn release_viewport(handle: Handle, slot: u32) -> i32 {
    let Some(free) = unsafe { native(handle) }.free else {
        return FEATURE_FAILED;
    };
    let viewport = records::viewport(slot);
    if unsafe { free(FEATURE_DLSS, &viewport) } == RESULT_OK {
        OK
    } else {
        FEATURE_FAILED
    }
}

unsafe extern "C" fn shutdown(handle: Handle) -> i32 {
    let Some(shutdown) = unsafe { native(handle) }.shutdown else {
        return FEATURE_FAILED;
    };
    if unsafe { shutdown() } == RESULT_OK {
        OK
    } else {
        FEATURE_FAILED
    }
}

unsafe extern "C" fn report(text: *const c_char) {
    say(&unsafe { CStr::from_ptr(text) }.to_string_lossy());
}

static STARTUP: Startup = Startup {
    allocate,
    dispose,
    set_log,
    missing_entry,
    bind_entries,
    init: init_owned,
    host_module: rsf_streamline_host_module,
    host_device: rsf_dlss_sr_host_device,
    register_device,
    lookup_feature: lookup_dlss_function,
    bind_features: bind_dlss_functions,
    report,
};

static PLANNING: planning::Platform = planning::Platform {
    d3d11_luid: rsf_dlss_sr_d3d11_luid,
    device_luid: rsf_dlss_sr_d3d12_luid,
    query_support,
    requirements,
    optimal,
};

static FRAME: frame::Platform = frame::Platform {
    owned_token,
    host_token: rsf_streamline_host_token,
    owned_constants,
    host_constants,
};

static PLATFORM: Platform = Platform {
    load: rsf_dlss_sr_load_sdk,
    startup: &STARTUP,
    planning: &PLANNING,
    lookup_present,
    present,
    frame: &FRAME,
    options: set_options,
    tags: set_tags,
    evaluate,
    drain,
    release_viewport,
    shutdown,
};

/// The typed SDK calls. The vendor headers check the record layouts they build in the
/// `dlss_sr_layout` test (`tests/dlss_sr_layout_facts.cpp`).
pub(super) fn platform() -> Option<&'static Platform> {
    Some(&PLATFORM)
}
