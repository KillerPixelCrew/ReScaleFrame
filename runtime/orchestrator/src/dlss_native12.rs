// SPDX-License-Identifier: MIT
//! DLSS SR on a native D3D12 device. A registration owns the Streamline session when no other
//! owner exists, borrows the D3D12 Streamline host's session when one is given, or borrows the
//! interposer the engine loaded; contexts share it. The record layouts are checked against the
//! vendor headers in the `dlss_sr_layout` test (`tests/dlss_sr_layout_facts.cpp`).
use rsf_game_sys::backend_result::{
    ABI_MISMATCH, FEATURE_FAILED, INIT_FAILED, INVALID_ARGUMENT, LOAD_FAILED, MISSING_ENTRY_POINT,
    NOT_READY, NOT_SUPPORTED, OK, WRONG_API,
};
pub(super) mod exports;
use rsf_game_sys::xm;

use crate::com_ref;
use crate::sdk_module::{SdkModule, wide_path};
use crate::streamline_sdk::{
    self as sdk, DlssOptimal, DlssOptions, Entries, FEATURE_COMMON, FEATURE_DLSS,
    LOG_LEVEL_DEFAULT, PREFERENCE_FRAME_TAGGING, PREFERENCE_MANUAL_HOOKING, PREFERENCES,
    Preferences, RENDER_API_D3D12, RESULT_OK, SDK_VERSION, TagRecords, viewport,
};
use crate::streamline_sr::{Constants, Options, TagBatch, dimensions, dlss_mode};
use crate::sys::rsf_dlss_setup as Setup;
use crate::sys::{RSF_DLSS_ABI_VERSION, rsf_sr_frame as Frame};
use rsf_streamline_vk::sdk_api::{self, GetOptimalSettings, SetOptions};
use std::ffi::{CStr, c_void};
use std::ptr::{NonNull, null_mut};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use windows::core::{IUnknown, Interface};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GetModuleFileNameW, GetModuleHandleExW,
};

type Handle = *mut c_void;
const QUALITY: u32 = 1;

/// Whether every core entry the registration calls was resolved.
fn has_core(entries: &Entries) -> bool {
    entries.init.is_some()
        && entries.set_d3d_device.is_some()
        && entries.is_feature_supported.is_some()
        && entries.get_feature_function.is_some()
        && entries.shutdown.is_some()
        && entries.get_new_frame_token.is_some()
        && entries.set_constants.is_some()
        && entries.set_tag_for_frame.is_some()
        && entries.evaluate_feature.is_some()
        && entries.free_resources.is_some()
}

/// Two feature functions of `feature`. A refusal or a null address is a missing entry point.
unsafe fn feature_pair(
    entries: &Entries,
    feature: u32,
    names: [&CStr; 2],
) -> Result<[Handle; 2], i32> {
    let get = entries.get_feature_function.ok_or(INIT_FAILED)?;
    let mut addresses = [null_mut(); 2];
    for (name, address) in names.into_iter().zip(&mut addresses) {
        if unsafe { get(feature, name.as_ptr(), address) } != RESULT_OK {
            return Err(MISSING_ENTRY_POINT);
        }
    }
    if addresses.iter().any(|address| address.is_null()) {
        Err(MISSING_ENTRY_POINT)
    } else {
        Ok(addresses)
    }
}

enum Route {
    Owned,
    Shared(NonNull<c_void>),
    BorrowedEngine,
}

