// SPDX-License-Identifier: MIT
// Generic D3D12 SR owner behind rescaleframe/native_sr_d3d12.h. This owner decides setup
// admission, vendor selection, resource lifetime, input validation and its cache, temporal
// history, the normalization dispatch, capture ordering, sharpening, output reinsertion and the
// handoff of accepted normalized depth and motion to the D3D12 frame generation owner.
// sr12_platform.cpp supplies one call per D3D12, vendor, capture or sharpening operation.
use crate::com_ref::ComRef;
use crate::gl_pipeline_abi::{rsf_dlss_setup, rsf_sr12_setup as Setup};
use crate::render_abi::*;
use rsf_game_sys::{
    RSF_FRAME_FLAG_AMBIGUOUS_ID, RSF_FRAME_FLAG_NO_FG, RSF_FRAME_FLAG_RESET,
    RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_FRAME_ABI_VERSION, RSF_GAME_RENDER_NO_FG,
    RSF_GAME_RENDER_RESET, rsf_camera_frame, rsf_frame_record, rsf_game_render_pass as Pass,
};
use std::ffi::{CStr, c_char, c_void};
use std::mem::{MaybeUninit, offset_of};
use std::ptr::{self, NonNull};
use std::sync::{Mutex, MutexGuard, PoisonError};

#[cfg(test)]
mod tests;

const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const INIT_FAILED: i32 = -6;
const NOT_READY: i32 = -8;
const FEATURE_FAILED: i32 = -9;
const STALE_RESOURCES: i32 = -10;

// Public backend IDs and the D3D12/DXGI values below are pinned by static_asserts in
// sr12_platform.cpp.
const SR_NONE: u32 = 0;
const SR_DLSS: u32 = 1;
const MAX_QUALITY: u32 = 5;

const RGBA32_FLOAT: u32 = 2;
const RGBA16_FLOAT: u32 = 10;
const RG32_FLOAT: u32 = 16;
const R32G8X24_TYPELESS: u32 = 19;
const D32_FLOAT_S8X24_UINT: u32 = 20;
const R32_FLOAT_X8X24_TYPELESS: u32 = 21;
const R11G11B10_FLOAT: u32 = 26;
const RG16_FLOAT: u32 = 34;
const R32_TYPELESS: u32 = 39;
const D32_FLOAT: u32 = 40;
const R32_FLOAT: u32 = 41;
const R24G8_TYPELESS: u32 = 44;
const D24_UNORM_S8_UINT: u32 = 45;
const R24_UNORM_X8_TYPELESS: u32 = 46;
const R16_FLOAT: u32 = 54;

const RENDER_TARGET: u32 = 0x4;
const UNORDERED_ACCESS: u32 = 0x8;
const SHADER_READ: u32 = 0x40;
const COPY_DEST: u32 = 0x400;
const COPY_SOURCE: u32 = 0x800;
const TEXTURE2D: u32 = 3;

const DUMP_RAW: u32 = 0;
const DUMP_HDR: u32 = 3;
const NO_DISPATCH_RESET: u32 = u32::MAX;
const NO_PREVIEW: u32 = u32::MAX;
const ALL_SLOTS: u32 = u32::MAX;

const SLOTS: usize = 3;
const COLOR: usize = 0;
const DEPTH: usize = 1;
const MOTION: usize = 2;
const OUTPUT: usize = 3;
// Unordered views of the normalized colour, depth and motion, in register order.
const NORMALIZED_FORMATS: [u32; 3] = [RGBA16_FLOAT, R32_FLOAT, RG16_FLOAT];
const EXPOSURE_SRV: u32 = 3;
const NORMALIZED_UAV: u32 = 4;
const EXPOSURE_UAV: u32 = 7;

// Four 8x8 RG16F tiles, each in a 256-byte row pitch footprint.
const TILE: u32 = 8;
const TILE_BYTES: u64 = 2048;
const READBACK_BYTES: u64 = 4 * TILE_BYTES;
const DIAGNOSTIC_FRAMES: u32 = 12;

type Handle = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub(crate) struct TextureDesc {
    width: u64,
    height: u32,
    dimension: u32,
    array_size: u32,
    samples: u32,
    format: u32,
    srv_format: u32,
}

