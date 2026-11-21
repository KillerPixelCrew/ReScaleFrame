// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::gl_pipeline_abi::rsf_dlss_setup as Setup;
use crate::render_abi::{RSF_DLSS_ABI_VERSION, rsf_sr_frame as Frame};
use crate::vulkan_sdk::SdkModule;
use std::ffi::{CStr, c_char, c_void};
use std::ptr::{NonNull, null_mut};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GetModuleFileNameW, GetModuleHandleExW,
    GetModuleHandleW,
};
use windows_sys::core::w;

type Handle = *mut c_void;
const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const LOAD_FAILED: i32 = -4;
const INIT_FAILED: i32 = -6;
const NOT_READY: i32 = -8;
const FEATURE_FAILED: i32 = -9;
const QUALITY: u32 = 1;

// C++ owns the Streamline types and SDK calls. The table is static in each consumer.
#[repr(C)]
struct Platform {
    load: unsafe extern "C" fn(*const u16, &Setup) -> Handle,
    verify: unsafe extern "C" fn(*const u16) -> bool,
    shared_module: unsafe extern "C" fn(Handle, Handle, &mut Handle) -> i32,
    resolve: unsafe extern "C" fn(Handle, *const u16, &mut Handle) -> i32,
    init: unsafe extern "C" fn(Handle, &Setup) -> i32,
    set_device: unsafe extern "C" fn(Handle, Handle) -> i32,
    configure: unsafe extern "C" fn(Handle, Handle, &Setup, u32) -> i32,
    shutdown: unsafe extern "C" fn(Handle),
    close: unsafe extern "C" fn(Handle),
    present: unsafe extern "C" fn(Handle, Handle, u32, u32, u32) -> i32,
    plan: unsafe extern "C" fn(Handle, u32, u32, u32, &mut u32, &mut u32) -> i32,
    validate: unsafe extern "C" fn(Handle, *const Frame) -> i32,
    token: unsafe extern "C" fn(Handle, u32, &mut Handle) -> i32,
    shared_token: unsafe extern "C" fn(Handle, u64) -> Handle,
    constants: unsafe extern "C" fn(Handle, Handle, u32, &Frame) -> i32,
    options: unsafe extern "C" fn(Handle, u32, &Frame, u32, u32, u32) -> i32,
    dispatch: unsafe extern "C" fn(Handle, Handle, u32, Handle, &Frame) -> i32,
    free_viewport: unsafe extern "C" fn(Handle, u32),
}

const _: () = assert!(size_of::<Platform>() == 18 * size_of::<Handle>());

enum Route {
    Owned,
    Shared(NonNull<c_void>),
    BorrowedEngine,
}

impl Route {
    fn viewport(&self) -> u32 {
        // The shared presentation host reserves zero for completed-frame generation.
        u32::from(!matches!(self, Self::Owned))
    }

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

unsafe fn wide_path(path: *const c_char) -> Option<Vec<u16>> {
    if path.is_null() {
        return None;
    }
    let path = unsafe { CStr::from_ptr(path) }.to_str().ok()?;
    (!path.is_empty()).then(|| path.encode_utf16().chain([0]).collect())
}

struct DeviceRegistration {
    platform: &'static Platform,
    native: NonNull<c_void>,
    device: ComRef,
    route: Route,
    initialized: bool,
    _module: Option<SdkModule>,
    // The typed Streamline preferences borrow the directory through SDK shutdown.
    _directory: Vec<u16>,
}

// The registration is immutable once published, with a retained D3D12 device and module lease
// (or a host that the caller keeps live). Callers serialize SDK use and quiesce before teardown.
unsafe impl Send for DeviceRegistration {}
unsafe impl Sync for DeviceRegistration {}

impl DeviceRegistration {
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
        let mut native = null_mut();
        let result = unsafe { (platform.resolve)(resolved, directory.as_ptr(), &mut native) };
        if result != OK {
            if !native.is_null() {
                unsafe { (platform.close)(native) };
            }
            return Err(result);
        }
        let native = NonNull::new(native).ok_or(INIT_FAILED)?;
        let mut registration = Self {
            platform,
            native,
            device: unsafe { ComRef::retain(device) },
            route,
            initialized: false,
            _module: lease,
            _directory: directory,
        };
        let owned = matches!(registration.route, Route::Owned);
        if owned {
            let result = unsafe { (platform.init)(native.as_ptr(), setup) };
            if result != OK {
                return Err(result);
            }
            registration.initialized = true;
            let result = unsafe { (platform.set_device)(native.as_ptr(), device) };
            if result != OK {
                return Err(result);
            }
        }
        let result = unsafe { (platform.configure)(native.as_ptr(), device, setup, owned.into()) };
        if result != OK {
            return Err(result);
        }
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
            if self.initialized {
                (self.platform.shutdown)(native);
            }
            (self.platform.close)(native);
        }
    }
}

