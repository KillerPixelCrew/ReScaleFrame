// SPDX-License-Identifier: MIT
// Generic D3D12 SR owner behind rescaleframe/native_sr_d3d12.h. This owner decides setup
// admission, vendor selection, resource lifetime, input validation and its cache, temporal
// history, the normalization dispatch, capture ordering, sharpening, output reinsertion and the
// handoff of accepted normalized depth and motion to the D3D12 frame generation owner.
// Its native calls go through `native` (sr12/native.rs; fakes under test).
use crate::com_ref;
use crate::dxgi_format::{
    D24_UNORM_S8_UINT, D32_FLOAT, D32_FLOAT_S8X24_UINT, R24_UNORM_X8_TYPELESS, R24G8_TYPELESS,
    R32_TYPELESS, R32G8X24_TYPELESS, RGBA16_FLOAT, RGBA32_FLOAT, half_to_f32,
};
use crate::retain_object;
use crate::sharpening_policy;
use crate::sr_session::checked;
use crate::sync::lock;
use crate::sys::rsf_sr12_setup as Setup;
use crate::sys::*;
use crate::sys::{RSF_SR_AUTO, RSF_SR_DLSS, RSF_SR_NONE};
use rsf_game_sys::backend_result::{
    ABI_MISMATCH, FEATURE_FAILED, INIT_FAILED, INVALID_ARGUMENT, NOT_READY, OK, STALE_RESOURCES,
};
use rsf_game_sys::{
    RSF_FRAME_FLAG_AMBIGUOUS_ID, RSF_FRAME_FLAG_NO_FG, RSF_FRAME_FLAG_RESET,
    RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_FRAME_ABI_VERSION, RSF_GAME_RENDER_NO_FG,
    RSF_GAME_RENDER_RESET, rsf_camera_frame, rsf_frame_record, rsf_game_render_pass as Pass,
};
use rsf_graphics::dump_options::Log;
use std::ffi::{CStr, c_char, c_void};
use std::mem::{MaybeUninit, offset_of};
use std::ptr::{self, NonNull};
use std::sync::Mutex;
use windows::core::{IUnknown, Interface};

#[cfg(not(test))]
mod native;
#[cfg(test)]
mod tests;
#[cfg(test)]
use tests::native;

// ABI values consumed by the SR planner and normalization pass.
const MAX_QUALITY: u32 = 5;
#[cfg(not(test))]
const PLAN_ABI_VERSION: u32 = 1;

const RG32_FLOAT: u32 = 16;
const R32_FLOAT_X8X24_TYPELESS: u32 = 21;
const R11G11B10_FLOAT: u32 = 26;
const RG16_FLOAT: u32 = 34;
const R32_FLOAT: u32 = 41;
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
const EXPOSURE_UPLOAD_BYTES: u64 = 256;

// 8x8 tiles of four bytes per texel, each in a 256-byte row pitch footprint. A motion check
// reads the four quarter-point tiles of three textures, in this order: the normalized motion,
// the engine's motion before normalization and the normalized depth.
const TILE: u32 = 8;
const TILE_BYTES: u64 = 2048;
const RAW_MOTION_SECTION: u64 = 4 * TILE_BYTES;
const DEPTH_SECTION: u64 = 8 * TILE_BYTES;
const READBACK_BYTES: u64 = 12 * TILE_BYTES;
// Every evaluation of a new context is checked for this many frames, then one in
// DIAGNOSTIC_INTERVAL, so a long session keeps reporting while the camera moves.
const DIAGNOSTIC_FRAMES: u32 = 12;
const DIAGNOSTIC_INTERVAL: u64 = 600;
// A tile moves when the camera alone shifts its static content by at least this many pixels.
const MOVING_PIXELS: f32 = 1.0;

type Handle = *mut c_void;

// Only the rsf_sr12_create_planned export, which unit tests compile out, writes it.
#[cfg_attr(test, allow(dead_code))]
#[repr(C)]
struct PlanResult {
    struct_size: u32,
    abi_version: u32,
    backend: u32,
    render_width: u32,
    render_height: u32,
    requested_result: i32,
}

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

fn raw(object: Option<&IUnknown>) -> Handle {
    object.map_or(ptr::null_mut(), Interface::as_raw)
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

/// The pass facts a motion check compares its readback with, kept until the slot's readback
/// is mapped.
#[derive(Clone, Copy)]
struct MotionSample {
    frame: u64,
    extent: (u32, u32),
    clip_to_previous: [f32; 16],
    jitter: [f32; 2],
    motion_to_uv: [f32; 2],
    camera_motion_included: u32,
    motion_jittered: u32,
}

/// One quarter-point tile at its centre texel: the normalized pixel motion found there, the
/// engine's motion before normalization, the device depth, and the motion the camera alone
/// gives static content there.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TileCheck {
    origin: (u32, u32),
    found: [f32; 2],
    raw: [f32; 2],
    depth: f32,
    expected: Option<[f32; 2]>,
}