impl Route {
    /// The interposer and its entries: loaded here, borrowed from the host, or held from the
    /// engine's module.
    fn module(
        &self,
        device: Handle,
        setup: &Setup,
        interposer: &[u16],
        engine_module: Handle,
    ) -> Result<(Entries, Option<SdkModule>), i32> {
        let verified = |path: *const u16| {
            setup.require_signature == 0
                || unsafe { rsf_streamline_vk::admission::nvidia_signed(path) }
        };
        match *self {
            Self::Owned => {
                let log = crate::input_dump::Log::new(setup.log, setup.log_user);
                let lease = SdkModule::adopt_system(unsafe {
                    sdk::load_interposer(interposer.as_ptr(), setup.require_signature != 0, &log)
                })
                .ok_or(LOAD_FAILED)?;
                Ok((unsafe { Entries::resolve(lease.as_ptr()) }, Some(lease)))
            }
            Self::Shared(host) => {
                if !verified(interposer.as_ptr()) {
                    return Err(LOAD_FAILED);
                }
                let shared = unsafe { crate::streamline_host::shared(host.as_ptr().cast()) };
                let Some(shared) = shared.filter(|shared| shared.native_device == device) else {
                    return Err(WRONG_API);
                };
                if shared.module.is_null() {
                    return Err(LOAD_FAILED);
                }
                Ok((shared.entries, None))
            }
            Self::BorrowedEngine => {
                let mut held = null_mut();
                if unsafe {
                    GetModuleHandleExW(
                        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                        engine_module.cast(),
                        &mut held,
                    )
                } == 0
                {
                    return Err(LOAD_FAILED);
                }
                let lease = SdkModule::adopt_system(held).ok_or(LOAD_FAILED)?;
                if held != engine_module {
                    return Err(LOAD_FAILED);
                }
                let mut path = vec![0; 32768];
                let length =
                    unsafe { GetModuleFileNameW(held, path.as_mut_ptr(), path.len() as u32) };
                if length == 0 || length as usize >= path.len() || !verified(path.as_ptr()) {
                    return Err(LOAD_FAILED);
                }
                Ok((unsafe { Entries::resolve(held) }, Some(lease)))
            }
        }
    }
}

/// What slInit borrows from the registration through SDK shutdown.
struct InitStrings {
    plugin_paths: [*const u16; 1],
    feature: u32,
    _directory: Vec<u16>,
}

struct RegistrationResources {
    device: IUnknown,
    _module: Option<SdkModule>,
    _strings: Box<InitStrings>,
}

struct DeviceRegistration {
    entries: Entries,
    optimal: Option<GetOptimalSettings<DlssOptions, DlssOptimal>>,
    options: Option<SetOptions<DlssOptions>>,
    // slHookPresent and slHookAfterPresent, resolved only on the owned route.
    present: Option<[Handle; 2]>,
    route: Route,
    initialized: bool,
    resources: Option<RegistrationResources>,
}

// The registration is immutable once published, with a retained D3D12 device and module lease
// (or a host that the caller keeps live). Callers serialize SDK use and quiesce before teardown.
unsafe impl Send for DeviceRegistration {}
unsafe impl Sync for DeviceRegistration {}

impl DeviceRegistration {
    fn device(&self) -> Handle {
        self.resources
            .as_ref()
            .expect("live registration")
            .device
            .as_raw()
    }

