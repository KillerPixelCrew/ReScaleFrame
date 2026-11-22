// SPDX-License-Identifier: MIT
use crate::motion_resolve_params::dense_resolve_params;
use crate::render_abi::*;
use rsf_game_sys::{
    RSF_FRAME_FLAG_RESET, RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record,
};
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

const INVALID: i32 = -1;
const INIT_FAILED: i32 = -6;
const FEATURE_FAILED: i32 = -9;
const EXPOSURE_SETTLE_FRAMES: u32 = 30;
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

type Bridge = c_void;
type Resolve = c_void;
type Frame = rsf_dlss_frame;

#[repr(C)]
struct Services {
    create: unsafe extern "C" fn(*const rsf_sr_session_setup, *mut *mut Bridge) -> i32,
    select: unsafe extern "C" fn(*mut Bridge, u32, u32, u64) -> i32,
    status: unsafe extern "C" fn(*const Bridge, *mut rsf_sr_session_status) -> i32,
    exposure: unsafe extern "C" fn(*mut Bridge, u32) -> i32,
    evaluate: unsafe extern "C" fn(*mut Bridge, *mut c_void, *const rsf_sr_frame) -> i32,
    warm_up: unsafe extern "C" fn(*mut Bridge, u32, u32, u32, u32, u32) -> i32,
    destroy: unsafe extern "C" fn(*mut Bridge),
    create_resolve: unsafe extern "C" fn(*mut c_void, u32, u32, *mut *mut Resolve) -> i32,
    resolve: unsafe extern "C" fn(
        *mut Resolve,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *const rsf_motion_resolve_params,
    ) -> i32,
    depth: unsafe extern "C" fn(*mut Resolve) -> *mut c_void,
    motion: unsafe extern "C" fn(*mut Resolve) -> *mut c_void,
    destroy_resolve: unsafe extern "C" fn(*mut Resolve),
    dump_active: unsafe extern "C" fn() -> i32,
    dump_frame: unsafe extern "C" fn(*mut c_void, *const Frame, u32),
}
const _: () = assert!(size_of::<Services>() == 14 * size_of::<*const c_void>());

struct Exposure {
    auto: bool,
    mismatch: u32,
    fresh: bool,
}

impl Exposure {
    fn update(&mut self, auto: bool, apply: impl FnOnce(bool) -> i32) -> i32 {
        if auto == self.auto {
            self.mismatch = 0;
        } else {
            // A missing exposure needs auto immediately. Manual waits for a
            // stable run, except before the first frame or a backend switch.
            let change = if self.fresh || auto {
                true
            } else {
                self.mismatch = self.mismatch.saturating_add(1);
                self.mismatch >= EXPOSURE_SETTLE_FRAMES
            };
            if change {
                let result = apply(auto);
                if result != 0 {
                    return result;
                }
                self.auto = auto;
                self.mismatch = 0;
            }
        }
        self.fresh = false;
        0
    }
}

pub(super) struct Adapter {
    services: &'static Services,
    bridge: *mut Bridge,
    resolve: *mut Resolve,
    device: *mut c_void,
    resolve_extent: [u32; 2],
    generation: u32,
    frame_id: u64,
    session_id: u64,
    previous_qpc: i64,
    frequency: i64,
    units_to_meters: f32,
    exposure: Exposure,
}

impl Drop for Adapter {
    fn drop(&mut self) {
        unsafe {
            (self.services.destroy)(self.bridge);
            (self.services.destroy_resolve)(self.resolve);
        }
    }
}

