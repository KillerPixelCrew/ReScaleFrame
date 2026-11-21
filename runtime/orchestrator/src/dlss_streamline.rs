// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::dlss_streamline_abi::{rsf_dlss_plan as Plan, rsf_dlss_support as Support};
use crate::gl_pipeline_abi::{rsf_dlss_log_fn as Log, rsf_dlss_setup as Setup};
use crate::render_abi::{RSF_DLSS_ABI_VERSION, rsf_dlss_frame as Frame};
use crate::sr_session::has_capacity;
use std::ffi::c_void;
use std::ptr::{NonNull, addr_of, null_mut};
use std::sync::Mutex;

type Handle = *mut c_void;
const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const MISSING_ENTRY: i32 = -5;
const INIT_FAILED: i32 = -6;
const NOT_READY: i32 = -8;
const FEATURE_FAILED: i32 = -9;
const OPTIMAL: u32 = 1;
const OPTIONS: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct Options {
    quality: u32,
    width: u32,
    height: u32,
    hdr: u32,
    auto_exposure: u32,
    alpha: u32,
    preset: u32,
}

#[repr(C)]
struct Platform {
    open: unsafe extern "C" fn(&Setup, &mut Handle) -> i32,
    borrow_host: unsafe extern "C" fn(Handle, Log, Handle, &mut Handle, &mut Handle) -> i32,
    device: unsafe extern "C" fn(Handle, Handle, &mut u32) -> i32,
    support: unsafe extern "C" fn(Handle, Handle, u32, *mut Support) -> i32,
    plan: unsafe extern "C" fn(Handle, *mut Plan) -> i32,
    present: unsafe extern "C" fn(Handle, Handle, u32, u32, u32) -> i32,
    token: unsafe extern "C" fn(Handle, Handle, u64, u32) -> Handle,
    constants: unsafe extern "C" fn(Handle, Handle, Handle, u64, u32, &Frame) -> i32,
    options: unsafe extern "C" fn(Handle, u32, &Options) -> i32,
    tags: unsafe extern "C" fn(Handle, Handle, Handle, u32, u32, &Frame) -> i32,
    evaluate: unsafe extern "C" fn(Handle, Handle, Handle, u32) -> i32,
    drain: unsafe extern "C" fn(Handle) -> u32,
    release_viewport: unsafe extern "C" fn(Handle, u32) -> i32,
    shutdown: unsafe extern "C" fn(Handle) -> i32,
    destroy: unsafe extern "C" fn(Handle),
}

const _: () = {
    assert!(size_of::<Options>() == 28);
    assert!(size_of::<Platform>() == 15 * size_of::<Handle>());
};

enum Route {
    Owned(Option<ComRef>),
    Shared {
        host: NonNull<c_void>,
        device: ComRef,
    },
}

impl Route {
    fn host(&self) -> Handle {
        match self {
            Self::Owned(_) => null_mut(),
            Self::Shared { host, .. } => host.as_ptr(),
        }
    }

    fn device(&self) -> Option<&ComRef> {
        match self {
            Self::Owned(device) => device.as_ref(),
            Self::Shared { device, .. } => Some(device),
        }
    }
}

struct Session {
    native: NonNull<c_void>,
    route: Route,
    features: u32,
    applied: [Option<Options>; 2],
}

// Native calls and access to the borrowed graphics host are serialized here. The caller keeps the
// shared host alive until SR shutdown, and each COM device is retained until its route is retired.
unsafe impl Send for Session {}
static SESSION: Mutex<Option<Session>> = Mutex::new(None);

