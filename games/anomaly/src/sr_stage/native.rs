// SPDX-License-Identifier: MIT
//! The production SR leaf table. The C11 leaf (sr_stage_platform.c) lends what Rust cannot express: the guarded copy,
//! the SEH engine calls, the five trampolines, the position-binder replacement and the hook set. This module reads
//! every engine address from the selected build's records (sr_profile.rs, native_profile.rs), hosts the swap owner
//! and answers every other leaf the stage driver calls. The callbacks run under the Rust stage lock, as the former C++
//! leaf did; only the trampolines run outside it.
use super::{Platform, STAGE, rsf_anomaly_sr_stage_configure_ini};
use crate::adapter_options::rsf_anomaly_sr_persist_int;
use crate::adapter_options::rsf_anomaly_sr_persist_text;
use crate::engine_guards::{Image, sr_guard_failure};
use crate::engine_slots::rsf_anomaly_slot_install;
use crate::engine_slots::rsf_anomaly_slot_set;
use crate::host_log::Log;
use crate::ini::rsf_anomaly_sr_ini_int;
use crate::ini::rsf_anomaly_sr_ini_text;
use crate::native_geometry::rsf_anomaly_geometry_active;
use crate::native_geometry::rsf_anomaly_geometry_set_near;
use crate::native_geometry::{CONSTANT_BUFFER_DATA, CONSTANT_BUFFER_SIZE, CONSTANT_BUFFER_SLOTS};
use crate::native_hooks::Site as HookSite;
use crate::native_observer::rsf_anomaly_observation;
use crate::native_profile::{PROFILES as NATIVE, Profile as NativeProfile};
use crate::runtime_module::rsf_anomaly_runtime_set_colour_transport;
use crate::sr_d3d11::native::rsf_anomaly_sr_context_device;
use crate::sr_d3d11::native::rsf_anomaly_sr_copy_subresource;
use crate::sr_d3d11::native::rsf_anomaly_sr_immediate_context;
use crate::sr_d3d11::native::rsf_anomaly_sr_map_read;
use crate::sr_d3d11::native::rsf_anomaly_sr_release;
use crate::sr_d3d11::native::rsf_anomaly_sr_set_full_viewport;
use crate::sr_d3d11::native::rsf_anomaly_sr_timer_clock;
use crate::sr_d3d11::native::rsf_anomaly_sr_timer_create_query;
use crate::sr_d3d11::native::rsf_anomaly_sr_timer_issue;
use crate::sr_d3d11::native::rsf_anomaly_sr_timer_retain_device;
use crate::sr_d3d11::native::rsf_anomaly_sr_timer_timestamp;
use crate::sr_d3d11::native::rsf_anomaly_sr_twin_refs;
use crate::sr_d3d11::native::rsf_anomaly_sr_unmap;
use crate::sr_d3d11::native::rsf_anomaly_sr_view_resource;
use crate::sr_d3d11::passes::rsf_anomaly_sr_run_pass;
use crate::sr_d3d11::passes::rsf_anomaly_sr_temporal_ready;
use crate::sr_d3d11::rsf_anomaly_sr_stage_make_twins;
use crate::sr_d3d11::rsf_anomaly_sr_stage_pass;
use crate::sr_d3d11::rsf_anomaly_sr_stage_reset_pass;
use crate::sr_d3d11::rsf_anomaly_sr_stage_resource_stats;
use crate::sr_d3d11::rsf_anomaly_sr_stage_retire_twins;
use crate::sr_d3d11::targets::TwinBatchInfo;
use crate::sr_d3d11::targets::rsf_anomaly_sr_twin_error_name;
use crate::sr_d3d11::{StageResources, rsf_anomaly_sr_stage_depth};
use crate::sr_profile::{
    BACKEND_XFORMS_OFFSET, CRT_SURFACE_OFFSET, PROFILES as SR, Profile as SrProfile,
    RENDER_TARGET_OFFSET, SAMPLER_MIP_LOD_BIAS_OFFSET, SITE_PHASE_BLUR, SITE_PHASE_COMBINE,
    SITE_PHASE_SCENE_PREPARE, SITE_PHASE_SUNSHAFTS, SITE_POSITION_SETUP, SITE_SET_MIP_LOD_BIAS,
    SITE_SET_P, SITE_SUN_CASCADES, WORLD_TARGETS, rsf_anomaly_sr_engine_layout,
};
use crate::sr_reconstruction::rsf_anomaly_sr_correct_position_constant;
use crate::sr_swap::{self as swap_owner, Handle};

