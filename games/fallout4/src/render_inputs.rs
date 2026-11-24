// SPDX-License-Identifier: MIT
//! Fallout 4 native render inputs: the code guards, the sampled native renderer owner and
//! temporal state, the TAA camera cache scan, the logical-to-platform target maps, resource and
//! raster admission, the steady camera history, and the GameSDK pass each reader leases.
//!
//! Rust samples the D3D11 resources and lends their owned references to the
//! native World caller. The native leaves format and report existing receipts.

mod native;

use crate::build_layout::{
    RENDERER_CHAIN, RENDERER_CONTEXT, RENDERER_DEVICE, RENDERER_OBJECT, RENDERER_POINTER,
    RENDERER_WINDOW,
};
use crate::camera::{self, CameraRaster, NativeCamera};
use crate::creation;
use crate::d3d11::same_native;
use crate::depth_domains::{self, Snapshot};
use crate::sys::{
    NativeFramebufferReceipt as FramebufferReceipt, NativeInputHistoryReceipt as HistoryReceipt,
    NativeInputReceipt as Receipt, NativeInputResourceFacts as ResourceFacts,
    NativeInputState as State, NativeInputSwapFacts as SwapFacts,
};
use crate::sys::{
    native_input_colour as SLOT_COLOUR, native_input_depth as SLOT_DEPTH,
    native_input_motion as SLOT_MOTION, native_input_no_slot as NO_SLOT,
    native_input_pipboy as SLOT_PIPBOY, native_input_refraction_normals as SLOT_REFRACTION_NORMALS,
    native_input_vats as SLOT_VATS,
};
use crate::{Plain, bits, guard, image, load, read, same_object};
use rsf_game_sys::RSF_SCREEN_UNKNOWN;
use rsf_game_sys::{
    RSF_GAME_RENDER_NO_FG, RSF_GAME_RENDER_RESET, RSF_GAME_RENDER_SR, rsf_game_render_pass,
};
use std::cell::Cell;
use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use windows::Win32::Graphics::Direct3D11::{ID3D11DeviceContext, ID3D11Texture2D};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;

type Flat = [f32; 16];
pub(crate) type Report = Option<ReportFn>;
type ReportFn = unsafe extern "C" fn(*const c_char);

const STATE_RVA: usize = 0x3d871a0;
const TARGETS_RVA: usize = 0x2f4d750;
const WORLD_GRAPH_POINTER: usize = 0x32dd338;
const DELTA_RVA: usize = 0x339f430;
/// The guarded SR call site the raster pass is keyed by.
const RASTER_PASS_RVA: usize = 0x21fa283;

// Enumerators compared by value; render_inputs.c asserts each against the SDK headers.
const DXGI_FORMAT_R11G11B10_FLOAT: u32 = 26;
const DXGI_FORMAT_R16G16_FLOAT: u32 = 34;
const DXGI_FORMAT_R32_TYPELESS: u32 = 39;
const DXGI_FORMAT_R32_FLOAT: u32 = 41;
const DXGI_FORMAT_R24G8_TYPELESS: u32 = 44;
const DXGI_FORMAT_R24_UNORM_X8_TYPELESS: u32 = 46;
const D3D11_SRV_DIMENSION_TEXTURE2D: u32 = 4;
const D3D11_RTV_DIMENSION_TEXTURE2D: u32 = 4;

static VERIFIED: AtomicBool = AtomicBool::new(false);
static REPORTED: AtomicU64 = AtomicU64::new(0);
static FRAMEBUFFER_REPORTED: AtomicU32 = AtomicU32::new(0);
static SEEN_CHANGES: [AtomicBool; 1024] = [const { AtomicBool::new(false) }; 1024];
static CHANGE_BUDGET: AtomicU32 = AtomicU32::new(0);
static SEEN_REASONS: [AtomicBool; 256] = [const { AtomicBool::new(false) }; 256];
static REASON_BUDGET: AtomicU32 = AtomicU32::new(0);

/// One native platform render target record (0x30 bytes at renderer+0xa58).
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[allow(dead_code)] // the copy and UAV fields are compared as part of the record
struct Target {
    texture: u64,
    copy_texture: u64,
    rtv: u64,
    srv: u64,
    copy_srv: u64,
    uav: u64,
}
// SAFETY: six integers; any bytes are a value.
unsafe impl Plain for Target {}

#[derive(Default)]
struct DeviceFacts {
    device: u64,
    chain_device: u64,
    immediate: u32,
    chain_device_read: u32,
}

#[derive(Default)]
struct ViewportFacts {
    count: u32,
    copied: u32,
    viewport: [f32; 6],
}

pub(crate) struct FramebufferLease {
    // Match the native lease's release order.
    pub(crate) texture: ID3D11Texture2D,
    pub(crate) chain: IDXGISwapChain,
    pub(crate) context: ID3D11DeviceContext,
    pub(crate) native_frame: u64,
    pub(crate) window_key: u64,
}

unsafe extern "C" {
    fn rsf_fallout4_input_object(inputs: *const c_void, slot: u32) -> *mut c_void;
    fn rsf_fallout4_inputs_report_reader(
        report: ReportFn,
        stage: *const c_char,
        receipt: &Receipt,
        pass: &rsf_game_render_pass,
    );
    fn rsf_fallout4_inputs_report_mismatch(
        report: ReportFn,
        before: &State,
        after: &State,
        mask: u32,
    );
    fn rsf_fallout4_inputs_report_history(report: ReportFn, receipt: &HistoryReceipt);
    fn rsf_fallout4_inputs_report_framebuffer(
        report: ReportFn,
        stage: *const c_char,
        owner_stage: *const c_char,
        receipt: &FramebufferReceipt,
    );
}

const _: () = {
    use std::mem::offset_of;
    assert!(size_of::<State>() == 128 && offset_of!(State, frame) == 56);
    assert!(offset_of!(State, ratio) == 92 && offset_of!(State, viewport) == 100);
    assert!(offset_of!(State, delta_seconds) == 116 && offset_of!(State, default_viewport) == 120);
    assert!(offset_of!(State, dynamic_viewport) == 121);
    assert!(size_of::<Target>() == 0x30);
    assert!(size_of::<SwapFacts>() == 24 && offset_of!(SwapFacts, read) == 8);
    assert!(size_of::<ResourceFacts>() == 80 && offset_of!(ResourceFacts, read) == 24);
    assert!(offset_of!(ResourceFacts, view_format) == 60);
    assert!(offset_of!(ResourceFacts, reserved) == 76);
    assert!(size_of::<Receipt>() == 912 && offset_of!(Receipt, native) == 128);
    assert!(offset_of!(Receipt, views) == 416 && offset_of!(Receipt, rtv) == 656);
    assert!(offset_of!(Receipt, swap) == 736 && offset_of!(Receipt, expected_renderer) == 760);
    assert!(offset_of!(Receipt, expected) == 784 && offset_of!(Receipt, after_frame) == 808);
    assert!(offset_of!(Receipt, camera_read) == 836 && offset_of!(Receipt, device_match) == 852);
    assert!(offset_of!(Receipt, viewport) == 864 && offset_of!(Receipt, confirmed_viewport) == 888);
    assert!(size_of::<HistoryReceipt>() == 96 && offset_of!(HistoryReceipt, raster) == 32);
    assert!(size_of::<FramebufferReceipt>() == 344);
    assert!(offset_of!(FramebufferReceipt, swap) == 256);
    assert!(offset_of!(FramebufferReceipt, target_texture) == 280);
    assert!(offset_of!(FramebufferReceipt, platform) == 304);
    assert!(offset_of!(FramebufferReceipt, rtv_mip) == 340);
};