fn counter() -> i64 {
    let mut now = 0;
    unsafe { QueryPerformanceCounter(&mut now) };
    now
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_sr_legacy_create(
    setup: &rsf_sr_session_setup,
    units_to_meters: f32,
    standard_depth: u32,
    auto_exposure: u32,
    services: &'static Services,
    out: *mut *mut Adapter,
) -> i32 {
    if out.is_null() || !crate::sr_session::valid_unit_scale(units_to_meters) {
        return INVALID;
    }
    unsafe { out.write(ptr::null_mut()) };
    let mut frequency = 0;
    if unsafe { QueryPerformanceFrequency(&mut frequency) } == 0 || frequency <= 0 {
        return INIT_FAILED;
    }
    let previous_qpc = counter();
    let mut setup = *setup;
    setup.open.hdr = 1;
    setup.open.dynamic_resolution = 1;
    setup.open.auto_exposure = u32::from(auto_exposure != 0);
    setup.open.inverted_depth = u32::from(standard_depth == 0);
    setup.open.depth_infinite = setup.open.inverted_depth;
    let mut adapter = Adapter {
        services,
        bridge: ptr::null_mut(),
        resolve: ptr::null_mut(),
        device: setup.open.device,
        resolve_extent: [0; 2],
        generation: 0,
        frame_id: 0,
        session_id: NEXT_SESSION.fetch_add(1, Ordering::Relaxed),
        previous_qpc,
        frequency,
        units_to_meters,
        exposure: Exposure {
            auto: auto_exposure != 0,
            mismatch: 0,
            fresh: true,
        },
    };
    let result = unsafe { (services.create)(&setup, &mut adapter.bridge) };
    if result != 0 {
        return result;
    }
    if adapter.bridge.is_null() {
        return INIT_FAILED;
    }
    let handle = crate::allocate_handle(adapter);
    if handle.is_null() {
        return INIT_FAILED;
    }
    unsafe { out.write(handle) };
    0
}

impl Adapter {
    fn status(&self) -> rsf_sr_session_status {
        let mut status = rsf_sr_session_status {
            struct_size: size_of::<rsf_sr_session_status>() as u32,
            ..unsafe { std::mem::zeroed() }
        };
        unsafe { (self.services.status)(self.bridge, &mut status) };
        status
    }

    fn resource(&self, texture: *mut c_void, width: u32, height: u32) -> rsf_backend_resource {
        rsf_backend_resource {
            struct_size: size_of::<rsf_backend_resource>() as u32,
            resource: texture,
            width,
            height,
            generation: self.generation,
            ..unsafe { std::mem::zeroed() }
        }
    }

    fn record(&mut self, input: &Frame) -> rsf_frame_record {
        self.frame_id = self.frame_id.wrapping_add(1);
        let now = counter();
        let milliseconds = (now - self.previous_qpc) as f64 * 1000.0 / self.frequency as f64;
        self.previous_qpc = now;
        let mut record = rsf_frame_record {
            struct_size: size_of::<rsf_frame_record>() as u32,
            abi_version: RSF_GAME_FRAME_ABI_VERSION,
            frame_id: self.frame_id,
            session_id: self.session_id,
            view_id: input.viewport,
            resource_generation: self.generation,
            flags: if input.reset != 0 {
                RSF_FRAME_FLAG_RESET
            } else {
                0
            },
            render_width: input.render_width,
            render_height: input.render_height,
            output_width: input.output_width,
            output_height: input.output_height,
            frame_time_ms: (milliseconds as f32).clamp(0.01, 1000.0),
            ..unsafe { std::mem::zeroed() }
        };
        record.camera.struct_size = size_of_val(&record.camera) as u32;
        record.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
        record.camera.near_plane = input.near_plane;
        record.camera.far_plane = input.far_plane;
        record.camera.vertical_fov_radians = input.vertical_fov;
        record.camera.depth_inverted = input.depth_inverted;
        record
    }

    fn dense_motion(&mut self, context: *mut c_void, input: &Frame, has_sentinel: u32) -> i32 {
        let extent = [input.render_width, input.render_height];
        if self.resolve.is_null() || self.resolve_extent != extent {
            let mut replacement = ptr::null_mut();
            let created = unsafe {
                (self.services.create_resolve)(self.device, extent[0], extent[1], &mut replacement)
            };
            if created == 0 || replacement.is_null() {
                if !replacement.is_null() {
                    unsafe { (self.services.destroy_resolve)(replacement) };
                }
                return INIT_FAILED;
            }
            let previous = std::mem::replace(&mut self.resolve, replacement);
            self.resolve_extent = extent;
            self.generation = self.generation.wrapping_add(1);
            unsafe { (self.services.destroy_resolve)(previous) };
        }
        let params = dense_resolve_params(
            input.clip_to_prev_clip,
            [input.render_width, input.render_height],
            [input.motion_scale_x, input.motion_scale_y],
            (input.motion_invalid_value, has_sentinel),
            input.motion_depth_layer,
            [0.0; 2],
        );
        if unsafe {
            (self.services.resolve)(self.resolve, context, input.motion, input.depth, &params)
        } == 0
        {
            return FEATURE_FAILED;
        }
        0
    }

    fn evaluate(
        &mut self,
        context: *mut c_void,
        input: &Frame,
        submitted: &mut Frame,
        sentinel: u32,
        sharpness: f32,
    ) -> i32 {
        let result = self
            .exposure
            .update(input.exposure.is_null(), |auto| unsafe {
                (self.services.exposure)(self.bridge, u32::from(auto))
            });
        if result != 0 {
            return result;
        }
        let result = self.dense_motion(context, input, sentinel);
        if result != 0 {
            return result;
        }
        let record = self.record(input);
        let depth = unsafe { (self.services.depth)(self.resolve) };
        let motion = unsafe { (self.services.motion)(self.resolve) };
        let render_resource =
            |texture| self.resource(texture, input.render_width, input.render_height);
        let mut frame = rsf_sr_frame {
            struct_size: size_of::<rsf_sr_frame>() as u32,
            record: &record,
            color: render_resource(input.color_in),
            depth: render_resource(depth),
            motion: render_resource(motion),
            output: self.resource(input.color_out, input.output_width, input.output_height),
            sharpness,
            jitter_x: input.jitter_x,
            jitter_y: input.jitter_y,
            motion_scale_x: 1.0,
            motion_scale_y: 1.0,
            pre_exposure: 1.0,
            view_space_to_meters: self.units_to_meters,
            reset: input.reset,
            ..unsafe { std::mem::zeroed() }
        };
        if !input.exposure.is_null() {
            frame.exposure = self.resource(input.exposure, 1, 1);
        }
        if !input.reactive_mask.is_null() {
            frame.reactive = render_resource(input.reactive_mask);
        }
        if !input.transparency_hint.is_null() {
            frame.transparency = render_resource(input.transparency_hint);
        }
        *submitted = Frame {
            depth,
            motion,
            motion_scale_x: 1.0,
            motion_scale_y: 1.0,
            camera_motion_included: 1,
            ..*input
        };
        if unsafe { (self.services.dump_active)() } != 0 {
            unsafe { (self.services.dump_frame)(context, submitted, self.status().effective) };
        }
        // These normalized textures are borrowed by the bridge only for this call.
        unsafe { (self.services.evaluate)(self.bridge, context, &frame) }
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_sr_legacy_select(
    adapter: *mut Adapter,
    backend: u32,
    quality: u32,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    let Some(adapter) = (unsafe { adapter.as_mut() }) else {
        return INVALID;
    };
    let result = unsafe { (adapter.services.select)(adapter.bridge, backend, quality, 0) };
    if result == 0 {
        adapter.exposure.fresh = true;
        let status = adapter.status();
        if !width.is_null() {
            unsafe { width.write(status.render_width) };
        }
        if !height.is_null() {
            unsafe { height.write(status.render_height) };
        }
    }
    result
}

pub(super) unsafe fn rsf_sr_legacy_evaluate(
    adapter: *mut Adapter,
    context: *mut c_void,
    input: &Frame,
    submitted: &mut Frame,
    sentinel: u32,
    sharpness: f32,
) -> i32 {
    let Some(adapter) = (unsafe { adapter.as_mut() }) else {
        return INVALID;
    };
    adapter.evaluate(context, input, submitted, sentinel, sharpness)
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_sr_legacy_warm_up(
    adapter: *mut Adapter,
    render_width: u32,
    render_height: u32,
    output_width: u32,
    output_height: u32,
) -> i32 {
    let Some(adapter) = (unsafe { adapter.as_mut() }) else {
        return INVALID;
    };
    unsafe {
        (adapter.services.warm_up)(
            adapter.bridge,
            render_width,
            render_height,
            output_width,
            output_height,
            u32::from(!adapter.exposure.auto),
        )
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_sr_legacy_destroy(adapter: *mut Adapter) {
    if !adapter.is_null() {
        drop(unsafe { Box::from_raw(adapter) });
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_sr_legacy_fg_inputs(
    adapter: *const Adapter,
    depth: *mut *mut c_void,
    motion: *mut *mut c_void,
) -> i32 {
    let Some(adapter) = (unsafe { adapter.as_ref() }) else {
        return 0;
    };
    if adapter.resolve.is_null() || depth.is_null() || motion.is_null() {
        return 0;
    }
    let (d, m) = unsafe {
        (
            (adapter.services.depth)(adapter.resolve),
            (adapter.services.motion)(adapter.resolve),
        )
    };
    unsafe {
        depth.write(d);
        motion.write(m);
    }
    i32::from(!d.is_null() && !m.is_null())
}

// A legacy adapter over a fake bridge, for the synthetic SR pipeline contracts.
#[cfg(test)]
pub(super) mod fake {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        pub(in crate) static SELECT: Cell<i32> = const { Cell::new(0) };
        pub(in crate) static RENDER: Cell<[u32; 2]> = const { Cell::new([320, 180]) };
        pub(in crate) static EVALUATE_RESULT: Cell<i32> = const { Cell::new(0) };
        pub(in crate) static EVALUATIONS: Cell<u32> = const { Cell::new(0) };
        pub(in crate) static DESTROYED: Cell<u32> = const { Cell::new(0) };
    }
    const BRIDGE: *mut Bridge = ptr::without_provenance_mut(0x40);
    const RESOLVE: *mut Resolve = ptr::without_provenance_mut(0x80);
    pub(crate) const DEPTH: *mut c_void = ptr::without_provenance_mut(0x90);
    pub(crate) const MOTION: *mut c_void = ptr::without_provenance_mut(0xa0);

    unsafe extern "C" fn create(_: *const rsf_sr_session_setup, out: *mut *mut Bridge) -> i32 {
        unsafe { out.write(BRIDGE) };
        0
    }
    unsafe extern "C" fn select(_: *mut Bridge, _: u32, _: u32, _: u64) -> i32 {
        SELECT.get()
    }
    unsafe extern "C" fn status(_: *const Bridge, out: *mut rsf_sr_session_status) -> i32 {
        let [width, height] = RENDER.get();
        unsafe {
            (*out).render_width = width;
            (*out).render_height = height;
        }
        0
    }
    unsafe extern "C" fn exposure(_: *mut Bridge, _: u32) -> i32 {
        0
    }
    unsafe extern "C" fn evaluate(_: *mut Bridge, _: *mut c_void, _: *const rsf_sr_frame) -> i32 {
        EVALUATIONS.set(EVALUATIONS.get() + 1);
        EVALUATE_RESULT.get()
    }
    unsafe extern "C" fn warm_up(_: *mut Bridge, _: u32, _: u32, _: u32, _: u32, _: u32) -> i32 {
        0
    }
    unsafe extern "C" fn destroy(_: *mut Bridge) {
        DESTROYED.set(DESTROYED.get() + 1);
    }
    unsafe extern "C" fn create_resolve(
        _: *mut c_void,
        _: u32,
        _: u32,
        resolve: *mut *mut Resolve,
    ) -> i32 {
        unsafe { resolve.write(RESOLVE) };
        1
    }
    unsafe extern "C" fn resolve(
        _: *mut Resolve,
        _: *mut c_void,
        _: *mut c_void,
        _: *mut c_void,
        _: *const rsf_motion_resolve_params,
    ) -> i32 {
        1
    }
    unsafe extern "C" fn depth(_: *mut Resolve) -> *mut c_void {
        DEPTH
    }
    unsafe extern "C" fn motion(_: *mut Resolve) -> *mut c_void {
        MOTION
    }
    unsafe extern "C" fn destroy_resolve(_: *mut Resolve) {}
    unsafe extern "C" fn dump_active() -> i32 {
        0
    }
    unsafe extern "C" fn dump_frame(_: *mut c_void, _: *const Frame, _: u32) {}

    static SERVICES: Services = Services {
        create,
        select,
        status,
        exposure,
        evaluate,
        warm_up,
        destroy,
        create_resolve,
        resolve,
        depth,
        motion,
        destroy_resolve,
        dump_active,
        dump_frame,
    };

    pub(crate) fn adapter() -> *mut Adapter {
        let setup: rsf_sr_session_setup = unsafe { std::mem::zeroed() };
        let mut out = ptr::null_mut();
        let result = unsafe { rsf_core_sr_legacy_create(&setup, 1.0, 0, 1, &SERVICES, &mut out) };
        assert_eq!(result, 0);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposure_settling_preserves_the_old_policy_on_refusal() {
        let mut exposure = Exposure {
            auto: true,
            mismatch: 0,
            fresh: false,
        };
        for _ in 1..EXPOSURE_SETTLE_FRAMES {
            assert_eq!(
                exposure.update(false, |_| panic!("manual switch must settle")),
                0
            );
        }
        assert_eq!(exposure.update(false, |_| FEATURE_FAILED), FEATURE_FAILED);
        assert!(exposure.auto);
        assert_eq!(
            exposure.update(false, |auto| {
                assert!(!auto);
                0
            }),
            0
        );
        assert!(!exposure.auto);
        assert_eq!(
            exposure.update(true, |auto| {
                assert!(auto);
                0
            }),
            0
        );
        assert!(exposure.auto);
        exposure.fresh = true;
        assert_eq!(
            exposure.update(false, |auto| {
                assert!(!auto);
                0
            }),
            0
        );
        assert!(!exposure.auto);
    }
}