use crate::sys::{RSF_ANOMALY_OBSERVATION_ABI, rsf_anomaly_native_observation};
use crate::sys::{
    RSF_ANOMALY_SWAP_MAKE_CREATED, RSF_ANOMALY_SWAP_MAKE_FAILED, RSF_ANOMALY_SWAP_SLOT_COUNT,
    rsf_anomaly_swap_counters, rsf_anomaly_swap_request, rsf_anomaly_swap_twins,
    rsf_anomaly_swap_view,
};
use crate::sys::{
    rsf_anomaly_sr_camera, rsf_anomaly_sr_controller, rsf_anomaly_sr_frame_facts,
    rsf_anomaly_sr_frame_reads, rsf_anomaly_sr_log, rsf_anomaly_sr_motion_target,
    rsf_anomaly_sr_swap_counters,
};
use rsf_game_sys::rsf_result;
use std::ffi::{CStr, c_char, c_void};
use std::mem::{MaybeUninit, size_of};
use std::ptr::{from_ref, null_mut};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

type Ptr = *mut c_void;

/// The C leaf table (sr_stage_platform.h).
pub(crate) use crate::sys::rsf_anomaly_sr_native as Native;

const _: () = {
    // The seam reads the generic0 CRT the world window swaps.
    assert!(WORLD_TARGETS[0].offset == 0x3a8);
};

/// One admitted executable: the native_profile.rs index and its native and SR records. Stage::install borrows it as
/// the opaque `build` the leaves receive.
pub(crate) struct Build {
    pub(crate) index: usize,
    pub(crate) native: &'static NativeProfile,
    pub(crate) sr: &'static SrProfile,
}

pub(crate) static BUILDS: [Build; 2] = [
    Build {
        index: 0,
        native: &NATIVE[0],
        sr: &SR[0],
    },
    Build {
        index: 1,
        native: &NATIVE[1],
        sr: &SR[1],
    },
];

// ---- engine state --------------------------------------------------------------------------------------------------
static NATIVE_LEAVES: AtomicPtr<Native> = AtomicPtr::new(null_mut());
// The exact build set_engine was given, with its image. Until then every read fails on the null image.
static IMAGE: AtomicUsize = AtomicUsize::new(0);
static BUILD: AtomicPtr<Build> = AtomicPtr::new(null_mut());
// Stage 3 engine functions (byte-guarded at install); zero until a temporal stage installs.
static SET_PROJECTION: AtomicUsize = AtomicUsize::new(0);
static SET_MIP_BIAS: AtomicUsize = AtomicUsize::new(0);
static CONSTANT_BUFFER: AtomicUsize = AtomicUsize::new(0);
// The D3D11 host's resource owner and the swap owner's prepare-time log.
static RESOURCES: AtomicPtr<StageResources> = AtomicPtr::new(null_mut());
static TWIN_LOG: AtomicPtr<Log> = AtomicPtr::new(null_mut());
static SWAP: OnceLock<usize> = OnceLock::new();


fn native() -> &'static Native {
    // Set by bind before the stage table is registered; the table is a static of the C leaf.
    unsafe { &*NATIVE_LEAVES.load(Ordering::Acquire) }
}

fn engine() -> Option<(usize, &'static Build)> {
    let image = IMAGE.load(Ordering::Acquire);
    let build = BUILD.load(Ordering::Acquire);
    // The build is one of BUILDS or a fixture record that outlives the installed stage.
    (image != 0 && !build.is_null()).then(|| (image, unsafe { &*build }))
}

fn sr() -> Option<&'static SrProfile> {
    engine().map(|(_, build)| build.sr)
}

fn read<T: Copy>(rva: u32) -> Option<T> {
    let (image, _) = engine()?;
    let mut value = MaybeUninit::<T>::uninit();
    unsafe {
        (native().copy.unwrap())(
            value.as_mut_ptr().cast(),
            (image + rva as usize) as *const c_void,
            size_of::<T>(),
        )
    }
    .then(|| unsafe { value.assume_init() })
}

fn read_into<T: Copy>(rva: u32, out: *mut T) -> bool {
    match read::<T>(rva) {
        Some(value) => {
            unsafe { out.write(value) };
            true
        }
        None => false,
    }
}

fn address(rva: u32) -> Ptr {
    engine().map_or(null_mut(), |(image, _)| (image + rva as usize) as Ptr)
}

