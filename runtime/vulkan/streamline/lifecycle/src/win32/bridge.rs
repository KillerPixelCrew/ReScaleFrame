// SPDX-License-Identifier: MIT
//! The `rsf_vk_sl_*` exports of the Streamline Vulkan bridge and its typed SDK
//! calls. The lifecycle owner admits, loads, initializes,
//! shuts down and unloads the interposer; these calls run under its session or
//! frame-token lease. admission.rs admits the interposer by the digest the build
//! selects.
use super::LIFECYCLE;
use crate::frame_generation::Options;
use crate::lifecycle::feature_mask;
use crate::sdk_api::{self, Entries, FEATURE_DLSS_G, FEATURE_PCL, FEATURE_REFLEX, OK, guarded};
use crate::sdk_fg_options::{self, Packed, Sample, State};
use crate::sdk_records::{self, rsf_slvk_pack_legacy_camera, rsf_slvk_pack_legacy_tags};
use crate::sys::{
    rsf_slvk_legacy_camera, rsf_slvk_legacy_image, rsf_slvk_sdk_constants, rsf_slvk_sdk_header,
    rsf_slvk_sdk_pcl_options, rsf_slvk_sdk_pcl_state, rsf_slvk_sdk_reflex_options,
    rsf_slvk_sdk_reflex_state, rsf_slvk_sdk_tag_batch,
};
use std::ffi::{CStr, c_char, c_void};
use std::marker::PhantomData;
use std::ptr::{self, null, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{PoisonError, RwLock};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

unsafe extern "C-unwind" {
    // sdk_guard.c: the sl::FrameToken virtual conversion.
    fn rsf_slvk_token_index(token: sdk_api::Token) -> u32;
}

/// The interposer's core entry points. slShutdown, a failed slInit and unload clear them.
static CORE: RwLock<Option<Entries>> = RwLock::new(None);

fn core() -> Entries {
    CORE.read()
        .unwrap_or_else(PoisonError::into_inner)
        .unwrap_or_default()
}

/// One typed feature function, bound through slGetFeatureFunction and cleared with the core.
struct Entry<T>(AtomicPtr<c_void>, PhantomData<T>);

impl<T: Copy> Entry<T> {
    const fn new() -> Self {
        Self(AtomicPtr::new(null_mut()), PhantomData)
    }

    fn get(&self) -> Option<T> {
        // Each static's T is the declared type of the address it stores.
        unsafe { sdk_api::function(self.0.load(Ordering::Acquire)) }
    }

    fn set(&self, address: *mut c_void) -> bool {
        self.0.store(address, Ordering::Release);
        !address.is_null()
    }

    fn clear(&self) {
        self.set(null_mut());
    }
}

static REFLEX_OPTIONS: Entry<sdk_api::Apply<rsf_slvk_sdk_reflex_options>> = Entry::new();
static REFLEX_SLEEP: Entry<sdk_api::ReflexSleep> = Entry::new();
static REFLEX_STATE: Entry<sdk_api::Read<rsf_slvk_sdk_reflex_state>> = Entry::new();
static PCL_MARKER: Entry<sdk_api::PclSetMarker> = Entry::new();
static PCL_STATE: Entry<sdk_api::Read<rsf_slvk_sdk_pcl_state>> = Entry::new();
static PCL_OPTIONS: Entry<sdk_api::Apply<rsf_slvk_sdk_pcl_options>> = Entry::new();
static FG_OPTIONS: Entry<sdk_api::SetOptions<Packed>> = Entry::new();
static FG_STATE: Entry<sdk_api::GetFgState<State, Packed>> = Entry::new();

const VIEWPORT: u32 = 0;

/// Resolves a feature function only when Streamline reports success and a
/// nonnull address.
fn bind<T: Copy>(entry: &Entry<T>, feature: u32, name: &CStr) -> bool {
    let Some(get) = core().get_feature_function else {
        return false;
    };
    let mut address = null_mut();
    guarded(|| unsafe { get(feature, name.as_ptr(), &mut address) }) == OK && entry.set(address)
}

/// The lifecycle's native leaves. Unit tests call fakes, so these are not compiled there.
#[cfg(not(test))]
pub(crate) mod lifecycle_leaf {
    use super::{CORE, Entries, FG_OPTIONS, FG_STATE, guarded};
    use super::{PCL_MARKER, PCL_OPTIONS, PCL_STATE, REFLEX_OPTIONS, REFLEX_SLEEP, REFLEX_STATE};
    use crate::sys::{rsf_slvk_sdk_log, rsf_slvk_sdk_preferences};
    use std::ffi::{c_char, c_void};
    use std::ptr::{NonNull, null_mut};
    use std::sync::PoisonError;
    use windows_sys::Win32::Foundation::{CloseHandle, FreeLibrary};
    use windows_sys::Win32::System::LibraryLoader::{
        GetModuleHandleW, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
        LoadLibraryExW,
    };

    pub(crate) const LOG: rsf_slvk_sdk_log = Some(super::super::rsf_slvk_log_message);

    pub(crate) unsafe fn load(path: *const u16) -> *mut c_void {
        let flags = LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS;
        unsafe { LoadLibraryExW(path, null_mut(), flags) }
    }

    pub(crate) unsafe fn free(module: NonNull<c_void>) {
        unsafe { FreeLibrary(module.as_ptr()) };
    }

    pub(crate) unsafe fn close(handle: *mut c_void) {
        unsafe { CloseHandle(handle) };
    }

    pub(crate) unsafe fn symbol(module: *mut c_void, name: *const c_char) -> *mut c_void {
        unsafe { crate::sdk_api::proc_address(module, name) }
    }

    pub(crate) unsafe fn loaded(path: *const u16) -> *mut c_void {
        unsafe { GetModuleHandleW(path) }
    }

    pub(crate) fn verbose() -> bool {
        std::env::var_os("VULKANSTORY_STREAMLINE_VERBOSE").is_some_and(|value| !value.is_empty())
    }

    pub(crate) unsafe fn open_verified(path: *const u16) -> *mut c_void {
        unsafe { crate::admission::open_by_digest(path) }
    }

    /// Publishes the core entry points; true when every one the lifecycle needs resolved.
    pub(crate) unsafe fn resolve(module: *mut c_void) -> bool {
        let entries = unsafe { Entries::resolve(module) };
        *CORE.write().unwrap_or_else(PoisonError::into_inner) = Some(entries);
        entries.init.is_some()
            && entries.shutdown.is_some()
            && entries.get_feature_function.is_some()
            && entries.get_new_frame_token.is_some()
            && entries.is_feature_supported.is_some()
            && entries.set_tag_for_frame.is_some()
            && entries.set_constants.is_some()
            && entries.free_resources.is_some()
    }

    pub(crate) unsafe fn init(request: *const rsf_slvk_sdk_preferences, version: u64) -> i32 {
        let Some(init) = super::core().init else {
            return -4;
        };
        guarded(|| unsafe { init(request, version) })
    }

    /// slShutdown's result, or None when the entry point is not bound.
    pub(crate) unsafe fn shutdown() -> Option<i32> {
        let shutdown = super::core().shutdown?;
        Some(guarded(|| unsafe { shutdown() }))
    }

    /// Clears the SDK entry points that slShutdown, a failed slInit or unload
    /// invalidates. The Vulkan dispatch entry points stay until unload.
    pub(crate) unsafe fn clear() {
        *CORE.write().unwrap_or_else(PoisonError::into_inner) = None;
        REFLEX_OPTIONS.clear();
        REFLEX_SLEEP.clear();
        REFLEX_STATE.clear();
        PCL_MARKER.clear();
        PCL_STATE.clear();
        PCL_OPTIONS.clear();
        FG_OPTIONS.clear();
        FG_STATE.clear();
    }
}
// Called by Streamline's present thread. The atomic store lets the render
// thread report or recover on its next state query.
unsafe extern "C" fn present_error(error: *const i32) {
    if let Some(&error) = unsafe { error.as_ref() } {
        LIFECYCLE.diagnostics.present_error(error);
    }
}

// Frame-generation leaves. Rust invokes them only under the initialized session.

pub(crate) fn fg_options_bound() -> bool {
    FG_OPTIONS.get().is_some()
}

pub(crate) unsafe fn fg_apply(request: &Packed) -> i32 {
    let Some(apply) = FG_OPTIONS.get() else {
        return -1;
    };
    let callback: sdk_api::ApiErrorCallback = present_error;
    let options = Packed {
        // The slot holds PFunOnAPIErrorCallback, which receives `const APIError&`.
        reserved_callback: Some(unsafe {
            std::mem::transmute::<sdk_api::ApiErrorCallback, unsafe extern "C" fn()>(callback)
        }),
        ..*request
    };
    let viewport = sdk_api::viewport(VIEWPORT);
    guarded(|| unsafe { apply(&viewport, &options) })
}

pub(crate) unsafe fn fg_release() -> i32 {
    let Some(free) = core().free_resources else {
        return -1;
    };
    let viewport = sdk_api::viewport(VIEWPORT);
    guarded(|| unsafe { free(FEATURE_DLSS_G, &viewport) })
}

/// Runs a synchronous call that pins the initialized SDK session.
fn with_sdk(inactive: i32, call: impl FnOnce() -> i32) -> i32 {
    LIFECYCLE.with_sdk(inactive, call)
}

/// Runs a call with the current token when `explicit` is zero, pinning the
/// initialized SDK until it returns.
fn with_token(explicit: usize, inactive: i32, call: impl FnOnce(sdk_api::Token) -> i32) -> i32 {
    LIFECYCLE
        .frame_token
        .with_token(explicit, inactive, |token| {
            call(ptr::with_exposed_provenance(token))
        })
}

/// Loads the verified interposer and initializes the selected PCL, Reflex and
/// DLSS feature set. A repeated request must match the active directory,
/// project and features. After shutdown, a later call reinitializes the
/// retained module from the same directory. Returns zero, a negative local
/// failure or the Streamline result.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_sl_initialize_features(
    directory: *const u16,
    project: *const c_char,
    reflex: u32,
    frame_generation: u32,
    super_resolution: u32,
) -> i32 {
    let features = feature_mask(reflex != 0, frame_generation != 0, super_resolution != 0);
    unsafe { LIFECYCLE.initialize(directory, project, features) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_sl_initialize(
    directory: *const u16,
    project: *const c_char,
    reflex: u32,
    frame_generation: u32,
) -> i32 {
    unsafe { rsf_vk_sl_initialize_features(directory, project, reflex, frame_generation, 0) }
}

/// Shuts Streamline down and clears the SDK entry points while retaining the
/// interposer. Call after provider work is retired; in-flight token calls
/// return busy. A nonzero SDK result is sticky and refuses later
/// initialization and unload.
#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_shutdown_checked() -> i32 {
    unsafe { LIFECYCLE.shutdown() }
}

/// The original void ABI for hosts predating checked shutdown.
#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_shutdown() {
    rsf_vk_sl_shutdown_checked();
}

