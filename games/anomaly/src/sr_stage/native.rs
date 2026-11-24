// SPDX-License-Identifier: MIT
//! The production SR leaves. The C11 leaf (sr_stage_platform.c) lends what Rust cannot express: the guarded copy,
//! the SEH engine calls, the five trampolines, the position-binder replacement and the hook set. This module reads
//! every engine address from the selected build's records (sr_profile.rs, native_profile.rs), hosts the swap owner
//! and answers every other leaf the stage driver calls. The callbacks run under the Rust stage lock, as the former C++
//! leaf did; only the trampolines run outside it. Under test the stage calls fakes, so this module is compiled but
//! unused.
#![cfg_attr(test, allow(dead_code))]
use super::{STAGE, rsf_anomaly_sr_stage_configure_ini};
use crate::engine_guards::{Image, sr_guard_failure};
use crate::engine_slots::rsf_anomaly_slot_set;
use crate::engine_slots::slot_install;
use crate::host_log::Log;
use crate::native_geometry::rsf_anomaly_geometry_active;
use crate::native_geometry::rsf_anomaly_geometry_set_near;
use crate::native_geometry::{CONSTANT_BUFFER_DATA, CONSTANT_BUFFER_SIZE, CONSTANT_BUFFER_SLOTS};
use crate::native_hooks::Site as HookSite;
use crate::native_profile::{PROFILES as NATIVE, Profile as NativeProfile};
use crate::runtime_module::rsf_anomaly_runtime_set_colour_transport;
use crate::sr_d3d11::native::rsf_anomaly_sr_set_full_viewport;
use crate::sr_d3d11::native::rsf_anomaly_sr_twin_refs;
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
use std::ffi::{CStr, c_char, c_void};
use std::mem::{MaybeUninit, size_of};
use std::ptr::{from_ref, null_mut};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