struct Registration(Arc<DeviceRegistration>);

struct Context {
    registration: Arc<DeviceRegistration>,
    quality: AtomicU32,
    preset: u32,
    sdr: AtomicBool,
    used_viewport: AtomicBool,
}

impl Context {
    fn new(registration: Arc<DeviceRegistration>, preset: u32) -> Self {
        Self {
            registration,
            quality: AtomicU32::new(QUALITY),
            preset,
            sdr: AtomicBool::new(false),
            used_viewport: AtomicBool::new(false),
        }
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        if *self.used_viewport.get_mut() {
            let registration = &self.registration;
            unsafe {
                (registration.platform.free_viewport)(
                    registration.native.as_ptr(),
                    registration.route.viewport(),
                );
            }
        }
    }
}

fn publish_context(
    registration: Arc<DeviceRegistration>,
    preset: u32,
    out: &mut *mut Context,
) -> i32 {
    *out = crate::allocate_handle(Context::new(registration, preset));
    if (*out).is_null() { INIT_FAILED } else { OK }
}

// Creation wrappers clear the output and validate the public setup before passing it here.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_native12_create(
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_native12_register(
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

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_native12_create_registered(
    device: Handle,
    setup: &Setup,
    registration: &Registration,
    out: &mut *mut Context,
) -> i32 {
    if registration.0.device.as_ptr() != device {
        return INVALID;
    }
    if setup.abi_version != RSF_DLSS_ABI_VERSION {
        return ABI_MISMATCH;
    }
    publish_context(Arc::clone(&registration.0), setup.preset, out)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_native12_registration_present(
    registration: &Registration,
    swapchain: Handle,
    interval: u32,
    flags: u32,
    begin: u32,
) -> i32 {
    unsafe { registration.0.present(swapchain, interval, flags, begin) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_native12_registration_destroy(registration: *mut Registration) {
    if !registration.is_null() {
        drop(unsafe { Box::from_raw(registration) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_native12_present(
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_native12_plan(
    context: &Context,
    width: u32,
    height: u32,
    quality: u32,
    render_width: *mut u32,
    render_height: *mut u32,
) -> i32 {
    let registration = &context.registration;
    let (mut planned_width, mut planned_height) = (0, 0);
    let result = unsafe {
        (registration.platform.plan)(
            registration.native.as_ptr(),
            width,
            height,
            quality,
            &mut planned_width,
            &mut planned_height,
        )
    };
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_native12_evaluate(
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
    let validated = unsafe { (platform.validate)(registration.device.as_ptr(), frame) };
    if validated != OK {
        return validated;
    }
    let frame = unsafe { &*frame };
    let frame_id = unsafe { (*frame.record).frame_id };
    let token = match unsafe { registration.token(frame_id, engine_token) } {
        Ok(token) => token.as_ptr(),
        Err(result) => return result,
    };
    let native = registration.native.as_ptr();
    let viewport = registration.route.viewport();
    let constants = unsafe { (platform.constants)(native, token, viewport, frame) };
    if constants != OK {
        return constants;
    }
    // From the first feature options call onward, this context can have SDK viewport state.
    context.used_viewport.store(true, Ordering::Relaxed);
    let options = unsafe {
        (platform.options)(
            native,
            viewport,
            frame,
            context.quality.load(Ordering::Relaxed),
            context.preset,
            context.sdr.load(Ordering::Relaxed).into(),
        )
    };
    if options != OK {
        return options;
    }
    unsafe { (platform.dispatch)(native, token, viewport, list, frame) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_native12_destroy(context: *mut Context) {
    if !context.is_null() {
        drop(unsafe { Box::from_raw(context) });
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_dlss_native12_set_sdr(context: &Context, sdr: u32) {
    context.sdr.store(sdr != 0, Ordering::Relaxed);
}