/// Render-pixel motion toward the previous frame that the camera alone gives a static point at
/// texel centre (x, y) with device depth `depth`. `clip_to_previous` is in the row-major,
/// row-vector layout the vendors read (Unity's column-major memory): previous = clip * M.
/// None when the previous position has no finite projection.
fn camera_motion(
    clip_to_previous: &[f32; 16],
    (x, y): (f32, f32),
    depth: f32,
    (width, height): (u32, u32),
) -> Option<[f32; 2]> {
    let (width, height) = (width as f32, height as f32);
    let clip = [x / width * 2.0 - 1.0, 1.0 - y / height * 2.0, depth, 1.0];
    let previous = |column: usize| -> f32 {
        (0..4)
            .map(|row| clip[row] * clip_to_previous[row * 4 + column])
            .sum()
    };
    let w = previous(3);
    if !w.is_finite() || w.abs() < 1e-6 {
        return None;
    }
    let motion = [
        (previous(0) / w + 1.0) * 0.5 * width - x,
        (1.0 - previous(1) / w) * 0.5 * height - y,
    ];
    motion
        .iter()
        .all(|value| value.is_finite())
        .then_some(motion)
}

fn length([x, y]: [f32; 2]) -> f32 {
    (x * x + y * y).sqrt()
}

/// Whether the motion texture carries camera motion. Only tiles the camera moves by at least
/// MOVING_PIXELS count: "missing" when every such tile holds under a tenth of its expectation,
/// "match" when at least half are within max(0.5 px, 25 %) of it (characters and other moving
/// objects may cover the rest), "mismatch" otherwise; "camera-still" without a moving tile and
/// "no-camera" without a usable previous-clip transform.
fn motion_verdict(tiles: &[TileCheck]) -> &'static str {
    if tiles.iter().all(|tile| tile.expected.is_none()) {
        return "no-camera";
    }
    let moving: Vec<_> = tiles
        .iter()
        .filter_map(|tile| Some((tile.found, tile.expected?)))
        .filter(|(_, expected)| length(*expected) >= MOVING_PIXELS)
        .collect();
    if moving.is_empty() {
        return "camera-still";
    }
    if moving
        .iter()
        .all(|(found, expected)| length(*found) < 0.1 * length(*expected))
    {
        return "missing";
    }
    let matched = moving
        .iter()
        .filter(|(found, expected)| {
            let error = [found[0] - expected[0], found[1] - expected[1]];
            length(error) <= (0.25 * length(*expected)).max(0.5)
        })
        .count();
    if matched * 2 >= moving.len() {
        "match"
    } else {
        "mismatch"
    }
}

/// The four tile checks of a mapped readback.
fn tile_checks(mapped: &[u8], sample: &MotionSample) -> [TileCheck; 4] {
    let centre = (TILE / 2) as usize;
    let at = |section: u64, tile: u32| -> usize {
        (section + u64::from(tile) * TILE_BYTES) as usize + centre * 256 + centre * 4
    };
    let half = |at: usize| half_to_f32(u16::from_le_bytes([mapped[at], mapped[at + 1]]));
    let pair = |at: usize| [half(at), half(at + 2)];
    std::array::from_fn(|tile| {
        let tile = tile as u32;
        let origin = tile_origin(tile, sample.extent.0, sample.extent.1);
        let depth_at = at(DEPTH_SECTION, tile);
        let depth = f32::from_le_bytes(mapped[depth_at..depth_at + 4].try_into().unwrap());
        let texel = (
            (origin.0 + TILE / 2) as f32 + 0.5,
            (origin.1 + TILE / 2) as f32 + 0.5,
        );
        TileCheck {
            origin,
            found: pair(at(0, tile)),
            raw: pair(at(RAW_MOTION_SECTION, tile)),
            depth,
            expected: depth
                .is_finite()
                .then(|| camera_motion(&sample.clip_to_previous, texel, depth, sample.extent))
                .flatten(),
        }
    })
}