/// Unloads the interposer. Call only after shutdown and after every Vulkan
/// object and API table that references it is destroyed.
#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_unload_checked() -> i32 {
    unsafe { LIFECYCLE.unload() }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_unload() {
    rsf_vk_sl_unload_checked();
}

/// Queries DLSS-G support for the selected physical device after the proxy
/// has seen device creation. Returns the Streamline result or -1.
#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_is_frame_generation_supported(physical: *mut c_void) -> i32 {
    if physical.is_null() {
        return -1;
    }
    with_sdk(-1, || {
        let Some(supported) = core().is_feature_supported else {
            return -1;
        };
        let adapter = sdk_api::adapter(physical);
        guarded(|| unsafe { supported(FEATURE_DLSS_G, &adapter) })
    })
}

/// Resolves the Reflex options, sleep and state entry points.
#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_bind_reflex() -> i32 {
    with_sdk(-1, || {
        if !bind(&REFLEX_OPTIONS, FEATURE_REFLEX, c"slReflexSetOptions") {
            -1
        } else if !bind(&REFLEX_SLEEP, FEATURE_REFLEX, c"slReflexSleep") {
            -2
        } else if !bind(&REFLEX_STATE, FEATURE_REFLEX, c"slReflexGetState") {
            -3
        } else {
            0
        }
    })
}