const NO_CAMERA: NativeCamera = NativeCamera {
    view: [0.0; 16],
    projection: [0.0; 16],
    view_projection_unjittered: [0.0; 16],
    previous_view_projection_unjittered: [0.0; 16],
    position_adjust: [0.0; 3],
    previous_position_adjust: [0.0; 3],
    jitter_ndc: [0.0; 2],
};

const NO_RASTER: CameraRaster = CameraRaster {
    xywh: [0.0; 4],
    crop: [0; 4],
};

/// Reader refusal stages. The once-per-process receipt mask is indexed by them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
enum Stage {
    Unverified,
    Owner,
    Dimensions,
    Delta,
    CameraOwner,
    Cache,
    TemporalFlags,
    ViewportRead,
    ViewportBounds,
    Ratio,
    ColourMap,
    MotionMap,
    DepthMap,
    CacheRead,
    CacheDuplicate,
    CacheMissing,
    CameraRead,
    TargetRead,
    Context,
    Device,
    ColourSrv,
    MotionSrv,
    DepthSrv,
    Swap,
    ColourRtv,
    ExtentFormat,
    CameraMath,
    StateChanged,
    CameraChanged,
    HistoryChanged,
    Accepted,
    Exception,
    ProducerChanged,
}

const NAMES: [&CStr; 33] = [
    c"unverified",
    c"owner",
    c"dimensions",
    c"delta",
    c"camera-owner",
    c"cache",
    c"temporal-flags",
    c"viewport-read",
    c"viewport-bounds",
    c"ratio",
    c"colour-map",
    c"motion-map",
    c"depth-map",
    c"cache-read",
    c"cache-duplicate",
    c"cache-missing",
    c"camera-read",
    c"target-read",
    c"context",
    c"device",
    c"colour-srv",
    c"motion-srv",
    c"depth-srv",
    c"swap",
    c"colour-rtv",
    c"extent-format",
    c"camera-math",
    c"state-changed",
    c"camera-changed",
    c"history-changed",
    c"accepted",
    c"exception",
    c"producer-changed",
];
const _: () = assert!(Stage::ProducerChanged as usize + 1 == NAMES.len() && NAMES.len() <= 64);

/// Framebuffer refusal stages, with their own receipt mask.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
enum Framebuffer {
    Unverified,
    Owner,
    Map,
    Target,
    Context,
    Device,
    Resource,
    TargetTexture,
    ChainDevice,
    TextureDevice,
    Extent,
    Rtv,
    Swap,
    Window,
    SwapExtent,
    OwnerChanged,
    MapChanged,
    TargetChanged,
    Accepted,
}

const FRAMEBUFFER_NAMES: [&CStr; 19] = [
    c"unverified",
    c"owner",
    c"map",
    c"target",
    c"context",
    c"device",
    c"resource",
    c"target-texture",
    c"chain-device",
    c"texture-device",
    c"extent",
    c"rtv",
    c"swap",
    c"window",
    c"swap-extent",
    c"owner-changed",
    c"map-changed",
    c"target-changed",
    c"accepted",
];
const _: () = assert!(
    Framebuffer::Accepted as usize + 1 == FRAMEBUFFER_NAMES.len() && FRAMEBUFFER_NAMES.len() <= 32
);

/// The platform target record for a platform slot.
fn target_at(renderer: usize, platform: u32) -> usize {
    renderer.wrapping_add(0xa58 + platform as usize * 0x30)
}

/// The depth target record for a depth slot; its SRV is 0x88 further.
fn depth_at(renderer: usize, slot: u32) -> usize {
    renderer.wrapping_add(0x1d48 + slot as usize * 0x98)
}

/// The platform slots of the logical colour, motion and depth targets, bounds-checked.
fn slot_maps() -> Option<[u32; 3]> {
    let targets = image().wrapping_add(TARGETS_RVA);
    let slot = |offset: usize, limit: u32| {
        read::<u32>(targets.wrapping_add(offset)).filter(|&slot| slot < limit)
    };
    Some([
        slot(0xdf4 + 4, 101)?,
        slot(0xdf4 + 32 * 4, 101)?,
        slot(0xf84 + 4, 13)?,
    ])
}

/// The logical-to-platform colour map entry for `logical`.
fn colour_map(logical: u32) -> usize {
    image().wrapping_add(TARGETS_RVA + 0xdf4 + logical as usize * 4)
}

/// The fields `owner()` reads.
fn same_native_owner(a: &State, b: &State) -> bool {
    a.renderer == b.renderer
        && a.device == b.device
        && a.context == b.context
        && a.chain == b.chain
        && a.window == b.window
        && a.frame == b.frame
        && a.width == b.width
        && a.height == b.height
}

/// Every field by exact bits for the floats, frame timing excluded.
fn same_native_state(a: &State, b: &State) -> bool {
    same_native_owner(a, b)
        && a.camera == b.camera
        && a.cache == b.cache
        && a.count == b.count
        && a.taa == b.taa
        && a.disable == b.disable
        && a.colour == b.colour
        && a.motion == b.motion
        && a.depth == b.depth
        && a.default_viewport == b.default_viewport
        && a.dynamic_viewport == b.dynamic_viewport
        && bits(&a.ratio) == bits(&b.ratio)
        && bits(&a.viewport) == bits(&b.viewport)
}

fn same_camera(a: &NativeCamera, b: &NativeCamera) -> bool {
    bits(&a.view) == bits(&b.view)
        && bits(&a.projection) == bits(&b.projection)
        && bits(&a.view_projection_unjittered) == bits(&b.view_projection_unjittered)
        && bits(&a.previous_view_projection_unjittered)
            == bits(&b.previous_view_projection_unjittered)
        && bits(&a.position_adjust) == bits(&b.position_adjust)
        && bits(&a.previous_position_adjust) == bits(&b.previous_position_adjust)
        && bits(&a.jitter_ndc) == bits(&b.jitter_ndc)
}

/// Two matrices are finite and within a thousandth element by element.
fn close_matrix(a: &Flat, b: &Flat) -> bool {
    rsf_game_sys::camera_math::finite_matrix_close(a, b, 0.001)
}

/// The renderer owner, device, context, window, chain, native frame and output size.
fn owner(out: &mut State) -> Result<(), Stage> {
    let base = image();
    let graphics = base.wrapping_add(STATE_RVA);
    if !load(base.wrapping_add(RENDERER_POINTER), &mut out.renderer)
        || out.renderer != base.wrapping_add(RENDERER_OBJECT)
        || !load(out.renderer.wrapping_add(RENDERER_DEVICE), &mut out.device)
        || out.device == 0
        || !load(
            out.renderer.wrapping_add(RENDERER_CONTEXT),
            &mut out.context,
        )
        || out.context == 0
        || !load(out.renderer.wrapping_add(RENDERER_WINDOW), &mut out.window)
        || out.window == 0
        || !load(out.renderer.wrapping_add(RENDERER_CHAIN), &mut out.chain)
        || out.chain == 0
    {
        return Err(Stage::Owner);
    }
    if !load(graphics.wrapping_add(0x98), &mut out.frame)
        || !load(graphics.wrapping_add(0x78), &mut out.width)
        || !load(graphics.wrapping_add(0x7c), &mut out.height)
        || out.width == 0
        || out.height == 0
        || out.width > 16384
        || out.height > 16384
    {
        return Err(Stage::Dimensions);
    }
    Ok(())
}