type Ptr = *mut c_void;

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
        c::copy(
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
    *SWAP.get_or_init(|| swap_owner::create(rsf_anomaly_sr_engine_layout(), null_mut()) as usize)
        as *mut Handle
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
/// The swap owner's host (sr_swap's `host`); the context argument is the owner's null host pointer.
#[cfg(not(test))]
pub(crate) mod swap_host {
    pub(crate) use super::{
        correct_position_constant, drop_twins, make_twins, set_mip_bias, set_viewport,
        swap_read as read, swap_write as write, upload_projection,
    };
}

pub(crate) unsafe fn swap_read(_: Ptr, address: *const c_void, output: Ptr, size: usize) -> bool {
    unsafe { c::copy(output, address, size) }
}

pub(crate) unsafe fn swap_write(_: Ptr, address: Ptr, input: *const c_void, size: usize) -> bool {
    unsafe { c::copy(address, input, size) }
}

pub(crate) unsafe fn make_twins(
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

pub(crate) unsafe fn drop_twins(_: Ptr) {
    // Reference counts after our own views are gone. The engine's surface_get() before its generic_temp copy takes
    // references it never releases (stock leaks them on its own targets too), so this is diagnostic only.
    rsf_anomaly_sr_stage_retire_twins(unsafe { resources().as_mut() }, rsf_anomaly_sr_twin_refs);
}

pub(crate) unsafe fn set_viewport(_: Ptr, width: u32, height: u32) {
    unsafe { rsf_anomaly_sr_set_full_viewport(context(), width, height) };
}

pub(crate) unsafe fn upload_projection(_: Ptr, matrix: *const c_void) -> bool {
    let function = SET_PROJECTION.load(Ordering::Acquire);
    let Some(sr) = sr().filter(|_| function != 0) else {
        return false;
    };
    unsafe {
        c::set_projection(
            function as Ptr,
            address(sr.backend_rva + BACKEND_XFORMS_OFFSET),
            matrix,
        )
    }
}

pub(crate) unsafe fn set_mip_bias(_: Ptr, value: f32) -> bool {
    let function = SET_MIP_BIAS.load(Ordering::Acquire);
    let Some(sr) = sr().filter(|_| function != 0) else {
        return false;
    };
    unsafe { c::set_mip_bias(function as Ptr, address(sr.sampler_cache_rva), value) }
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
        unsafe { c::copy(out, address as *const c_void, size) }
    }

    pub(crate) unsafe fn write(address: usize, input: *const c_void, size: usize) -> bool {
        unsafe { c::copy(address as Ptr, input, size) }
    }

    pub(crate) unsafe fn buffer(constant: usize, stage: u32) -> usize {
        let function = CONSTANT_BUFFER.load(Ordering::Acquire);
        let Some(sr) = sr().filter(|_| function != 0) else {
            return 0;
        };
        unsafe {
            c::constant_buffer(
                function as Ptr,
                address(sr.backend_rva + BACKEND_CONSTANTS_OFFSET),
                constant,
                stage,
            )
        }
    }
}

pub(crate) unsafe fn correct_position_constant(_: Ptr, constant: Ptr, x: f32, y: f32) -> u32 {
    rsf_anomaly_sr_correct_position_constant(&POSITION_LAYOUT, constant as usize, x, y)
}

// ---- the stage leaves ----------------------------------------------------------------------------------------------
pub(crate) unsafe fn render_target() -> Ptr {
    sr().and_then(|sr| read::<usize>(sr.render_implementation_rva + RENDER_TARGET_OFFSET))
        .map_or(null_mut(), |target| target as Ptr)
}

pub(crate) fn frame_facts(world_window: bool, temporal: bool) -> rsf_anomaly_sr_frame_reads {
    let mut reads = rsf_anomaly_sr_frame_reads::default();
    let Some((image, build)) = engine() else {
        return reads;
    };
    let (sr, native_profile) = (build.sr, build.native);
    let out = &mut reads.facts;
    out.backend = address(sr.backend_rva);
    read_into(sr.hw_device_rva, &mut out.device);
    out.generation = crate::native_observer::resets();
    read_into(native_profile.device_width_rva, &mut out.device_width);
    read_into(native_profile.device_height_rva, &mut out.device_height);
    const UNSET: u8 = 0xff;
    let view = unsafe { c::current_view(image as *const c_void, build.index) };
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
    reads
}

pub(crate) fn loading_registered() -> Option<u8> {
    read(sr()?.load_screen_registered_rva)
}

pub(crate) fn precache_frame() -> Option<u32> {
    read(sr()?.device_precache_frame_rva)
}

pub(crate) unsafe fn engine_frame() -> u32 {
    engine()
        .and_then(|(_, build)| read::<u32>(build.native.device_frame_rva))
        .unwrap_or(0)
}

pub(crate) fn device_size() -> (u32, u32) {
    let Some((_, build)) = engine() else {
        return (0, 0);
    };
    (
        read(build.native.device_width_rva).unwrap_or(0),
        read(build.native.device_height_rva).unwrap_or(0),
    )
}

pub(crate) unsafe fn read_camera(out: *mut rsf_anomaly_sr_camera) -> bool {
    let out = unsafe { &mut *out };
    sr().is_some_and(|sr| {
        read_into(sr.device_view_rva, &mut out.view)
            && read_into(sr.device_project_rva, &mut out.projection)
    })
}

pub(crate) unsafe fn seam_surface(target: Ptr) -> Ptr {
    let copy = c::copy;
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

pub(crate) unsafe fn back_buffer() -> Ptr {
    let base = sr()
        .and_then(|sr| read::<usize>(sr.hw_base_rt_rva))
        .unwrap_or(0);
    if base == 0 {
        return null_mut();
    }
    unsafe { rsf_anomaly_sr_view_resource(base as Ptr) }
}

pub(crate) unsafe fn stage_context() -> Ptr {
    context()
}

pub(crate) unsafe fn stage_device() -> Ptr {
    device()
}

pub(crate) fn now() -> (i64, i64) {
    let (mut value, mut rate) = (0i64, 0i64);
    unsafe {
        QueryPerformanceCounter(&mut value);
        QueryPerformanceFrequency(&mut rate);
    }
    (value, rate)
}

pub(crate) unsafe fn geometry_active() -> bool {
    rsf_anomaly_geometry_active()
}

pub(crate) unsafe fn build_sha256() -> *const c_char {
    engine().map_or(c"".as_ptr(), |(_, build)| build.native.sha256.as_ptr())
}

pub(crate) unsafe fn close_stale() {
    swap_owner::close_stale(swap());
}

pub(crate) unsafe fn prepare(
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

pub(crate) unsafe fn prepared() {
    swap_owner::prepared(swap());
}

pub(crate) unsafe fn swap_handle() -> Ptr {
    swap().cast()
}

pub(crate) unsafe fn combine_end() {
    swap_owner::combine_end(swap());
}

pub(crate) unsafe fn frame_end() {
    swap_owner::frame_end(swap());
}

pub(crate) unsafe fn rm_near() {
    swap_owner::rm_near(swap());
}

pub(crate) unsafe fn sun_begin() -> bool {
    swap_owner::sun_begin(swap())
}

pub(crate) unsafe fn sun_end(suspended: bool) {
    swap_owner::sun_end(swap(), suspended);
}

pub(crate) unsafe fn position_setup(constant: Ptr) {
    swap_owner::position_constant(swap(), constant);
}

pub(crate) unsafe fn shutdown() -> bool {
    swap_owner::shutdown(swap())
}

pub(crate) unsafe fn set_swap_enabled(enabled: bool) {
    swap_owner::set_enabled(swap(), enabled);
}

pub(crate) fn controller() -> rsf_anomaly_sr_controller {
    let handle = swap();
    let view = if handle.is_null() {
        rsf_anomaly_swap_view {
            refusal: c"The SR swap owner could not be created.".as_ptr(),
            ..Default::default()
        }
    } else {
        unsafe { *swap_owner::view_of(handle) }
    };
    rsf_anomaly_sr_controller {
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
    }
}

pub(crate) fn swap_counters() -> rsf_anomaly_sr_swap_counters {
    let handle = swap();
    if handle.is_null() {
        rsf_anomaly_swap_counters::default()
    } else {
        unsafe { (*swap_owner::view_of(handle)).counters }
    }
}

pub(crate) fn host_stats() -> (u64, u64) {
    let (mut bytes, mut refs) = (0, 0);
    rsf_anomaly_sr_stage_resource_stats(unsafe { resources().as_ref() }, &mut bytes, &mut refs);
    (bytes, refs)
}

pub(crate) unsafe fn host_reset() {
    rsf_anomaly_sr_stage_reset_pass(unsafe { resources().as_mut() });
}

pub(crate) unsafe fn pixel_bytes(format: u32) -> u32 {
    unsafe { c::pixel_bytes(format) }
}

pub(crate) unsafe fn draw_pass(kind: u32, views: *const crate::sr_d3d11::DrawViews) -> bool {
    let pass = rsf_anomaly_sr_stage_pass(unsafe { resources().as_ref() });
    rsf_anomaly_sr_run_pass(unsafe { pass.as_ref() }, context(), kind, unsafe {
        &*views
    }) != 0
}

pub(crate) unsafe fn temporal_ready() -> bool {
    let resources = unsafe { resources().as_ref() };
    let pass = rsf_anomaly_sr_stage_pass(resources);
    rsf_anomaly_sr_temporal_ready(unsafe { pass.as_ref() }) != 0
        && !rsf_anomaly_sr_stage_depth(resources).is_null()
}

pub(crate) fn motion_target() -> rsf_anomaly_sr_motion_target {
    let target = crate::motion::stage::frame_target();
    rsf_anomaly_sr_motion_target {
        texture: target.texture,
        written: target.written,
        main: target.main,
        frame: target.frame,
        width: target.width,
        height: target.height,
    }
}

pub(crate) unsafe fn create_motion_view(device: Ptr, texture: Ptr) -> Ptr {
    unsafe { c::create_view(device, texture) }
}

pub(crate) unsafe fn depth_texture() -> Ptr {
    rsf_anomaly_sr_stage_depth(unsafe { resources().as_ref() })
}

pub(crate) unsafe fn verify_guards(
    module: Ptr,
    image_size: u32,
    build: *const c_void,
    world_window: bool,
    temporal: bool,
) -> Option<&'static CStr> {
    // Stage::install lends the build record it was given.
    let build = unsafe { &*build.cast::<Build>() };
    let image = Image::new(module.cast_const().cast(), image_size, c::copy);
    sr_guard_failure(&image, build.sr, world_window, temporal)
}

pub(crate) unsafe fn hooks_created() -> bool {
    unsafe { c::hooks_created() }
}

pub(crate) unsafe fn set_engine(module: Ptr, build: *const c_void, temporal: bool) {
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

pub(crate) unsafe fn install_hooks() -> bool {
    if unsafe { c::hooks_created() } {
        return unsafe { c::enable_hooks() };
    }
    let Some((image, build)) = engine() else {
        return false;
    };
    let mut sites = DETOURS.map(|index| {
        let site = &build.sr.function_sites[index];
        // The leaf fills `replacement` and `original`.
        HookSite {
            name: site.name,
            rva: site.rva,
            expected: site.expected,
            replacement: null_mut(),
            original: null_mut(),
        }
    });
    unsafe {
        c::install_hooks(
            image as Ptr,
            build.native.image_size,
            // native_hooks::Site is the bound rsf_anomaly_hook_site, which this leaf's header forward-declares.
            sites.as_mut_ptr().cast(),
            sites.len(),
        )
    }
}

const POSITION_SLOT: u32 = 1; // RSF_ANOMALY_POSITION_SLOT (engine_slots.h)

pub(crate) unsafe fn install_position_slot() -> bool {
    let Some((image, build)) = engine() else {
        return false;
    };
    let site = &build.sr.function_sites[SITE_POSITION_SETUP];
    let checked = Image::new(image as *const u8, build.native.image_size, c::copy);
    // The slot must hold the profiled function, and the function its guard.
    checked.matches(site) && {
        slot_install(
            POSITION_SLOT,
            &checked,
            build.sr.position_setup_vtable_rva,
            site.rva,
            0,
            c::position_setup as unsafe extern "C" fn(Ptr, Ptr) as Ptr,
            c::compare_slot,
        )
    }
}

pub(crate) unsafe fn disable_hooks() {
    unsafe {
        let _ = rsf_anomaly_slot_set(POSITION_SLOT, false);
        let _ = c::disable_hooks();
    }
}

unsafe extern "C" fn rm_near_callback(_: Ptr) {
    STAGE.rm_near();
}

pub(crate) unsafe fn set_rm_near(on: bool) {
    rsf_anomaly_geometry_set_near(on.then_some(rm_near_callback as unsafe extern "C" fn(Ptr)));
}

pub(crate) unsafe fn remove_hooks() -> bool {
    unsafe {
        rsf_anomaly_geometry_set_near(None);
        let restored = rsf_anomaly_slot_set(POSITION_SLOT, false);
        let disabled = c::disable_hooks();
        restored && disabled
    }
}

pub(crate) unsafe fn apply_colour_transport(enabled: bool) {
    rsf_anomaly_runtime_set_colour_transport(enabled);
}

/// The stage's leaves in production (`sr_stage::leaves`): these functions, the D3D11 host's typed context calls, the
/// adapter's INI and option persistence and the Rust temporal driver (sr_temporal/stage_bridge.rs).
#[cfg(not(test))]
pub(crate) mod leaves {
    pub(crate) use super::{
        apply_colour_transport, back_buffer, build_sha256, close_stale, combine_end, controller,
        create_motion_view, depth_texture, device_size, disable_hooks, draw_pass, engine_frame,
        frame_end, frame_facts, geometry_active, hooks_created, host_reset, host_stats,
        install_hooks, install_position_slot, loading_registered, motion_target, now, pixel_bytes,
        position_setup, precache_frame, prepare, prepared, read_camera, remove_hooks,
        render_target, rm_near, seam_surface, set_engine, set_rm_near, set_swap_enabled, shutdown,
        stage_context as context, stage_device as device, sun_begin, sun_end, swap_counters,
        swap_handle, temporal_ready, verify_guards,
    };
    pub(crate) use crate::adapter_options::{
        rsf_anomaly_sr_persist_int as persist_int, rsf_anomaly_sr_persist_text as persist_text,
    };
    pub(crate) use crate::com::release;
    pub(crate) use crate::ini::{
        rsf_anomaly_sr_ini_int as ini_int, rsf_anomaly_sr_ini_text as ini_text,
    };
    pub(crate) use crate::sr_d3d11::native::{
        rsf_anomaly_sr_context_device as context_device,
        rsf_anomaly_sr_copy_subresource as copy_subresource,
        rsf_anomaly_sr_immediate_context as immediate_context, rsf_anomaly_sr_map_read as map_read,
        rsf_anomaly_sr_timer_clock as timer_clock,
        rsf_anomaly_sr_timer_create_query as timer_create_query,
        rsf_anomaly_sr_timer_issue as timer_issue,
        rsf_anomaly_sr_timer_retain_device as timer_retain_device,
        rsf_anomaly_sr_timer_timestamp as timer_timestamp, rsf_anomaly_sr_unmap as unmap,
    };
    pub(crate) use crate::sr_temporal::stage_bridge::{
        bind as driver_bind, boundary as driver_boundary, configure as configured,
        count_prepared as driver_count_prepared, enabled as driver_enabled,
        evaluate as driver_evaluate, fallback as driver_fallback,
        output_format as driver_output_format, overlay_report as driver_overlay_report,
        plan as driver_planned, report as driver_report, request as driver_request,
        request_dump as driver_request_dump, stop as driver_stop,
    };
}

#[unsafe(no_mangle)]
pub(crate) unsafe fn rsf_anomaly_sr_stage_configure_native(
    ini: *const u16,
    apply: bool,
    log: rsf_anomaly_sr_log,
    user: Ptr,
) -> bool {
    STAGE.bind();
    unsafe { rsf_anomaly_sr_stage_configure_ini(ini, apply, log, user) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_stage_install_build(image: Ptr, build: usize) -> bool {
    let Some(selected) = BUILDS.get(build) else {
        return false;
    };
    let size = selected.native.image_size;
    STAGE.bind();
    STAGE.install(image, size, size, from_ref(selected).cast())
}
#[cfg(feature = "native-entry")]
pub(crate) fn rsf_anomaly_sr_depth_rva(build: usize) -> u32 {
    SR.get(build).map_or(0, |profile| profile.hw_base_zb_rva)
}

// The C leaves: sr_stage_platform.c (guarded copy, SEH engine calls, the stage hook set and the position-binder
// replacement), engine_slots.c, native_geometry_platform.c and the Graphics leaves. Unit tests never reach this
// production module, so they link stand-ins.
#[cfg(not(test))]
mod c {
    use super::Ptr;
    use std::ffi::c_void;

    unsafe extern "C" {
        #[link_name = "rsf_anomaly_sr_native_copy"]
        pub(super) fn copy(output: Ptr, input: *const c_void, size: usize) -> bool;
        #[link_name = "rsf_anomaly_sr_native_set_projection"]
        pub(super) fn set_projection(function: Ptr, xforms: Ptr, matrix: *const c_void) -> bool;
        #[link_name = "rsf_anomaly_sr_native_set_mip_bias"]
        pub(super) fn set_mip_bias(function: Ptr, cache: Ptr, bias: f32) -> bool;
        #[link_name = "rsf_anomaly_sr_native_constant_buffer"]
        pub(super) fn constant_buffer(
            function: Ptr,
            constants: Ptr,
            constant: usize,
            stage: u32,
        ) -> usize;
        #[link_name = "rsf_anomaly_sr_native_install_hooks"]
        pub(super) fn install_hooks(image: Ptr, size: u32, sites: Ptr, count: usize) -> bool;
        #[link_name = "rsf_anomaly_sr_native_hooks_created"]
        pub(super) fn hooks_created() -> bool;
        #[link_name = "rsf_anomaly_sr_native_enable_hooks"]
        pub(super) fn enable_hooks() -> bool;
        #[link_name = "rsf_anomaly_sr_native_disable_hooks"]
        pub(super) fn disable_hooks() -> bool;
        /// The cl_pos_decompress_params::setup vtable replacement; only its address is taken.
        #[link_name = "rsf_anomaly_sr_position_setup_detour"]
        pub(super) fn position_setup(this: Ptr, constant: Ptr);
        #[link_name = "rsf_anomaly_compare_slot"]
        pub(super) fn compare_slot(
            address: Ptr,
            expected: Ptr,
            value: Ptr,
            previous: *mut Ptr,
        ) -> bool;
        #[link_name = "rsf_anomaly_geometry_current_view"]
        pub(super) fn current_view(module: *const c_void, build: usize) -> u8;
        #[link_name = "rsf_texture_pixel_bytes"]
        pub(super) fn pixel_bytes(format: u32) -> u32;
        /// An owned default shader view, or null.
        #[link_name = "rsf_d3d11_create_shader_view"]
        pub(super) fn create_view(device: Ptr, texture: Ptr) -> Ptr;
    }
}

#[cfg(test)]
mod c {
    use super::Ptr;
    use std::ffi::c_void;

    pub(super) unsafe extern "C" fn copy(_: Ptr, _: *const c_void, _: usize) -> bool {
        unreachable!("C leaf in a unit test")
    }
    pub(super) unsafe fn set_projection(_: Ptr, _: Ptr, _: *const c_void) -> bool {
        unreachable!("C leaf in a unit test")
    }
    pub(super) unsafe fn set_mip_bias(_: Ptr, _: Ptr, _: f32) -> bool {
        unreachable!("C leaf in a unit test")
    }
    pub(super) unsafe fn constant_buffer(_: Ptr, _: Ptr, _: usize, _: u32) -> usize {
        unreachable!("C leaf in a unit test")
    }
    pub(super) unsafe fn install_hooks(_: Ptr, _: u32, _: Ptr, _: usize) -> bool {
        unreachable!("C leaf in a unit test")
    }
    pub(super) unsafe fn hooks_created() -> bool {
        unreachable!("C leaf in a unit test")
    }
    pub(super) unsafe fn enable_hooks() -> bool {
        unreachable!("C leaf in a unit test")
    }
    pub(super) unsafe fn disable_hooks() -> bool {
        unreachable!("C leaf in a unit test")
    }
    pub(super) unsafe extern "C" fn position_setup(_: Ptr, _: Ptr) {
        unreachable!("C leaf in a unit test")
    }
    pub(super) unsafe extern "C" fn compare_slot(_: Ptr, _: Ptr, _: Ptr, _: *mut Ptr) -> bool {
        unreachable!("C leaf in a unit test")
    }
    pub(super) unsafe fn current_view(_: *const c_void, _: usize) -> u8 {
        unreachable!("C leaf in a unit test")
    }
    pub(super) unsafe fn pixel_bytes(_: u32) -> u32 {
        unreachable!("C leaf in a unit test")
    }
    pub(super) unsafe fn create_view(_: Ptr, _: Ptr) -> Ptr {
        unreachable!("C leaf in a unit test")
    }
}