fn swap() -> *mut Handle {
    *SWAP.get_or_init(|| {
        swap_owner::create(
            rsf_anomaly_sr_engine_layout(),
            swap_owner::Platform {
                user: null_mut(),
                read: swap_read,
                write: swap_write,
                make_twins,
                drop_twins,
                set_viewport,
                upload_projection,
                set_mip_bias,
                correct_position_constant,
            },
        ) as usize
    }) as *mut Handle
}

fn resources() -> *mut StageResources {
    RESOURCES.load(Ordering::Acquire)
}

fn device() -> Ptr {
    sr().and_then(|sr| read::<usize>(sr.hw_device_rva))
        .map_or(null_mut(), |value| value as Ptr)
}

fn context() -> Ptr {
    sr().and_then(|sr| read::<usize>(sr.hw_context_rva))
        .map_or(null_mut(), |value| value as Ptr)
}

// ---- the swap owner's host -----------------------------------------------------------------------------------------

unsafe extern "C" fn swap_read(_: Ptr, address: *const c_void, output: Ptr, size: usize) -> bool {
    unsafe { (native().copy.unwrap())(output, address, size) }
}

unsafe extern "C" fn swap_write(_: Ptr, address: Ptr, input: *const c_void, size: usize) -> bool {
    unsafe { (native().copy.unwrap())(address, input, size) }
}

unsafe extern "C" fn make_twins(
    _: Ptr,
    request: *const rsf_anomaly_swap_request,
    out: *mut rsf_anomaly_swap_twins,
) -> u32 {
    let borrowed = TWIN_LOG.load(Ordering::Acquire);
    // The prepare leaf lends its log for the duration of the swap owner's call.
    let log = unsafe { borrowed.as_ref() }.copied().unwrap_or_default();
    let request = unsafe { &*request };
    let d3d = device();
    if d3d.is_null() {
        log.say("Anomaly SR stage: HW.pDevice is null; twins not created.");
        return RSF_ANOMALY_SWAP_MAKE_FAILED;
    }
    let mut info = TwinBatchInfo::default();
    let mut handle = resources();
    let result = rsf_anomaly_sr_stage_make_twins(
        &mut handle,
        unsafe { &*(native().d3d11.unwrap())().cast() },
        d3d,
        request,
        unsafe { &mut *out },
        &mut info,
        log.callback,
        log.user as Ptr,
    );
    RESOURCES.store(handle, Ordering::Release);
    if info.pass_refused {
        log.say(if request.stage == 1 {
            "Anomaly SR stage: round-trip shader could not be created."
        } else {
            "Anomaly SR stage: upscale shader could not be created."
        });
        return result;
    }
    if result != RSF_ANOMALY_SWAP_MAKE_CREATED {
        let error = unsafe { CStr::from_ptr(rsf_anomaly_sr_twin_error_name(info.cause as u8)) }
            .to_string_lossy();
        let text = if info.failed_index < RSF_ANOMALY_SWAP_SLOT_COUNT {
            let i = info.failed_index as usize;
            let spec = &request.spec[i];
            format!(
                "Anomaly SR stage: twin of {} refused ({error}) at {}x{}; this frame stays stock.",
                WORLD_TARGETS[i].name.to_string_lossy(),
                spec.width,
                spec.height
            )
        } else if info.failed_index == RSF_ANOMALY_SWAP_SLOT_COUNT {
            format!(
                "Anomaly SR stage: depth twin refused ({error}) at {}x{}.",
                request.render_width, request.render_height
            )
        } else {
            format!(
                "Anomaly SR stage: HW.pBaseRT stand-in refused ({error}) at {}x{}.",
                request.render_width, request.render_height
            )
        };
        log.say(&text);
        return result;
    }
    let out = unsafe { &*out };
    let text = if request.stage == 1 {
        format!(
            "Anomaly SR stage: FP16 twins created for generic0/generic_temp at {}x{} (uav={}/{}).",
            out.targets[0].width,
            out.targets[0].height,
            i32::from(!out.targets[0].uav.is_null()),
            i32::from(!out.targets[1].uav.is_null())
        )
    } else {
        format!(
            "Anomaly SR stage 2: {} world twins, a depth twin and a HW.pBaseRT stand-in created at render {}x{} for output {}x{} ({:.1} MiB).",
            info.made,
            request.render_width,
            request.render_height,
            request.output_width,
            request.output_height,
            info.bytes as f64 / (1024.0 * 1024.0)
        )
    };
    log.say(&text);
    RSF_ANOMALY_SWAP_MAKE_CREATED
}

unsafe extern "C" fn drop_twins(_: Ptr) {
    // Reference counts after our own views are gone. The engine's surface_get() before its generic_temp copy takes
    // references it never releases (stock leaks them on its own targets too), so this is diagnostic only.
    rsf_anomaly_sr_stage_retire_twins(unsafe { resources().as_mut() }, rsf_anomaly_sr_twin_refs);
}