/// The owner plus timing, world camera, TAA cache and flags, viewport metadata, ratios and the
/// colour, motion and depth slot maps.
fn state(out: &mut State) -> Result<(), Stage> {
    let base = image();
    let graphics = base.wrapping_add(STATE_RVA);
    let targets = base.wrapping_add(TARGETS_RVA);
    let mut graph = 0usize;
    owner(out)?;
    if !load(base.wrapping_add(DELTA_RVA), &mut out.delta_seconds)
        || !out.delta_seconds.is_finite()
        || out.delta_seconds < 0.0
    {
        return Err(Stage::Delta);
    }
    if !load(base.wrapping_add(WORLD_GRAPH_POINTER), &mut graph)
        || graph == 0
        || !load(graph.wrapping_add(0x140), &mut out.camera)
        || out.camera == 0
    {
        return Err(Stage::CameraOwner);
    }
    if !load(graphics.wrapping_add(0x140), &mut out.cache)
        || out.cache == 0
        || !load(graphics.wrapping_add(0x150), &mut out.count)
        || out.count == 0
        || out.count > 256
    {
        return Err(Stage::Cache);
    }
    if !load(graphics.wrapping_add(0xac), &mut out.taa)
        || out.taa == 0
        || !load(graphics.wrapping_add(0xb0), &mut out.disable)
        || out.disable != 0
    {
        return Err(Stage::TemporalFlags);
    }
    if !load(graphics.wrapping_add(0x88), &mut out.viewport)
        || !load(targets.wrapping_add(0xfb8), &mut out.ratio)
        || !load(targets.wrapping_add(0xfe4), &mut out.default_viewport)
        || !load(targets.wrapping_add(0xfe5), &mut out.dynamic_viewport)
    {
        return Err(Stage::ViewportRead);
    }
    if !load(targets.wrapping_add(0xdf4 + 4), &mut out.colour) || out.colour >= 101 {
        return Err(Stage::ColourMap);
    }
    if !load(targets.wrapping_add(0xdf4 + 32 * 4), &mut out.motion) || out.motion >= 101 {
        return Err(Stage::MotionMap);
    }
    if !load(targets.wrapping_add(0xf84 + 4), &mut out.depth) || out.depth >= 13 {
        return Err(Stage::DepthMap);
    }
    // State+88 is metadata, not the Context viewport submitted by the renderer. Reduced raster
    // still needs its separate producer/consumer contract.
    if out
        .ratio
        .iter()
        .any(|&ratio| !ratio.is_finite() || ratio <= 0.0 || ratio > 1.0)
    {
        return Err(Stage::Ratio);
    }
    Ok(())
}

/// The one bound viewport: `(admitted, count, viewport)`. The count is the copied count when the
/// first count was one.
fn active_full_viewport(
    context: usize,
    width: u32,
    height: u32,
    full: bool,
) -> (bool, u32, [f32; 6]) {
    let facts = native::viewport(context);
    if facts.count != 1 {
        return (false, facts.count, facts.viewport);
    }
    let [x, y, w, h, min, max] = facts.viewport;
    let admitted = facts.copied == 1
        && facts.viewport.iter().all(|value| value.is_finite())
        && x >= 0.0
        && y >= 0.0
        && w > 0.0
        && h > 0.0
        && f64::from(x) + f64::from(w) <= f64::from(width)
        && f64::from(y) + f64::from(h) <= f64::from(height)
        && (!full || (x == 0.0 && y == 0.0 && w == width as f32 && h == height as f32))
        // Depth endpoints describe the last native draw, not the projection or encoding of every
        // pixel already stored in the scene-depth texture.
        && min >= 0.0
        && min < max
        && max <= 1.0;
    (admitted, facts.copied, facts.viewport)
}

fn read_camera(entry: usize, out: &mut NativeCamera) -> bool {
    load(entry.wrapping_add(0x50), &mut out.view)
        && load(entry.wrapping_add(0x90), &mut out.projection)
        && load(
            entry.wrapping_add(0x110),
            &mut out.view_projection_unjittered,
        )
        && load(
            entry.wrapping_add(0x190),
            &mut out.previous_view_projection_unjittered,
        )
        && load(entry.wrapping_add(0x210), &mut out.position_adjust)
        && load(entry.wrapping_add(0x228), &mut out.previous_position_adjust)
        && load(image().wrapping_add(STATE_RVA + 4), &mut out.jitter_ndc)
}

/// The facts of one shader-resource view whose texture the lease holds in `slot`. `format` None
/// admits the two native depth SRV formats.
struct ViewCheck<'a> {
    facts: &'a mut ResourceFacts,
    resource_match: &'a mut u32,
    device_match: &'a mut u32,
}

#[allow(clippy::too_many_arguments)] // the view, its expectations and the receipt facts
fn view_texture(
    lease: &mut native::NativeLease,
    slot: u32,
    view: u64,
    expected: u64,
    owner: u64,
    format: Option<u32>,
    check: ViewCheck<'_>,
) -> bool {
    if view == 0 || expected == 0 {
        return false;
    }
    *check.facts = native::resource(lease, slot, view, false);
    let facts = *check.facts;
    // A missing resource or a failed Texture2D query.
    if facts.pointer == 0 {
        return false;
    }
    *check.resource_match = u32::from(same_native(facts.identity, expected));
    if *check.resource_match == 0 {
        return false;
    }
    *check.device_match = u32::from(same_object(facts.device, owner));
    let format = match format {
        None => {
            facts.view_format == DXGI_FORMAT_R24_UNORM_X8_TYPELESS
                || facts.view_format == DXGI_FORMAT_R32_FLOAT
        }
        Some(format) => facts.view_format == format,
    };
    *check.device_match != 0
        && facts.view_dimension == D3D11_SRV_DIMENSION_TEXTURE2D
        && facts.view_mip == 0
        && facts.view_mips == 1
        && format
}

/// The device and swap-chain device are the state's owner, read from the native chain.
fn same_devices(devices: &DeviceFacts, before: &State) -> bool {
    same_native(devices.device, before.device as u64)
        && devices.chain_device_read != 0
        && same_object(devices.device, devices.chain_device)
}

/// The swap chain was read and still has the state's window and size.
fn swap_matches(swap: &SwapFacts, before: &State) -> bool {
    swap.read != 0
        && swap.window == before.window as u64
        && swap.width == before.width
        && swap.height == before.height
}

fn extent(facts: &ResourceFacts, width: u32, height: u32) -> bool {
    facts.width == width
        && facts.height == height
        && facts.mip_levels == 1
        && facts.array_size == 1
        && facts.sample_count == 1
}

/// An optional full-output allocation for a logical target, leased into `slot` only when its
/// native record, view, device and extent agree before and after. Only full-output allocations
/// share the verified Main viewport/crop; the native format is deliberately not inferred from the
/// logical role.
fn auxiliary(
    lease: &mut native::NativeLease,
    native: &State,
    device: u64,
    logical: u32,
    slot: u32,
) -> bool {
    let map = colour_map(logical);
    let (mut platform, mut confirmed_platform) = (0u32, 0u32);
    let (mut target, mut confirmed_target) = (Target::default(), Target::default());
    if !load(map, &mut platform)
        || platform >= 101
        || !load(target_at(native.renderer, platform), &mut target)
        || target.texture == 0
        || target.rtv == 0
    {
        return false;
    }
    let facts = native::resource(lease, slot, target.rtv, true);
    let admitted = facts.pointer != 0
        && same_native(facts.identity, target.texture)
        && same_object(facts.device, device)
        && extent(&facts, native.width, native.height)
        && facts.view_dimension == D3D11_RTV_DIMENSION_TEXTURE2D
        && facts.view_mip == 0
        && load(map, &mut confirmed_platform)
        && platform == confirmed_platform
        && load(target_at(native.renderer, platform), &mut confirmed_target)
        && target == confirmed_target;
    if !admitted {
        native::release(lease, slot);
    }
    admitted
}