/// The periodic motion line: the normalized statistics, the declared motion facts, each tile
/// and the verdict.
fn motion_check_line(mapped: &[u8], sample: &MotionSample) -> String {
    let (mean, maximum, invalid) = motion_statistics(mapped);
    let tiles = tile_checks(mapped, sample);
    let mut line = format!(
        "Normalized motion samples: mean={mean:.6} max={maximum:.6} render pixels invalid={invalid} frame={} camera_motion_included={} motion_to_uv={},{} motion_jittered={} jitter={:.3},{:.3}",
        sample.frame,
        sample.camera_motion_included,
        sample.motion_to_uv[0],
        sample.motion_to_uv[1],
        sample.motion_jittered,
        sample.jitter[0],
        sample.jitter[1]
    );
    for tile in &tiles {
        let expected = tile
            .expected
            .map_or_else(|| "n/a".to_owned(), |[x, y]| format!("{x:.3},{y:.3}"));
        line.push_str(&format!(
            " tile={},{} found={:.3},{:.3} raw={:.6},{:.6} depth={:.6} expected={expected}",
            tile.origin.0,
            tile.origin.1,
            tile.found[0],
            tile.found[1],
            tile.raw[0],
            tile.raw[1],
            tile.depth
        ));
    }
    line.push_str(" verdict=");
    line.push_str(motion_verdict(&tiles));
    line
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
    vendor: Vendor,
    capture: Handle,
    sharpen: NonNull<c_void>,
}

impl Drop for Native {
    fn drop(&mut self) {
        unsafe {
            match self.vendor {
                Vendor::None => {}
                Vendor::Dlss(context) => native::dlss_destroy(context.as_ptr().cast()),
                Vendor::Session(session) => native::session_destroy(session.as_ptr().cast()),
            }
            native::capture_destroy(self.capture);
            native::sharpen_destroy(self.sharpen.as_ptr());
        }
    }
}

struct Gpu {
    root: IUnknown,
    pipeline: IUnknown,
    heaps: [IUnknown; SLOTS],
    normalized: [IUnknown; 3],
    output: Option<IUnknown>,
    exposure: Option<IUnknown>,
    exposure_upload: Option<IUnknown>,
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
    validated: [Option<IUnknown>; 4],
    view_formats: [u32; 3],
    // The engine motion's resource format, which the motion check copies it in.
    motion_format: u32,
    validated_generation: u32,
    validated_list: Option<IUnknown>,
    exposure_input: Option<(IUnknown, u32)>,
    exposure_initialized: bool,
    history: History,
    captured: [Captured; SLOTS],
    motion_readback: [Option<IUnknown>; SLOTS],
    motion_samples: [Option<MotionSample>; SLOTS],
    diagnostic_frames: u32,
    evaluations: u64,
    sharpen_reported: bool,
}

