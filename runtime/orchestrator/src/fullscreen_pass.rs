// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::render_abi::rsf_backend_log_fn as LogFn;
use crate::sr_session::has_capacity;
use std::ffi::{CStr, c_char, c_void};
use std::ptr::{self, null_mut};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, TryLockError};

type Handle = *mut c_void;
type Resources = [Handle; 9];
const ABI: u32 = 1;
const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const RESOURCE_FAILED: i32 = -4;
const NO_BACK_BUFFER: i32 = -5;
const COPY: u32 = 0;
const TONEMAP: u32 = 1;
const PREMULTIPLIED: u32 = 2;
const PREMULTIPLIED_SRGB: u32 = 4;
const PREMULTIPLIED_GAMMA22: u32 = 5;
const COPY_RGBA: u32 = 6;
const POINT: u32 = 0;
const LINEAR: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
struct Setup {
    struct_size: u32,
    abi_version: u32,
    log: LogFn,
    log_user: Handle,
}

#[repr(C)]
struct Draw {
    struct_size: u32,
    mode: u32,
    exposure: f32,
    width: u32,
    height: u32,
}

#[repr(C)]
struct Constants {
    mode: u32,
    exposure: f32,
    padding: [u32; 2],
}

#[repr(C)]
struct Platform {
    make: unsafe extern "C" fn(Handle, &Setup, u32, &mut Resources) -> i32,
    admit: unsafe extern "C" fn(Handle, Handle, Handle, Handle) -> i32,
    extent: unsafe extern "C" fn(Handle, &mut [u32; 2]) -> i32,
    draw: unsafe extern "C" fn(
        Handle,
        Handle,
        Handle,
        &Resources,
        u32,
        u32,
        u32,
        u32,
        *const c_void,
        u32,
    ) -> i32,
    log: unsafe extern "C" fn(LogFn, Handle, *const c_char),
}

#[repr(C)]
struct Present {
    back_buffer: unsafe extern "C" fn(Handle, &mut [u32; 2]) -> Handle,
    target: unsafe extern "C" fn(Handle, Handle) -> Handle,
    source: unsafe extern "C" fn(Handle, Handle) -> Handle,
}

const _: () = {
    assert!(size_of::<Setup>() == 24 && size_of::<Draw>() == 20);
    assert!(size_of::<Constants>() == 16);
    assert!(size_of::<Platform>() == 5 * size_of::<Handle>());
    assert!(size_of::<Present>() == 3 * size_of::<Handle>());
};

struct Pass {
    resources: [ComRef; 9],
    device: ComRef,
    filter: AtomicU32,
    drawing: Mutex<()>,
    platform: &'static Platform,
}

impl Pass {
    fn draw(&self, context: Handle, target: Handle, source: Handle, parameters: &Draw) -> i32 {
        if parameters.mode > COPY_RGBA
            || unsafe { (self.platform.admit)(self.device.as_ptr(), context, target, source) } == 0
        {
            return INVALID;
        }
        let mut size = [parameters.width, parameters.height];
        if (size[0] == 0 || size[1] == 0)
            && unsafe { (self.platform.extent)(target, &mut size) } == 0
        {
            return INVALID;
        }
        let _drawing = match self.drawing.try_lock() {
            Ok(drawing) => drawing,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => return RESOURCE_FAILED,
        };
        let mode = parameters.mode;
        let constants = Constants {
            mode,
            exposure: if parameters.exposure > 0.0 {
                parameters.exposure
            } else {
                1.0
            },
            padding: [0; 2],
        };
        let linear = self.filter.load(Ordering::Relaxed) == LINEAR;
        let blend = matches!(
            mode,
            PREMULTIPLIED | PREMULTIPLIED_SRGB | PREMULTIPLIED_GAMMA22
        );
        let resources = std::array::from_fn(|i| self.resources[i].as_ptr());
        let status = unsafe {
            (self.platform.draw)(
                context,
                target,
                source,
                &resources,
                size[0],
                size[1],
                linear.into(),
                blend.into(),
                ptr::from_ref(&constants).cast(),
                size_of::<Constants>() as u32,
            )
        };
        if status != 0 { OK } else { RESOURCE_FAILED }
    }
}