/// The previous admitted frame on this render thread.
#[derive(Clone, Copy)]
struct History {
    seen: bool,
    reset: bool,
    camera: usize,
    frame: u32,
    width: u32,
    height: u32,
    native: NativeCamera,
    raster: CameraRaster,
    previous_raster: CameraRaster,
    previous_raster_valid: bool,
    jitter: [f32; 2],
    previous_jitter: [f32; 2],
}

const NO_HISTORY: History = History {
    seen: false,
    reset: false,
    camera: 0,
    frame: 0,
    width: 0,
    height: 0,
    native: NO_CAMERA,
    raster: NO_RASTER,
    previous_raster: NO_RASTER,
    previous_raster_valid: false,
    jitter: [0.0; 2],
    previous_jitter: [0.0; 2],
};

thread_local! {
    static HISTORY: Cell<History> = const { Cell::new(NO_HISTORY) };
}

/// The reader's locals that its receipts print.
struct Reading {
    receipt: Receipt,
    after: State,
    pass: rsf_game_render_pass,
}

impl Reading {
    fn new() -> Self {
        Self {
            receipt: Receipt {
                before: State::default(),
                native: NO_CAMERA,
                views: [ResourceFacts::default(); 3],
                rtv: ResourceFacts::default(),
                swap: SwapFacts::default(),
                expected_renderer: image().wrapping_add(RENDERER_OBJECT) as u64,
                selected: 0,
                last_key: 0,
                expected: [0; 3],
                after_frame: 0,
                scanned: 0,
                camera_matches: 0,
                matching_jitter: 0,
                last_jitter: 0,
                viewport_count: 0,
                confirmed_viewport_count: 0,
                camera_read: 0,
                resource_match: [0; 3],
                device_match: [0; 3],
                viewport: [0.0; 6],
                confirmed_viewport: [0.0; 6],
            },
            after: State::default(),
            // SAFETY: the SDK record holds only integers, floats and raw pointers.
            pass: unsafe { std::mem::zeroed() },
        }
    }
}

/// One bundle per named stage, at most one per stage per process. Loading cannot spend the
/// budget reserved for a later distinct refusal stage.
fn receipt(report: Report, stage: Stage, reading: &mut Reading) {
    let Some(report) = report else {
        return;
    };
    let bit = 1u64 << stage as u32;
    if REPORTED.fetch_or(bit, Ordering::Relaxed) & bit != 0 {
        return;
    }
    reading.receipt.after_frame = reading.after.frame;
    unsafe {
        rsf_fallout4_inputs_report_reader(
            report,
            NAMES[stage as usize].as_ptr(),
            &reading.receipt,
            &reading.pass,
        )
    };
}

pub(crate) fn verify() -> bool {
    VERIFIED.store(false, Ordering::Release);
    let okay = guard(0x18a246, b"\x48\x8d\x1d\xc3\xba\x89\x03")
        && guard(0x18a28c, b"\x48\x89\x1d\x0d\x71\x75\x03")
        && guard(
            0xc37dd0,
            b"\x48\x8b\x05\x61\x55\x6a\x02\x48\x8b\x80\x40\x01\x00\x00\xc3",
        )
        && guard(0x21f0e3d, b"\x48\x8d\x0d\x5c\x63\xb9\x01")
        && guard(0x1839be2, b"\x8b\xb4\x81\xf4\x0d\x00\x00")
        && guard(0x1839cbb, b"\x8b\x8c\x81\x84\x0f\x00\x00")
        && guard(
            0x183a4a0,
            b"\x48\x63\x8c\x81\x84\x0f\x00\x00\x48\x69\xc1\x98\x00\x00\x00",
        )
        && guard(
            0x182bf4a,
            b"\x48\x8d\xb1\x40\x01\x00\x00\x8b\x46\x10\x85\xc0\x74\x39",
        )
        && guard(
            0x182bf6a,
            b"\x49\x39\xb8\x38\x02\x00\x00\x75\x09\x41\x38\x98\x40\x02\x00\x00\x74\x09",
        )
        && guard(
            0x182da63,
            b"\xf3\x41\x0f\x59\x5e\x04\x0f\x57\xc0\xf3\x41\x0f\x59\x56\x08",
        )
        && guard(0x182e226, b"\x8b\x05\x0c\x90\x55\x02")
        && guard(0xc33a60, b"\x48\x8d\x0d\xb9\xb9\x76\x02")
        && guard(0x165ba9f, b"\xf3\x41\x0f\x11\x50\x10")
        && guard(0x165bab0, b"\xf3\x41\x0f\x11\x40\x14")
        && guard(0x165baba, b"\xf3\x0f\x59\x05\x76\xff\x8e\x01")
        && guard(0x165bac6, b"\xf3\x41\x0f\x11\x40\x10")
        && guard(0x29293f8, b"\x6f\x12\x83\x3a")
        && guard(0x1839e34, b"\x80\xb9\xe4\x0f\x00\x00\x00")
        && guard(0x1817fd1, b"\xf3\x0f\x59\x81\xd0\x06\x00\x00")
        && guard(0x182430d, b"\x4c\x8d\x87\x90\x00\x00\x00")
        && guard(0x182431c, b"\xff\x90\x60\x01\x00\x00");
    VERIFIED.store(okay, Ordering::Release);
    okay
}

#[derive(Default, PartialEq, Eq)]
struct InputRecords {
    colour: Target,
    motion: Target,
    depth: u64,
    depth_srv: u64,
}

impl InputRecords {
    fn read(renderer: usize, [colour, motion, depth]: [u32; 3]) -> Option<Self> {
        let target = depth_at(renderer, depth);
        Some(Self {
            colour: read(target_at(renderer, colour))?,
            motion: read(target_at(renderer, motion))?,
            depth: read(target)?,
            depth_srv: read(target.wrapping_add(0x88))?,
        })
    }

    fn current(&self, renderer: usize, slots: [u32; 3]) -> bool {
        Self::read(renderer, slots).as_ref() == Some(self)
    }
}

#[derive(Clone, Copy)]
enum DepthRequirement {
    Temporal,
    Raster,
}

impl DepthRequirement {
    fn accepts(self, depth: u32) -> bool {
        depth == DXGI_FORMAT_R24G8_TYPELESS
            || matches!(self, Self::Temporal)
                && matches!(depth, DXGI_FORMAT_R32_TYPELESS | DXGI_FORMAT_R32_FLOAT)
    }
}

fn lease_views(
    lease: &mut native::NativeLease,
    records: &InputRecords,
    device: u64,
    requirement: DepthRequirement,
    receipt: &mut Receipt,
) -> Result<(), Stage> {
    let depth_format = match requirement {
        DepthRequirement::Temporal => None,
        DepthRequirement::Raster => Some(DXGI_FORMAT_R24_UNORM_X8_TYPELESS),
    };
    let views = [
        (
            SLOT_COLOUR,
            records.colour.srv,
            records.colour.texture,
            Some(DXGI_FORMAT_R11G11B10_FLOAT),
            Stage::ColourSrv,
        ),
        (
            SLOT_MOTION,
            records.motion.srv,
            records.motion.texture,
            Some(DXGI_FORMAT_R16G16_FLOAT),
            Stage::MotionSrv,
        ),
        (
            SLOT_DEPTH,
            records.depth_srv,
            records.depth,
            depth_format,
            Stage::DepthSrv,
        ),
    ];
    for (index, (slot, view, expected, format, stage)) in views.into_iter().enumerate() {
        let check = ViewCheck {
            facts: &mut receipt.views[index],
            resource_match: &mut receipt.resource_match[index],
            device_match: &mut receipt.device_match[index],
        };
        if !view_texture(lease, slot, view, expected, device, format, check) {
            return Err(stage);
        }
    }
    Ok(())
}