/// Resolves the DLSS-G option and state entry points.
#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_bind_frame_generation() -> i32 {
    with_sdk(-4, || {
        if !bind(&FG_OPTIONS, FEATURE_DLSS_G, c"slDLSSGSetOptions") {
            -4
        } else if !bind(&FG_STATE, FEATURE_DLSS_G, c"slDLSSGGetState") {
            -5
        } else {
            0
        }
    })
}

fn reflex_state() -> rsf_slvk_sdk_reflex_state {
    rsf_slvk_sdk_reflex_state {
        base: sdk_api::REFLEX_STATE,

        // The SDK writes the frame reports; this caller reads only the leading facts.
        ..Default::default()
    }
}

/// Queries low-latency and latency-report availability. Outputs are written
/// only after a successful query.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_sl_get_reflex_state(available: *mut u32, reports: *mut u32) -> i32 {
    if available.is_null() || reports.is_null() {
        return -1;
    }
    with_sdk(-1, || {
        let Some(read) = REFLEX_STATE.get() else {
            return -1;
        };
        let mut state = Box::new(reflex_state());
        let result = guarded(|| unsafe { read(&mut *state) });
        if result != OK {
            return result;
        }
        unsafe {
            available.write(u32::from(state.low_latency_available != 0));
            reports.write(u32::from(state.latency_report_available != 0));
        }
        0
    })
}