unsafe fn create(
    device: Handle,
    setup: *const Setup,
    out: *mut *mut Pass,
    platform: &'static Platform,
    filter: u32,
) -> i32 {
    if device.is_null() || out.is_null() || !unsafe { has_capacity(setup) } {
        return INVALID;
    }
    let setup = unsafe { &*setup };
    if setup.abi_version != ABI {
        return ABI_MISMATCH;
    }
    unsafe { out.write(null_mut()) };
    let mut raw = [null_mut(); 9];
    let status = unsafe { (platform.make)(device, setup, size_of::<Constants>() as u32, &mut raw) };
    let resources = raw.map(|handle| unsafe { ComRef::from_owned(handle) });
    if status != OK {
        return status;
    }
    let [
        Some(vs),
        Some(ps),
        Some(point),
        Some(linear),
        Some(opaque),
        Some(premultiplied),
        Some(depth),
        Some(raster),
        Some(constants),
    ] = resources
    else {
        return RESOURCE_FAILED;
    };
    let pass = crate::allocate_handle(Pass {
        resources: [
            vs,
            ps,
            point,
            linear,
            opaque,
            premultiplied,
            depth,
            raster,
            constants,
        ],
        device: unsafe { ComRef::retain(device) },
        filter: AtomicU32::new(filter),
        drawing: Mutex::new(()),
        platform,
    });
    if pass.is_null() {
        return RESOURCE_FAILED;
    }
    if setup.log.is_some() {
        let say = |text: &CStr| unsafe { (platform.log)(setup.log, setup.log_user, text.as_ptr()) };
        say(c"fullscreen pass ready");
        if filter == LINEAR {
            say(c"present blit ready");
        }
    }
    unsafe { out.write(pass) };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fullscreen_create(
    device: Handle,
    setup: *const Setup,
    out: *mut *mut Pass,
    platform: &'static Platform,
) -> i32 {
    unsafe { create(device, setup, out, platform, POINT) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_present_blit_create(
    device: Handle,
    setup: *const Setup,
    out: *mut *mut Pass,
    platform: &'static Platform,
) -> i32 {
    unsafe { create(device, setup, out, platform, LINEAR) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fullscreen_draw(
    pass: *const Pass,
    context: Handle,
    target: Handle,
    source: Handle,
    parameters: *const Draw,
) -> i32 {
    let Some(pass) = (unsafe { pass.as_ref() }) else {
        return INVALID;
    };
    if context.is_null()
        || target.is_null()
        || source.is_null()
        || !unsafe { has_capacity(parameters) }
    {
        return INVALID;
    }
    pass.draw(context, target, source, unsafe { &*parameters })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_present_blit_draw(
    pass: *const Pass,
    context: Handle,
    swapchain: Handle,
    source: Handle,
    tonemap: u32,
    platform: &Present,
) -> i32 {
    let Some(pass) = (unsafe { pass.as_ref() }) else {
        return INVALID;
    };
    if context.is_null() || swapchain.is_null() || source.is_null() {
        return INVALID;
    }
    let mut size = [0; 2];
    let back_buffer = unsafe { ComRef::from_owned((platform.back_buffer)(swapchain, &mut size)) };
    let Some(back_buffer) = back_buffer else {
        return NO_BACK_BUFFER;
    };
    let device = pass.device.as_ptr();
    let target = unsafe { ComRef::from_owned((platform.target)(device, back_buffer.as_ptr())) };
    let Some(target) = target else {
        return RESOURCE_FAILED;
    };
    let view = unsafe { ComRef::from_owned((platform.source)(device, source)) };
    let Some(view) = view else {
        return RESOURCE_FAILED;
    };
    let status = pass.draw(
        context,
        target.as_ptr(),
        view.as_ptr(),
        &Draw {
            struct_size: size_of::<Draw>() as u32,
            mode: if tonemap != 0 { TONEMAP } else { COPY },
            exposure: 1.0,
            width: size[0],
            height: size[1],
        },
    );
    match status {
        OK | INVALID | ABI_MISMATCH => status,
        _ => RESOURCE_FAILED,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fullscreen_filter(pass: *const Pass, filter: u32) {
    if let Some(pass) = unsafe { pass.as_ref() } {
        pass.filter.store(filter, Ordering::Relaxed);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fullscreen_destroy(pass: *mut Pass) {
    if !pass.is_null() {
        drop(unsafe { Box::from_raw(pass) });
    }
}