fn viewport_slot(viewport: u32) -> u32 {
    if viewport < 2 { viewport } else { 0 }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_streamline_load(
    setup: *const Setup,
    platform: &Platform,
) -> i32 {
    if !unsafe { has_capacity(setup) } {
        return INVALID;
    }
    let setup = unsafe { &*setup };
    if setup.interposer_path_utf8.is_null() {
        return INVALID;
    }
    if setup.abi_version != RSF_DLSS_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let Ok(mut state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    if state.is_some() {
        return OK;
    }
    let mut native = null_mut();
    let result = unsafe { (platform.open)(setup, &mut native) };
    if result != OK {
        return result;
    }
    let Some(native) = NonNull::new(native) else {
        return INIT_FAILED;
    };
    *state = Some(Session {
        native,
        route: Route::Owned(None),
        features: 0,
        applied: [None; 2],
    });
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_streamline_share_host(
    host: Handle,
    log: Log,
    user: Handle,
    platform: &Platform,
) -> i32 {
    let Some(host) = NonNull::new(host) else {
        return NOT_READY;
    };
    let Ok(mut state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    if state.is_some() {
        return NOT_READY;
    }
    let mut native = null_mut();
    let mut device = null_mut();
    let status =
        unsafe { (platform.borrow_host)(host.as_ptr(), log, user, &mut native, &mut device) };
    if status != OK {
        return status;
    }
    let Some(native) = NonNull::new(native) else {
        return NOT_READY;
    };
    if device.is_null() {
        unsafe { (platform.destroy)(native.as_ptr()) };
        return NOT_READY;
    }
    *state = Some(Session {
        native,
        route: Route::Shared {
            host,
            device: unsafe { ComRef::retain(device) },
        },
        features: OPTIMAL | OPTIONS,
        applied: [None; 2],
    });
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_streamline_set_device(
    device: Handle,
    platform: &Platform,
) -> i32 {
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
        return INVALID;
    }
    let Some(session) = state.as_mut() else {
        return NOT_READY;
    };
    let mut capabilities = 0;
    let result = unsafe { (platform.device)(session.native.as_ptr(), device, &mut capabilities) };
    if matches!(result, OK | MISSING_ENTRY) {
        session.route = Route::Owned(Some(unsafe { ComRef::retain(device) }));
        session.features = capabilities;
        session.applied = [None; 2];
    }
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_streamline_support(
    support: *mut Support,
    platform: &Platform,
) -> i32 {
    if !unsafe { has_capacity(support) } {
        return INVALID;
    }
    let Ok(state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    let Some(session) = state.as_ref() else {
        return NOT_READY;
    };
    let Some(device) = session.route.device() else {
        return NOT_READY;
    };
    let shared = (!session.route.host().is_null()).into();
    unsafe { (platform.support)(session.native.as_ptr(), device.as_ptr(), shared, support) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_streamline_plan(plan: *mut Plan, platform: &Platform) -> i32 {
    if !unsafe { has_capacity(plan) } {
        return INVALID;
    }
    // The caller only needs to initialize the inputs.
    let (width, height) = unsafe {
        (
            addr_of!((*plan).output_width).read(),
            addr_of!((*plan).output_height).read(),
        )
    };
    if width == 0 || height == 0 {
        return INVALID;
    }
    let Ok(state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    let Some(session) = state
        .as_ref()
        .filter(|session| session.features & OPTIMAL != 0)
    else {
        return NOT_READY;
    };
    unsafe { (platform.plan)(session.native.as_ptr(), plan) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_streamline_present(
    chain: Handle,
    interval: u32,
    flags: u32,
    begin: u32,
    platform: &Platform,
) -> i32 {
    if chain.is_null() || begin > 1 {
        return INVALID;
    }
    let Ok(state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    let Some(session) = state.as_ref() else {
        return NOT_READY;
    };
    if !session.route.host().is_null() {
        return OK;
    }
    if session.route.device().is_none() {
        return NOT_READY;
    }
    unsafe { (platform.present)(session.native.as_ptr(), chain, interval, flags, begin) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_streamline_evaluate(
    context: Handle,
    frame: *const Frame,
    source: u64,
    platform: &Platform,
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
        return INVALID;
    }
    let frame = unsafe { &*frame };
    if frame.abi_version != RSF_DLSS_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if [frame.color_in, frame.color_out, frame.depth, frame.motion]
        .into_iter()
        .any(|input| input.is_null())
    {
        return INVALID;
    }
    let Some(session) = state
        .as_mut()
        .filter(|session| session.features & OPTIONS != 0)
    else {
        return NOT_READY;
    };
    let native = session.native.as_ptr();
    let token = unsafe { (platform.token)(native, host, source, frame.frame_index) };
    if token.is_null() {
        return if shared { NOT_READY } else { FEATURE_FAILED };
    }
    let slot = viewport_slot(frame.viewport);
    let constants = unsafe { (platform.constants)(native, host, token, source, slot, frame) };
    if constants != OK {
        return constants;
    }

    let options = Options {
        quality: frame.quality.min(5),
        width: frame.output_width,
        height: frame.output_height,
        hdr: (frame.color_encoded == 0).into(),
        auto_exposure: (frame.exposure.is_null() && frame.color_encoded == 0).into(),
        alpha: (frame.alpha != 0).into(),
        preset: frame.preset,
    };
    let applied = &mut session.applied[slot as usize];
    if *applied != Some(options) {
        let status = unsafe { (platform.options)(native, slot, &options) };
        if status != OK {
            if status != INVALID {
                *applied = None;
            }
            return status;
        }
        *applied = Some(options);
    }
    let tagged = unsafe { (platform.tags)(native, context, token, slot, shared.into(), frame) };
    if tagged != OK {
        return tagged;
    }
    unsafe { (platform.evaluate)(native, context, token, slot) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_streamline_release(viewport: u32, platform: &Platform) -> i32 {
    let Ok(mut state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    let Some(session) = state.as_mut() else {
        return NOT_READY;
    };
    let host = session.route.host();
    if !host.is_null() && unsafe { (platform.drain)(host) } == 0 {
        return FEATURE_FAILED;
    }
    let slot = viewport_slot(viewport);
    session.applied[slot as usize] = None;
    unsafe { (platform.release_viewport)(session.native.as_ptr(), slot) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_dlss_streamline_shutdown(platform: &Platform) -> i32 {
    let Ok(mut state) = SESSION.try_lock() else {
        return NOT_READY;
    };
    let Some(session) = state.as_ref() else {
        return NOT_READY;
    };
    let native = session.native.as_ptr();
    if matches!(&session.route, Route::Owned(_)) {
        let stopped = unsafe { (platform.shutdown)(native) };
        if stopped != OK {
            return stopped;
        }
    }
    drop(state.take());
    unsafe { (platform.destroy)(native) };
    OK
}
