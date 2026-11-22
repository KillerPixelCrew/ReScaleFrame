// SPDX-License-Identifier: MIT
use crate::sys::*;
use rsf_game_sys::backend_result::{
    ABI_MISMATCH, INIT_FAILED, INVALID_ARGUMENT, NOT_READY, STALE_RESOURCES, WRONG_API,
};

#[cfg(feature = "test-fixtures")]
mod fixture;
use rsf_game_sys::{
    RSF_FRAME_FLAG_NO_SR, RSF_FRAME_FLAG_RESET, RSF_GAME_CAMERA_ABI_VERSION,
    RSF_GAME_FRAME_ABI_VERSION, rsf_camera_frame,
};
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::{self, NonNull};
use std::time::Instant;

const ULTRA_QUALITY: u32 = 5;
const PATH_CAPACITY: usize = 1024;

type Evaluate = unsafe extern "C" fn(*mut c_void, *mut c_void, *const rsf_sr_frame) -> i32;
type Close = unsafe extern "C" fn(*mut c_void);

struct Context {
    handle: NonNull<c_void>,
    evaluate: Evaluate,
    close: Close,
}

impl Drop for Context {
    fn drop(&mut self) {
        unsafe { (self.close)(self.handle.as_ptr()) };
    }
}

#[derive(Default)]
struct FrameIdentity {
    frame: u64,
    session: u64,
    view: u32,
    generation: u32,
}

pub(super) struct Session {
    open: rsf_sr_open_desc,
    directories: [[c_char; PATH_CAPACITY]; 4],
    fsr: *const rsf_sr_provider,
    xess: *const rsf_sr_provider,
    active: Option<Context>,
    status: rsf_sr_session_status,
    reset: bool,
    last: FrameIdentity,
}

pub(crate) fn valid_unit_scale(units_to_meters: f32) -> bool {
    units_to_meters.is_finite() && units_to_meters > 0.0
}

// A caller may provide only the size field when reporting a short structure.
// The sized-record checks are shared with rsf-graphics through the SDK.
pub(crate) use rsf_game_sys::ffi_util::{checked, has_capacity};

// Shared wire validation for direct provider calls, sessions and the D3D11 bridge.
pub(super) unsafe extern "C" fn rsf_core_validate_sr_frame(frame: *const rsf_sr_frame) -> i32 {
    let Some(frame) = (unsafe { checked(frame) }) else {
        return INVALID_ARGUMENT;
    };
    let Some(record) = (unsafe { checked(frame.record) }) else {
        return INVALID_ARGUMENT;
    };
    let camera = &record.camera;
    if record.abi_version != RSF_GAME_FRAME_ABI_VERSION
        || camera.abi_version != RSF_GAME_CAMERA_ABI_VERSION
    {
        return ABI_MISMATCH;
    }
    let render = [record.render_width, record.render_height];
    let output = [record.output_width, record.output_height];
    if camera.struct_size < size_of::<rsf_camera_frame>() as u32
        || record.frame_id == 0
        || render.contains(&0)
        || output.contains(&0)
        || render[0] > output[0]
        || render[1] > output[1]
        || !(0.0..=1.0).contains(&frame.sharpness)
        || ![frame.jitter_x, frame.jitter_y]
            .into_iter()
            .all(f32::is_finite)
        || ![frame.motion_scale_x, frame.motion_scale_y]
            .into_iter()
            .all(|value| value.is_finite() && value != 0.0)
        || ![
            frame.pre_exposure,
            frame.view_space_to_meters,
            record.frame_time_ms,
            camera.near_plane,
        ]
        .into_iter()
        .all(|value| value.is_finite() && value > 0.0)
        || !camera.far_plane.is_finite()
        || camera.far_plane < 0.0
        || !(camera.vertical_fov_radians > 0.0
            && camera.vertical_fov_radians < std::f32::consts::PI.next_up())
    {
        return INVALID_ARGUMENT;
    }
    if record.flags & RSF_FRAME_FLAG_NO_SR != 0 {
        return NOT_READY;
    }
    for (resource, [width, height]) in [
        (&frame.color, render),
        (&frame.depth, render),
        (&frame.motion, render),
        (&frame.output, output),
        (&frame.exposure, [0, 0]),
    ] {
        if width == 0 && resource.resource.is_null() {
            continue;
        }
        if resource.struct_size < size_of::<rsf_backend_resource>() as u32
            || resource.resource.is_null()
            || resource.lifetime != 0 // RSF_LIFETIME_ONLY_NOW
            || resource.x != 0
            || resource.y != 0
            || resource.width < width
            || resource.height < height
        {
            return INVALID_ARGUMENT;
        }
        if resource.generation != record.resource_generation {
            return STALE_RESOURCES;
        }
    }
    0
}

#[cfg(not(test))]
use crate::d3d12_sr::{rsf_fsr_managed_sr_provider, rsf_xess_sr_provider};

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_sr_session_create(
    setup: *const rsf_sr_session_setup,
    out: *mut *mut Session,
) -> i32 {
    unsafe {
        create(
            setup,
            out,
            rsf_fsr_managed_sr_provider(),
            rsf_xess_sr_provider(),
        )
    }
}

