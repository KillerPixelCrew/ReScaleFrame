// SPDX-License-Identifier: MIT
pub(super) mod exports;
#[cfg(not(test))]
mod sdk;
// Also the Vulkan DLSS SR and DLSS-G singularity gate (vulkan_streamline_camera).
pub(crate) mod xm;

use crate::com_ref::ComRef;
use crate::sdk_module::{SdkModule, wide_path};
use crate::streamline_sr::sdk::{
    FEATURE_DLSS, PREFERENCE_FRAME_TAGGING, PREFERENCE_MANUAL_HOOKING, RENDER_API_D3D12,
};
use crate::streamline_sr::{
    Constants, Dimensions, Optimal, OptionPayload, Options, TagBatch, dimensions, dlss_mode,
};
use crate::sys::rsf_dlss_setup as Setup;
use crate::sys::{RSF_DLSS_ABI_VERSION, rsf_sr_frame as Frame};
use std::ffi::{CStr, c_char, c_void};
use std::ptr::{NonNull, null_mut};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GetModuleFileNameW, GetModuleHandleExW,
    GetModuleHandleW, GetProcAddress,
};
use windows_sys::core::w;

type Handle = *mut c_void;
const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const LOAD_FAILED: i32 = -4;
const MISSING_ENTRY: i32 = -5;
const INIT_FAILED: i32 = -6;
const NOT_SUPPORTED: i32 = -7;
const NOT_READY: i32 = -8;
const FEATURE_FAILED: i32 = -9;
const QUALITY: u32 = 1;
const DLSS_FEATURE: u32 = 0;
const COMMON_FEATURE: u32 = 1;

const CORE_NAMES: [&CStr; 10] = [
    c"slInit",
    c"slSetD3DDevice",
    c"slIsFeatureSupported",
    c"slGetFeatureFunction",
    c"slShutdown",
    c"slGetNewFrameToken",
    c"slSetConstants",
    c"slSetTagForFrame",
    c"slEvaluateFeature",
    c"slFreeResources",
];
type CoreEntries = [Handle; CORE_NAMES.len()];
type FeatureEntries = [Handle; 4];
type FeatureLookup = unsafe extern "C" fn(Handle, u32, *const c_char, &mut Handle) -> i32;

/// Resolve against the interposer held by the registration. The native leaf binds typed entries
/// only after every required address is present.
unsafe fn core_entries(module: Handle, entries: &mut CoreEntries) -> i32 {
    for (entry, name) in entries.iter_mut().zip(CORE_NAMES) {
        *entry = unsafe { GetProcAddress(module, name.as_ptr().cast()) }
            .map_or(null_mut(), |function| function as *const () as Handle);
    }
    if entries.iter().any(|entry| entry.is_null()) {
        MISSING_ENTRY
    } else {
        OK
    }
}

unsafe fn feature_entries(
    native: Handle,
    owns_present: u32,
    lookup: FeatureLookup,
    entries: &mut FeatureEntries,
) -> i32 {
    let resolve_pair = |group, names: [&CStr; 2], slots: &mut [Handle]| {
        for (name, address) in names.into_iter().zip(slots.iter_mut()) {
            let result = unsafe { lookup(native, group, name.as_ptr(), address) };
            if result != OK {
                return result;
            }
        }
        if slots.iter().any(|entry| entry.is_null()) {
            MISSING_ENTRY
        } else {
            OK
        }
    };
    let (dlss, present) = entries.split_at_mut(2);
    let result = resolve_pair(
        DLSS_FEATURE,
        [c"slDLSSGetOptimalSettings", c"slDLSSSetOptions"],
        dlss,
    );
    if result != OK || owns_present == 0 {
        return result;
    }
    resolve_pair(
        COMMON_FEATURE,
        [c"slHookPresent", c"slHookAfterPresent"],
        present,
    )
}

#[repr(C)]
struct Preferences {
    flags: u64,
    engine_version: *const c_char,
    project_id: *const c_char,
    engine: u32,
    application_id: u32,
    render_api: u32,
    feature: u32,
}

const _: () = {
    assert!(size_of::<Preferences>() == 40);
};