/// Resolves the PCL entry points and makes the calling host thread the SDK
/// marker thread.
#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_bind_pcl() -> i32 {
    with_sdk(-1, || {
        if !bind(&PCL_MARKER, FEATURE_PCL, c"slPCLSetMarker") {
            return -1;
        }
        if !bind(&PCL_STATE, FEATURE_PCL, c"slPCLGetState") {
            return -2;
        }
        if !bind(&PCL_OPTIONS, FEATURE_PCL, c"slPCLSetOptions") {
            return -3;
        }
        let Some(apply) = PCL_OPTIONS.get() else {
            return -3;
        };
        let mut options = rsf_slvk_sdk_pcl_options::default();
        sdk_records::rsf_slvk_pack_pcl_options(unsafe { GetCurrentThreadId() }, &mut options);
        guarded(|| unsafe { apply(&options) })
    })
}

/// The PCL statistics latency-ping window message, or zero.
#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_pcl_window_message() -> u32 {
    let mut message = 0;
    let result = with_sdk(-1, || {
        let Some(read) = PCL_STATE.get() else {
            return -1;
        };
        let mut state = rsf_slvk_sdk_pcl_state {
            base: sdk_api::PCL_STATE,

            window_message: 0,
        };
        let result = guarded(|| unsafe { read(&mut state) });
        if result == OK {
            message = state.window_message;
        }
        result
    });
    if result == 0 { message } else { 0 }
}

/// Applies Off, On or Boost (other modes select Off) with a minimum interval
/// in microseconds, or frames per second, where zero means no cap. The caller
/// selects the single pacing authority.
fn reflex_options(mode: i32, limit: u32, limit_is_fps: bool) -> i32 {
    let mode = match mode {
        1 => 1,
        2 => 2,
        _ => 0,
    };
    let interval = if limit_is_fps {
        1_000_000u32.checked_div(limit).unwrap_or(0)
    } else {
        limit
    };
    with_sdk(-1, || {
        let Some(apply) = REFLEX_OPTIONS.get() else {
            return -1;
        };
        let options = sdk_records::reflex_options(mode, interval, unsafe { GetCurrentThreadId() });
        guarded(|| unsafe { apply(&options) })
    })
}

#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_set_reflex_interval(mode: i32, interval_us: u32) -> i32 {
    reflex_options(mode, interval_us, false)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_set_reflex(mode: i32, max_fps: u32) -> i32 {
    reflex_options(mode, max_fps, true)
}