// Field order matches `Platform` in sr12_platform.cpp.
#[repr(C)]
pub(crate) struct Platform {
    struct_size: u32,
    dlss_create: unsafe extern "C" fn(Handle, *const rsf_dlss_setup, *mut Handle) -> i32,
    dlss_create_shared:
        unsafe extern "C" fn(Handle, *const rsf_dlss_setup, Handle, *mut Handle) -> i32,
    dlss_create_registered:
        unsafe extern "C" fn(Handle, *const rsf_dlss_setup, Handle, *mut Handle) -> i32,
    dlss_plan: unsafe extern "C" fn(Handle, u32, u32, u32, *mut u32, *mut u32) -> i32,
    dlss_set_sdr: unsafe extern "C" fn(Handle, u32),
    dlss_evaluate: unsafe extern "C" fn(Handle, Handle, *const rsf_sr_frame) -> i32,
    dlss_evaluate_borrowed:
        unsafe extern "C" fn(Handle, Handle, *const rsf_sr_frame, Handle) -> i32,
    dlss_present: unsafe extern "C" fn(Handle, Handle, u32, u32, u32) -> i32,
    dlss_destroy: unsafe extern "C" fn(Handle),
    session_create: unsafe extern "C" fn(*const rsf_sr_session_setup, *mut Handle) -> i32,
    session_select: unsafe extern "C" fn(Handle, u32, u32, u64) -> i32,
    session_get_status: unsafe extern "C" fn(Handle, *mut rsf_sr_session_status) -> i32,
    session_evaluate: unsafe extern "C" fn(Handle, Handle, *const rsf_sr_frame) -> i32,
    session_destroy: unsafe extern "C" fn(Handle),
    create_pipeline:
        unsafe extern "C" fn(Handle, rsf_backend_log_fn, Handle, *mut Handle, *mut Handle) -> i32,
    create_heap: unsafe extern "C" fn(Handle) -> Handle,
    create_texture: unsafe extern "C" fn(Handle, u32, u32, u32) -> Handle,
    create_unit_exposure: unsafe extern "C" fn(Handle, *mut Handle, *mut Handle) -> i32,
    create_readback: unsafe extern "C" fn(Handle, u64) -> Handle,
    describe: unsafe extern "C" fn(Handle, *mut TextureDesc),
    owned_by: unsafe extern "C" fn(Handle, Handle) -> i32,
    write_srv: unsafe extern "C" fn(Handle, Handle, u32, Handle, u32),
    write_uav: unsafe extern "C" fn(Handle, Handle, u32, Handle, u32),
    dispatch: unsafe extern "C" fn(Handle, Handle, Handle, Handle, *const u32, u32, u32),
    transition: unsafe extern "C" fn(Handle, Handle, u32, u32),
    copy_region: unsafe extern "C" fn(Handle, Handle, Handle, u32, u32),
    copy_unit_exposure: unsafe extern "C" fn(Handle, Handle, Handle),
    copy_motion_tile: unsafe extern "C" fn(Handle, Handle, u64, Handle, u32, u32),
    map: unsafe extern "C" fn(Handle, u64) -> *const u8,
    unmap: unsafe extern "C" fn(Handle),
    depth_domains: unsafe extern "C" fn(*const Pass) -> u32,
    capture_create: unsafe extern "C" fn() -> Handle,
    capture_destroy: unsafe extern "C" fn(Handle),
    capture_enabled: unsafe extern "C" fn(Handle) -> i32,
    capture_configure: unsafe extern "C" fn(Handle, *const c_char, u64, u32) -> i32,
    capture_flush: unsafe extern "C" fn(Handle, u32) -> i32,
    capture_record: unsafe extern "C" fn(
        Handle,
        Handle,
        Handle,
        u32,
        *const rsf_frame_record,
        *const c_char,
        u32,
        u32,
        u32,
        u32,
    ) -> i32,
    capture_event:
        unsafe extern "C" fn(Handle, *const rsf_frame_record, *const c_char, u64, u32, i32) -> i32,
    capture_wants: unsafe extern "C" fn(Handle, u64) -> i32,
    capture_force_reset: unsafe extern "C" fn(Handle, u64) -> i32,
    sharpen_create: unsafe extern "C" fn() -> Handle,
    sharpen_destroy: unsafe extern "C" fn(Handle),
    sharpening_valid: unsafe extern "C" fn(*const rsf_sharpening_options) -> i32,
    sharpening_defaults: unsafe extern "C" fn(*mut rsf_sharpening_options),
    vendor_sharpness: unsafe extern "C" fn(*const rsf_sharpening_options, u32) -> f32,
    sharpen_run: unsafe extern "C" fn(
        Handle,
        Handle,
        Handle,
        Handle,
        Handle,
        Handle,
        *const rsf_sharpening_options,
        u32,
        u32,
        f32,
        f32,
        u32,
    ) -> i32,
    dump_begin_record: unsafe extern "C" fn(*const rsf_frame_record, u32) -> i32,
    dump_pass_metadata: unsafe extern "C" fn(*const Pass),
    dump_sr_frame: unsafe extern "C" fn(*const rsf_sr_frame, u32, u32),
    dump_result: unsafe extern "C" fn(*const rsf_frame_record, *const c_char, u32, i32),
    fg_bind: unsafe extern "C" fn(),
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

unsafe fn retain(object: Handle) -> Option<ComRef> {
    (!object.is_null()).then(|| unsafe { ComRef::retain(object) })
}

fn raw(object: Option<&ComRef>) -> Handle {
    object.map_or(ptr::null_mut(), ComRef::as_ptr)
}

fn preview(input_sdr: bool) -> u32 {
    if input_sdr { DUMP_RAW } else { DUMP_HDR }
}

// The readable plane of a depth format, R32_FLOAT otherwise.
fn depth_view_format(srv_format: u32) -> u32 {
    if matches!(srv_format, R32_FLOAT_X8X24_TYPELESS | R24_UNORM_X8_TYPELESS) {
        srv_format
    } else {
        R32_FLOAT
    }
}

// Colour, depth, motion and engine output. Returns the SRV format the input reads with; the
// output is only copied into. Extents and device ownership are checked by the caller.
fn accepted_format(index: usize, desc: &TextureDesc) -> Option<u32> {
    let readable = desc.srv_format;
    match index {
        COLOR => matches!(readable, RGBA16_FLOAT | R11G11B10_FLOAT).then_some(readable),
        DEPTH => matches!(
            desc.format,
            R32_FLOAT
                | R32_TYPELESS
                | D32_FLOAT
                | D32_FLOAT_S8X24_UINT
                | R32G8X24_TYPELESS
                | R32_FLOAT_X8X24_TYPELESS
                | D24_UNORM_S8_UINT
                | R24G8_TYPELESS
        )
        .then(|| depth_view_format(readable)),
        MOTION => (readable == RG16_FLOAT).then_some(readable),
        _ => (readable == RGBA16_FLOAT).then_some(readable),
    }
}

fn single_texture(desc: &TextureDesc) -> bool {
    desc.dimension == TEXTURE2D && desc.samples == 1 && desc.array_size == 1
}

// External exposure: the first component of a 1x1 floating-point texture.
fn exposure_accepted(desc: &TextureDesc) -> bool {
    single_texture(desc)
        && desc.width == 1
        && desc.height == 1
        && matches!(
            desc.srv_format,
            R32_FLOAT | R16_FLOAT | RG32_FLOAT | RG16_FLOAT | RGBA32_FLOAT | RGBA16_FLOAT
        )
}

// The four diagnostic tiles sit at the quarter points. Every 8x8 box must lie inside the
// normalized motion texture, so the extent needs at least 32 pixels on each axis.
fn diagnostic_fits(width: u32, height: u32) -> bool {
    width >= 4 * TILE && height >= 4 * TILE
}

fn tile_origin(tile: u32, width: u32, height: u32) -> (u32, u32) {
    let quarter = |extent: u32, far: bool| (u64::from(extent) * if far { 3 } else { 1 } / 4) as u32;
    (
        quarter(width, tile % 2 == 1),
        quarter(height, tile / 2 == 1),
    )
}

fn half_to_f32(half: u16) -> f32 {
    let sign = u32::from(half & 0x8000) << 16;
    let exponent = u32::from((half >> 10) & 0x1f);
    let mantissa = u32::from(half & 0x3ff);
    match exponent {
        0 => {
            let magnitude = mantissa as f32 / 16_777_216.0;
            if sign != 0 { -magnitude } else { magnitude }
        }
        0x1f => f32::from_bits(sign | 0x7f80_0000 | (mantissa << 13)),
        _ => f32::from_bits(sign | ((exponent + 112) << 23) | (mantissa << 13)),
    }
}

// Mean and maximum pixel-vector magnitude and the count of non-finite samples.
fn motion_statistics(mapped: &[u8]) -> (f32, f32, u32) {
    let (mut sum, mut maximum, mut invalid) = (0.0f32, 0.0f32, 0u32);
    for tile in 0..4usize {
        for y in 0..TILE as usize {
            for x in 0..TILE as usize {
                let offset = tile * TILE_BYTES as usize + y * 256 + x * 4;
                let read =
                    |at: usize| half_to_f32(u16::from_le_bytes([mapped[at], mapped[at + 1]]));
                let (vx, vy) = (read(offset), read(offset + 2));
                if !vx.is_finite() || !vy.is_finite() {
                    invalid += 1;
                    continue;
                }
                let magnitude = (vx * vx + vy * vy).sqrt();
                sum += magnitude;
                maximum = maximum.max(magnitude);
            }
        }
    }
    (sum / 256.0, maximum, invalid)
}

#[derive(Clone, Copy, Default)]
struct History {
    frame: u64,
    view: u64,
    session: u64,
    generation: u32,
}

impl History {
    fn continues(&self, pass: &Pass) -> bool {
        pass.flags & RSF_GAME_RENDER_RESET == 0
            && self.frame.wrapping_add(1) == pass.native_frame
            && self.view == pass.history_key
            && self.session == pass.session_id
            && self.generation == pass.resource_generation
    }
}

fn frame_id(pass: &Pass) -> u64 {
    if pass.source_frame_id != 0 {
        pass.source_frame_id
    } else {
        pass.native_frame
    }
}

enum Vendor {
    None,
    Dlss(NonNull<c_void>),
    Session(NonNull<c_void>),
}

// Vendor context and native helper objects. Dropped before the GPU resources.
struct Native {
    p: &'static Platform,
    vendor: Vendor,
    capture: NonNull<c_void>,
    sharpen: NonNull<c_void>,
}

impl Drop for Native {
    fn drop(&mut self) {
        let p = self.p;
        unsafe {
            match self.vendor {
                Vendor::None => {}
                Vendor::Dlss(context) => (p.dlss_destroy)(context.as_ptr()),
                Vendor::Session(session) => (p.session_destroy)(session.as_ptr()),
            }
            (p.capture_destroy)(self.capture.as_ptr());
            (p.sharpen_destroy)(self.sharpen.as_ptr());
        }
    }
}

struct Gpu {
    root: ComRef,
    pipeline: ComRef,
    heaps: [ComRef; SLOTS],
    normalized: [ComRef; 3],
    output: Option<ComRef>,
    exposure: Option<ComRef>,
    exposure_upload: Option<ComRef>,
}

struct Captured {
    record: rsf_frame_record,
    list: Handle,
}

// Per-evaluation state. The command thread holds it for an evaluation; capture and teardown
// calls observe it between evaluations.
struct State {
    // Colour, depth, motion and engine output as last validated, with the generation they were
    // validated in and the SRV formats they read with. The references keep a freed resource's
    // address from passing for the cached one.
    validated: [Option<ComRef>; 4],
    view_formats: [u32; 3],
    validated_generation: u32,
    validated_list: Option<ComRef>,
    exposure_input: Option<ComRef>,
    exposure_initialized: bool,
    history: History,
    captured: [Captured; SLOTS],
    motion_readback: [Option<ComRef>; SLOTS],
    motion_pending: [bool; SLOTS],
    diagnostic_frames: u32,
    sharpen_reported: bool,
}

pub(crate) struct Sr12 {
    // Fields drop in order: vendor contexts first, then the resources they referenced.
    native: Native,
    gpu: Gpu,
    device: ComRef,
    log: rsf_backend_log_fn,
    user: Handle,
    backend: u32,
    input_sdr: bool,
    external_exposure: bool,
    normalize_color: bool,
    width: u32,
    height: u32,
    output_width: u32,
    output_height: u32,
    sharpening: Mutex<rsf_sharpening_options>,
    state: Mutex<State>,
}

impl Drop for Sr12 {
    fn drop(&mut self) {
        // The caller drained all submitted work before destruction.
        unsafe { (self.native.p.capture_flush)(self.native.capture.as_ptr(), ALL_SLOTS) };
    }
}

unsafe fn describe(p: &Platform, resource: Handle) -> TextureDesc {
    let mut desc = TextureDesc::default();
    unsafe { (p.describe)(resource, &mut desc) };
    desc
}

// Copies the setup prefix the caller declared; later fields stay zero.
unsafe fn read_setup(setup: *const Setup) -> Option<(Setup, u32)> {
    if setup.is_null() {
        return None;
    }
    let size = unsafe { setup.cast::<u32>().read_unaligned() };
    let mut copy = MaybeUninit::<Setup>::zeroed();
    let bytes = (size as usize).min(size_of::<Setup>());
    unsafe {
        ptr::copy_nonoverlapping(setup.cast::<u8>(), copy.as_mut_ptr().cast::<u8>(), bytes);
        Some((copy.assume_init(), size))
    }
}

unsafe fn open_vendor(
    p: &'static Platform,
    caller: *const Setup,
    setup: &Setup,
    abi2: bool,
    external_exposure: bool,
) -> (Vendor, i32, u32, u32) {
    let device = setup.device;
    if setup.backend == SR_NONE {
        let width = if setup.render_width != 0 {
            setup.render_width
        } else {
            setup.output_width
        };
        let height = if setup.render_height != 0 {
            setup.render_height
        } else {
            setup.output_height
        };
        return (Vendor::None, OK, width, height);
    }
    if setup.backend == SR_DLSS {
        // DLSS reads its setup from the caller's own structure.
        let dlss = unsafe { ptr::addr_of!((*caller).dlss) };
        let mut context = ptr::null_mut();
        let mut result = unsafe {
            if abi2 && !setup.dlss_registration.is_null() {
                (p.dlss_create_registered)(device, dlss, setup.dlss_registration, &mut context)
            } else if !setup.streamline_host.is_null() {
                (p.dlss_create_shared)(device, dlss, setup.streamline_host, &mut context)
            } else {
                (p.dlss_create)(device, dlss, &mut context)
            }
        };
        let vendor = NonNull::new(context).map_or(Vendor::None, Vendor::Dlss);
        let (mut width, mut height) = (0, 0);
        if result == OK {
            result = unsafe {
                (p.dlss_plan)(
                    context,
                    setup.output_width,
                    setup.output_height,
                    setup.quality,
                    &mut width,
                    &mut height,
                )
            };
        }
        if result == OK {
            unsafe { (p.dlss_set_sdr)(context, setup.input_sdr) };
        }
        return (vendor, result, width, height);
    }
    let mut alternate: rsf_sr_session_setup = unsafe { std::mem::zeroed() };
    alternate.struct_size = size_of::<rsf_sr_session_setup>() as u32;
    alternate.abi_version = RSF_SR_SESSION_ABI_VERSION;
    alternate.open.struct_size = size_of::<rsf_sr_open_desc>() as u32;
    alternate.open.abi_version = RSF_BACKEND_ABI_VERSION;
    alternate.open.api = RSF_API_D3D12;
    alternate.open.device = device;
    alternate.open.output_width = setup.output_width;
    alternate.open.output_height = setup.output_height;
    alternate.open.dynamic_resolution = 1;
    alternate.open.hdr = u32::from(setup.input_sdr == 0);
    alternate.open.auto_exposure = u32::from(setup.input_sdr == 0 && !external_exposure);
    alternate.open.inverted_depth = setup.inverted_depth;
    alternate.open.depth_infinite = 0;
    alternate.open.log = setup.log;
    alternate.open.log_user = setup.user;
    alternate.fsr2_directory_utf8 = setup.fsr2_directory_utf8;
    alternate.fsr3_directory_utf8 = setup.fsr3_directory_utf8;
    alternate.fsr4_directory_utf8 = setup.fsr4_directory_utf8;
    alternate.xess_directory_utf8 = setup.xess_directory_utf8;
    let mut session = ptr::null_mut();
    let mut result = unsafe { (p.session_create)(&alternate, &mut session) };
    let vendor = NonNull::new(session).map_or(Vendor::None, Vendor::Session);
    if result == OK {
        result = unsafe { (p.session_select)(session, setup.backend, setup.quality, 0) };
    }
    let (mut width, mut height) = (0, 0);
    if result == OK {
        let mut status: rsf_sr_session_status = unsafe { std::mem::zeroed() };
        status.struct_size = size_of::<rsf_sr_session_status>() as u32;
        result = unsafe { (p.session_get_status)(session, &mut status) };
        (width, height) = (status.render_width, status.render_height);
    }
    (vendor, result, width, height)
}

unsafe fn create_gpu(
    p: &Platform,
    setup: &Setup,
    (width, height): (u32, u32),
    normalize_color: bool,
    external_exposure: bool,
) -> Option<Gpu> {
    let device = setup.device;
    let (mut root, mut pipeline) = (ptr::null_mut(), ptr::null_mut());
    if unsafe { (p.create_pipeline)(device, setup.log, setup.user, &mut root, &mut pipeline) } == 0
    {
        return None;
    }
    let root = unsafe { ComRef::from_owned(root) }?;
    let pipeline = unsafe { ComRef::from_owned(pipeline) }?;
    let heap = || unsafe { ComRef::from_owned((p.create_heap)(device)) };
    let heaps = [heap()?, heap()?, heap()?];
    let texture =
        |w, h, format| unsafe { ComRef::from_owned((p.create_texture)(device, w, h, format)) };
    let (color_width, color_height) = if normalize_color {
        (width, height)
    } else {
        (1, 1)
    };
    let normalized = [
        texture(color_width, color_height, RGBA16_FLOAT)?,
        texture(width, height, R32_FLOAT)?,
        texture(width, height, RG16_FLOAT)?,
    ];
    let output = if setup.backend != SR_NONE {
        Some(texture(
            setup.output_width,
            setup.output_height,
            RGBA16_FLOAT,
        )?)
    } else {
        None
    };
    let (mut exposure, mut exposure_upload) = (None, None);
    if setup.input_sdr != 0
        && !external_exposure
        && setup.backend != SR_NONE
        && setup.backend != SR_DLSS
    {
        // SDR has no changing pre-exposure. XeSS requires an exposure texture when auto
        // exposure is disabled; feed the actual constant scale, not HDR adaptation.
        let (mut texture, mut upload) = (ptr::null_mut(), ptr::null_mut());
        let made = unsafe { (p.create_unit_exposure)(device, &mut texture, &mut upload) };
        let texture = unsafe { ComRef::from_owned(texture) };
        let upload = unsafe { ComRef::from_owned(upload) };
        if made == 0 {
            return None;
        }
        (exposure, exposure_upload) = (Some(texture?), Some(upload?));
    }
    if external_exposure {
        exposure = Some(texture(1, 1, R32_FLOAT)?);
    }
    Some(Gpu {
        root,
        pipeline,
        heaps,
        normalized,
        output,
        exposure,
        exposure_upload,
    })
}

unsafe fn create(setup: *const Setup, p: &'static Platform) -> Result<Sr12, i32> {
    let Some((s, size)) = (unsafe { read_setup(setup) }) else {
        return Err(INVALID);
    };
    if (size as usize) < offset_of!(Setup, engine_input_extents)
        || s.device.is_null()
        || s.output_width == 0
        || s.output_height == 0
        || s.backend > RSF_SR_XESS
        || s.quality > MAX_QUALITY
    {
        return Err(INVALID);
    }
    if s.abi_version != 1 && s.abi_version != 2 {
        return Err(ABI_MISMATCH);
    }
    let abi2 = s.abi_version == 2;
    if abi2 && (size as usize) < size_of::<Setup>() {
        return Err(INVALID);
    }
    let engine_extents = abi2 && s.engine_input_extents != 0;
    if engine_extents
        && (s.render_width == 0
            || s.render_height == 0
            || s.render_width > s.output_width
            || s.render_height > s.output_height)
    {
        return Err(INVALID);
    }
    let capture = NonNull::new(unsafe { (p.capture_create)() }).ok_or(INIT_FAILED)?;
    let Some(sharpen) = NonNull::new(unsafe { (p.sharpen_create)() }) else {
        unsafe { (p.capture_destroy)(capture.as_ptr()) };
        return Err(INIT_FAILED);
    };
    let mut native = Native {
        p,
        vendor: Vendor::None,
        capture,
        sharpen,
    };
    let device = unsafe { ComRef::retain(s.device) };
    let external_exposure = abi2 && s.external_exposure != 0;
    let (vendor, result, mut width, mut height) =
        unsafe { open_vendor(p, setup, &s, abi2, external_exposure) };
    // A vendor that reports success must hand out its context.
    let opened = s.backend == SR_NONE || !matches!(vendor, Vendor::None);
    native.vendor = vendor;
    if result != OK {
        return Err(result);
    }
    if !opened {
        return Err(INIT_FAILED);
    }
    if engine_extents {
        (width, height) = (s.render_width, s.render_height);
    }
    let normalize_color =
        s.backend != SR_NONE || unsafe { (p.capture_enabled)(capture.as_ptr()) } != 0;
    let gpu = unsafe { create_gpu(p, &s, (width, height), normalize_color, external_exposure) }
        .ok_or(INIT_FAILED)?;
    let mut sharpening: rsf_sharpening_options = unsafe { std::mem::zeroed() };
    unsafe { (p.sharpening_defaults)(&mut sharpening) };
    Ok(Sr12 {
        native,
        gpu,
        device,
        log: s.log,
        user: s.user,
        backend: s.backend,
        input_sdr: s.input_sdr != 0,
        external_exposure,
        normalize_color,
        width,
        height,
        output_width: s.output_width,
        output_height: s.output_height,
        sharpening: Mutex::new(sharpening),
        state: Mutex::new(State {
            validated: [const { None }; 4],
            view_formats: [0; 3],
            validated_generation: 0,
            validated_list: None,
            exposure_input: None,
            exposure_initialized: external_exposure,
            history: History::default(),
            captured: std::array::from_fn(|_| Captured {
                record: unsafe { std::mem::zeroed() },
                list: ptr::null_mut(),
            }),
            motion_readback: [const { None }; SLOTS],
            motion_pending: [false; SLOTS],
            diagnostic_frames: 0,
            sharpen_reported: false,
        }),
    })
}

impl Sr12 {
    fn p(&self) -> &'static Platform {
        self.native.p
    }