    unsafe fn open(
        device: Handle,
        setup: &Setup,
        host: Handle,
        module: Handle,
    ) -> Result<Arc<Self>, i32> {
        let route = if let Some(host) = NonNull::new(host) {
            Route::Shared(host)
        } else if module.is_null() {
            Route::Owned
        } else {
            Route::BorrowedEngine
        };
        if matches!(route, Route::Owned) && sdk::interposer_loaded() {
            return Err(NOT_READY);
        }
        let interposer =
            unsafe { wide_path(setup.interposer_path_utf8) }.ok_or(INVALID_ARGUMENT)?;
        let directory =
            unsafe { wide_path(setup.plugin_directory_utf8) }.ok_or(INVALID_ARGUMENT)?;
        let (entries, lease) = route.module(device, setup, &interposer, module)?;
        if !has_core(&entries) {
            return Err(MISSING_ENTRY_POINT);
        }
        let mut strings = Box::new(InitStrings {
            plugin_paths: [directory.as_ptr()],
            feature: FEATURE_DLSS,
            _directory: directory,
        });
        let plugin_paths = strings.plugin_paths.as_mut_ptr();
        let feature = &raw const strings.feature;
        let owned = matches!(route, Route::Owned);
        let mut registration = Self {
            entries,
            optimal: None,
            options: None,
            present: None,
            route,
            initialized: false,
            resources: Some(RegistrationResources {
                device: unsafe { com_ref::retain(device) },
                _module: lease,
                _strings: strings,
            }),
        };
        if owned {
            let preferences = Preferences {
                base: PREFERENCES,
                log_level: LOG_LEVEL_DEFAULT,
                plugin_paths,
                num_plugin_paths: 1,
                flags: PREFERENCE_MANUAL_HOOKING | PREFERENCE_FRAME_TAGGING,
                features: feature,
                num_features: 1,
                application_id: setup.application_id,
                engine: crate::streamline_policy::engine_type(setup.engine),
                engine_version: setup.engine_version_utf8,
                project: setup.project_id_utf8,
                render_api: RENDER_API_D3D12,
                ..Default::default()
            };
            let init = entries.init.expect("core entry");
            if unsafe { init(&preferences, SDK_VERSION) } != RESULT_OK {
                return Err(INIT_FAILED);
            }
            registration.initialized = true;
            let set_device = entries.set_d3d_device.expect("core entry");
            if unsafe { set_device(device) } != RESULT_OK {
                return Err(INIT_FAILED);
            }
        }
        let mut luid = unsafe { crate::streamline_sr::d3d12_luid(device) };
        let adapter = sdk::adapter(&mut luid);
        let supported = entries.is_feature_supported.expect("core entry");
        let support = unsafe { supported(FEATURE_DLSS, &adapter) } as u32;
        if support == 0 {
            let [optimal, options] = unsafe {
                feature_pair(
                    &entries,
                    FEATURE_DLSS,
                    [c"slDLSSGetOptimalSettings", c"slDLSSSetOptions"],
                )
            }?;
            registration.optimal = unsafe { sdk_api::function(optimal) };
            registration.options = unsafe { sdk_api::function(options) };
            if owned {
                registration.present = Some(unsafe {
                    feature_pair(
                        &entries,
                        FEATURE_COMMON,
                        [c"slHookPresent", c"slHookAfterPresent"],
                    )
                }?);
            }
        }
        let log = crate::input_dump::Log::new(setup.log, setup.log_user);
        if support != 0 {
            log.say(&format!(
                "DLSS native D3D12 support query returned Streamline result {support}"
            ));
            return Err(NOT_SUPPORTED);
        }
        log.say("DLSS SR registered on the caller's native D3D12 device.");
        Ok(Arc::new(registration))
    }

    unsafe fn present(&self, swapchain: Handle, interval: u32, flags: u32, begin: u32) -> i32 {
        match self.route {
            Route::Owned => {
                let Some(entries) = self.present else {
                    return FEATURE_FAILED;
                };
                if unsafe { sdk::hook_present(entries, swapchain, interval, flags, begin != 0) } {
                    OK
                } else {
                    FEATURE_FAILED
                }
            }
            Route::Shared(_) | Route::BorrowedEngine => OK,
        }
    }

    unsafe fn token(&self, frame_id: u64, engine_token: Handle) -> Result<NonNull<c_void>, i32> {
        let token = match self.route {
            Route::BorrowedEngine => engine_token,
            Route::Shared(host) => unsafe {
                crate::streamline_host::rsf_streamline_host_token(host.as_ptr().cast(), frame_id)
            },
            Route::Owned => {
                let token = self.entries.get_new_frame_token.expect("core entry");
                let mut frame = std::ptr::null();
                let index = frame_id as u32;
                if unsafe { token(&mut frame, &index) } != RESULT_OK {
                    return Err(FEATURE_FAILED);
                }
                frame.cast_mut()
            }
        };
        NonNull::new(token).ok_or(NOT_READY)
    }
}