fn validate_views(
    lease: &mut native::NativeLease,
    colour_rtv: u64,
    [width, height]: [u32; 2],
    requirement: DepthRequirement,
    receipt: &mut Receipt,
) -> Result<(), Stage> {
    let [c, m, d] = receipt.views;
    receipt.rtv = native::resource(lease, NO_SLOT, colour_rtv, true);
    let rtv = receipt.rtv;
    if !same_object(rtv.identity, c.identity)
        || rtv.view_dimension != D3D11_RTV_DIMENSION_TEXTURE2D
        || rtv.view_format != DXGI_FORMAT_R11G11B10_FLOAT
        || rtv.view_mip != 0
    {
        return Err(Stage::ColourRtv);
    }
    if !receipt
        .views
        .iter()
        .all(|facts| extent(facts, width, height))
        || c.format != DXGI_FORMAT_R11G11B10_FLOAT
        || m.format != DXGI_FORMAT_R16G16_FLOAT
        || !requirement.accepts(d.format)
        || matches!(requirement, DepthRequirement::Raster)
            && receipt.views.iter().any(|facts| facts.sample_quality != 0)
    {
        return Err(Stage::ExtentFormat);
    }
    Ok(())
}

/// The full reader. Supplies native resource/camera/rect/frame facts only; the caller supplies
/// the verified session, source, submission, scope, role, screen and eligibility facts.
fn read_inputs(
    lease: &mut native::NativeLease,
    report: Report,
    producer: Option<&Snapshot>,
    r: &mut Reading,
) -> Result<(), Stage> {
    if !VERIFIED.load(Ordering::Acquire) {
        return Err(Stage::Unverified);
    }
    state(&mut r.receipt.before)?;
    let before = r.receipt.before;
    if let Some(p) = producer {
        if !depth_domains::snapshot_current(p, None)
            || p.native_frame != u64::from(before.frame)
            || p.world_camera_key != before.camera as u64
            || p.width != before.width
            || p.height != before.height
            || p.device_context_key != before.context as u64
            || p.cache_key != before.cache as u64
        {
            return Err(Stage::StateChanged);
        }
    } else if before.ratio[0] != 1.0 || before.ratio[1] != 1.0 {
        return Err(Stage::Ratio);
    }
    let mut selected = 0usize;
    for index in 0..before.count {
        let entry = before.cache.wrapping_add(index as usize * 0x250);
        let (mut key, mut jitter) = (0usize, 0u8);
        if !load(entry.wrapping_add(0x238), &mut key)
            || !load(entry.wrapping_add(0x240), &mut jitter)
        {
            return Err(Stage::CacheRead);
        }
        let rc = &mut r.receipt;
        rc.scanned += 1;
        rc.last_key = key as u64;
        rc.last_jitter = u32::from(jitter);
        if key == before.camera {
            rc.camera_matches += 1;
            rc.matching_jitter = u32::from(jitter);
        }
        if key == before.camera && jitter == 1 {
            if selected != 0 {
                return Err(Stage::CacheDuplicate);
            }
            selected = entry;
            rc.selected = entry as u64;
        }
    }
    if selected == 0 {
        return Err(Stage::CacheMissing);
    }
    if !read_camera(selected, &mut r.receipt.native) {
        return Err(Stage::CameraRead);
    }
    r.receipt.camera_read = 1;
    let native = r.receipt.native;
    let slots = [before.colour, before.motion, before.depth];
    let records = InputRecords::read(before.renderer, slots).ok_or(Stage::TargetRead)?;
    let colour = records.colour;
    let depth_texture = records.depth;
    r.receipt.expected = [colour.texture, records.motion.texture, depth_texture];
    let devices = native::owner(lease, before.context, before.chain);
    if devices.immediate == 0 {
        return Err(Stage::Context);
    }
    if !same_devices(&devices, &before) {
        return Err(Stage::Device);
    }
    let full = producer.is_none();
    let (admitted, count, viewport) =
        active_full_viewport(before.context, before.width, before.height, full);
    r.receipt.viewport_count = count;
    r.receipt.viewport = viewport;
    if !admitted {
        return Err(Stage::ViewportBounds);
    }
    lease_views(
        lease,
        &records,
        devices.device,
        DepthRequirement::Temporal,
        &mut r.receipt,
    )?;
    let [c, m, d] = r.receipt.views;
    r.receipt.swap = native::swap(before.chain);
    let swap = r.receipt.swap;
    if !swap_matches(&swap, &before) || colour.rtv == 0 {
        return Err(Stage::Swap);
    }
    validate_views(
        lease,
        colour.rtv,
        [before.width, before.height],
        DepthRequirement::Temporal,
        &mut r.receipt,
    )?;
    let mut raster = NO_RASTER;
    if let Some(p) = producer {
        raster.xywh.copy_from_slice(&p.actual_raster[..4]);
        raster.crop = p.render_rect;
        let w = &p.world_camera;
        if p.depth_key != depth_texture
            || !close_matrix(&native.view, &w.view)
            || !close_matrix(&native.projection, &w.projection)
            || !close_matrix(
                &native.view_projection_unjittered,
                &w.view_projection_unjittered,
            )
            || bits(&native.position_adjust) != bits(&w.origin)
        {
            return Err(Stage::CameraChanged);
        }
    } else {
        raster.xywh[2] = before.width as f32;
        raster.xywh[3] = before.height as f32;
        raster.crop[2] = before.width as i32;
        raster.crop[3] = before.height as i32;
    }
    let width = raster.crop[2].wrapping_sub(raster.crop[0]) as u32;
    let height = raster.crop[3].wrapping_sub(raster.crop[1]) as u32;
    // Optional allocations; a refused one excludes only its consumer.
    auxiliary(lease, &before, devices.device, 14, SLOT_REFRACTION_NORMALS);
    auxiliary(lease, &before, devices.device, 30, SLOT_PIPBOY);
    auxiliary(lease, &before, devices.device, 15, SLOT_VATS);
    let history = HISTORY.get();
    let previous_raster = if history.seen && history.frame != before.frame {
        Some(history.raster)
    } else if history.previous_raster_valid {
        Some(history.previous_raster)
    } else {
        None
    };
    let mut valid_history = false;
    let pass = &mut r.pass;
    if !camera::camera_frame(
        &native,
        width,
        height,
        &mut pass.camera,
        &mut pass.previous_clip_to_clip,
        &mut valid_history,
        Some(&raster),
        previous_raster.as_ref(),
    ) {
        return Err(Stage::CameraMath);
    }
    state(&mut r.after)?;
    let after = r.after;
    if !same_native_state(&before, &after) {
        let changed = [
            !same_native_owner(&before, &after),
            before.camera != after.camera,
            before.cache != after.cache || before.count != after.count,
            before.taa != after.taa || before.disable != after.disable,
            before.colour != after.colour
                || before.motion != after.motion
                || before.depth != after.depth,
            before.default_viewport != after.default_viewport,
            before.dynamic_viewport != after.dynamic_viewport,
            bits(&before.ratio) != bits(&after.ratio),
            bits(&before.viewport) != bits(&after.viewport),
            before.delta_seconds.to_bits() != after.delta_seconds.to_bits(),
        ];
        let mask = changed
            .iter()
            .enumerate()
            .fold(0u32, |mask, (bit, &value)| mask | (u32::from(value) << bit));
        if let Some(report) = report
            && !SEEN_CHANGES[mask as usize].swap(true, Ordering::Relaxed)
            && CHANGE_BUDGET.fetch_add(1, Ordering::Relaxed) < 16
        {
            unsafe { rsf_fallout4_inputs_report_mismatch(report, &before, &after, mask) };
        }
        return Err(Stage::StateChanged);
    }
    let (admitted, count, confirmed_viewport) =
        active_full_viewport(before.context, before.width, before.height, full);
    r.receipt.confirmed_viewport_count = count;
    r.receipt.confirmed_viewport = confirmed_viewport;
    if !admitted || bits(&viewport) != bits(&confirmed_viewport) {
        return Err(Stage::ViewportBounds);
    }
    if let Some(p) = producer
        && !depth_domains::snapshot_current(p, report)
    {
        return Err(Stage::ProducerChanged);
    }
    if !records.current(before.renderer, slots) {
        return Err(Stage::StateChanged);
    }
    let mut confirmed = NO_CAMERA;
    if !read_camera(selected, &mut confirmed) || !same_camera(&native, &confirmed) {
        return Err(Stage::CameraChanged);
    }
    if !history.seen || history.frame != before.frame {
        let previous = history;
        let mut next = NO_HISTORY;
        next.seen = true;
        next.camera = before.camera;
        next.frame = before.frame;
        next.width = width;
        next.height = height;
        next.native = native;
        next.raster = raster;
        next.previous_raster = previous.raster;
        next.previous_raster_valid = previous.seen;
        next.jitter = r.pass.camera.jitter_pixels;
        let terms = [
            !previous.seen,
            !valid_history,
            previous.camera != before.camera,
            before.frame != previous.frame.wrapping_add(1),
            previous.width != width || previous.height != height,
            !previous.raster.same_bits(&raster),
            !close_matrix(
                &native.previous_view_projection_unjittered,
                &previous.native.view_projection_unjittered,
            ),
            bits(&native.previous_position_adjust) != bits(&previous.native.position_adjust),
        ];
        let reasons = terms
            .iter()
            .enumerate()
            .fold(0u32, |mask, (bit, &value)| mask | (u32::from(value) << bit));
        next.reset = reasons != 0;
        if let Some(report) = report
            && !SEEN_REASONS[reasons as usize].swap(true, Ordering::Relaxed)
            && REASON_BUDGET.fetch_add(1, Ordering::Relaxed) < 32
        {
            let text = HistoryReceipt {
                frame: before.frame,
                previous_frame: previous.frame,
                reset: u32::from(next.reset),
                reasons,
                width,
                height,
                previous_width: previous.width,
                previous_height: previous.height,
                raster,
                previous_raster: previous.raster,
            };
            unsafe { rsf_fallout4_inputs_report_history(report, &text) };
        }
        if !next.reset {
            next.previous_jitter = previous.jitter;
        }
        HISTORY.set(next);
    } else if history.camera != before.camera
        || history.width != width
        || history.height != height
        || !close_matrix(
            &native.view_projection_unjittered,
            &history.native.view_projection_unjittered,
        )
        || !history.raster.same_bits(&raster)
    {
        return Err(Stage::HistoryChanged);
    }
    let history = HISTORY.get();
    let pass = &mut r.pass;
    pass.struct_size = size_of::<rsf_game_render_pass>() as u32;
    pass.native_frame = u64::from(before.frame);
    pass.family_key = before.camera as u64;
    pass.view_key = before.camera as u64;
    pass.history_key = before.camera as u64;
    pass.viewport_key = before.window as u64;
    pass.window_key = before.window as u64;
    pass.rhi_viewport_key = before.window as u64;
    if history.reset {
        pass.flags |= RSF_GAME_RENDER_RESET;
    }
    pass.camera.output_width = before.width;
    pass.camera.output_height = before.height;
    pass.camera.frame_time_seconds = before.delta_seconds;
    pass.camera.previous_jitter_pixels = history.previous_jitter;
    pass.render_rect = raster.crop;
    pass.output_rect[2] = before.width as i32;
    pass.output_rect[3] = before.height as i32;
    let colour_texture = c.pointer as *mut c_void;
    pass.color_input = colour_texture;
    pass.color_output = colour_texture;
    pass.color_output_readable = colour_texture;
    pass.depth = d.pointer as *mut c_void;
    pass.motion = m.pointer as *mut c_void;
    pass.swapchain = before.chain as *mut c_void;
    pass.camera_valid = 1;
    // Native velocity encodes previous-current UV in the native camera's viewport metric. The
    // steady-history crop scales that displacement.
    let Some(motion_to_uv) = camera::motion_to_uv(&raster) else {
        return Err(Stage::CameraMath);
    };
    pass.motion_to_uv = motion_to_uv;
    pass.motion_camera_included = 1;
    Ok(())
}