unsafe extern "C" fn set_viewport(_: Ptr, width: u32, height: u32) {
    unsafe { rsf_anomaly_sr_set_full_viewport(context(), width, height) };
}

unsafe extern "C" fn upload_projection(_: Ptr, matrix: *const c_void) -> bool {
    let function = SET_PROJECTION.load(Ordering::Acquire);
    let Some(sr) = sr().filter(|_| function != 0) else {
        return false;
    };
    unsafe {
        (native().set_projection.unwrap())(
            function as Ptr,
            address(sr.backend_rva + BACKEND_XFORMS_OFFSET),
            matrix,
        )
    }
}

unsafe extern "C" fn set_mip_bias(_: Ptr, value: f32) -> bool {
    let function = SET_MIP_BIAS.load(Ordering::Acquire);
    let Some(sr) = sr().filter(|_| function != 0) else {
        return false;
    };
    unsafe {
        (native().set_mip_bias.unwrap())(function as Ptr, address(sr.sampler_cache_rva), value)
    }
}

// Constant-buffer fields used after the native binder has written the stock vector.
static POSITION_LAYOUT: crate::sr_reconstruction::Layout = crate::sr_reconstruction::Layout {
    destination: 0x10,
    load: 0x14,
    buffer_size: CONSTANT_BUFFER_SIZE,
    buffer_data: CONSTANT_BUFFER_DATA,
    buffer_changed: 0x68,
    buffer_slots: CONSTANT_BUFFER_SLOTS,
    stage_bits: [1, 2, 8, 16, 32, 64],
    stage_shifts: [16, 12, 8, 20, 24, 28],
};

/// The engine memory and constant-buffer leaves of position correction (sr_reconstruction.rs).
#[cfg(not(test))]
pub(crate) mod position {
    use super::*;

    // RCache.constants (R_constants) inside CBackend.
    const BACKEND_CONSTANTS_OFFSET: u32 = 0x5c0;

    pub(crate) unsafe fn read(address: usize, out: Ptr, size: usize) -> bool {
        unsafe { (native().copy.unwrap())(out, address as *const c_void, size) }
    }

    pub(crate) unsafe fn write(address: usize, input: *const c_void, size: usize) -> bool {
        unsafe { (native().copy.unwrap())(address as Ptr, input, size) }
    }

    pub(crate) unsafe fn buffer(constant: usize, stage: u32) -> usize {
        let function = CONSTANT_BUFFER.load(Ordering::Acquire);
        let Some(sr) = sr().filter(|_| function != 0) else {
            return 0;
        };
        unsafe {
            (native().constant_buffer.unwrap())(
                function as Ptr,
                address(sr.backend_rva + BACKEND_CONSTANTS_OFFSET),
                constant,
                stage,
            )
        }
    }
}

unsafe extern "C" fn correct_position_constant(_: Ptr, constant: Ptr, x: f32, y: f32) -> u32 {
    rsf_anomaly_sr_correct_position_constant(&POSITION_LAYOUT, constant as usize, x, y)
}

// ---- the stage leaves ----------------------------------------------------------------------------------------------
unsafe extern "C" fn render_target() -> Ptr {
    sr().and_then(|sr| read::<usize>(sr.render_implementation_rva + RENDER_TARGET_OFFSET))
        .map_or(null_mut(), |target| target as Ptr)
}