impl Drop for DeviceRegistration {
    fn drop(&mut self) {
        if self.initialized && unsafe { self.entries.shutdown.expect("core entry")() } != RESULT_OK
        {
            // Shutdown can fail after partial teardown. With a void public
            // destroy entry, retain the SDK and its dependencies for the process.
            std::mem::forget(self.resources.take());
        }
    }
}

pub(super) struct Registration(Arc<DeviceRegistration>);

// Viewports 0 and 1 can be used by generation or D3D11 SR on a shared host.
static NEXT_VIEWPORT: AtomicU32 = AtomicU32::new(2);

fn reserve_viewport(next: &AtomicU32) -> Option<u32> {
    next.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .ok()
}

#[test]
fn independent_viewports_do_not_alias_or_wrap_to_reserved_ids() {
    let next = AtomicU32::new(2);
    assert_eq!(reserve_viewport(&next), Some(2));
    assert_eq!(reserve_viewport(&next), Some(3));
    next.store(u32::MAX - 1, Ordering::Relaxed);
    assert_eq!(reserve_viewport(&next), Some(u32::MAX - 1));
    assert_eq!(reserve_viewport(&next), None);
}

pub(crate) struct Context {
    registration: Arc<DeviceRegistration>,
    quality: AtomicU32,
    preset: u32,
    sdr: AtomicBool,
    viewport: u32,
    used_viewport: AtomicBool,
}

impl Context {
    fn new(registration: Arc<DeviceRegistration>, preset: u32, viewport: u32) -> Self {
        Self {
            registration,
            quality: AtomicU32::new(QUALITY),
            preset,
            sdr: AtomicBool::new(false),
            viewport,
            used_viewport: AtomicBool::new(false),
        }
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        if *self.used_viewport.get_mut() {
            let free = self
                .registration
                .entries
                .free_resources
                .expect("core entry");
            // Teardown has no way to report a failed release.
            let _ = unsafe { free(FEATURE_DLSS, &viewport(self.viewport)) };
        }
    }
}

fn publish_context(
    registration: Arc<DeviceRegistration>,
    preset: u32,
    out: &mut *mut Context,
) -> i32 {
    let Some(viewport) = reserve_viewport(&NEXT_VIEWPORT) else {
        return INIT_FAILED;
    };
    *out = crate::allocate_handle(Context::new(registration, preset, viewport));
    if (*out).is_null() { INIT_FAILED } else { OK }
}

fn rsf_core_dlss_native12_create_registered(
    device: Handle,
    setup: &Setup,
    registration: &Registration,
    out: &mut *mut Context,
) -> i32 {
    if registration.0.device() != device {
        return INVALID_ARGUMENT;
    }
    if setup.abi_version != RSF_DLSS_ABI_VERSION {
        return ABI_MISMATCH;
    }
    publish_context(Arc::clone(&registration.0), setup.preset, out)
}

unsafe fn rsf_core_dlss_native12_plan(
    context: &Context,
    width: u32,
    height: u32,
    quality: u32,
    render_width: *mut u32,
    render_height: *mut u32,
) -> i32 {
    let Some(optimal) = context.registration.optimal else {
        return FEATURE_FAILED;
    };
    let options = sdk::plan_options(&dimensions(quality, width, height));
    let mut settings = sdk::dlss_optimal();
    if unsafe { optimal(&options, &mut settings) } != RESULT_OK {
        return FEATURE_FAILED;
    }
    let (planned_width, planned_height) = (settings.width, settings.height);
    if planned_width == 0 || planned_height == 0 {
        return FEATURE_FAILED;
    }
    unsafe {
        render_width.write(planned_width);
        render_height.write(planned_height);
    }
    context.quality.store(quality, Ordering::Relaxed);
    OK
}