/// Raster/resource lease only. No temporal camera, history or depth admission. All downstream
/// allocations are conservatively required before native reduction.
fn read_raster(
    lease: &mut native::NativeLease,
    frame: &rsf_game_render_pass,
    producer: Option<&Snapshot>,
) -> Option<rsf_game_render_pass> {
    let base = image();
    let mut before = State::default();
    let (mut graph, mut view) = (0usize, 0u64);
    if !VERIFIED.load(Ordering::Acquire)
        || owner(&mut before).is_err()
        || frame.native_frame != u64::from(before.frame)
        || frame.window_key != before.window as u64
        || frame.swapchain as usize != before.chain
        || !load(base.wrapping_add(WORLD_GRAPH_POINTER), &mut graph)
        || graph == 0
        || !load(graph.wrapping_add(0x140), &mut view)
        || view != frame.view_key
    {
        return None;
    }
    if let Some(p) = producer
        && (!depth_domains::raster_current(p)
            || p.native_frame != u64::from(before.frame)
            || p.world_camera_key != view
            || p.device_context_key != before.context as u64
            || p.window_key != before.window as u64
            || p.chain_key != before.chain as u64)
    {
        return None;
    }
    let slots = slot_maps()?;
    let records = InputRecords::read(before.renderer, slots)?;
    let colour = records.colour;
    let depth_texture = records.depth;
    let devices = native::owner(lease, before.context, before.chain);
    if devices.immediate == 0 || !same_devices(&devices, &before) {
        return None;
    }
    let swap = native::swap(before.chain);
    if !swap_matches(&swap, &before) || !creation::accept_window(before.window as *mut c_void) {
        return None;
    }
    let mut receipt = Reading::new().receipt;
    lease_views(
        lease,
        &records,
        devices.device,
        DepthRequirement::Raster,
        &mut receipt,
    )
    .ok()?;
    if colour.rtv == 0 {
        return None;
    }
    validate_views(
        lease,
        colour.rtv,
        [before.width, before.height],
        DepthRequirement::Raster,
        &mut receipt,
    )
    .ok()?;
    let [c, m, d] = receipt.views;
    // No inactive-consumer predicate has been established. Require all three native
    // allocations, independently, rather than omitting a missing field.
    let refraction = auxiliary(lease, &before, devices.device, 14, SLOT_REFRACTION_NORMALS);
    let pipboy = auxiliary(lease, &before, devices.device, 30, SLOT_PIPBOY);
    let vats = auxiliary(lease, &before, devices.device, 15, SLOT_VATS);
    if !refraction || !pipboy || !vats {
        return None;
    }
    let mut after = State::default();
    let (mut confirmed_graph, mut confirmed_view) = (0usize, 0u64);
    if owner(&mut after).is_err()
        || !same_native_owner(&before, &after)
        || !load(base.wrapping_add(WORLD_GRAPH_POINTER), &mut confirmed_graph)
        || confirmed_graph != graph
        || !load(graph.wrapping_add(0x140), &mut confirmed_view)
        || confirmed_view != view
        || slot_maps() != Some(slots)
        || !records.current(before.renderer, slots)
    {
        return None;
    }
    if let Some(p) = producer
        && (p.depth_key != depth_texture || !depth_domains::raster_current(p))
    {
        return None;
    }
    let mut pass = *frame;
    pass.role = RSF_GAME_RENDER_SR;
    pass.pass_key = base.wrapping_add(RASTER_PASS_RVA) as u64;
    pass.flags |= RSF_GAME_RENDER_NO_FG;
    pass.screen = RSF_SCREEN_UNKNOWN;
    // SAFETY: both SDK records hold only integers and floats.
    pass.camera = unsafe { std::mem::zeroed() };
    pass.camera_valid = 0;
    pass.depth_domains = unsafe { std::mem::zeroed() };
    let colour_texture = c.pointer as *mut c_void;
    pass.color_input = colour_texture;
    pass.color_output = colour_texture;
    pass.color_output_readable = colour_texture;
    pass.depth = d.pointer as *mut c_void;
    pass.motion = m.pointer as *mut c_void;
    pass.swapchain = before.chain as *mut c_void;
    pass.output_rect = [0, 0, before.width as i32, before.height as i32];
    pass.camera.output_width = before.width;
    pass.camera.output_height = before.height;
    pass.render_rect = producer.map_or(pass.output_rect, |p| p.render_rect);
    Some(pass)
}