unsafe extern "C" fn frame_facts(
    world_window: bool,
    temporal: bool,
    reads: *mut rsf_anomaly_sr_frame_reads,
) {
    let reads = unsafe { &mut *reads };
    *reads = rsf_anomaly_sr_frame_reads::default();
    let Some((image, build)) = engine() else {
        return;
    };
    let (sr, native_profile) = (build.sr, build.native);
    let out = &mut reads.facts;
    out.backend = address(sr.backend_rva);
    read_into(sr.hw_device_rva, &mut out.device);
    let mut observation = rsf_anomaly_native_observation {
        struct_size: size_of::<rsf_anomaly_native_observation>() as u32,
        abi_version: RSF_ANOMALY_OBSERVATION_ABI,
        ..Default::default()
    };
    const OK: rsf_result = 0; // RSF_OK
    if unsafe { rsf_anomaly_observation(&mut observation) } == OK {
        out.generation = observation.resets;
    }
    read_into(native_profile.device_width_rva, &mut out.device_width);
    read_into(native_profile.device_height_rva, &mut out.device_height);
    const UNSET: u8 = 0xff;
    let view = unsafe { (native().current_view.unwrap())(image as *const c_void, build.index) };
    reads.view_read = view != UNSET;
    if view != UNSET {
        reads.render_view = u32::from(view);
    }
    reads.options_read = read_into(sr.render_options_word3_rva, &mut reads.options_word3);
    let out = &mut reads.facts;
    if world_window {
        out.device_size = address(native_profile.device_width_rva);
        out.base_zb = address(sr.hw_base_zb_rva);
        out.base_rt = address(sr.hw_base_rt_rva);
        out.ssao_viewport = address(sr.ssao_viewport_rva);
        out.ssao_viewport_guard = address(sr.ssao_viewport_guard_rva);
    }
    if temporal {
        out.device_view = address(sr.device_view_rva);
        out.device_project = address(sr.device_project_rva);
        out.device_full_transform = address(sr.device_full_transform_rva);
        out.sampler_bias = address(sr.sampler_cache_rva + SAMPLER_MIP_LOD_BIAS_OFFSET);
    }
}

unsafe extern "C" fn loading_registered(registered: *mut u8) -> bool {
    sr().is_some_and(|sr| read_into(sr.load_screen_registered_rva, registered))
}

unsafe extern "C" fn precache_frame(frame: *mut u32) -> bool {
    sr().is_some_and(|sr| read_into(sr.device_precache_frame_rva, frame))
}

unsafe extern "C" fn engine_frame() -> u32 {
    engine()
        .and_then(|(_, build)| read::<u32>(build.native.device_frame_rva))
        .unwrap_or(0)
}

unsafe extern "C" fn device_size(width: *mut u32, height: *mut u32) {
    if let Some((_, build)) = engine() {
        read_into(build.native.device_width_rva, width);
        read_into(build.native.device_height_rva, height);
    }
}

unsafe extern "C" fn read_camera(out: *mut rsf_anomaly_sr_camera) -> bool {
    let out = unsafe { &mut *out };
    sr().is_some_and(|sr| {
        read_into(sr.device_view_rva, &mut out.view)
            && read_into(sr.device_project_rva, &mut out.projection)
    })
}

unsafe extern "C" fn seam_surface(target: Ptr) -> Ptr {
    let copy = native().copy.unwrap();
    let mut crt = 0usize;
    let mut surface = 0usize;
    let crt_field = (target as usize).wrapping_add(WORLD_TARGETS[0].offset as usize);
    if unsafe {
        copy(
            (&raw mut crt).cast(),
            crt_field as *const c_void,
            size_of::<usize>(),
        )
    } && crt != 0
        && unsafe {
            copy(
                (&raw mut surface).cast(),
                (crt + CRT_SURFACE_OFFSET as usize) as *const c_void,
                size_of::<usize>(),
            )
        }
    {
        return surface as Ptr;
    }
    null_mut()
}

unsafe extern "C" fn back_buffer() -> Ptr {
    let base = sr()
        .and_then(|sr| read::<usize>(sr.hw_base_rt_rva))
        .unwrap_or(0);
    if base == 0 {
        return null_mut();
    }
    unsafe { rsf_anomaly_sr_view_resource(base as Ptr) }
}

unsafe extern "C" fn stage_context() -> Ptr {
    context()
}

unsafe extern "C" fn stage_device() -> Ptr {
    device()
}

unsafe extern "C" fn now(counter: *mut i64, frequency: *mut i64) {
    let (mut value, mut rate) = (0i64, 0i64);
    unsafe {
        QueryPerformanceCounter(&mut value);
        QueryPerformanceFrequency(&mut rate);
        counter.write(value);
        frequency.write(rate);
    }
}

unsafe extern "C" fn geometry_active() -> bool {
    rsf_anomaly_geometry_active()
}

unsafe extern "C" fn build_sha256() -> *const c_char {
    engine().map_or(c"".as_ptr(), |(_, build)| build.native.sha256.as_ptr())
}

unsafe extern "C" fn close_stale() {
    swap_owner::close_stale(swap());
}

unsafe extern "C" fn prepare(
    facts: *const rsf_anomaly_sr_frame_facts,
    log: rsf_anomaly_sr_log,
    user: Ptr,
) -> bool {
    let mut call_log = Log {
        callback: log,
        user: user as usize,
    };
    let previous = TWIN_LOG.swap(&raw mut call_log, Ordering::AcqRel);
    let prepared = swap_owner::prepare(swap(), unsafe { facts.as_ref() });
    TWIN_LOG.store(previous, Ordering::Release);
    prepared
}