unsafe fn rsf_core_dlss_native12_evaluate(
    context: &Context,
    list: Handle,
    frame: *const Frame,
    engine_token: Handle,
) -> i32 {
    let registration = &context.registration;
    if !engine_token.is_null() && !matches!(registration.route, Route::BorrowedEngine) {
        return INVALID_ARGUMENT;
    }
    let validated = unsafe { crate::streamline_sr::validate_d3d12(registration.device(), frame) };
    if validated != OK {
        return validated;
    }
    let frame = unsafe { &*frame };
    let record = unsafe { &*frame.record };
    let frame_id = record.frame_id;
    let token = match unsafe { registration.token(frame_id, engine_token) } {
        Ok(token) => token.as_ptr().cast_const(),
        Err(result) => return result,
    };
    let entries = &registration.entries;
    let viewport = viewport(context.viewport);
    let camera = &record.camera;
    // The determinant and inverse follow DirectXMath so the SR singularity gate keeps its numerics.
    let (inverted, determinant) = xm::inverse(&xm::load(&camera.clip_to_previous_clip));
    if !determinant.is_finite() || determinant.abs() < 1e-12 {
        return INVALID_ARGUMENT;
    }
    let constants = Constants {
        view_to_clip: camera.view_to_clip,
        clip_to_view: camera.clip_to_view,
        clip_to_previous: camera.clip_to_previous_clip,
        previous_to_clip: xm::flatten(&inverted),
        jitter: [frame.jitter_x, frame.jitter_y],
        motion_scale: [
            1.0 / record.render_width as f32,
            1.0 / record.render_height as f32,
        ],
        pinhole: [0.0; 2],
        position: std::array::from_fn(|i| camera.view_to_world[12 + i]),
        right: std::array::from_fn(|i| camera.view_to_world[i]),
        up: std::array::from_fn(|i| camera.view_to_world[4 + i]),
        forward: crate::streamline_policy::camera_forward(
            &camera.view_to_world,
            &camera.view_to_clip,
            -1.0,
        ),
        near_plane: camera.near_plane,
        far_plane: camera.far_plane,
        fov: camera.vertical_fov_radians,
        aspect: record.output_width as f32 / record.output_height as f32,
        invalid_motion: f32::MAX,
        depth_inverted: u8::from(camera.depth_inverted != 0),
        camera_motion: 1,
        motion_3d: 0,
        reset: u8::from(frame.reset != 0),
    };
    // The record's motionVectorsJittered stays eFalse: the evaluation path supplies unjittered vectors.
    let constants = sdk::constants(&constants);
    let set_constants = entries.set_constants.expect("core entry");
    if unsafe { set_constants(&constants, token, &viewport) } != RESULT_OK {
        return FEATURE_FAILED;
    }
    // From the first feature options call onward, this context can have SDK viewport state.
    context.used_viewport.store(true, Ordering::Relaxed);
    let sdr = context.sdr.load(Ordering::Relaxed);
    let options = Options {
        mode: dlss_mode(context.quality.load(Ordering::Relaxed)),
        width: record.output_width,
        height: record.output_height,
        hdr: (!sdr).into(),
        auto_exposure: (!sdr && frame.exposure.resource.is_null()).into(),
        alpha: 0,
        preset: context.preset,
    };
    let Some(set_options) = registration.options else {
        return FEATURE_FAILED;
    };
    if unsafe { set_options(&viewport, &sdk::options(&options.payload())) } != RESULT_OK {
        return FEATURE_FAILED;
    }
    let mut tags = TagRecords::new();
    tags.fill(&TagBatch::d3d12(frame, record));
    let (tags, count) = tags.tags();
    let set_tags = entries.set_tag_for_frame.expect("core entry");
    if unsafe { set_tags(token, &viewport, tags, count, list) } != RESULT_OK {
        return FEATURE_FAILED;
    }
    let inputs = [&raw const viewport.base];
    let evaluate = entries.evaluate_feature.expect("core entry");
    if unsafe { evaluate(FEATURE_DLSS, token, inputs.as_ptr(), 1, list) } == RESULT_OK {
        OK
    } else {
        FEATURE_FAILED
    }
}