pub(crate) struct Sr12 {
    // Fields drop in order: vendor contexts first, then the resources they referenced.
    native: Native,
    gpu: Gpu,
    device: IUnknown,
    log: Log,
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
        unsafe { native::capture_flush(self.native.capture, ALL_SLOTS) };
    }
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
    caller: *const Setup,
    setup: &Setup,
    abi2: bool,
    external_exposure: bool,
) -> (Vendor, i32, u32, u32) {
    let device = setup.device;
    if setup.backend == RSF_SR_NONE {
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
    if setup.backend == RSF_SR_DLSS {
        // DLSS reads its setup from the caller's own structure.
        let dlss = unsafe { ptr::addr_of!((*caller).dlss) };
        let mut context: Handle = ptr::null_mut();
        let out = ptr::from_mut(&mut context).cast();
        let mut result = unsafe {
            if abi2 && !setup.dlss_registration.is_null() {
                native::dlss_create_registered(device, dlss, setup.dlss_registration.cast(), out)
            } else if !setup.streamline_host.is_null() {
                native::dlss_create_shared(device, dlss, setup.streamline_host, out)
            } else {
                native::dlss_create(device, dlss, out)
            }
        };
        let vendor = NonNull::new(context).map_or(Vendor::None, Vendor::Dlss);
        let (mut width, mut height) = (0, 0);
        if result == OK {
            result = unsafe {
                native::dlss_plan(
                    context.cast(),
                    setup.output_width,
                    setup.output_height,
                    setup.quality,
                    &mut width,
                    &mut height,
                )
            };
        }
        if result == OK {
            unsafe { native::dlss_set_sdr(context.cast(), setup.input_sdr) };
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
    let mut session: Handle = ptr::null_mut();
    let mut result =
        unsafe { native::session_create(&alternate, ptr::from_mut(&mut session).cast()) };
    let vendor = NonNull::new(session).map_or(Vendor::None, Vendor::Session);
    if result == OK {
        result = unsafe { native::session_select(session.cast(), setup.backend, setup.quality, 0) };
    }
    let (mut width, mut height) = (0, 0);
    if result == OK {
        let mut status: rsf_sr_session_status = unsafe { std::mem::zeroed() };
        status.struct_size = size_of::<rsf_sr_session_status>() as u32;
        result = unsafe { native::session_get_status(session.cast_const().cast(), &mut status) };
        (width, height) = (status.render_width, status.render_height);
    }
    (vendor, result, width, height)
}

unsafe fn create_gpu(
    setup: &Setup,
    (width, height): (u32, u32),
    normalize_color: bool,
    external_exposure: bool,
) -> Option<Gpu> {
    let device = setup.device;
    let (root, pipeline) = unsafe { native::create_pipeline(device, setup.log, setup.user) }?;
    let root = unsafe { com_ref::owned(root) }?;
    let pipeline = unsafe { com_ref::owned(pipeline) }?;
    let heap = || unsafe { com_ref::owned(native::create_heap(device)) };
    let heaps = [heap()?, heap()?, heap()?];
    let texture =
        |w, h, format| unsafe { com_ref::owned(native::create_texture(device, w, h, format)) };
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
    let output = if setup.backend != RSF_SR_NONE {
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
        && setup.backend != RSF_SR_NONE
        && setup.backend != RSF_SR_DLSS
    {
        // SDR has no changing pre-exposure. XeSS requires an exposure texture when auto
        // exposure is disabled; feed the actual constant scale, not HDR adaptation.
        let texture = unsafe { com_ref::owned(native::create_exposure_texture(device)) }?;
        let upload = unsafe {
            com_ref::owned(native::create_exposure_upload(
                device,
                EXPOSURE_UPLOAD_BYTES,
            ))
        }?;
        if unsafe { native::write_exposure_upload(upload.as_raw(), 1.0) } == 0 {
            return None;
        }
        (exposure, exposure_upload) = (Some(texture), Some(upload));
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

unsafe fn create(setup: *const Setup) -> Result<Sr12, i32> {
    let Some((s, size)) = (unsafe { read_setup(setup) }) else {
        return Err(INVALID_ARGUMENT);
    };
    if (size as usize) < offset_of!(Setup, engine_input_extents)
        || s.device.is_null()
        || s.output_width == 0
        || s.output_height == 0
        || s.backend > RSF_SR_XESS
        || s.quality > MAX_QUALITY
    {
        return Err(INVALID_ARGUMENT);
    }
    if s.abi_version != 1 && s.abi_version != 2 {
        return Err(ABI_MISMATCH);
    }
    let abi2 = s.abi_version == 2;
    if abi2 && (size as usize) < size_of::<Setup>() {
        return Err(INVALID_ARGUMENT);
    }
    let engine_extents = abi2 && s.engine_input_extents != 0;
    if engine_extents
        && (s.render_width == 0
            || s.render_height == 0
            || s.render_width > s.output_width
            || s.render_height > s.output_height)
    {
        return Err(INVALID_ARGUMENT);
    }
    // Capture is optional. An allocation failure leaves SR usable and capture unavailable.
    let capture = native::capture_create();
    let Some(sharpen) = NonNull::new(native::sharpen_create()) else {
        unsafe { native::capture_destroy(capture) };
        return Err(INIT_FAILED);
    };
    let mut native = Native {
        vendor: Vendor::None,
        capture,
        sharpen,
    };
    let device = unsafe { com_ref::retain(s.device) };
    let external_exposure = abi2 && s.external_exposure != 0;
    let (vendor, result, mut width, mut height) =
        unsafe { open_vendor(setup, &s, abi2, external_exposure) };
    // A vendor that reports success must hand out its context.
    let opened = s.backend == RSF_SR_NONE || !matches!(vendor, Vendor::None);
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
        s.backend != RSF_SR_NONE || unsafe { native::capture_enabled(capture) } != 0;
    let gpu = unsafe { create_gpu(&s, (width, height), normalize_color, external_exposure) }
        .ok_or(INIT_FAILED)?;
    let sharpening = sharpening_policy::defaults();
    Ok(Sr12 {
        native,
        gpu,
        device,
        log: Log::new(s.log, s.user),
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
            motion_format: 0,
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
            motion_samples: [None; SLOTS],
            diagnostic_frames: 0,
            evaluations: 0,
            sharpen_reported: false,
        }),
    })
}

impl Sr12 {
    fn capture(&self) -> Handle {
        self.native.capture
    }

    unsafe fn owned(&self, child: Handle) -> bool {
        unsafe { native::owned_by(child, self.device.as_raw()) != 0 }
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
        if unsafe { native::depth_domains(pass) } != 0 || !self.camera_matches(pass) {
            return false;
        }
        if self.external_exposure {
            if pass.exposure.is_null() {
                return false;
            }
            let desc = unsafe { native::describe(pass.exposure) };
            if !unsafe { self.owned(pass.exposure) } || !exposure_accepted(&desc) {
                return false;
            }
            state.exposure_input =
                unsafe { retain_object(pass.exposure) }.map(|texture| (texture, desc.srv_format));
        }
        let pointers = [pass.color_input, pass.depth, pass.motion, pass.color_output];
        let checked = |index: usize| index != OUTPUT || self.backend != RSF_SR_NONE;
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
            let desc = unsafe { native::describe(resource) };
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
            if index == MOTION {
                state.motion_format = desc.format;
            }
        }
        state.validated = pointers.map(|resource| unsafe { retain_object(resource) });
        state.validated_generation = pass.resource_generation;
        true
    }

    // Reads back the quarter-point tiles of the normalized motion, the engine's motion and the
    // normalized depth after normalization, and on the slot's next use logs them beside the
    // motion the pass camera predicts there (motion_check_line).
    unsafe fn motion_diagnostic(&self, state: &mut State, list: Handle, pass: &Pass, slot: usize) {
        if self.log.callback.is_none() {
            return;
        }
        if let Some(sample) = state.motion_samples[slot].take()
            && let Some(buffer) = state.motion_readback[slot].as_ref()
        {
            let mapped = unsafe { native::map(buffer.as_raw(), READBACK_BYTES) };
            if !mapped.is_null() {
                let bytes = unsafe { std::slice::from_raw_parts(mapped, READBACK_BYTES as usize) };
                let line = motion_check_line(bytes, &sample);
                unsafe { native::unmap(buffer.as_raw()) };
                self.log.say(&line);
            }
        }
        state.evaluations += 1;
        if (state.diagnostic_frames >= DIAGNOSTIC_FRAMES
            && !state.evaluations.is_multiple_of(DIAGNOSTIC_INTERVAL))
            || !diagnostic_fits(self.width, self.height)
        {
            return;
        }
        if state.motion_readback[slot].is_none() {
            state.motion_readback[slot] = unsafe {
                com_ref::owned(native::create_readback(
                    self.device.as_raw(),
                    READBACK_BYTES,
                ))
            };
        }
        let Some(buffer) = state.motion_readback[slot].as_ref() else {
            return;
        };
        let sources = [
            (0, self.gpu.normalized[MOTION].as_raw(), RG16_FLOAT),
            (RAW_MOTION_SECTION, pass.motion, state.motion_format),
            (
                DEPTH_SECTION,
                self.gpu.normalized[DEPTH].as_raw(),
                R32_FLOAT,
            ),
        ];
        unsafe {
            for (section, source, format) in sources {
                native::transition(list, source, SHADER_READ, COPY_SOURCE);
                for tile in 0..4 {
                    let (x, y) = tile_origin(tile, self.width, self.height);
                    native::copy_tile(
                        list,
                        buffer.as_raw(),
                        section + u64::from(tile) * TILE_BYTES,
                        source,
                        format,
                        x,
                        y,
                    );
                }
                native::transition(list, source, COPY_SOURCE, SHADER_READ);
            }
        }
        state.motion_samples[slot] = Some(MotionSample {
            frame: frame_id(pass),
            extent: (self.width, self.height),
            clip_to_previous: pass.camera.clip_to_previous_clip,
            jitter: pass.camera.jitter_pixels,
            motion_to_uv: pass.motion_to_uv,
            camera_motion_included: pass.motion_camera_included,
            motion_jittered: pass.camera.motion_jittered,
        });
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
        if pass.source_frame_id == 0 {
            record.flags |= RSF_FRAME_FLAG_AMBIGUOUS_ID;
        }
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
            native::capture_record(
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
        let device = self.device.as_raw();
        let heap = self.gpu.heaps[slot].as_raw();
        let inputs = [pass.color_input, pass.depth, pass.motion];
        unsafe {
            for (index, &input) in inputs.iter().enumerate() {
                native::write_srv(device, heap, index as u32, input, state.view_formats[index]);
            }
            let (exposure_input, exposure_view) = state
                .exposure_input
                .as_ref()
                .map_or((ptr::null_mut(), R32_FLOAT), |(texture, view)| {
                    (texture.as_raw(), *view)
                });
            native::write_srv(device, heap, EXPOSURE_SRV, exposure_input, exposure_view);
            for (index, texture) in self.gpu.normalized.iter().enumerate() {
                native::write_uav(
                    device,
                    heap,
                    NORMALIZED_UAV + index as u32,
                    texture.as_raw(),
                    NORMALIZED_FORMATS[index],
                );
            }
            let exposure_output = if self.external_exposure {
                raw(self.gpu.exposure.as_ref())
            } else {
                ptr::null_mut()
            };
            native::write_uav(device, heap, EXPOSURE_UAV, exposure_output, R32_FLOAT);
            let constants = self.constants(pass);
            native::dispatch(
                list,
                heap,
                self.gpu.root.as_raw(),
                self.gpu.pipeline.as_raw(),
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
        unsafe {
            for texture in &self.gpu.normalized {
                native::transition(list, texture.as_raw(), before, after);
            }
            if self.external_exposure {
                native::transition(list, raw(self.gpu.exposure.as_ref()), before, after);
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
        match self.native.vendor {
            Vendor::None => OK,
            Vendor::Dlss(context) if !pass.native_frame_context.is_null() => unsafe {
                native::dlss_evaluate_borrowed(
                    context.as_ptr().cast(),
                    list,
                    frame,
                    pass.native_frame_context,
                )
            },
            Vendor::Dlss(context) => unsafe {
                native::dlss_evaluate(context.as_ptr().cast(), list, frame)
            },
            Vendor::Session(session) => unsafe {
                native::session_evaluate(session.as_ptr().cast(), list, frame)
            },
        }
    }

    unsafe fn evaluate(&self, list: Handle, pass: &Pass, slot: usize) -> i32 {
        let mut guard = lock(&self.state);
        let state = &mut *guard;
        if !unsafe { self.inputs_valid(state, pass) } {
            return INVALID_ARGUMENT;
        }
        let sharpen = *lock(&self.sharpening);
        unsafe { native::capture_flush(self.capture(), slot as u32) };
        if raw(state.validated_list.as_ref()) != list {
            if !unsafe { self.owned(list) } {
                return INVALID_ARGUMENT;
            }
            state.validated_list = unsafe { retain_object(list) };
        }
        unsafe {
            self.normalize(state, list, pass, slot);
            self.transition_inputs(list, UNORDERED_ACCESS, SHADER_READ);
            // The caller joins a slot's previous GPU completion before entering this evaluation.
            self.motion_diagnostic(state, list, pass, slot);
        }
        let reset = !state.history.continues(pass);
        let record = self.record(pass, reset);
        let normalized = self.gpu.normalized.each_ref().map(Interface::as_raw);
        let view = preview(self.input_sdr);
        unsafe {
            if native::dump_begin_record(&record, self.backend) != 0 {
                native::dump_pass_metadata(pass);
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
                    native::copy_unit_exposure(list, exposure.as_raw(), upload.as_raw());
                    native::transition(list, exposure.as_raw(), COPY_DEST, SHADER_READ);
                }
                state.exposure_initialized = true;
            }
            frame.exposure =
                unsafe { self.resource(exposure.as_raw(), false, record.resource_generation) };
            (frame.exposure.width, frame.exposure.height) = (1, 1);
        }
        let output = raw(self.gpu.output.as_ref());
        unsafe {
            frame.color = self.resource(normalized[COLOR], false, record.resource_generation);
            frame.depth = self.resource(normalized[DEPTH], false, record.resource_generation);
            frame.motion = self.resource(normalized[MOTION], false, record.resource_generation);
            frame.output = self.resource(output, true, record.resource_generation);
            frame.sharpness = sharpening_policy::vendor_sharpness(&sharpen, self.backend);
        }
        frame.jitter_x = pass.camera.jitter_pixels[0];
        frame.jitter_y = pass.camera.jitter_pixels[1];
        // The producer measures pixel jitter from the actual GPU projection, including its Y
        // conversion.
        (frame.motion_scale_x, frame.motion_scale_y) = (1.0, 1.0);
        (frame.pre_exposure, frame.view_space_to_meters) = (1.0, 1.0);
        frame.reset = u32::from(
            reset || unsafe { native::capture_force_reset(self.capture(), record.frame_id) } != 0,
        );
        unsafe {
            native::dump_sr_frame(&frame, self.backend, u32::from(self.input_sdr));
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
            native::dump_result(&record, c"sr".as_ptr(), self.backend, result);
            native::capture_event(
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
                native::fg_handoff(list, &record, normalized[DEPTH], normalized[MOTION]);
            }
        }
        if result == OK && self.backend != RSF_SR_NONE {
            unsafe { self.sharpen(state, list, &frame, pass, &sharpen, slot) };
        }
        unsafe { self.transition_inputs(list, SHADER_READ, UNORDERED_ACCESS) };
        if result != OK {
            state.history.frame = 0;
            return result;
        }
        if self.backend != RSF_SR_NONE {
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
        let accepted = unsafe {
            native::sharpen_run(
                self.native.sharpen.as_ptr(),
                list,
                raw(self.gpu.output.as_ref()),
                self.gpu.normalized[MOTION].as_raw(),
                self.gpu.normalized[DEPTH].as_raw(),
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
            self.log
                .say("D3D12 sharpening unavailable; retaining unsharpened SR output");
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
            native::transition(list, output, UNORDERED_ACCESS, COPY_SOURCE);
            native::copy_region(
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
            native::transition(list, output, COPY_SOURCE, UNORDERED_ACCESS);
        }
    }

    unsafe fn capture_scene(&self, list: Handle, pass: &Pass, slot: usize) -> i32 {
        if unsafe { native::depth_domains(pass) } != 0 {
            return INVALID_ARGUMENT;
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
        let desc = unsafe { native::describe(scene) };
        if !unsafe { self.owned(scene) }
            || desc.width != u64::from(record.output_width)
            || desc.height != record.output_height
            || desc.samples != 1
        {
            return INVALID_ARGUMENT;
        }
        if unsafe { native::capture_wants(self.capture(), record.frame_id) } == 0 {
            return NOT_READY;
        }
        let recorded = unsafe {
            native::capture_record(
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

#[unsafe(export_name = "rsf_sr12_create")]
pub(crate) unsafe extern "C" fn rsf_core_sr12_create(
    setup: *const Setup,
    out: *mut *mut Sr12,
) -> i32 {
    if out.is_null() {
        return INVALID_ARGUMENT;
    }
    unsafe { out.write(ptr::null_mut()) };
    match unsafe { create(setup) } {
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

#[unsafe(export_name = "rsf_sr12_quality_supported")]
extern "C" fn rsf_core_sr12_quality_supported(backend: u32, quality: u32) -> u32 {
    u32::from(
        backend <= RSF_SR_AUTO
            && quality <= MAX_QUALITY
            && !(backend == RSF_SR_DLSS && quality == MAX_QUALITY),
    )
}

#[cfg(not(test))]
unsafe fn create_checked(setup: &Setup) -> Result<Sr12, i32> {
    if rsf_core_sr12_quality_supported(setup.backend, setup.quality) == 0 {
        return Err(INVALID_ARGUMENT);
    }
    let owner = unsafe { create(setup) }?;
    if owner.width == 0
        || owner.height == 0
        || owner.width > owner.output_width
        || owner.height > owner.output_height
    {
        return Err(FEATURE_FAILED);
    }
    Ok(owner)
}

#[cfg(not(test))]
#[unsafe(export_name = "rsf_sr12_create_planned")]
unsafe extern "C" fn rsf_core_sr12_create_planned(
    setup: *const Setup,
    plan: *mut PlanResult,
    out: *mut *mut Sr12,
) -> i32 {
    if out.is_null() {
        return INVALID_ARGUMENT;
    }
    unsafe { out.write(ptr::null_mut()) };
    if plan.is_null() || (unsafe { plan.cast::<u32>().read() } as usize) < size_of::<PlanResult>() {
        return INVALID_ARGUMENT;
    }
    if unsafe { (*plan).abi_version } != PLAN_ABI_VERSION {
        return ABI_MISMATCH;
    }
    unsafe {
        plan.write(PlanResult {
            struct_size: size_of::<PlanResult>() as u32,
            abi_version: PLAN_ABI_VERSION,
            backend: RSF_SR_NONE,
            render_width: 0,
            render_height: 0,
            requested_result: INVALID_ARGUMENT,
        });
    }
    let plan = unsafe { &mut *plan };
    let Some((mut setup, size)) = (unsafe { read_setup(setup) }) else {
        return INVALID_ARGUMENT;
    };
    if (size as usize) < offset_of!(Setup, engine_input_extents)
        || setup.device.is_null()
        || setup.output_width == 0
        || setup.output_height == 0
        || setup.quality > MAX_QUALITY
        || (setup.backend > RSF_SR_XESS && setup.backend != RSF_SR_AUTO)
    {
        return INVALID_ARGUMENT;
    }
    if setup.abi_version != 1 && setup.abi_version != 2 {
        plan.requested_result = ABI_MISMATCH;
        return ABI_MISMATCH;
    }
    if setup.abi_version == 2 && (size as usize) < size_of::<Setup>() {
        return INVALID_ARGUMENT;
    }
    if setup.backend == RSF_SR_AUTO {
        let device = unsafe { crate::d3d12_host::borrow(&setup.device) };
        let Some((vendor, software)) = crate::d3d12_host::adapter_identity(device) else {
            plan.requested_result = NOT_READY;
            return NOT_READY;
        };
        setup.backend = crate::rsf_gpu_default_sr_backend(vendor, u32::from(software));
    }
    let owner = match unsafe { create_checked(&setup) } {
        Ok(owner) => {
            plan.requested_result = OK;
            owner
        }
        Err(result) => {
            plan.requested_result = result;
            if setup.backend == RSF_SR_NONE {
                return result;
            }
            setup.backend = RSF_SR_NONE;
            match unsafe { create_checked(&setup) } {
                Ok(owner) => owner,
                Err(result) => return result,
            }
        }
    };
    let (backend, width, height) = (owner.backend, owner.width, owner.height);
    let handle = crate::allocate_handle(owner);
    if handle.is_null() {
        if plan.requested_result == OK {
            plan.requested_result = INIT_FAILED;
        }
        return INIT_FAILED;
    }
    plan.backend = backend;
    plan.render_width = width;
    plan.render_height = height;
    unsafe { out.write(handle) };
    OK
}

#[unsafe(export_name = "rsf_sr12_plan")]
pub(super) unsafe extern "C" fn rsf_core_sr12_plan(
    owner: *const Sr12,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if width.is_null() || height.is_null() {
        return INVALID_ARGUMENT;
    }
    unsafe {
        width.write(owner.width);
        height.write(owner.height);
    }
    OK
}

#[unsafe(export_name = "rsf_sr12_evaluate")]
pub(super) unsafe extern "C" fn rsf_core_sr12_evaluate(
    owner: *mut Sr12,
    list: Handle,
    pass: *const Pass,
    slot: u32,
) -> i32 {
    let (Some(owner), Some(pass)) = (unsafe { owner.as_ref() }, unsafe { checked(pass) }) else {
        return INVALID_ARGUMENT;
    };
    if list.is_null() || slot as usize >= SLOTS {
        return INVALID_ARGUMENT;
    }
    unsafe { owner.evaluate(list, pass, slot as usize) }
}

#[unsafe(export_name = "rsf_sr12_set_sharpening")]
pub(super) unsafe extern "C" fn rsf_core_sr12_set_sharpening(
    owner: *mut Sr12,
    options: *const rsf_sharpening_options,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if options.is_null()
        || (unsafe { options.cast::<u32>().read() } as usize) < size_of::<rsf_sharpening_options>()
    {
        return INVALID_ARGUMENT;
    }
    let options = unsafe { options.read() };
    if options.abi_version != RSF_SHARPENING_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if !sharpening_policy::valid(&options) {
        return INVALID_ARGUMENT;
    }
    *lock(&owner.sharpening) = options;
    OK
}

#[unsafe(export_name = "rsf_sr12_destroy")]
pub(super) unsafe extern "C" fn rsf_core_sr12_destroy(owner: *mut Sr12) {
    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner) });
    }
}

#[unsafe(export_name = "rsf_sr12_present")]
pub(super) unsafe extern "C" fn rsf_core_sr12_present(
    owner: *mut Sr12,
    swapchain: Handle,
    sync_interval: u32,
    flags: u32,
    begin: u32,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if swapchain.is_null() {
        return INVALID_ARGUMENT;
    }
    match owner.native.vendor {
        Vendor::Dlss(context) => unsafe {
            native::dlss_present(
                context.as_ptr().cast(),
                swapchain,
                sync_interval,
                flags,
                begin,
            )
        },
        _ => OK,
    }
}

#[unsafe(export_name = "rsf_sr12_capture")]
pub(super) unsafe extern "C" fn rsf_core_sr12_capture(
    owner: *mut Sr12,
    directory: *const c_char,
    first_frame: u64,
    frames: u32,
) -> i32 {
    match unsafe { owner.as_ref() } {
        Some(owner)
            if unsafe {
                native::capture_configure(owner.capture(), directory, first_frame, frames)
            } != 0 =>
        {
            OK
        }
        _ => INVALID_ARGUMENT,
    }
}

#[unsafe(export_name = "rsf_sr12_capture_flush")]
pub(super) unsafe extern "C" fn rsf_core_sr12_capture_flush(owner: *mut Sr12, slot: u32) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if slot as usize >= SLOTS {
        return INVALID_ARGUMENT;
    }
    if unsafe { native::capture_flush(owner.capture(), slot) } != 0 {
        OK
    } else {
        FEATURE_FAILED
    }
}

#[unsafe(export_name = "rsf_sr12_capture_scene")]
pub(super) unsafe extern "C" fn rsf_core_sr12_capture_scene(
    owner: *mut Sr12,
    list: Handle,
    pass: *const Pass,
    slot: u32,
) -> i32 {
    let (Some(owner), Some(pass)) = (unsafe { owner.as_ref() }, unsafe { checked(pass) }) else {
        return INVALID_ARGUMENT;
    };
    if list.is_null() || slot as usize >= SLOTS || pass.color_input.is_null() {
        return INVALID_ARGUMENT;
    }
    unsafe { owner.capture_scene(list, pass, slot as usize) }
}
