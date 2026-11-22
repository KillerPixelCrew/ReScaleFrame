// SPDX-License-Identifier: MIT
//! Fallout 4 native render inputs: the code guards, the sampled native renderer owner and
//! temporal state, the TAA camera cache scan, the logical-to-platform target maps, resource and
//! raster admission, the steady camera history, and the GameSDK pass each reader leases.
//!
//! render_inputs.cpp supplies narrow leaves through `Leaves` (render_inputs_platform.h): canonical
//! COM identity, the lease that holds the context, chain and textures as typed COM references, the
//! D3D11 and DXGI facts, and the receipts with their original text. Engine memory is read here
//! through the guarded `crate::read`. Camera math goes through `camera`, producer agreement through
//! `depth_domains`.
//!
//! Every refusal names the stage the C++ named and reports it once per stage when a report
//! callback is supplied. Fields written before the failing step remain in the reported copy. A
//! pass is written only on admission; the lease the C++ wrapper discards on refusal.

use crate::build_layout::{
    RENDERER_CHAIN, RENDERER_CONTEXT, RENDERER_DEVICE, RENDERER_OBJECT, RENDERER_POINTER,
    RENDERER_WINDOW,
};
use crate::camera::{self, CameraRaster, NativeCamera};
use crate::creation;
use crate::depth_domains::{self, Snapshot};
use crate::{Plain, image, read};
use rsf_game_sys::{
    RSF_GAME_RENDER_NO_FG, RSF_GAME_RENDER_RESET, RSF_GAME_RENDER_SR, rsf_game_render_pass,
};
use std::cell::Cell;
use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

type Flat = [f32; 16];
type Report = Option<unsafe extern "C" fn(*const c_char)>;
/// render_inputs.cpp `NativeInputLease`, opaque here.
type Lease = *mut c_void;

const STATE_RVA: usize = 0x3d871a0;
const TARGETS_RVA: usize = 0x2f4d750;
const WORLD_GRAPH_POINTER: usize = 0x32dd338;
const DELTA_RVA: usize = 0x339f430;
/// The guarded SR call site the raster pass is keyed by.
const RASTER_PASS_RVA: usize = 0x21fa283;

// Enumerators compared by value; render_inputs.cpp asserts each against the SDK headers.
const DXGI_FORMAT_R11G11B10_FLOAT: u32 = 26;
const DXGI_FORMAT_R16G16_FLOAT: u32 = 34;
const DXGI_FORMAT_R32_TYPELESS: u32 = 39;
const DXGI_FORMAT_R32_FLOAT: u32 = 41;
const DXGI_FORMAT_R24G8_TYPELESS: u32 = 44;
const DXGI_FORMAT_R24_UNORM_X8_TYPELESS: u32 = 46;
const D3D11_SRV_DIMENSION_TEXTURE2D: u32 = 4;
const D3D11_RTV_DIMENSION_TEXTURE2D: u32 = 4;
/// game_frame.h `RSF_SCREEN_UNKNOWN`.
const RSF_SCREEN_UNKNOWN: u32 = 0;

// render_inputs_platform.h lease slots.
const SLOT_COLOUR: u32 = 0;
const SLOT_MOTION: u32 = 1;
const SLOT_DEPTH: u32 = 2;
const SLOT_REFRACTION_NORMALS: u32 = 3;
const SLOT_PIPBOY: u32 = 4;
const SLOT_VATS: u32 = 5;
const NO_SLOT: u32 = u32::MAX;

static VERIFIED: AtomicBool = AtomicBool::new(false);
static REPORTED: AtomicU64 = AtomicU64::new(0);
static FRAMEBUFFER_REPORTED: AtomicU32 = AtomicU32::new(0);
static SEEN_CHANGES: [AtomicBool; 1024] = [const { AtomicBool::new(false) }; 1024];
static CHANGE_BUDGET: AtomicU32 = AtomicU32::new(0);
static SEEN_REASONS: [AtomicBool; 256] = [const { AtomicBool::new(false) }; 256];
static REASON_BUDGET: AtomicU32 = AtomicU32::new(0);

/// native_input_state.h `detail::NativeState`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct State {
    renderer: usize,
    camera: usize,
    cache: usize,
    context: usize,
    device: usize,
    chain: usize,
    window: usize,
    frame: u32,
    width: u32,
    height: u32,
    count: u32,
    taa: u32,
    disable: u32,
    colour: u32,
    motion: u32,
    depth: u32,
    ratio: [f32; 2],
    viewport: [f32; 4],
    delta_seconds: f32,
    default_viewport: u8,
    dynamic_viewport: u8,
}

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