    fn capture(&self) -> Handle {
        self.native.capture.as_ptr()
    }

    fn say(&self, message: &CStr) {
        if let Some(log) = self.log {
            unsafe { log(self.user, message.as_ptr()) };
        }
    }

    unsafe fn owned(&self, child: Handle) -> bool {
        unsafe { (self.p().owned_by)(child, self.device.as_ptr()) != 0 }
    }

    fn camera_matches(&self, pass: &Pass) -> bool {
        let camera = &pass.camera;
        pass.camera_valid != 0
            && camera.struct_size as usize == size_of::<rsf_camera_frame>()
            && camera.abi_version == RSF_GAME_CAMERA_ABI_VERSION
            && pass.native_frame != 0
            && pass.session_id != 0
            && pass.history_key != 0
            && camera.render_width == self.width
            && camera.render_height == self.height
            && camera.output_width == self.output_width
            && camera.output_height == self.output_height
            && pass
                .motion_to_uv
                .iter()
                .all(|scale| scale.is_finite() && *scale != 0.0)
    }

    unsafe fn inputs_valid(&self, state: &mut State, pass: &Pass) -> bool {
        let p = self.p();
        if unsafe { (p.depth_domains)(pass) } != 0 || !self.camera_matches(pass) {
            return false;
        }
        if self.external_exposure {
            if pass.exposure.is_null() {
                return false;
            }
            let desc = unsafe { describe(p, pass.exposure) };
            if !unsafe { self.owned(pass.exposure) } || !exposure_accepted(&desc) {
                return false;
            }
            state.exposure_input = unsafe { retain(pass.exposure) };
        }
        let pointers = [pass.color_input, pass.depth, pass.motion, pass.color_output];
        let checked = |index: usize| index != OUTPUT || self.backend != SR_NONE;
        // The resources' descriptions cannot change, and the extents they are checked against
        // are fixed at create, so a set already accepted in this generation needs no second look.
        let cached = state.validated_generation == pass.resource_generation
            && (0..4).all(|index| {
                !checked(index)
                    || (!pointers[index].is_null()
                        && raw(state.validated[index].as_ref()) == pointers[index])
            });
        if cached {
            return true;
        }
        state.validated = [const { None }; 4];
        for (index, &resource) in pointers.iter().enumerate() {
            if !checked(index) {
                continue;
            }
            if resource.is_null() {
                return false;
            }
            let desc = unsafe { describe(p, resource) };
            let (width, height) = if index == OUTPUT {
                (self.output_width, self.output_height)
            } else {
                (self.width, self.height)
            };
            if !unsafe { self.owned(resource) }
                || !single_texture(&desc)
                || desc.width < u64::from(width)
                || desc.height < height
            {
                return false;
            }
            let Some(view) = accepted_format(index, &desc) else {
                return false;
            };
            if index != OUTPUT {
                state.view_formats[index] = view;
            }
        }
        state.validated = pointers.map(|resource| unsafe { retain(resource) });
        state.validated_generation = pass.resource_generation;
        true
    }