// The typed Streamline calls live in sdk.rs, which unit tests do not compile.
#[repr(C)]
struct Platform {
    load: unsafe extern "C" fn(*const u16, &Setup) -> Handle,
    verify: unsafe extern "C" fn(*const u16) -> bool,
    shared_module: unsafe extern "C" fn(Handle, Handle, &mut Handle) -> i32,
    allocate: unsafe extern "C" fn(*const u16) -> Handle,
    bind_core: unsafe extern "C" fn(Handle, &CoreEntries),
    init: unsafe extern "C" fn(Handle, &Preferences) -> i32,
    set_device: unsafe extern "C" fn(Handle, Handle) -> i32,
    support: unsafe extern "C" fn(Handle, u64, &mut u32) -> i32,
    lookup_feature: FeatureLookup,
    bind_features: unsafe extern "C" fn(Handle, &FeatureEntries, u32),
    shutdown: unsafe extern "C" fn(Handle) -> i32,
    close: unsafe extern "C" fn(Handle),
    present: unsafe extern "C" fn(Handle, Handle, u32, u32, u32) -> i32,
    plan: unsafe extern "C" fn(Handle, &Dimensions, &mut Optimal) -> i32,
    validate: unsafe extern "C" fn(Handle, *const Frame) -> i32,
    token: unsafe extern "C" fn(Handle, u32, &mut Handle) -> i32,
    shared_token: unsafe extern "C" fn(Handle, u64) -> Handle,
    inverse: unsafe extern "C" fn(&[f32; 16], &mut [f32; 16]) -> f32,
    constants: unsafe extern "C" fn(Handle, Handle, u32, &Constants) -> i32,
    options: unsafe extern "C" fn(Handle, u32, &OptionPayload) -> i32,
    dispatch: unsafe extern "C" fn(Handle, Handle, u32, Handle, &TagBatch) -> i32,
    free_viewport: unsafe extern "C" fn(Handle, u32),
}

const _: () = assert!(size_of::<Platform>() == 22 * size_of::<Handle>());

enum Route {
    Owned,
    Shared(NonNull<c_void>),
    BorrowedEngine,
}

impl Route {
    fn module(
        &self,
        platform: &Platform,
        device: Handle,
        setup: &Setup,
        interposer: &[u16],
        engine_module: Handle,
    ) -> Result<(Handle, Option<SdkModule>), i32> {
        match *self {
            Self::Owned => {
                let handle = unsafe { (platform.load)(interposer.as_ptr(), setup) };
                let lease = SdkModule::adopt_system(handle).ok_or(LOAD_FAILED)?;
                Ok((handle, Some(lease)))
            }
            Self::Shared(host) => {
                if setup.require_signature != 0
                    && !unsafe { (platform.verify)(interposer.as_ptr()) }
                {
                    return Err(LOAD_FAILED);
                }
                let mut module = null_mut();
                let result =
                    unsafe { (platform.shared_module)(device, host.as_ptr(), &mut module) };
                if result != OK {
                    return Err(result);
                }
                if module.is_null() {
                    return Err(LOAD_FAILED);
                }
                Ok((module, None))
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
                if length == 0
                    || length as usize >= path.len()
                    || (setup.require_signature != 0
                        && !unsafe { (platform.verify)(path.as_ptr()) })
                {
                    return Err(LOAD_FAILED);
                }
                Ok((held, Some(lease)))
            }
        }
    }
}

struct RegistrationResources {
    device: ComRef,
    _module: Option<SdkModule>,
    // The typed Streamline preferences borrow the directory through SDK shutdown.
    _directory: Vec<u16>,
}