/// render_inputs_platform.h `NativeInputDeviceFacts`.
#[repr(C)]
#[derive(Default)]
struct DeviceFacts {
    device: u64,
    chain_device: u64,
    immediate: u32,
    chain_device_read: u32,
}

/// render_inputs_platform.h `NativeInputViewportFacts`.
#[repr(C)]
#[derive(Default)]
struct ViewportFacts {
    count: u32,
    copied: u32,
    viewport: [f32; 6],
}

/// render_inputs_platform.h `NativeInputSwapFacts`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
#[allow(dead_code)] // padding
struct SwapFacts {
    window: u64,
    read: u32,
    width: u32,
    height: u32,
    reserved: u32,
}

/// render_inputs_platform.h `NativeInputResourceFacts`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
#[allow(dead_code)] // read and bind_flags are printed by the C++ receipt leaf
struct ResourceFacts {
    identity: u64,
    pointer: u64,
    device: u64,
    read: u32,
    width: u32,
    height: u32,
    format: u32,
    mip_levels: u32,
    array_size: u32,
    sample_count: u32,
    sample_quality: u32,
    bind_flags: u32,
    view_format: u32,
    view_dimension: u32,
    view_mip: u32,
    view_mips: u32,
    reserved: u32,
}

/// render_inputs_platform.h `NativeInputReceipt`: every local the former reader receipt printed.
#[repr(C)]
#[derive(Clone, Copy)]
#[allow(dead_code)] // printed by the C++ receipt leaf
struct Receipt {
    before: State,
    native: NativeCamera,
    views: [ResourceFacts; 3],
    rtv: ResourceFacts,
    swap: SwapFacts,
    expected_renderer: u64,
    selected: u64,
    last_key: u64,
    expected: [u64; 3],
    after_frame: u32,
    scanned: u32,
    camera_matches: u32,
    matching_jitter: u32,
    last_jitter: u32,
    viewport_count: u32,
    confirmed_viewport_count: u32,
    camera_read: u32,
    resource_match: [u32; 3],
    device_match: [u32; 3],
    viewport: [f32; 6],
    confirmed_viewport: [f32; 6],
}

/// render_inputs_platform.h `NativeInputHistoryReceipt`.
#[repr(C)]
#[allow(dead_code)] // printed by the C++ receipt leaf
struct HistoryReceipt {
    frame: u32,
    previous_frame: u32,
    reset: u32,
    reasons: u32,
    width: u32,
    height: u32,
    previous_width: u32,
    previous_height: u32,
    raster: CameraRaster,
    previous_raster: CameraRaster,
}

/// render_inputs_platform.h `NativeFramebufferReceipt`.
#[repr(C)]
#[derive(Default)]
#[allow(dead_code)] // printed by the C++ receipt leaf
struct FramebufferReceipt {
    before: State,
    after: State,
    swap: SwapFacts,
    target_texture: u64,
    target_rtv: u64,
    leased: u64,
    platform: u32,
    confirmed_platform: u32,
    width: u32,
    height: u32,
    mip_levels: u32,
    array_size: u32,
    sample_count: u32,
    format: u32,
    rtv_dimension: u32,
    rtv_mip: u32,
}

/// render_inputs_platform.h `NativeFramebufferFacts`: the accepted framebuffer identity.
#[repr(C)]
#[allow(dead_code)] // read by the C++ wrapper
struct FramebufferFacts {
    native_frame: u64,
    window_key: u64,
}

/// render_inputs_platform.h `NativeInputLeaves`, built by render_inputs.cpp.
#[repr(C)]
#[allow(dead_code)] // constructed by the C++ leaf table
pub(crate) struct Leaves {
    identity: Option<unsafe extern "C" fn(u64) -> u64>,
    owner: Option<unsafe extern "C" fn(Lease, u64, u64, *mut DeviceFacts)>,
    viewport: Option<unsafe extern "C" fn(u64, *mut ViewportFacts)>,
    swap: Option<unsafe extern "C" fn(u64, *mut SwapFacts)>,
    resource: Option<unsafe extern "C" fn(Lease, u32, u64, u32, *mut ResourceFacts)>,
    release: Option<unsafe extern "C" fn(Lease, u32)>,
    reader: Option<
        unsafe extern "C" fn(Report, *const c_char, *const Receipt, *const rsf_game_render_pass),
    >,
    mismatch: Option<unsafe extern "C" fn(Report, *const State, *const State, u32)>,
    history: Option<unsafe extern "C" fn(Report, *const HistoryReceipt)>,
    framebuffer: Option<
        unsafe extern "C" fn(Report, *const c_char, *const c_char, *const FramebufferReceipt),
    >,
}