unsafe fn create(
    setup: *const rsf_sr_session_setup,
    out: *mut *mut Session,
    fsr: *const rsf_sr_provider,
    xess: *const rsf_sr_provider,
) -> i32 {
    if out.is_null()
        || !unsafe { has_capacity(setup) && has_capacity(ptr::addr_of!((*setup).open)) }
    {
        return INVALID_ARGUMENT;
    }
    let setup = unsafe { &*setup };
    let mut open = setup.open;
    if open.device.is_null() || open.output_width == 0 || open.output_height == 0 {
        return INVALID_ARGUMENT;
    }
    unsafe { out.write(ptr::null_mut()) };
    if setup.abi_version != RSF_SR_SESSION_ABI_VERSION
        || open.abi_version != RSF_BACKEND_ABI_VERSION
    {
        return ABI_MISMATCH;
    }
    if open.api != RSF_API_D3D12 {
        return WRONG_API;
    }
    open.runtime_directory_utf8 = ptr::null();
    open.struct_size = size_of::<rsf_sr_open_desc>() as u32;
    let mut directories = [[0; PATH_CAPACITY]; 4];
    let paths = [
        setup.fsr2_directory_utf8,
        setup.fsr3_directory_utf8,
        setup.fsr4_directory_utf8,
        setup.xess_directory_utf8,
    ];
    for (directory, path) in directories.iter_mut().zip(paths) {
        if !path.is_null() {
            for (i, byte) in directory.iter_mut().enumerate() {
                *byte = unsafe { *path.add(i) };
                if *byte == 0 {
                    break;
                }
                if i + 1 == PATH_CAPACITY {
                    return INVALID_ARGUMENT;
                }
            }
        }
    }
    let status = rsf_sr_session_status {
        struct_size: size_of::<rsf_sr_session_status>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    let handle = crate::allocate_handle(Session {
        open,
        directories,
        fsr,
        xess,
        active: None,
        status,
        reset: true,
        last: FrameIdentity::default(),
    });
    if handle.is_null() {
        return INIT_FAILED;
    }
    unsafe { out.write(handle) };
    0
}

impl Session {
    fn log(&self, message: impl FnOnce() -> String) {
        if let Some(log) = self.open.log
            && let Ok(message) = CString::new(message())
        {
            unsafe { log(self.open.log_user, message.as_ptr()) };
        }
    }

    fn select(&mut self, backend: u32, quality: u32, version_id: u64, force: bool) -> i32 {
        if (backend != 0 && !(RSF_SR_FSR2..=RSF_SR_XESS).contains(&backend))
            || quality > ULTRA_QUALITY
        {
            return INVALID_ARGUMENT;
        }
        self.status.requested = backend;
        if backend == 0 {
            self.active = None;
            self.status.effective = 0;
            self.status.version_id = 0;
            self.status.version_name[0] = 0;
            self.reset = true;
        } else if force
            || self.active.is_none()
            || self.status.effective != backend
            || self.status.quality != quality
            || (version_id != 0 && self.status.version_id != version_id)
        {
            let result = self.replace(backend, quality, version_id);
            self.status.last_switch_result = result;
            if result != 0 {
                self.log(|| {
                    format!(
                        "SR switch to {backend} refused ({result}); keeping {}",
                        self.status.effective
                    )
                });
            }
            return result;
        }
        self.status.last_switch_result = 0;
        0
    }

    fn replace(&mut self, backend: u32, quality: u32, version_id: u64) -> i32 {
        let provider = if backend == RSF_SR_XESS {
            self.xess
        } else {
            self.fsr
        };
        if !unsafe { has_capacity(provider) } {
            return INIT_FAILED;
        }
        let provider = unsafe { &*provider };
        let (Some(start), Some(plan), Some(evaluate), Some(close)) = (
            provider.open,
            provider.plan,
            provider.evaluate,
            provider.close,
        ) else {
            return INIT_FAILED;
        };
        let open = rsf_sr_open_desc {
            quality,
            fsr_major: if backend == RSF_SR_XESS { 0 } else { backend },
            version_id,
            runtime_directory_utf8: self.directories[(backend - RSF_SR_FSR2) as usize].as_ptr(),
            ..self.open
        };
        let opening = Instant::now();
        let mut replacement = ptr::null_mut();
        let result = unsafe { start(&open, &mut replacement) };
        if result != 0 {
            return result;
        }
        let Some(handle) = NonNull::new(replacement) else {
            return INIT_FAILED;
        };
        let replacement = Context {
            handle,
            evaluate,
            close,
        };
        self.log(|| {
            format!(
                "SR backend {backend} context created in {:.0} ms",
                opening.elapsed().as_secs_f64() * 1000.0
            )
        });
        let (mut width, mut height) = (0, 0);
        let result = unsafe { plan(handle.as_ptr(), quality, &mut width, &mut height) };
        if result != 0 {
            return result;
        }
        if width == 0 || height == 0 || width > open.output_width || height > open.output_height {
            return INIT_FAILED;
        }
        let (mut id, mut name) = (0, ptr::null());
        if let Some(version) = provider.get_version
            && unsafe { version(handle.as_ptr(), &mut id, &mut name) } != 0
        {
            id = 0;
            name = ptr::null();
        }
        let name = if name.is_null() {
            c"unknown"
        } else {
            unsafe { CStr::from_ptr(name) }
        };
        let mut version_name = [0; 128];
        for (dst, &src) in version_name[..127].iter_mut().zip(name.to_bytes()) {
            *dst = src as c_char;
        }
        self.active = Some(replacement);
        self.status.effective = backend;
        self.status.quality = quality;
        self.status.render_width = width;
        self.status.render_height = height;
        self.status.version_id = id;
        self.status.version_name = version_name;
        self.status.last_switch_result = 0;
        self.reset = true;
        if let Some(log) = open.log {
            unsafe { log(open.log_user, self.status.version_name.as_ptr()) };
        }
        0
    }

    fn dispatch(&mut self, command: *mut c_void, frame: *const rsf_sr_frame, warm_up: bool) -> i32 {
        if command.is_null() {
            return INVALID_ARGUMENT;
        }
        let result = unsafe { rsf_core_validate_sr_frame(frame) };
        if result != 0 {
            return result;
        }
        let Some(active) = &self.active else {
            return NOT_READY;
        };
        let record = unsafe { &*(*frame).record };
        if record.output_width != self.open.output_width
            || record.output_height != self.open.output_height
        {
            return STALE_RESOURCES;
        }
        let same_view = record.session_id == self.last.session && record.view_id == self.last.view;
        if !warm_up && same_view && record.frame_id <= self.last.frame {
            return INVALID_ARGUMENT;
        }
        let reset = self.reset
            || !same_view
            || record.flags & RSF_FRAME_FLAG_RESET != 0
            || self.last.frame.checked_add(1) != Some(record.frame_id)
            || record.resource_generation != self.last.generation;
        let identity = FrameIdentity {
            frame: record.frame_id,
            session: record.session_id,
            view: record.view_id,
            generation: record.resource_generation,
        };
        // Keep the resource lease synchronous. The copied descriptor advertises
        // only the prefix this runtime actually owns.
        let mut submitted = std::mem::MaybeUninit::<rsf_sr_frame>::uninit();
        let copy = submitted.as_mut_ptr();
        let result = unsafe {
            copy.copy_from_nonoverlapping(frame, 1);
            (*copy).struct_size = size_of::<rsf_sr_frame>() as u32;
            if warm_up {
                (*copy).reset = 1;
            } else {
                (*copy).reset |= u32::from(reset);
            }
            (active.evaluate)(active.handle.as_ptr(), command, copy)
        };
        if warm_up {
            self.reset = true;
        } else {
            self.last = identity;
        }
        result
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_sr_session_select(
    session: *mut Session,
    backend: u32,
    quality: u32,
    version_id: u64,
) -> i32 {
    let Some(session) = (unsafe { session.as_mut() }) else {
        return INVALID_ARGUMENT;
    };
    session.select(backend, quality, version_id, false)
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_sr_session_set_auto_exposure(
    session: *mut Session,
    enabled: u32,
) -> i32 {
    let Some(session) = (unsafe { session.as_mut() }) else {
        return INVALID_ARGUMENT;
    };
    if enabled > 1 {
        return INVALID_ARGUMENT;
    }
    if session.open.auto_exposure == enabled {
        return 0;
    }
    let previous = session.open.auto_exposure;
    session.open.auto_exposure = enabled;
    let status = session.status;
    let result = session.select(status.effective, status.quality, status.version_id, true);
    if result != 0 {
        session.open.auto_exposure = previous;
    }
    result
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_sr_session_evaluate(
    session: *mut Session,
    command: *mut c_void,
    frame: *const rsf_sr_frame,
) -> i32 {
    let Some(session) = (unsafe { session.as_mut() }) else {
        return INVALID_ARGUMENT;
    };
    let result = session.dispatch(command, frame, false);
    session.reset = result != 0;
    if result != 0 && session.status.last_frame_result != result {
        session.log(|| {
            format!(
                "SR frame refused ({result}), backend {}",
                session.status.effective
            )
        });
    }
    session.status.last_frame_result = result;
    if result == 0 {
        session.status.frames_evaluated = session.status.frames_evaluated.wrapping_add(1);
    } else {
        session.status.frames_refused = session.status.frames_refused.wrapping_add(1);
    }
    result
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_sr_session_warm_up(
    session: *mut Session,
    command: *mut c_void,
    frame: *const rsf_sr_frame,
) -> i32 {
    let Some(session) = (unsafe { session.as_mut() }) else {
        return INVALID_ARGUMENT;
    };
    session.dispatch(command, frame, true)
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_sr_session_get_status(
    session: *const Session,
    status: *mut rsf_sr_session_status,
) -> i32 {
    let Some(session) = (unsafe { session.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if !unsafe { has_capacity(status) } {
        return INVALID_ARGUMENT;
    }
    unsafe { status.write(session.status) };
    0
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_sr_session_destroy(session: *mut Session) {
    if !session.is_null() {
        drop(unsafe { Box::from_raw(session) });
    }
}