unsafe extern "C" fn prepared() {
    swap_owner::prepared(swap());
}

unsafe extern "C" fn swap_handle() -> Ptr {
    swap().cast()
}

unsafe extern "C" fn combine_end() {
    swap_owner::combine_end(swap());
}

unsafe extern "C" fn frame_end() {
    swap_owner::frame_end(swap());
}

unsafe extern "C" fn rm_near() {
    swap_owner::rm_near(swap());
}

unsafe extern "C" fn sun_begin() -> bool {
    swap_owner::sun_begin(swap())
}

unsafe extern "C" fn sun_end(suspended: bool) {
    swap_owner::sun_end(swap(), suspended);
}

unsafe extern "C" fn position_setup(constant: Ptr) {
    swap_owner::position_constant(swap(), constant);
}

unsafe extern "C" fn shutdown() -> bool {
    swap_owner::shutdown(swap())
}

unsafe extern "C" fn set_swap_enabled(enabled: bool) {
    swap_owner::set_enabled(swap(), enabled);
}

unsafe extern "C" fn controller(out: *mut rsf_anomaly_sr_controller) {
    let handle = swap();
    let view = if handle.is_null() {
        rsf_anomaly_swap_view {
            refusal: c"The SR swap owner could not be created.".as_ptr(),
            ..Default::default()
        }
    } else {
        unsafe { *swap_owner::view_of(handle) }
    };
    unsafe {
        out.write(rsf_anomaly_sr_controller {
            window_open: view.open,
            refused: !view.refusal.is_null(),
            jitter_applied: view.jitter_applied,
            mip_bias_applied: view.mip_bias_applied,
            stage: view.stage,
            render_width: view.render_width,
            render_height: view.render_height,
            window_mip_bias: view.window_mip_bias,
            saved_mip_bias: view.saved_mip_bias,
            twin_generation: view.counters.twin_generations,
            refusal: view.refusal,
        })
    };
}

unsafe extern "C" fn swap_counters(out: *mut rsf_anomaly_sr_swap_counters) {
    let handle = swap();
    let counters = if handle.is_null() {
        rsf_anomaly_swap_counters::default()
    } else {
        unsafe { (*swap_owner::view_of(handle)).counters }
    };
    unsafe { out.write(counters) };
}

unsafe extern "C" fn host_stats(bytes: *mut u64, refs: *mut u64) {
    rsf_anomaly_sr_stage_resource_stats(
        unsafe { resources().as_ref() },
        unsafe { &mut *bytes },
        unsafe { &mut *refs },
    );
}

unsafe extern "C" fn host_reset() {
    rsf_anomaly_sr_stage_reset_pass(unsafe { resources().as_mut() });
}

unsafe extern "C" fn pixel_bytes(format: u32) -> u32 {
    unsafe { (native().pixel_bytes.unwrap())(format) }
}

unsafe extern "C" fn draw_pass(kind: u32, views: *const crate::sr_d3d11::DrawViews) -> bool {
    let pass = rsf_anomaly_sr_stage_pass(unsafe { resources().as_ref() });
    rsf_anomaly_sr_run_pass(unsafe { pass.as_ref() }, context(), kind, unsafe {
        &*views
    }) != 0
}

unsafe extern "C" fn temporal_ready() -> bool {
    let resources = unsafe { resources().as_ref() };
    let pass = rsf_anomaly_sr_stage_pass(resources);
    rsf_anomaly_sr_temporal_ready(unsafe { pass.as_ref() }) != 0
        && !rsf_anomaly_sr_stage_depth(resources).is_null()
}

unsafe extern "C" fn temporal_platform() -> *const c_void {
    unsafe { (native().d3d11.unwrap())() }
}

unsafe extern "C" fn motion_target(out: *mut rsf_anomaly_sr_motion_target) -> bool {
    let target = crate::motion::stage::frame_target();
    unsafe {
        out.write(rsf_anomaly_sr_motion_target {
            texture: target.texture,
            written: target.written,
            main: target.main,
            frame: target.frame,
            width: target.width,
            height: target.height,
        })
    };
    !target.texture.is_null()
}

unsafe extern "C" fn create_motion_view(device: Ptr, texture: Ptr) -> Ptr {
    unsafe { (native().create_view.unwrap())(device, texture) }
}

unsafe extern "C" fn depth_texture() -> Ptr {
    rsf_anomaly_sr_stage_depth(unsafe { resources().as_ref() })
}