    unsafe fn motion_diagnostic(&self, state: &mut State, list: Handle, slot: usize) {
        if self.log.is_none() {
            return;
        }
        let p = self.p();
        if state.motion_pending[slot] {
            if let Some(buffer) = state.motion_readback[slot].as_ref() {
                let mapped = unsafe { (p.map)(buffer.as_ptr(), READBACK_BYTES) };
                if !mapped.is_null() {
                    let bytes =
                        unsafe { std::slice::from_raw_parts(mapped, READBACK_BYTES as usize) };
                    let (mean, maximum, invalid) = motion_statistics(bytes);
                    unsafe { (p.unmap)(buffer.as_ptr()) };
                    let text = format!(
                        "Normalized motion samples: mean={mean:.6} max={maximum:.6} render pixels invalid={invalid}"
                    );
                    if let Ok(text) = std::ffi::CString::new(text) {
                        self.say(&text);
                    }
                }
            }
            state.motion_pending[slot] = false;
        }
        if state.diagnostic_frames >= DIAGNOSTIC_FRAMES || !diagnostic_fits(self.width, self.height)
        {
            return;
        }
        if state.motion_readback[slot].is_none() {
            state.motion_readback[slot] = unsafe {
                ComRef::from_owned((p.create_readback)(self.device.as_ptr(), READBACK_BYTES))
            };
        }
        let Some(buffer) = state.motion_readback[slot].as_ref() else {
            return;
        };
        let motion = self.gpu.normalized[MOTION].as_ptr();
        unsafe {
            (p.transition)(list, motion, SHADER_READ, COPY_SOURCE);
            for tile in 0..4 {
                let (x, y) = tile_origin(tile, self.width, self.height);
                (p.copy_motion_tile)(
                    list,
                    buffer.as_ptr(),
                    u64::from(tile) * TILE_BYTES,
                    motion,
                    x,
                    y,
                );
            }
            (p.transition)(list, motion, COPY_SOURCE, SHADER_READ);
        }
        state.motion_pending[slot] = true;
        state.diagnostic_frames += 1;
    }