const _: () = {
    use std::mem::offset_of;
    assert!(size_of::<State>() == 128 && offset_of!(State, frame) == 56);
    assert!(offset_of!(State, ratio) == 92 && offset_of!(State, viewport) == 100);
    assert!(offset_of!(State, delta_seconds) == 116 && offset_of!(State, default_viewport) == 120);
    assert!(offset_of!(State, dynamic_viewport) == 121);
    assert!(size_of::<Target>() == 0x30);
    assert!(size_of::<DeviceFacts>() == 24 && offset_of!(DeviceFacts, immediate) == 16);
    assert!(size_of::<ViewportFacts>() == 32 && offset_of!(ViewportFacts, viewport) == 8);
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
    assert!(size_of::<FramebufferFacts>() == 16);
    assert!(size_of::<Leaves>() == 10 * 8);
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

/// Reader refusal stages in the C++ order. The once-per-process receipt mask is indexed by them.
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

/// Framebuffer refusal stages in the C++ order, with their own receipt mask.
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

impl Leaves {
    fn complete(&self) -> bool {
        self.identity.is_some()
            && self.owner.is_some()
            && self.viewport.is_some()
            && self.swap.is_some()
            && self.resource.is_some()
            && self.release.is_some()
            && self.reader.is_some()
            && self.mismatch.is_some()
            && self.history.is_some()
            && self.framebuffer.is_some()
    }

    /// Canonical identity of a live COM object, 0 when null or not an object.
    fn identity(&self, object: u64) -> u64 {
        self.identity.map_or(0, |leaf| unsafe { leaf(object) })
    }

    /// `object` is the canonical identity of a live object, `native` a native pointer: the C++
    /// `same_object`, which required both and compared their IUnknown identities.
    fn same_native(&self, object: u64, native: u64) -> bool {
        object != 0 && native != 0 && self.identity(native) == object
    }

    /// Leases the native context and chain, then reports their devices.
    fn owner(&self, lease: Lease, context: usize, chain: usize) -> DeviceFacts {
        let mut facts = DeviceFacts::default();
        if let Some(leaf) = self.owner {
            unsafe { leaf(lease, context as u64, chain as u64, &mut facts) };
        }
        facts
    }

    fn viewport(&self, context: usize) -> ViewportFacts {
        let mut facts = ViewportFacts::default();
        if let Some(leaf) = self.viewport {
            unsafe { leaf(context as u64, &mut facts) };
        }
        facts
    }

    fn swap(&self, chain: usize) -> SwapFacts {
        let mut facts = SwapFacts::default();
        if let Some(leaf) = self.swap {
            unsafe { leaf(chain as u64, &mut facts) };
        }
        facts
    }

    /// The resource of a shader-resource or render-target view, leased into `slot` unless it is
    /// `NO_SLOT`.
    fn resource(&self, lease: Lease, slot: u32, view: u64, render_target: bool) -> ResourceFacts {
        let mut facts = ResourceFacts::default();
        if let Some(leaf) = self.resource {
            unsafe { leaf(lease, slot, view, u32::from(render_target), &mut facts) };
        }
        facts
    }

    fn release(&self, lease: Lease, slot: u32) {
        if let Some(leaf) = self.release {
            unsafe { leaf(lease, slot) };
        }
    }
}

/// Two canonical identities of live objects name the same object.
fn same_object(a: u64, b: u64) -> bool {
    a != 0 && b != 0 && a == b
}

/// The C++ `read(address, field)`: the whole value into `into`, or nothing and false. Address 0
/// is refused before the read, as the C++ did.
fn load<T: Plain>(address: usize, into: &mut T) -> bool {
    if address == 0 {
        return false;
    }
    match read::<T>(address) {
        Some(value) => {
            *into = value;
            true
        }
        None => false,
    }
}

fn guard<const N: usize>(rva: usize, expected: &[u8; N]) -> bool {
    let mut actual = [0u8; N];
    load(image().wrapping_add(rva), &mut actual) && actual == *expected
}

fn bits<const N: usize>(values: &[f32; N]) -> [u32; N] {
    values.map(f32::to_bits)
}

/// The platform target record for a platform slot.
fn target_at(renderer: usize, platform: u32) -> usize {
    renderer.wrapping_add(0xa58 + platform as usize * 0x30)
}

/// The depth target record for a depth slot; its SRV is 0x88 further.
fn depth_at(renderer: usize, slot: u32) -> usize {
    renderer.wrapping_add(0x1d48 + slot as usize * 0x98)
}

/// The logical-to-platform colour map entry for `logical`.
fn colour_map(logical: u32) -> usize {
    image().wrapping_add(TARGETS_RVA + 0xdf4 + logical as usize * 4)
}

/// `same_native_owner`: owner() reads only these fields.
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

/// `same_native_state`: exact field bits for the floats, frame timing excluded.
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

/// `memcmp` equality of two copied cameras.
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

/// `memcmp` equality of two rasters.
fn same_raster(a: &CameraRaster, b: &CameraRaster) -> bool {
    bits(&a.xywh) == bits(&b.xywh) && a.crop == b.crop
}

fn close_matrix(a: &Flat, b: &Flat) -> bool {
    a.iter()
        .zip(b)
        .all(|(&a, &b)| a.is_finite() && b.is_finite() && (a - b).abs() <= 0.001)
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
/// first count was one, as the C++ reported it.
fn active_full_viewport(
    leaves: &Leaves,
    context: usize,
    width: u32,
    height: u32,
    full: bool,
) -> (bool, u32, [f32; 6]) {
    let facts = leaves.viewport(context);
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

#[allow(clippy::too_many_arguments)] // the C++ view_texture arguments and its facts
fn view_texture(
    leaves: &Leaves,
    lease: Lease,
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
    *check.facts = leaves.resource(lease, slot, view, false);
    let facts = *check.facts;
    // A missing resource or a failed Texture2D query.
    if facts.pointer == 0 {
        return false;
    }
    *check.resource_match = u32::from(leaves.same_native(facts.identity, expected));
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
    leaves: &Leaves,
    lease: Lease,
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
    let facts = leaves.resource(lease, slot, target.rtv, true);
    let admitted = facts.pointer != 0
        && leaves.same_native(facts.identity, target.texture)
        && same_object(facts.device, device)
        && extent(&facts, native.width, native.height)
        && facts.view_dimension == D3D11_RTV_DIMENSION_TEXTURE2D
        && facts.view_mip == 0
        && load(map, &mut confirmed_platform)
        && platform == confirmed_platform
        && load(target_at(native.renderer, platform), &mut confirmed_target)
        && target == confirmed_target;
    if !admitted {
        leaves.release(lease, slot);
    }
    admitted
}

/// thread_local `History`: the previous admitted frame on this render thread.
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
fn receipt(leaves: &Leaves, report: Report, stage: Stage, reading: &mut Reading) {
    if report.is_none() {
        return;
    }
    let bit = 1u64 << stage as u32;
    if REPORTED.fetch_or(bit, Ordering::Relaxed) & bit != 0 {
        return;
    }
    reading.receipt.after_frame = reading.after.frame;
    if let Some(leaf) = leaves.reader {
        unsafe {
            leaf(
                report,
                NAMES[stage as usize].as_ptr(),
                &reading.receipt,
                &reading.pass,
            )
        };
    }
}

fn verify(admit: impl FnOnce() -> bool) -> bool {
    VERIFIED.store(false, Ordering::Release);
    let okay = admit()
        && guard(0x18a246, b"\x48\x8d\x1d\xc3\xba\x89\x03")
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

/// The full reader. Supplies native resource/camera/rect/frame facts only; the caller supplies
/// the verified session, source, submission, scope, role, screen and eligibility facts.
fn read_inputs(
    leaves: &Leaves,
    depth: Option<&depth_domains::Leaves>,
    lease: Lease,
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
        if !depth.is_some_and(|depth| depth_domains::snapshot_current(depth, p, None))
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
    let (mut colour, mut motion) = (Target::default(), Target::default());
    let (mut depth_texture, mut depth_view) = (0u64, 0u64);
    if !load(target_at(before.renderer, before.colour), &mut colour)
        || !load(target_at(before.renderer, before.motion), &mut motion)
        || !load(depth_at(before.renderer, before.depth), &mut depth_texture)
        || !load(
            depth_at(before.renderer, before.depth).wrapping_add(0x88),
            &mut depth_view,
        )
    {
        return Err(Stage::TargetRead);
    }
    r.receipt.expected = [colour.texture, motion.texture, depth_texture];
    let devices = leaves.owner(lease, before.context, before.chain);
    if devices.immediate == 0 {
        return Err(Stage::Context);
    }
    if !leaves.same_native(devices.device, before.device as u64)
        || devices.chain_device_read == 0
        || !same_object(devices.device, devices.chain_device)
    {
        return Err(Stage::Device);
    }
    let full = producer.is_none();
    let (admitted, count, viewport) =
        active_full_viewport(leaves, before.context, before.width, before.height, full);
    r.receipt.viewport_count = count;
    r.receipt.viewport = viewport;
    if !admitted {
        return Err(Stage::ViewportBounds);
    }
    let views = [
        (
            SLOT_COLOUR,
            colour.srv,
            colour.texture,
            Some(DXGI_FORMAT_R11G11B10_FLOAT),
            Stage::ColourSrv,
        ),
        (
            SLOT_MOTION,
            motion.srv,
            motion.texture,
            Some(DXGI_FORMAT_R16G16_FLOAT),
            Stage::MotionSrv,
        ),
        (SLOT_DEPTH, depth_view, depth_texture, None, Stage::DepthSrv),
    ];
    for (index, (slot, view, expected, format, stage)) in views.into_iter().enumerate() {
        let rc = &mut r.receipt;
        let check = ViewCheck {
            facts: &mut rc.views[index],
            resource_match: &mut rc.resource_match[index],
            device_match: &mut rc.device_match[index],
        };
        if !view_texture(
            leaves,
            lease,
            slot,
            view,
            expected,
            devices.device,
            format,
            check,
        ) {
            return Err(stage);
        }
    }
    let [c, m, d] = r.receipt.views;
    r.receipt.swap = leaves.swap(before.chain);
    let swap = r.receipt.swap;
    if swap.read == 0
        || swap.window != before.window as u64
        || swap.width != before.width
        || swap.height != before.height
        || colour.rtv == 0
    {
        return Err(Stage::Swap);
    }
    r.receipt.rtv = leaves.resource(lease, NO_SLOT, colour.rtv, true);
    let rtv = r.receipt.rtv;
    if !same_object(rtv.identity, c.identity)
        || rtv.view_dimension != D3D11_RTV_DIMENSION_TEXTURE2D
        || rtv.view_format != DXGI_FORMAT_R11G11B10_FLOAT
        || rtv.view_mip != 0
    {
        return Err(Stage::ColourRtv);
    }
    if !extent(&c, before.width, before.height)
        || !extent(&d, before.width, before.height)
        || !extent(&m, before.width, before.height)
        || c.format != DXGI_FORMAT_R11G11B10_FLOAT
        || m.format != DXGI_FORMAT_R16G16_FLOAT
        || (d.format != DXGI_FORMAT_R24G8_TYPELESS
            && d.format != DXGI_FORMAT_R32_TYPELESS
            && d.format != DXGI_FORMAT_R32_FLOAT)
    {
        return Err(Stage::ExtentFormat);
    }
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
    auxiliary(
        leaves,
        lease,
        &before,
        devices.device,
        14,
        SLOT_REFRACTION_NORMALS,
    );
    auxiliary(leaves, lease, &before, devices.device, 30, SLOT_PIPBOY);
    auxiliary(leaves, lease, &before, devices.device, 15, SLOT_VATS);
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
        if report.is_some()
            && !SEEN_CHANGES[mask as usize].swap(true, Ordering::Relaxed)
            && CHANGE_BUDGET.fetch_add(1, Ordering::Relaxed) < 16
            && let Some(leaf) = leaves.mismatch
        {
            unsafe { leaf(report, &before, &after, mask) };
        }
        return Err(Stage::StateChanged);
    }
    let (admitted, count, confirmed_viewport) =
        active_full_viewport(leaves, before.context, before.width, before.height, full);
    r.receipt.confirmed_viewport_count = count;
    r.receipt.confirmed_viewport = confirmed_viewport;
    if !admitted || bits(&viewport) != bits(&confirmed_viewport) {
        return Err(Stage::ViewportBounds);
    }
    if let Some(p) = producer
        && !depth.is_some_and(|depth| depth_domains::snapshot_current(depth, p, report))
    {
        return Err(Stage::ProducerChanged);
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
            !same_raster(&previous.raster, &raster),
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
        if report.is_some()
            && !SEEN_REASONS[reasons as usize].swap(true, Ordering::Relaxed)
            && REASON_BUDGET.fetch_add(1, Ordering::Relaxed) < 32
            && let Some(leaf) = leaves.history
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
            unsafe { leaf(report, &text) };
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
        || !same_raster(&history.raster, &raster)
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
    leaves: &Leaves,
    depth: Option<&depth_domains::Leaves>,
    lease: Lease,
    frame: &rsf_game_render_pass,
    producer: Option<&Snapshot>,
) -> Option<rsf_game_render_pass> {
    let base = image();
    let targets = base.wrapping_add(TARGETS_RVA);
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
        && (!depth.is_some_and(|depth| depth_domains::raster_current(depth, p))
            || p.native_frame != u64::from(before.frame)
            || p.world_camera_key != view
            || p.device_context_key != before.context as u64
            || p.window_key != before.window as u64
            || p.chain_key != before.chain as u64)
    {
        return None;
    }
    let (mut colour_slot, mut motion_slot, mut depth_slot) = (0u32, 0u32, 0u32);
    let (mut colour, mut motion) = (Target::default(), Target::default());
    let (mut depth_texture, mut depth_srv) = (0u64, 0u64);
    if !load(targets.wrapping_add(0xdf4 + 4), &mut colour_slot)
        || colour_slot >= 101
        || !load(targets.wrapping_add(0xdf4 + 32 * 4), &mut motion_slot)
        || motion_slot >= 101
        || !load(targets.wrapping_add(0xf84 + 4), &mut depth_slot)
        || depth_slot >= 13
        || !load(target_at(before.renderer, colour_slot), &mut colour)
        || !load(target_at(before.renderer, motion_slot), &mut motion)
        || !load(depth_at(before.renderer, depth_slot), &mut depth_texture)
        || !load(
            depth_at(before.renderer, depth_slot).wrapping_add(0x88),
            &mut depth_srv,
        )
    {
        return None;
    }
    let devices = leaves.owner(lease, before.context, before.chain);
    if devices.immediate == 0
        || !leaves.same_native(devices.device, before.device as u64)
        || devices.chain_device_read == 0
        || !same_object(devices.device, devices.chain_device)
    {
        return None;
    }
    let swap = leaves.swap(before.chain);
    if swap.read == 0
        || swap.window != before.window as u64
        || swap.width != before.width
        || swap.height != before.height
        || !creation::accept_window(before.window as *mut c_void)
    {
        return None;
    }
    let mut views = [ResourceFacts::default(); 3];
    let (mut matches, mut devices_match) = ([0u32; 3], [0u32; 3]);
    let checks = [
        (
            SLOT_COLOUR,
            colour.srv,
            colour.texture,
            DXGI_FORMAT_R11G11B10_FLOAT,
        ),
        (
            SLOT_MOTION,
            motion.srv,
            motion.texture,
            DXGI_FORMAT_R16G16_FLOAT,
        ),
        (
            SLOT_DEPTH,
            depth_srv,
            depth_texture,
            DXGI_FORMAT_R24_UNORM_X8_TYPELESS,
        ),
    ];
    for (index, (slot, view, expected, format)) in checks.into_iter().enumerate() {
        let check = ViewCheck {
            facts: &mut views[index],
            resource_match: &mut matches[index],
            device_match: &mut devices_match[index],
        };
        if !view_texture(
            leaves,
            lease,
            slot,
            view,
            expected,
            devices.device,
            Some(format),
            check,
        ) {
            return None;
        }
    }
    if colour.rtv == 0 {
        return None;
    }
    let [c, m, d] = views;
    let rtv = leaves.resource(lease, NO_SLOT, colour.rtv, true);
    if !same_object(rtv.identity, c.identity)
        || rtv.view_format != DXGI_FORMAT_R11G11B10_FLOAT
        || rtv.view_dimension != D3D11_RTV_DIMENSION_TEXTURE2D
        || rtv.view_mip != 0
        || !extent(&c, before.width, before.height)
        || c.format != DXGI_FORMAT_R11G11B10_FLOAT
        || !extent(&d, before.width, before.height)
        || d.format != DXGI_FORMAT_R24G8_TYPELESS
        || !extent(&m, before.width, before.height)
        || m.format != DXGI_FORMAT_R16G16_FLOAT
        || c.sample_quality != 0
        || d.sample_quality != 0
        || m.sample_quality != 0
    {
        return None;
    }
    // No inactive-consumer predicate has been established. Require all three native
    // allocations, independently, rather than omitting a missing field.
    let refraction = auxiliary(
        leaves,
        lease,
        &before,
        devices.device,
        14,
        SLOT_REFRACTION_NORMALS,
    );
    let pipboy = auxiliary(leaves, lease, &before, devices.device, 30, SLOT_PIPBOY);
    let vats = auxiliary(leaves, lease, &before, devices.device, 15, SLOT_VATS);
    if !refraction || !pipboy || !vats {
        return None;
    }
    let mut after = State::default();
    let (mut confirmed_graph, mut confirmed_view) = (0usize, 0u64);
    let (mut confirmed_colour, mut confirmed_motion, mut confirmed_depth) = (0u32, 0u32, 0u32);
    let (mut colour_target, mut motion_target) = (Target::default(), Target::default());
    let (mut confirmed_depth_texture, mut confirmed_depth_srv) = (0u64, 0u64);
    if owner(&mut after).is_err()
        || !same_native_owner(&before, &after)
        || !load(base.wrapping_add(WORLD_GRAPH_POINTER), &mut confirmed_graph)
        || confirmed_graph != graph
        || !load(graph.wrapping_add(0x140), &mut confirmed_view)
        || confirmed_view != view
        || !load(targets.wrapping_add(0xdf4 + 4), &mut confirmed_colour)
        || confirmed_colour != colour_slot
        || !load(targets.wrapping_add(0xdf4 + 32 * 4), &mut confirmed_motion)
        || confirmed_motion != motion_slot
        || !load(targets.wrapping_add(0xf84 + 4), &mut confirmed_depth)
        || confirmed_depth != depth_slot
        || !load(target_at(before.renderer, colour_slot), &mut colour_target)
        || colour != colour_target
        || !load(target_at(before.renderer, motion_slot), &mut motion_target)
        || motion != motion_target
        || !load(
            depth_at(before.renderer, depth_slot),
            &mut confirmed_depth_texture,
        )
        || confirmed_depth_texture != depth_texture
        || !load(
            depth_at(before.renderer, depth_slot).wrapping_add(0x88),
            &mut confirmed_depth_srv,
        )
        || confirmed_depth_srv != depth_srv
    {
        return None;
    }
    if let Some(p) = producer
        && (p.depth_key != depth_texture
            || !depth.is_some_and(|depth| depth_domains::raster_current(depth, p)))
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
    leaves: &Leaves,
    lease: Lease,
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
    let devices = leaves.owner(lease, before.context, before.chain);
    if devices.immediate == 0 {
        return Err(Framebuffer::Context);
    }
    let facts = leaves.resource(lease, SLOT_COLOUR, target.rtv, true);
    if !leaves.same_native(devices.device, before.device as u64) {
        return Err(Framebuffer::Device);
    }
    // A missing resource or a failed Texture2D query.
    r.leased = facts.pointer;
    if facts.pointer == 0 {
        return Err(Framebuffer::Resource);
    }
    if target.texture != 0 && !leaves.same_native(facts.identity, target.texture) {
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
    r.swap = leaves.swap(before.chain);
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
    leaves: &Leaves,
    report: Report,
    stage: Framebuffer,
    owner_failure: Stage,
    r: &FramebufferReceipt,
) {
    if report.is_none() {
        return;
    }
    let bit = 1u32 << stage as u32;
    if FRAMEBUFFER_REPORTED.fetch_or(bit, Ordering::Relaxed) & bit != 0 {
        return;
    }
    if let Some(leaf) = leaves.framebuffer {
        unsafe {
            leaf(
                report,
                FRAMEBUFFER_NAMES[stage as usize].as_ptr(),
                NAMES[owner_failure as usize].as_ptr(),
                r,
            )
        };
    }
}

fn complete<'a>(leaves: *const Leaves) -> Option<&'a Leaves> {
    unsafe { leaves.as_ref() }.filter(|leaves| leaves.complete())
}

pub(crate) fn verify_for_build(_build: &creation::ExactBuild) -> bool {
    verify(|| true)
}

/// Checks the executable and the expected bytes of every native site the readers depend on.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_fallout4_inputs_verify() -> bool {
    verify(|| crate::ops().is_some_and(|ops| creation::verify_executable(ops).unwrap_or(false)))
}

/// The reader's exception receipt is due: its stage bit was not yet spent.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_inputs_claim_exception() -> bool {
    let bit = 1u64 << Stage::Exception as u32;
    REPORTED.fetch_or(bit, Ordering::Relaxed) & bit == 0
}

/// The full reader. `output` is cleared first and written only on admission; the C++ wrapper
/// keeps the lease only then.
///
/// # Safety
/// `leaves` is the complete process-lived table, `depth` the depth-domain table or null, `lease`
/// the caller's empty `NativeInputLease`, `producer` null or readable and `output` writable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_inputs_read(
    leaves: *const Leaves,
    depth: *const depth_domains::Leaves,
    lease: Lease,
    report: Report,
    producer: *const Snapshot,
    output: *mut rsf_game_render_pass,
) -> bool {
    let Some(output) = (unsafe { output.as_mut() }) else {
        return false;
    };
    // SAFETY: the SDK record holds only integers, floats and raw pointers.
    *output = unsafe { std::mem::zeroed() };
    let Some(leaves) = complete(leaves) else {
        return false;
    };
    if lease.is_null() {
        return false;
    }
    let depth = depth_domains::complete(depth);
    let producer = unsafe { producer.as_ref() }.copied();
    let mut reading = Reading::new();
    match read_inputs(
        leaves,
        depth,
        lease,
        report,
        producer.as_ref(),
        &mut reading,
    ) {
        Ok(()) => {
            receipt(leaves, report, Stage::Accepted, &mut reading);
            *output = reading.pass;
            true
        }
        Err(stage) => {
            receipt(leaves, report, stage, &mut reading);
            false
        }
    }
}

/// The raster reader. `output` is cleared first and written only on admission.
///
/// # Safety
/// `leaves` is the complete process-lived table, `depth` the depth-domain table or null, `lease`
/// the caller's empty `NativeInputLease`, `frame` readable, `producer` null or readable and
/// `output` writable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_inputs_read_raster(
    leaves: *const Leaves,
    depth: *const depth_domains::Leaves,
    lease: Lease,
    frame: *const rsf_game_render_pass,
    producer: *const Snapshot,
    output: *mut rsf_game_render_pass,
) -> bool {
    if output.is_null() {
        return false;
    }
    // SAFETY: the SDK record holds only integers, floats and raw pointers.
    unsafe { output.write(std::mem::zeroed()) };
    let Some(leaves) = complete(leaves) else {
        return false;
    };
    if lease.is_null() || frame.is_null() {
        return false;
    }
    let frame = unsafe { frame.read() };
    let depth = depth_domains::complete(depth);
    let producer = unsafe { producer.as_ref() }.copied();
    match read_raster(leaves, depth, lease, &frame, producer.as_ref()) {
        Some(pass) => {
            unsafe { output.write(pass) };
            true
        }
        None => false,
    }
}

/// The framebuffer reader. `output` is cleared first and written only on admission.
///
/// # Safety
/// `leaves` is the complete process-lived table, `lease` the caller's empty `NativeInputLease`
/// and `output` writable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_inputs_read_framebuffer(
    leaves: *const Leaves,
    lease: Lease,
    report: Report,
    output: *mut FramebufferFacts,
) -> bool {
    let Some(output) = (unsafe { output.as_mut() }) else {
        return false;
    };
    *output = FramebufferFacts {
        native_frame: 0,
        window_key: 0,
    };
    let Some(leaves) = complete(leaves) else {
        return false;
    };
    if lease.is_null() {
        return false;
    }
    let mut receipt = FramebufferReceipt::default();
    let mut owner_failure = Stage::Owner;
    match read_framebuffer(leaves, lease, &mut receipt, &mut owner_failure) {
        Ok(()) => {
            framebuffer_receipt(
                leaves,
                report,
                Framebuffer::Accepted,
                owner_failure,
                &receipt,
            );
            *output = FramebufferFacts {
                native_frame: u64::from(receipt.before.frame),
                window_key: receipt.before.window as u64,
            };
            true
        }
        Err(stage) => {
            framebuffer_receipt(leaves, report, stage, owner_failure, &receipt);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_numbers_and_names_follow_the_cpp_order() {
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

    // tests/fallout4_input_state.cpp checks the same rules on the C++ header.
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
        let mut after = before;
        after.default_viewport = 1;
        assert!(!same_native_state(&before, &after) && same_native_owner(&before, &after));
    }

    #[test]
    fn camera_and_raster_comparisons_are_bytewise() {
        let a = NO_CAMERA;
        let mut b = a;
        assert!(same_camera(&a, &b));
        b.jitter_ndc[1] = -0.0;
        assert!(!same_camera(&a, &b));
        let mut raster = NO_RASTER;
        assert!(same_raster(&raster, &NO_RASTER));
        raster.crop[3] = 1;
        assert!(!same_raster(&raster, &NO_RASTER));
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