unsafe extern "C" fn verify_guards(
    module: Ptr,
    image_size: u32,
    build: *const c_void,
    world_window: bool,
    temporal: bool,
) -> *const c_char {
    // Stage::install lends the build record it was given.
    let build = unsafe { &*build.cast::<Build>() };
    let image = Image::new(
        module.cast_const().cast(),
        image_size,
        native().copy.unwrap(),
    );
    sr_guard_failure(&image, build.sr, world_window, temporal)
}

unsafe extern "C" fn hooks_created() -> bool {
    unsafe { (native().hooks_created.unwrap())() }
}

unsafe extern "C" fn set_engine(module: Ptr, build: *const c_void, temporal: bool) {
    let image = module as usize;
    BUILD.store(build.cast_mut().cast(), Ordering::Release);
    IMAGE.store(image, Ordering::Release);
    if !temporal {
        return;
    }
    // Stage::install lends the build record it was given.
    let sr = unsafe { &*build.cast::<Build>() }.sr;
    let at = |rva: u32| image.wrapping_add(rva as usize);
    CONSTANT_BUFFER.store(at(sr.get_constant_buffer_rva), Ordering::Release);
    SET_PROJECTION.store(at(sr.function_sites[SITE_SET_P].rva), Ordering::Release);
    SET_MIP_BIAS.store(
        at(sr.function_sites[SITE_SET_MIP_LOD_BIAS].rva),
        Ordering::Release,
    );
}

// The stage detours in the leaf's trampoline order (sr_stage_platform.h).
const DETOURS: [usize; crate::sys::RSF_ANOMALY_SR_DETOUR_COUNT as usize] = [
    SITE_PHASE_SCENE_PREPARE,
    SITE_PHASE_COMBINE,
    SITE_PHASE_SUNSHAFTS,
    SITE_PHASE_BLUR,
    SITE_SUN_CASCADES,
];

unsafe extern "C" fn install_hooks() -> bool {
    let leaves = native();
    if unsafe { (leaves.hooks_created.unwrap())() } {
        return unsafe { (leaves.enable_hooks.unwrap())() };
    }
    let Some((image, build)) = engine() else {
        return false;
    };
    let mut sites = DETOURS.map(|index| {
        let site = &build.sr.function_sites[index];
        // The leaf fills `replacement` and `original`.
        HookSite {
            _name: site.name,
            rva: site.rva,
            expected: site.expected,
            replacement: null_mut(),
            original: null_mut(),
        }
    });
    unsafe {
        (leaves.install_hooks.unwrap())(
            image as Ptr,
            build.native.image_size,
            // The header only forward-declares rsf_anomaly_hook_site; native_hooks::Site is its layout (48 bytes, checked
            // on both sides).
            sites.as_mut_ptr().cast(),
            sites.len(),
        )
    }
}

const POSITION_SLOT: u32 = 1; // RSF_ANOMALY_POSITION_SLOT (engine_slots.h)

unsafe extern "C" fn install_position_slot() -> bool {
    let Some((image, build)) = engine() else {
        return false;
    };
    let leaves = native();
    let site = &build.sr.function_sites[SITE_POSITION_SETUP];
    let checked = Image::new(
        image as *const u8,
        build.native.image_size,
        leaves.copy.unwrap(),
    );
    // The slot must hold the profiled function, and the function its guard.
    checked.matches(site) && {
        rsf_anomaly_slot_install(
            POSITION_SLOT,
            &checked,
            build.sr.position_setup_vtable_rva,
            site.rva,
            0,
            leaves.position_setup,
            leaves.compare_slot.unwrap(),
        )
    }
}

unsafe extern "C" fn disable_hooks() {
    unsafe {
        let _ = rsf_anomaly_slot_set(POSITION_SLOT, false);
        let _ = (native().disable_hooks.unwrap())();
    }
}

unsafe extern "C" fn rm_near_callback(_: Ptr) {
    STAGE.rm_near();
}

unsafe extern "C" fn set_rm_near(on: bool) {
    rsf_anomaly_geometry_set_near(on.then_some(rm_near_callback as unsafe extern "C" fn(Ptr)));
}

unsafe extern "C" fn remove_hooks() -> bool {
    unsafe {
        rsf_anomaly_geometry_set_near(None);
        let restored = rsf_anomaly_slot_set(POSITION_SLOT, false);
        let disabled = (native().disable_hooks.unwrap())();
        restored && disabled
    }
}

unsafe extern "C" fn apply_colour_transport(enabled: bool) {
    rsf_anomaly_runtime_set_colour_transport(enabled);
}