    fn record(&self, pass: &Pass, reset: bool) -> rsf_frame_record {
        let mut record: rsf_frame_record = unsafe { std::mem::zeroed() };
        record.struct_size = size_of::<rsf_frame_record>() as u32;
        record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
        record.session_id = pass.session_id;
        record.frame_id = frame_id(pass);
        record.view_id = pass.view_key as u32;
        record.resource_generation = pass.resource_generation;
        record.camera = pass.camera;
        record.screen = pass.screen;
        record.render_width = self.width;
        record.render_height = self.height;
        record.output_width = self.output_width;
        record.output_height = self.output_height;
        record.frame_time_ms = pass.camera.frame_time_seconds * 1000.0;
        record.flags = if reset { RSF_FRAME_FLAG_RESET } else { 0 }
            | if pass.flags & RSF_GAME_RENDER_NO_FG != 0 {
                RSF_FRAME_FLAG_NO_FG
            } else {
                0
            };
        record
    }

    #[allow(clippy::too_many_arguments)]
    unsafe fn capture_record(
        &self,
        list: Handle,
        source: Handle,
        state: u32,
        record: &rsf_frame_record,
        role: &CStr,
        slot: usize,
        dispatch_reset: u32,
        view: u32,
    ) {
        unsafe {
            (self.p().capture_record)(
                self.capture(),
                list,
                source,
                state,
                record,
                role.as_ptr(),
                slot as u32,
                0,
                dispatch_reset,
                view,
            )
        };
    }