/// Obtains the Streamline token for a 32-bit renderer frame. Success replaces
/// the lifecycle's current borrowed token. Returns the SDK result, -1 without
/// token acquisition or -7 for an overlapping acquisition.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_sl_begin_frame_checked(index: u32, token: *mut usize) -> i32 {
    let Some(token) = (unsafe { token.as_mut() }) else {
        return -1;
    };
    *token = 0;
    let acquired = LIFECYCLE.frame_token.begin(|output| {
        let Some(acquire) = core().get_new_frame_token else {
            return -1;
        };
        let mut frame = null();
        let result = guarded(|| unsafe { acquire(&mut frame, &index) });
        *output = frame.expose_provenance();
        result
    });
    match acquired {
        Ok(acquired) => {
            *token = acquired;
            0
        }
        Err(result) => result,
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_begin_frame(index: u32) -> i32 {
    let mut token = 0;
    unsafe { rsf_vk_sl_begin_frame_checked(index, &mut token) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_token_matches(token: usize, index: u32) -> i32 {
    if token == 0 {
        return 0;
    }
    let matched = with_token(token, 0, |frame| {
        guarded(|| i32::from(unsafe { rsf_slvk_token_index(frame) } == index))
    });
    i32::from(matched == 1)
}

/// Runs Reflex sleep for the current token before host input polling.
#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_reflex_sleep() -> i32 {
    with_token(0, -1, |frame| {
        REFLEX_SLEEP
            .get()
            .map_or(-1, |sleep| guarded(|| unsafe { sleep(frame) }))
    })
}

fn emit_marker(token: usize, marker: u32) -> i32 {
    with_token(token, -1, |frame| {
        PCL_MARKER
            .get()
            .map_or(-1, |emit| guarded(|| unsafe { emit(marker, frame) }))
    })
}

/// Emits a PCL marker for the current token.
#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_marker(marker: u32) -> i32 {
    emit_marker(0, marker)
}

/// Emits a PCL marker for a retained token during asynchronous presentation.
/// The token stays borrowed and does not replace the current frame.
#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_marker_for_token(token: usize, marker: u32) -> i32 {
    if token == 0 {
        return -1;
    }
    emit_marker(token, marker)
}

fn submit_tags(
    frame: sdk_api::Token,
    commands: *mut c_void,
    images: Option<&[*const rsf_slvk_legacy_image; 4]>,
) -> i32 {
    let Some(set_tags) = core().set_tag_for_frame else {
        return -1;
    };
    let mut batch = rsf_slvk_sdk_tag_batch::default();
    unsafe {
        rsf_slvk_pack_legacy_tags(images.map_or(null(), |images| images.as_ptr()), &mut batch)
    };
    if images.is_some() {
        for (tag, resource) in batch.tags.iter_mut().zip(&mut batch.resources) {
            tag.resource = ptr::from_mut(resource).cast();
        }
    }
    let viewport = sdk_api::viewport(VIEWPORT);
    let count = batch.tags.len() as u32;
    guarded(|| unsafe { set_tags(frame, &viewport, batch.tags.as_ptr(), count, commands) })
}

/// Sets camera constants and tags depth, motion, HUD-less color and UI on the
/// current token. The inputs use eOnlyValidNow, so the SDK copies them on the
/// recording command buffer before the renderer reuses its staging images.
/// Returns the SDK result, or -1 for a missing binding or pointer.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_sl_tag_frame(
    commands: *mut c_void,
    depth: *const rsf_slvk_legacy_image,
    motion: *const rsf_slvk_legacy_image,
    hudless: *const rsf_slvk_legacy_image,
    ui: *const rsf_slvk_legacy_image,
    camera: *const rsf_slvk_legacy_camera,
) -> i32 {
    with_token(0, -1, |frame| {
        let images = [depth, motion, hudless, ui];
        let entries = core();
        let (Some(set_constants), Some(_)) = (entries.set_constants, entries.set_tag_for_frame)
        else {
            return -1;
        };
        if images.iter().any(|image| image.is_null()) {
            return -1;
        }
        let mut constants = rsf_slvk_sdk_constants::default();
        if unsafe { rsf_slvk_pack_legacy_camera(camera, &mut constants) } == 0 {
            return -1;
        }
        let viewport = sdk_api::viewport(VIEWPORT);
        let result = guarded(|| unsafe { set_constants(&constants, frame, &viewport) });
        if result != OK {
            return result;
        }
        submit_tags(frame, commands, Some(&images))
    })
}

/// Clears the scene resource tags of the current token, for non-scene frames
/// or before resources or provider state change.
#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_invalidate_frame_tags() -> i32 {
    with_token(0, -1, |frame| submit_tags(frame, null_mut(), None))
}

/// Applies DLSS-G enablement, interpolation count and presentation metadata.
/// Identical successful options are reused. Returns the SDK result, -1
/// without options or -2 for an enabled zero count.
#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_set_frame_generation(
    enabled: i32,
    count: u32,
    width: u32,
    height: u32,
    format: u32,
    back_buffers: u32,
) -> i32 {
    let enabled = enabled != 0;
    let inactive = if enabled { -1 } else { 0 };
    let options = Options::new(count, width, height, format, back_buffers);
    LIFECYCLE.with_frame_generation(inactive, |fg, revision| unsafe {
        fg.set(revision, enabled, options)
    })
}