fn leaves() -> Platform {
    Platform {
        render_target: Some(render_target),
        frame_facts: Some(frame_facts),
        loading_registered: Some(loading_registered),
        precache_frame: Some(precache_frame),
        engine_frame: Some(engine_frame),
        device_size: Some(device_size),
        read_camera: Some(read_camera),
        seam_surface: Some(seam_surface),
        back_buffer: Some(back_buffer),
        context: Some(stage_context),
        device: Some(stage_device),
        now: Some(now),
        geometry_active: Some(geometry_active),
        build_sha256: Some(build_sha256),
        close_stale: Some(close_stale),
        prepare: Some(prepare),
        prepared: Some(prepared),
        swap_handle: Some(swap_handle),
        combine_end: Some(combine_end),
        frame_end: Some(frame_end),
        rm_near: Some(rm_near),
        sun_begin: Some(sun_begin),
        sun_end: Some(sun_end),
        position_setup: Some(position_setup),
        shutdown: Some(shutdown),
        set_swap_enabled: Some(set_swap_enabled),
        controller: Some(controller),
        swap_counters: Some(swap_counters),
        timer_retain_device: Some(rsf_anomaly_sr_timer_retain_device),
        timer_create_query: Some(rsf_anomaly_sr_timer_create_query),
        timer_issue: Some(rsf_anomaly_sr_timer_issue),
        timer_clock: Some(rsf_anomaly_sr_timer_clock),
        timer_timestamp: Some(rsf_anomaly_sr_timer_timestamp),
        host_stats: Some(host_stats),
        host_reset: Some(host_reset),
        immediate_context: Some(rsf_anomaly_sr_immediate_context),
        context_device: Some(rsf_anomaly_sr_context_device),
        copy_subresource: Some(rsf_anomaly_sr_copy_subresource),
        release: Some(rsf_anomaly_sr_release),
        map_read: Some(rsf_anomaly_sr_map_read),
        unmap: Some(rsf_anomaly_sr_unmap),
        pixel_bytes: Some(pixel_bytes),
        draw_pass: Some(draw_pass),
        temporal_ready: Some(temporal_ready),
        temporal_platform: Some(temporal_platform),
        motion_target: Some(motion_target),
        create_motion_view: Some(create_motion_view),
        depth_texture: Some(depth_texture),
        // stage_bridge::production connects these to the Rust temporal driver.
        driver_bind: None,
        driver_enabled: None,
        driver_output_format: None,
        driver_planned: None,
        driver_boundary: None,
        driver_evaluate: None,
        driver_request_dump: None,
        driver_fallback: None,
        driver_count_prepared: None,
        driver_stop: None,
        driver_report: None,
        driver_overlay_report: None,
        driver_request: None,
        verify_guards: Some(verify_guards),
        hooks_created: Some(hooks_created),
        set_engine: Some(set_engine),
        install_hooks: Some(install_hooks),
        install_position_slot: Some(install_position_slot),
        disable_hooks: Some(disable_hooks),
        set_rm_near: Some(set_rm_near),
        remove_hooks: Some(remove_hooks),
        ini_int: Some(rsf_anomaly_sr_ini_int),
        ini_text: Some(rsf_anomaly_sr_ini_text),
        persist_int: Some(rsf_anomaly_sr_persist_int),
        persist_text: Some(rsf_anomaly_sr_persist_text),
        apply_colour_transport: Some(apply_colour_transport),
        configured: None,
    }
}

/// Adopts the leaf table and registers the production stage table built over it.
fn bind(leaves_table: &'static Native) {
    NATIVE_LEAVES.store(from_ref(leaves_table).cast_mut(), Ordering::Release);
    STAGE.register(crate::sr_temporal::stage_bridge::production(leaves()));
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_anomaly_sr_stage_configure_native(
    leaves_table: &'static Native,
    ini: *const u16,
    apply: bool,
    log: rsf_anomaly_sr_log,
    user: Ptr,
) -> bool {
    bind(leaves_table);
    unsafe { rsf_anomaly_sr_stage_configure_ini(ini, apply, log, user) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_install_build(
    leaves_table: &'static Native,
    image: Ptr,
    build: usize,
) -> bool {
    let Some(selected) = BUILDS.get(build) else {
        return false;
    };
    let size = selected.native.image_size;
    bind(leaves_table);
    STAGE.install(image, size, size, from_ref(selected).cast())
}

#[cfg(feature = "native-entry")]
pub(crate) fn rsf_anomaly_sr_depth_rva(build: usize) -> u32 {
    SR.get(build).map_or(0, |profile| profile.hw_base_zb_rva)
}