/// Logical FrameBuffer0 resolved after imagespace.
fn read_framebuffer(
    lease: &mut native::NativeLease,
    r: &mut FramebufferReceipt,
    owner_failure: &mut Stage,
) -> Result<(), Framebuffer> {
    if !VERIFIED.load(Ordering::Acquire) {
        return Err(Framebuffer::Unverified);
    }
    let map = colour_map(0);
    if let Err(stage) = owner(&mut r.before) {
        *owner_failure = stage;
        return Err(Framebuffer::Owner);
    }
    let before = r.before;
    if !load(map, &mut r.platform) || r.platform >= 101 {
        return Err(Framebuffer::Map);
    }
    let platform = r.platform;
    let mut target = Target::default();
    let target_read = load(target_at(before.renderer, platform), &mut target);
    r.target_texture = target.texture;
    r.target_rtv = target.rtv;
    if !target_read || target.rtv == 0 {
        return Err(Framebuffer::Target);
    }
    let devices = native::owner(lease, before.context, before.chain);
    if devices.immediate == 0 {
        return Err(Framebuffer::Context);
    }
    let facts = native::resource(lease, SLOT_COLOUR, target.rtv, true);
    if !same_native(devices.device, before.device as u64) {
        return Err(Framebuffer::Device);
    }
    // A missing resource or a failed Texture2D query.
    r.leased = facts.pointer;
    if facts.pointer == 0 {
        return Err(Framebuffer::Resource);
    }
    if target.texture != 0 && !same_native(facts.identity, target.texture) {
        return Err(Framebuffer::TargetTexture);
    }
    if devices.chain_device_read == 0 || !same_object(devices.device, devices.chain_device) {
        return Err(Framebuffer::ChainDevice);
    }
    r.rtv_dimension = facts.view_dimension;
    r.rtv_mip = facts.view_mip;
    if !same_object(facts.device, devices.device) {
        return Err(Framebuffer::TextureDevice);
    }
    r.width = facts.width;
    r.height = facts.height;
    r.mip_levels = facts.mip_levels;
    r.array_size = facts.array_size;
    r.sample_count = facts.sample_count;
    r.format = facts.format;
    if !extent(&facts, before.width, before.height) {
        return Err(Framebuffer::Extent);
    }
    if facts.view_dimension != D3D11_RTV_DIMENSION_TEXTURE2D || facts.view_mip != 0 {
        return Err(Framebuffer::Rtv);
    }
    r.swap = native::swap(before.chain);
    if r.swap.read == 0 {
        return Err(Framebuffer::Swap);
    }
    if r.swap.window != before.window as u64 {
        return Err(Framebuffer::Window);
    }
    if r.swap.width != before.width || r.swap.height != before.height {
        return Err(Framebuffer::SwapExtent);
    }
    if let Err(stage) = owner(&mut r.after) {
        *owner_failure = stage;
        return Err(Framebuffer::OwnerChanged);
    }
    if !same_native_owner(&before, &r.after) {
        return Err(Framebuffer::OwnerChanged);
    }
    if !load(map, &mut r.confirmed_platform) || platform != r.confirmed_platform {
        return Err(Framebuffer::MapChanged);
    }
    let mut confirmed = Target::default();
    if !load(target_at(before.renderer, platform), &mut confirmed) || target != confirmed {
        return Err(Framebuffer::TargetChanged);
    }
    Ok(())
}

fn framebuffer_receipt(
    report: Report,
    stage: Framebuffer,
    owner_failure: Stage,
    r: &FramebufferReceipt,
) {
    let Some(report) = report else {
        return;
    };
    let bit = 1u32 << stage as u32;
    if FRAMEBUFFER_REPORTED.fetch_or(bit, Ordering::Relaxed) & bit != 0 {
        return;
    }
    unsafe {
        rsf_fallout4_inputs_report_framebuffer(
            report,
            FRAMEBUFFER_NAMES[stage as usize].as_ptr(),
            NAMES[owner_failure as usize].as_ptr(),
            r,
        )
    };
}

/// The reader's exception receipt is due: its stage bit was not yet spent.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_inputs_claim_exception() -> bool {
    let bit = 1u64 << Stage::Exception as u32;
    REPORTED.fetch_or(bit, Ordering::Relaxed) & bit == 0
}

/// The full reader. `output` is cleared first and written only on admission; the caller keeps
/// the lease only then.
///
/// # Safety
/// `transferred` points to the caller's empty lease, `producer` null or readable and `output` writable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_inputs_read(
    transferred: *mut native::TransferredLease,
    report: Report,
    producer: *const Snapshot,
    output: *mut rsf_game_render_pass,
) -> bool {
    let Some(output) = (unsafe { output.as_mut() }) else {
        return false;
    };
    // SAFETY: the SDK record holds only integers, floats and raw pointers.
    *output = unsafe { std::mem::zeroed() };
    if transferred.is_null() {
        return false;
    }
    let producer = unsafe { producer.as_ref() }.copied();
    let mut reading = Reading::new();
    let mut lease = native::NativeLease::default();
    match read_inputs(&mut lease, report, producer.as_ref(), &mut reading) {
        Ok(()) => {
            receipt(report, Stage::Accepted, &mut reading);
            *output = reading.pass;
            unsafe { transferred.write(lease.transfer()) };
            true
        }
        Err(stage) => {
            receipt(report, stage, &mut reading);
            false
        }
    }
}

/// The raster reader. `output` is cleared first and written only on admission.
///
/// # Safety
/// `transferred` points to the caller's empty lease, `frame` readable, `producer` null or readable and
/// `output` writable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_inputs_read_raster(
    transferred: *mut native::TransferredLease,
    frame: *const rsf_game_render_pass,
    producer: *const Snapshot,
    output: *mut rsf_game_render_pass,
) -> bool {
    if output.is_null() {
        return false;
    }
    // SAFETY: the SDK record holds only integers, floats and raw pointers.
    unsafe { output.write(std::mem::zeroed()) };
    if transferred.is_null() || frame.is_null() {
        return false;
    }
    let frame = unsafe { frame.read() };
    let producer = unsafe { producer.as_ref() }.copied();
    let mut lease = native::NativeLease::default();
    match read_raster(&mut lease, &frame, producer.as_ref()) {
        Some(pass) => {
            unsafe {
                output.write(pass);
                transferred.write(lease.transfer())
            };
            true
        }
        None => false,
    }
}