    unsafe fn normalize(&self, state: &State, list: Handle, pass: &Pass, slot: usize) {
        let p = self.p();
        let device = self.device.as_ptr();
        let heap = self.gpu.heaps[slot].as_ptr();
        let inputs = [pass.color_input, pass.depth, pass.motion];
        unsafe {
            for (index, &input) in inputs.iter().enumerate() {
                (p.write_srv)(device, heap, index as u32, input, state.view_formats[index]);
            }
            let exposure_input = if self.external_exposure {
                raw(state.exposure_input.as_ref())
            } else {
                ptr::null_mut()
            };
            let exposure_view = if exposure_input.is_null() {
                R32_FLOAT
            } else {
                describe(p, exposure_input).srv_format
            };
            (p.write_srv)(device, heap, EXPOSURE_SRV, exposure_input, exposure_view);
            for (index, texture) in self.gpu.normalized.iter().enumerate() {
                (p.write_uav)(
                    device,
                    heap,
                    NORMALIZED_UAV + index as u32,
                    texture.as_ptr(),
                    NORMALIZED_FORMATS[index],
                );
            }
            let exposure_output = if self.external_exposure {
                raw(self.gpu.exposure.as_ref())
            } else {
                ptr::null_mut()
            };
            (p.write_uav)(device, heap, EXPOSURE_UAV, exposure_output, R32_FLOAT);
            let constants = self.constants(pass);
            (p.dispatch)(
                list,
                heap,
                self.gpu.root.as_ptr(),
                self.gpu.pipeline.as_ptr(),
                constants.as_ptr(),
                self.width.div_ceil(8),
                self.height.div_ceil(8),
            );
        }
    }

    // extent, pixel-motion scale, colour bypass and external exposure, in cbuffer order.
    fn constants(&self, pass: &Pass) -> [u32; 6] {
        [
            self.width,
            self.height,
            (-(self.width as f32) * pass.motion_to_uv[0]).to_bits(),
            (-(self.height as f32) * pass.motion_to_uv[1]).to_bits(),
            u32::from(!self.normalize_color),
            u32::from(self.external_exposure),
        ]
    }

    unsafe fn transition_inputs(&self, list: Handle, before: u32, after: u32) {
        let p = self.p();
        unsafe {
            for texture in &self.gpu.normalized {
                (p.transition)(list, texture.as_ptr(), before, after);
            }
            if self.external_exposure {
                (p.transition)(list, raw(self.gpu.exposure.as_ref()), before, after);
            }
        }
    }

    unsafe fn resource(
        &self,
        texture: Handle,
        output: bool,
        generation: u32,
    ) -> rsf_backend_resource {
        let mut resource: rsf_backend_resource = unsafe { std::mem::zeroed() };
        resource.struct_size = size_of::<rsf_backend_resource>() as u32;
        resource.resource = texture;
        (resource.width, resource.height) = if output {
            (self.output_width, self.output_height)
        } else {
            (self.width, self.height)
        };
        resource.generation = generation;
        resource.state = if output {
            UNORDERED_ACCESS
        } else {
            SHADER_READ
        };
        resource
    }

    unsafe fn vendor_evaluate(&self, list: Handle, frame: &rsf_sr_frame, pass: &Pass) -> i32 {
        let p = self.p();
        match self.native.vendor {
            Vendor::None => OK,
            Vendor::Dlss(context) if !pass.native_frame_context.is_null() => unsafe {
                (p.dlss_evaluate_borrowed)(context.as_ptr(), list, frame, pass.native_frame_context)
            },
            Vendor::Dlss(context) => unsafe { (p.dlss_evaluate)(context.as_ptr(), list, frame) },
            Vendor::Session(session) => unsafe {
                (p.session_evaluate)(session.as_ptr(), list, frame)
            },
        }
    }