struct DeviceRegistration {
    platform: &'static Platform,
    native: NonNull<c_void>,
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
            .as_ptr()
    }

    unsafe fn open(
        platform: &'static Platform,
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
        if matches!(route, Route::Owned)
            && !unsafe { GetModuleHandleW(w!("sl.interposer.dll")) }.is_null()
        {
            return Err(NOT_READY);
        }
        let interposer = unsafe { wide_path(setup.interposer_path_utf8) }.ok_or(INVALID)?;
        let directory = unsafe { wide_path(setup.plugin_directory_utf8) }.ok_or(INVALID)?;
        let (resolved, lease) = route.module(platform, device, setup, &interposer, module)?;
        let native =
            NonNull::new(unsafe { (platform.allocate)(directory.as_ptr()) }).ok_or(INIT_FAILED)?;
        let mut entries = [null_mut(); CORE_NAMES.len()];
        let result = unsafe { core_entries(resolved, &mut entries) };
        if result != OK {
            unsafe { (platform.close)(native.as_ptr()) };
            return Err(result);
        }
        unsafe { (platform.bind_core)(native.as_ptr(), &entries) };
        let mut registration = Self {
            platform,
            native,
            route,
            initialized: false,
            resources: Some(RegistrationResources {
                device: unsafe { ComRef::retain(device) },
                _module: lease,
                _directory: directory,
            }),
        };
        let owned = matches!(registration.route, Route::Owned);
        if owned {
            let preferences = Preferences {
                flags: PREFERENCE_MANUAL_HOOKING | PREFERENCE_FRAME_TAGGING,
                engine_version: setup.engine_version_utf8,
                project_id: setup.project_id_utf8,
                engine: crate::streamline_policy::engine_type(setup.engine),
                application_id: setup.application_id,
                render_api: RENDER_API_D3D12,
                feature: FEATURE_DLSS,
            };
            let result = unsafe { (platform.init)(native.as_ptr(), &preferences) };
            if result != OK {
                return Err(result);
            }
            registration.initialized = true;
            let result = unsafe { (platform.set_device)(native.as_ptr(), device) };
            if result != OK {
                return Err(result);
            }
        }
        let mut support = 0;
        let device_luid = unsafe { crate::streamline_sr::d3d12_luid(device) };
        let result = unsafe { (platform.support)(native.as_ptr(), device_luid, &mut support) };
        if result != OK {
            return Err(result);
        }
        if support == 0 {
            let mut features = [null_mut(); 4];
            let result = unsafe {
                feature_entries(
                    native.as_ptr(),
                    owned.into(),
                    platform.lookup_feature,
                    &mut features,
                )
            };
            if result != OK {
                return Err(result);
            }
            unsafe { (platform.bind_features)(native.as_ptr(), &features, owned.into()) };
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
            Route::Owned => unsafe {
                (self.platform.present)(self.native.as_ptr(), swapchain, interval, flags, begin)
            },
            Route::Shared(_) | Route::BorrowedEngine => OK,
        }
    }

    unsafe fn token(&self, frame_id: u64, engine_token: Handle) -> Result<NonNull<c_void>, i32> {
        let token = match self.route {
            Route::BorrowedEngine => engine_token,
            Route::Shared(host) => unsafe { (self.platform.shared_token)(host.as_ptr(), frame_id) },
            Route::Owned => {
                let mut token = null_mut();
                let result = unsafe {
                    (self.platform.token)(self.native.as_ptr(), frame_id as u32, &mut token)
                };
                if result != OK {
                    return Err(result);
                }
                token
            }
        };
        NonNull::new(token).ok_or(NOT_READY)
    }
}