/// Compare the World-owned interfaces in the producer's established order.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_inputs_same_resources(
    current: *const c_void,
    held: *const c_void,
    owners: bool,
) -> bool {
    use crate::sys::{native_input_chain, native_input_context};

    // SAFETY: Both pointers name the calling thread's native World leases.
    // Read each pair just before its COM comparison, preserving short-circuit
    // behavior and the fresh slot observations on reentrant calls.
    let same = |slot| unsafe {
        let current = rsf_fallout4_input_object(current, slot);
        let held = rsf_fallout4_input_object(held, slot);
        crate::d3d11::rsf_fallout4_render_same_object(current, held)
    };
    (!owners || (same(native_input_context) && same(native_input_chain)))
        && [
            SLOT_COLOUR,
            SLOT_DEPTH,
            SLOT_MOTION,
            SLOT_PIPBOY,
            SLOT_VATS,
            SLOT_REFRACTION_NORMALS,
        ]
        .into_iter()
        .all(same)
}

/// Own the accepted framebuffer directly for the duration of the render seam.
pub(crate) fn acquire_framebuffer(report: Report) -> Option<FramebufferLease> {
    let mut receipt = FramebufferReceipt::default();
    let mut owner_failure = Stage::Owner;
    let mut lease = native::NativeLease::default();
    match read_framebuffer(&mut lease, &mut receipt, &mut owner_failure) {
        Ok(()) => {
            framebuffer_receipt(report, Framebuffer::Accepted, owner_failure, &receipt);
            let (context, chain, texture) = lease.into_framebuffer()?;
            Some(FramebufferLease {
                context,
                chain,
                texture,
                native_frame: u64::from(receipt.before.frame),
                window_key: receipt.before.window as u64,
            })
        }
        Err(stage) => {
            framebuffer_receipt(report, stage, owner_failure, &receipt);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_numbers_and_names_are_stable() {
        assert_eq!(Stage::Unverified as u32, 0);
        assert_eq!(Stage::CacheRead as u32, 13);
        assert_eq!(Stage::Accepted as u32, 30);
        assert_eq!(Stage::Exception as u32, 31);
        assert_eq!(Stage::ProducerChanged as u32, 32);
        assert_eq!(NAMES[Stage::CameraOwner as usize], c"camera-owner");
        assert_eq!(NAMES[Stage::ExtentFormat as usize], c"extent-format");
        assert_eq!(Framebuffer::TargetChanged as u32, 17);
        assert_eq!(
            FRAMEBUFFER_NAMES[Framebuffer::TextureDevice as usize],
            c"texture-device"
        );
    }

    #[test]
    fn lease_detects_replaced_records_at_unchanged_slots() {
        let mut renderer = [0u64; 0x2000 / size_of::<u64>()];
        let base = renderer.as_mut_ptr() as usize;
        let slots = [1, 32, 1];
        let original = InputRecords::read(base, slots).unwrap();
        assert!(original.current(base, slots));
        for (slot, offset) in [(1, 16), (32, 24)] {
            let index = (target_at(base, slot) - base + offset) / size_of::<u64>();
            renderer[index] = 42;
            std::hint::black_box(&renderer);
            assert!(!original.current(base, slots));
            renderer[index] = 0;
        }
        let index = (depth_at(base, slots[2]) - base + 0x88) / size_of::<u64>();
        renderer[index] = 42;
        std::hint::black_box(&renderer);
        assert!(!original.current(base, slots));
    }

    #[test]
    fn owner_and_state_ignore_initialized_padding() {
        use std::mem::{MaybeUninit, offset_of};

        const PADDING: usize = offset_of!(State, dynamic_viewport) + size_of::<u8>();
        const SIZE: usize = size_of::<State>();
        const _: () = assert!(PADDING == 122 && SIZE == 128);

        let mut before = MaybeUninit::<State>::zeroed();
        let mut after = MaybeUninit::<State>::zeroed();
        // SAFETY: every State field accepts zero. Write the six trailing
        // bytes in place and borrow the values without moving their padding.
        unsafe {
            let before_padding = before.as_mut_ptr().cast::<u8>().add(PADDING);
            let after_padding = after.as_mut_ptr().cast::<u8>().add(PADDING);
            before_padding.write_bytes(0xa5, SIZE - PADDING);
            after_padding.write_bytes(0x5a, SIZE - PADDING);
            assert_ne!(before_padding.read(), after_padding.read());
            assert!(same_native_owner(
                before.assume_init_ref(),
                after.assume_init_ref()
            ));
            assert!(same_native_state(
                before.assume_init_ref(),
                after.assume_init_ref()
            ));
        }
    }

    #[test]
    fn owner_and_state_compare_exactly_their_fields() {
        let before = State::default();
        let pointers: [fn(&mut State); 7] = [
            |s| s.renderer = 1,
            |s| s.camera = 1,
            |s| s.cache = 1,
            |s| s.context = 1,
            |s| s.device = 1,
            |s| s.chain = 1,
            |s| s.window = 1,
        ];
        for (index, change) in pointers.iter().enumerate() {
            let mut after = before;
            change(&mut after);
            assert!(!same_native_state(&before, &after));
            let owner_field = index != 1 && index != 2;
            assert_eq!(same_native_owner(&before, &after), !owner_field);
        }
        let integers: [fn(&mut State); 9] = [
            |s| s.frame = 1,
            |s| s.width = 1,
            |s| s.height = 1,
            |s| s.count = 1,
            |s| s.taa = 1,
            |s| s.disable = 1,
            |s| s.colour = 1,
            |s| s.motion = 1,
            |s| s.depth = 1,
        ];
        for (index, change) in integers.iter().enumerate() {
            let mut after = before;
            change(&mut after);
            assert!(!same_native_state(&before, &after));
            assert_eq!(same_native_owner(&before, &after), index > 2);
        }
        let mut after = before;
        after.delta_seconds = 1.0;
        assert!(same_native_state(&before, &after), "timing is excluded");
        let mut after = before;
        after.ratio[0] = -0.0;
        assert!(
            !same_native_state(&before, &after),
            "signed zero is a change"
        );
        let mut nan = before;
        nan.viewport[1] = f32::from_bits(0x7fc0_0123);
        assert!(same_native_state(&nan, &nan), "identical NaN bits agree");
        let mut other = nan;
        other.viewport[1] = f32::from_bits(0x7fc0_0124);
        assert!(!same_native_state(&nan, &other));
        let temporal_only: [fn(&mut State); 2] = [
            |state| state.default_viewport = 1,
            |state| state.dynamic_viewport = 1,
        ];
        for change in temporal_only {
            let mut after = before;
            change(&mut after);
            assert!(!same_native_state(&before, &after) && same_native_owner(&before, &after));
        }
        for index in 0..before.ratio.len() {
            let mut after = before;
            after.ratio[index] = 1.0;
            assert!(!same_native_state(&before, &after) && same_native_owner(&before, &after));
        }
        for index in 0..before.viewport.len() {
            let mut after = before;
            after.viewport[index] = 1.0;
            assert!(!same_native_state(&before, &after) && same_native_owner(&before, &after));
        }
    }

    #[test]
    fn camera_and_raster_comparisons_are_bytewise() {
        let a = NO_CAMERA;
        let mut b = a;
        assert!(same_camera(&a, &b));
        b.jitter_ndc[1] = -0.0;
        assert!(!same_camera(&a, &b));
        let mut raster = NO_RASTER;
        assert!(raster.same_bits(&NO_RASTER));
        raster.xywh[3] = -0.0;
        assert!(!raster.same_bits(&NO_RASTER));
        raster = NO_RASTER;
        raster.crop[3] = 1;
        assert!(!raster.same_bits(&NO_RASTER));
    }

    #[test]
    fn close_matrix_needs_finite_values_within_a_thousandth() {
        let a = [1.0f32; 16];
        let mut b = a;
        b[7] = 1.0005;
        assert!(close_matrix(&a, &b));
        b[7] = 1.002;
        assert!(!close_matrix(&a, &b));
        b[7] = f32::NAN;
        assert!(!close_matrix(&b, &b));
    }
}