    unsafe fn evaluate(&self, list: Handle, pass: &Pass, slot: usize) -> i32 {
        let p = self.p();
        let mut guard = lock(&self.state);
        let state = &mut *guard;
        if !unsafe { self.inputs_valid(state, pass) } {
            return INVALID;
        }
        let sharpen = *lock(&self.sharpening);
        unsafe { (p.capture_flush)(self.capture(), slot as u32) };
        if raw(state.validated_list.as_ref()) != list {
            if !unsafe { self.owned(list) } {
                return INVALID;
            }
            state.validated_list = unsafe { retain(list) };
        }
        unsafe {
            self.normalize(state, list, pass, slot);
            self.transition_inputs(list, UNORDERED_ACCESS, SHADER_READ);
            // The caller joins a slot's previous GPU completion before entering this evaluation.
            self.motion_diagnostic(state, list, slot);
        }
        let reset = !state.history.continues(pass);
        let record = self.record(pass, reset);
        let normalized = self.gpu.normalized.each_ref().map(ComRef::as_ptr);
        let view = preview(self.input_sdr);
        let mut identity = record;
        if pass.source_frame_id == 0 {
            identity.flags |= RSF_FRAME_FLAG_AMBIGUOUS_ID;
        }
        unsafe {
            if (p.dump_begin_record)(&identity, self.backend) != 0 {
                (p.dump_pass_metadata)(pass);
                let none = NO_PREVIEW;
                for (source, role, shown) in [
                    (pass.color_input, c"sr-source-jittered", view),
                    (pass.depth, c"sr-scene-depth", none),
                    (pass.motion, c"sr-game-motion-uv", none),
                    (normalized[DEPTH], c"sr-depth", none),
                    (normalized[MOTION], c"sr-motion-pixels", none),
                ] {
                    self.capture_record(
                        list,
                        source,
                        SHADER_READ,
                        &record,
                        role,
                        slot,
                        NO_DISPATCH_RESET,
                        shown,
                    );
                }
            }
            if self.normalize_color {
                self.capture_record(
                    list,
                    normalized[COLOR],
                    SHADER_READ,
                    &record,
                    c"sr-linear-color",
                    slot,
                    NO_DISPATCH_RESET,
                    view,
                );
            }
            for (source, role) in [
                (normalized[DEPTH], c"fg-depth"),
                (normalized[MOTION], c"fg-motion-pixels"),
            ] {
                self.capture_record(
                    list,
                    source,
                    SHADER_READ,
                    &record,
                    role,
                    slot,
                    NO_DISPATCH_RESET,
                    NO_PREVIEW,
                );
            }
        }
        let mut frame: rsf_sr_frame = unsafe { std::mem::zeroed() };
        frame.struct_size = size_of::<rsf_sr_frame>() as u32;
        frame.record = &record;
        if let Some(exposure) = self.gpu.exposure.as_ref() {
            if !state.exposure_initialized
                && let Some(upload) = self.gpu.exposure_upload.as_ref()
            {
                unsafe {
                    (p.copy_unit_exposure)(list, exposure.as_ptr(), upload.as_ptr());
                    (p.transition)(list, exposure.as_ptr(), COPY_DEST, SHADER_READ);
                }
                state.exposure_initialized = true;
            }
            frame.exposure =
                unsafe { self.resource(exposure.as_ptr(), false, record.resource_generation) };
            (frame.exposure.width, frame.exposure.height) = (1, 1);
        }
        let output = raw(self.gpu.output.as_ref());
        unsafe {
            frame.color = self.resource(normalized[COLOR], false, record.resource_generation);
            frame.depth = self.resource(normalized[DEPTH], false, record.resource_generation);
            frame.motion = self.resource(normalized[MOTION], false, record.resource_generation);
            frame.output = self.resource(output, true, record.resource_generation);
            frame.sharpness = (p.vendor_sharpness)(&sharpen, self.backend);
        }
        frame.jitter_x = pass.camera.jitter_pixels[0];
        frame.jitter_y = pass.camera.jitter_pixels[1];
        // The producer measures pixel jitter from the actual GPU projection, including its Y
        // conversion.
        (frame.motion_scale_x, frame.motion_scale_y) = (1.0, 1.0);
        (frame.pre_exposure, frame.view_space_to_meters) = (1.0, 1.0);
        frame.reset = u32::from(
            reset || unsafe { (p.capture_force_reset)(self.capture(), record.frame_id) } != 0,
        );
        unsafe {
            (p.dump_sr_frame)(&frame, self.backend, u32::from(self.input_sdr));
            if !frame.exposure.resource.is_null() {
                self.capture_record(
                    list,
                    frame.exposure.resource,
                    SHADER_READ,
                    &record,
                    c"sr-exposure",
                    slot,
                    NO_DISPATCH_RESET,
                    NO_PREVIEW,
                );
            }
        }
        let result = unsafe { self.vendor_evaluate(list, &frame, pass) };
        unsafe {
            (p.dump_result)(&record, c"sr".as_ptr(), self.backend, result);
            (p.capture_event)(
                self.capture(),
                &record,
                c"sr-evaluate".as_ptr(),
                record.frame_id,
                frame.reset,
                result,
            );
        }
        if result == OK {
            // Exact copy leases of the accepted normalized inputs, recorded on this list after
            // the vendor read them and before sharpening or reinsertion. The FG owner holds
            // them until the producer's submission confirmation and queue retirement.
            unsafe {
                (p.fg_bind)();
                crate::fg12::handoff_inputs(list, &record, normalized[DEPTH], normalized[MOTION]);
            }
        }
        if result == OK && self.backend != SR_NONE {
            unsafe { self.sharpen(state, list, &frame, pass, &sharpen, slot) };
        }
        unsafe { self.transition_inputs(list, SHADER_READ, UNORDERED_ACCESS) };
        if result != OK {
            state.history.frame = 0;
            return result;
        }
        if self.backend != SR_NONE {
            unsafe { self.reinsert(list, pass, &record, frame.reset != 0, slot) };
        }
        state.captured[slot] = Captured { record, list };
        state.history = History {
            frame: pass.native_frame,
            view: pass.history_key,
            session: pass.session_id,
            generation: pass.resource_generation,
        };
        result
    }

    #[allow(clippy::too_many_arguments)]
    unsafe fn sharpen(
        &self,
        state: &mut State,
        list: Handle,
        frame: &rsf_sr_frame,
        pass: &Pass,
        options: &rsf_sharpening_options,
        slot: usize,
    ) {
        let p = self.p();
        let accepted = unsafe {
            (p.sharpen_run)(
                self.native.sharpen.as_ptr(),
                list,
                raw(self.gpu.output.as_ref()),
                self.gpu.normalized[MOTION].as_ptr(),
                self.gpu.normalized[DEPTH].as_ptr(),
                frame.exposure.resource,
                options,
                u32::from(!self.input_sdr),
                pass.camera.depth_inverted,
                pass.camera.near_plane,
                pass.camera.far_plane,
                slot as u32,
            )
        };
        if accepted == 0 && !state.sharpen_reported {
            state.sharpen_reported = true;
            self.say(c"D3D12 sharpening unavailable; retaining unsharpened SR output");
        }
    }