/// Disables an enabled DLSS-G feature before the host's checked drain and
/// release. Unused or disabled features return zero.
#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_disable_frame_generation_for_release() -> i32 {
    LIFECYCLE.with_frame_generation(0, |fg, revision| unsafe { fg.disable(revision) })
}

/// Frees DLSS-G resources after enablement is Off and the host drained proxy
/// GPU and present work. Ownership clears only when slFreeResources succeeds.
#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_free_frame_generation_resources() -> i32 {
    LIFECYCLE.with_frame_generation(0, |fg, _| unsafe { fg.free() })
}

fn read_fg_sample(output: impl FnOnce(&Sample)) -> i32 {
    with_sdk(-1, || {
        let Some(get_state) = FG_STATE.get() else {
            return -1;
        };
        let mut state = sdk_fg_options::state();
        let viewport = sdk_api::viewport(VIEWPORT);
        let result = guarded(|| unsafe { get_state(&viewport, &mut state, null()) });
        if result == OK {
            output(&Sample {
                status: state.status,
                actual_presents: state.actual_presents,
                max_generated: state.max_generated,
                min_dimension: state.min_dimension,
                vsync: state.vsync,
                dynamic_mfg: state.dynamic_mfg,
            });
        }
        result
    })
}

/// Queries status, actual presents and the generated-frame limit. Outputs
/// are written only after a successful query.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_sl_get_frame_generation_state(
    status: *mut u32,
    presented: *mut u32,
    max_generated: *mut u32,
) -> i32 {
    if status.is_null() || presented.is_null() || max_generated.is_null() {
        return -1;
    }
    read_fg_sample(|sample| unsafe {
        status.write(sample.status);
        presented.write(sample.actual_presents);
        max_generated.write(sample.max_generated);
    })
}

/// Writes six words: status, actual presents, maximum generated count,
/// minimum size, VSync availability and dynamic MFG support.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_sl_get_frame_generation_state_details(
    values: *mut u32,
    count: u32,
) -> i32 {
    if values.is_null() || count != 6 {
        return -1;
    }
    read_fg_sample(|sample| unsafe {
        values.cast::<[u32; 6]>().write([
            sample.status,
            sample.actual_presents,
            sample.max_generated,
            sample.min_dimension,
            u32::from(sample.vsync),
            u32::from(sample.dynamic_mfg),
        ]);
    })
}

const _: () = {
    assert!(size_of::<rsf_slvk_sdk_header>() == 32);
    assert!(size_of::<rsf_slvk_sdk_reflex_state>() == 12848);
    assert!(size_of::<rsf_slvk_sdk_pcl_state>() == 40);
    assert!(size_of::<State>() == 88);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_refuse_without_an_initialized_session_or_storage() {
        assert_eq!(rsf_vk_sl_is_frame_generation_supported(null_mut()), -1);
        assert_eq!(unsafe { rsf_vk_sl_begin_frame_checked(1, null_mut()) }, -1);
        assert_eq!(rsf_vk_sl_token_matches(0, 1), 0);
        assert_eq!(rsf_vk_sl_marker_for_token(0, 4), -1);
        assert_eq!(rsf_vk_sl_pcl_window_message(), 0);
        let mut values = [7u32; 6];
        assert_eq!(
            unsafe { rsf_vk_sl_get_frame_generation_state_details(values.as_mut_ptr(), 5) },
            -1
        );
        assert_eq!(values, [7; 6]);
        assert_eq!(
            unsafe { rsf_vk_sl_get_reflex_state(null_mut(), null_mut()) },
            -1
        );
    }

    #[test]
    fn entries_publish_only_nonnull_addresses() {
        let entry = Entry::<sdk_api::Shutdown>::new();
        assert!(entry.get().is_none());
        unsafe extern "C-unwind" fn shutdown() -> i32 {
            7
        }
        assert!(entry.set(shutdown as *mut c_void));
        assert_eq!(entry.get().map(|call| unsafe { call() }), Some(7));
        entry.clear();
        assert!(entry.get().is_none());
    }

    #[test]
    fn guarded_calls_return_the_callee_result() {
        assert_eq!(guarded(|| 39), sdk_api::OUT_OF_VRAM);
    }
}