impl Drop for DeviceRegistration {
    fn drop(&mut self) {
        let native = self.native.as_ptr();
        unsafe {
            if self.initialized && (self.platform.shutdown)(native) != OK {
                // Shutdown can fail after partial teardown. With a void public
                // destroy entry, retain the SDK and its dependencies for the process.
                std::mem::forget(self.resources.take());
                return;
            }
            (self.platform.close)(native);
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

struct Context {
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
            let registration = &self.registration;
            unsafe {
                (registration.platform.free_viewport)(registration.native.as_ptr(), self.viewport);
            }
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

// Creation wrappers clear the output and validate the public setup before passing it here.
unsafe fn rsf_core_dlss_native12_create(
    device: Handle,
    setup: &Setup,
    host: Handle,
    out: &mut *mut Context,
    platform: &'static Platform,
) -> i32 {
    match unsafe { DeviceRegistration::open(platform, device, setup, host, null_mut()) } {
        Ok(registration) => publish_context(registration, setup.preset, out),
        Err(result) => result,
    }
}

unsafe fn rsf_core_dlss_native12_register(
    device: Handle,
    setup: &Setup,
    module: Handle,
    out: &mut *mut Registration,
    platform: &'static Platform,
) -> i32 {
    match unsafe { DeviceRegistration::open(platform, device, setup, null_mut(), module) } {
        Ok(registration) => {
            *out = crate::allocate_handle(Registration(registration));
            if (*out).is_null() { INIT_FAILED } else { OK }
        }
        Err(result) => result,
    }
}

fn rsf_core_dlss_native12_create_registered(
    device: Handle,
    setup: &Setup,
    registration: &Registration,
    out: &mut *mut Context,
) -> i32 {
    if registration.0.device() != device {
        return INVALID;
    }
    if setup.abi_version != RSF_DLSS_ABI_VERSION {
        return ABI_MISMATCH;
    }
    publish_context(Arc::clone(&registration.0), setup.preset, out)
}

unsafe fn rsf_core_dlss_native12_registration_present(
    registration: &Registration,
    swapchain: Handle,
    interval: u32,
    flags: u32,
    begin: u32,
) -> i32 {
    unsafe { registration.0.present(swapchain, interval, flags, begin) }
}

unsafe fn rsf_core_dlss_native12_registration_destroy(registration: *mut Registration) {
    if !registration.is_null() {
        drop(unsafe { Box::from_raw(registration) });
    }
}

unsafe fn rsf_core_dlss_native12_present(
    context: &Context,
    swapchain: Handle,
    interval: u32,
    flags: u32,
    begin: u32,
) -> i32 {
    unsafe {
        context
            .registration
            .present(swapchain, interval, flags, begin)
    }
}

unsafe fn rsf_core_dlss_native12_plan(
    context: &Context,
    width: u32,
    height: u32,
    quality: u32,
    render_width: *mut u32,
    render_height: *mut u32,
) -> i32 {
    let registration = &context.registration;
    let mut optimal = Optimal::default();
    let result = unsafe {
        (registration.platform.plan)(
            registration.native.as_ptr(),
            &dimensions(quality, width, height),
            &mut optimal,
        )
    };
    let (planned_width, planned_height) = (optimal.width, optimal.height);
    if result != OK {
        return result;
    }
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
        return INVALID;
    }
    let platform = registration.platform;
    let validated = unsafe { (platform.validate)(registration.device(), frame) };
    if validated != OK {
        return validated;
    }
    let frame = unsafe { &*frame };
    let record = unsafe { &*frame.record };
    let frame_id = record.frame_id;
    let token = match unsafe { registration.token(frame_id, engine_token) } {
        Ok(token) => token.as_ptr(),
        Err(result) => return result,
    };
    let native = registration.native.as_ptr();
    let viewport = context.viewport;
    let camera = &record.camera;
    let mut previous_to_current = [0.0; 16];
    let determinant =
        unsafe { (platform.inverse)(&camera.clip_to_previous_clip, &mut previous_to_current) };
    if !determinant.is_finite() || determinant.abs() < 1e-12 {
        return INVALID;
    }
    let constants = Constants {
        view_to_clip: camera.view_to_clip,
        clip_to_view: camera.clip_to_view,
        clip_to_previous: camera.clip_to_previous_clip,
        previous_to_clip: previous_to_current,
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
    let constants = unsafe { (platform.constants)(native, token, viewport, &constants) };
    if constants != OK {
        return constants;
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
    let result = unsafe { (platform.options)(native, viewport, &options.payload()) };
    if result != OK {
        return result;
    }
    let tags = TagBatch::d3d12(frame, record);
    unsafe { (platform.dispatch)(native, token, viewport, list, &tags) }
}

unsafe fn rsf_core_dlss_native12_destroy(context: *mut Context) {
    if !context.is_null() {
        drop(unsafe { Box::from_raw(context) });
    }
}

fn rsf_core_dlss_native12_set_sdr(context: &Context, sdr: u32) {
    context.sdr.store(sdr != 0, Ordering::Relaxed);
}