    // The private staging output reaches the engine only on accepted evaluation.
    unsafe fn reinsert(
        &self,
        list: Handle,
        pass: &Pass,
        record: &rsf_frame_record,
        reset: bool,
        slot: usize,
    ) {
        let p = self.p();
        let output = raw(self.gpu.output.as_ref());
        let view = preview(self.input_sdr);
        let reset = u32::from(reset);
        unsafe {
            self.capture_record(
                list,
                output,
                UNORDERED_ACCESS,
                record,
                c"sr-output-linear",
                slot,
                reset,
                view,
            );
            (p.transition)(list, output, UNORDERED_ACCESS, COPY_SOURCE);
            (p.copy_region)(
                list,
                pass.color_output,
                output,
                self.output_width,
                self.output_height,
            );
            self.capture_record(
                list,
                pass.color_output,
                COPY_DEST,
                record,
                c"sr-engine-output",
                slot,
                reset,
                view,
            );
            (p.transition)(list, output, COPY_SOURCE, UNORDERED_ACCESS);
        }
    }

    unsafe fn capture_scene(&self, list: Handle, pass: &Pass, slot: usize) -> i32 {
        let p = self.p();
        if unsafe { (p.depth_domains)(pass) } != 0 {
            return INVALID;
        }
        let state = lock(&self.state);
        let captured = &state.captured[slot];
        let record = captured.record;
        if record.frame_id != frame_id(pass)
            || record.session_id != pass.session_id
            || captured.list != list
        {
            return STALE_RESOURCES;
        }
        drop(state);
        let scene = pass.color_input;
        let desc = unsafe { describe(p, scene) };
        if !unsafe { self.owned(scene) }
            || desc.width != u64::from(record.output_width)
            || desc.height != record.output_height
            || desc.samples != 1
        {
            return INVALID;
        }
        if unsafe { (p.capture_wants)(self.capture(), record.frame_id) } == 0 {
            return NOT_READY;
        }
        let recorded = unsafe {
            (p.capture_record)(
                self.capture(),
                list,
                scene,
                RENDER_TARGET,
                &record,
                c"fg-scene-output".as_ptr(),
                slot as u32,
                pass.pass_key as u32,
                NO_DISPATCH_RESET,
                NO_PREVIEW,
            )
        };
        if recorded != 0 { OK } else { FEATURE_FAILED }
    }
}

unsafe fn full_pass<'a>(pass: *const Pass) -> Option<&'a Pass> {
    (!pass.is_null() && unsafe { pass.cast::<u32>().read() } as usize >= size_of::<Pass>())
        .then(|| unsafe { &*pass })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_sr12_create(
    setup: *const Setup,
    out: *mut *mut Sr12,
    platform: *const Platform,
) -> i32 {
    if out.is_null() {
        return INVALID;
    }
    unsafe { out.write(ptr::null_mut()) };
    // The platform table is a static in the native library that called this entry.
    let Some(platform): Option<&'static Platform> = (unsafe { platform.as_ref() }) else {
        return INVALID;
    };
    assert_eq!(platform.struct_size as usize, size_of::<Platform>());
    match unsafe { create(setup, platform) } {
        Ok(owner) => {
            let handle = crate::allocate_handle(owner);
            if handle.is_null() {
                return INIT_FAILED;
            }
            unsafe { out.write(handle) };
            OK
        }
        Err(result) => result,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_sr12_plan(
    owner: *const Sr12,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INVALID;
    };
    if width.is_null() || height.is_null() {
        return INVALID;
    }
    unsafe {
        width.write(owner.width);
        height.write(owner.height);
    }
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_sr12_evaluate(
    owner: *mut Sr12,
    list: Handle,
    pass: *const Pass,
    slot: u32,
) -> i32 {
    let (Some(owner), Some(pass)) = (unsafe { owner.as_ref() }, unsafe { full_pass(pass) }) else {
        return INVALID;
    };
    if list.is_null() || slot as usize >= SLOTS {
        return INVALID;
    }
    unsafe { owner.evaluate(list, pass, slot as usize) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_sr12_set_sharpening(
    owner: *mut Sr12,
    options: *const rsf_sharpening_options,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INVALID;
    };
    if options.is_null()
        || (unsafe { options.cast::<u32>().read() } as usize) < size_of::<rsf_sharpening_options>()
    {
        return INVALID;
    }
    let options = unsafe { options.read() };
    if options.abi_version != RSF_SHARPENING_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if unsafe { (owner.p().sharpening_valid)(&options) } == 0 {
        return INVALID;
    }
    *lock(&owner.sharpening) = options;
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_sr12_destroy(owner: *mut Sr12) {
    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_sr12_present(
    owner: *mut Sr12,
    swapchain: Handle,
    sync_interval: u32,
    flags: u32,
    begin: u32,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INVALID;
    };
    if swapchain.is_null() {
        return INVALID;
    }
    match owner.native.vendor {
        Vendor::Dlss(context) => unsafe {
            (owner.p().dlss_present)(context.as_ptr(), swapchain, sync_interval, flags, begin)
        },
        _ => OK,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_sr12_capture(
    owner: *mut Sr12,
    directory: *const c_char,
    first_frame: u64,
    frames: u32,
) -> i32 {
    match unsafe { owner.as_ref() } {
        Some(owner)
            if unsafe {
                (owner.p().capture_configure)(owner.capture(), directory, first_frame, frames)
            } != 0 =>
        {
            OK
        }
        _ => INVALID,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_sr12_capture_flush(owner: *mut Sr12, slot: u32) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INVALID;
    };
    if slot as usize >= SLOTS {
        return INVALID;
    }
    if unsafe { (owner.p().capture_flush)(owner.capture(), slot) } != 0 {
        OK
    } else {
        FEATURE_FAILED
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_sr12_capture_scene(
    owner: *mut Sr12,
    list: Handle,
    pass: *const Pass,
    slot: u32,
) -> i32 {
    let (Some(owner), Some(pass)) = (unsafe { owner.as_ref() }, unsafe { full_pass(pass) }) else {
        return INVALID;
    };
    if list.is_null() || slot as usize >= SLOTS || pass.color_input.is_null() {
        return INVALID;
    }
    unsafe { owner.capture_scene(list, pass, slot as usize) }
}
