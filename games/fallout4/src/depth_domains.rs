// SPDX-License-Identifier: MIT
//! Fallout 4 native depth-domain admission and composition: the code guards, the sampled native
//! owner, camera cache and range records, producer raster and target admission, the camera math
//! that recovers the near and world source-clip maps, and the two composed GameSDK depth domains.
//!
//! Rust samples the D3D11 facts directly on the renderer thread. The native leaves format and
//! report refusal receipts. Engine memory is read here through the guarded
//! `crate::read`. Matrix work goes through `camera::xm` and the camera admission helpers, which
//! repeat the DirectXMath SSE2 operations the former C++ compiled to.
//!
//! Every refusal names the stage the C++ named, reports it once per stage when a report callback
//! is supplied, and leaves fields written before the failing step in the reported copy. Outputs
//! are cleared on entry and written only on admission.

mod native;

use crate::build_layout::{
    RENDERER_CHAIN, RENDERER_CONTEXT, RENDERER_DEVICE, RENDERER_OBJECT, RENDERER_POINTER,
    RENDERER_WINDOW,
};
use crate::camera::xm::{self, Matrix};
use crate::camera::{self, CameraDepthBound, CameraRaster};
use crate::cpu_abi::rsf_fallout4_cpu_identity;
use crate::cpu_events::{current_identity, render_source_is_current};
use crate::d3d11::same_native;
use crate::{Plain, image, read};
use rsf_game_sys::{
    RSF_DEPTH_DOMAIN_INCLUDE_MAX, RSF_DEPTH_DOMAIN_INCLUDE_MIN, RSF_DEPTH_DOMAINS_VERSION,
    RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_RENDER_PRIMARY, RSF_GAME_RENDER_SR,
    rsf_game_depth_domain, rsf_game_depth_domains, rsf_game_render_pass,
};
use std::ffi::{CStr, c_char};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

type Flat = [f32; 16];
type Identity = rsf_fallout4_cpu_identity;
type Report = Option<unsafe extern "C" fn(*const c_char)>;

const GRAPHICS_RVA: usize = 0x3d871a0;
const TARGETS_RVA: usize = 0x2f4d750;

// Enumerators compared by value; depth_domains.cpp asserts each against the SDK headers.
const DXGI_FORMAT_R24G8_TYPELESS: u32 = 44;
const DXGI_FORMAT_D24_UNORM_S8_UINT: u32 = 45;
const DXGI_FORMAT_R24_UNORM_X8_TYPELESS: u32 = 46;
const D3D11_DSV_DIMENSION_TEXTURE2D: u32 = 3;
const D3D11_RTV_DIMENSION_TEXTURE2D: u32 = 4;
const D3D11_SRV_DIMENSION_TEXTURE2D: u32 = 4;

static VERIFIED: AtomicBool = AtomicBool::new(false);
static REPORTED: AtomicU64 = AtomicU64::new(0);
static LEASE_BUDGET: AtomicU32 = AtomicU32::new(0);

/// depth_domains.h `NativeDepthCamera`: one copied native camera cache entry.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct DepthCamera {
    pub(crate) view: Flat,
    pub(crate) projection: Flat,
    view_projection: Flat,
    pub(crate) view_projection_unjittered: Flat,
    pub(crate) origin: [f32; 3],
    port: [f32; 4],
}

/// depth_domains.h `NativeDepthSnapshot`. Keys are opaque identities, never dereferenced from a
/// copy.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Snapshot {
    identity: Identity,
    pub(crate) native_frame: u64,
    renderer_key: u64,
    context_key: u64,
    pub(crate) device_context_key: u64,
    device_key: u64,
    pub(crate) window_key: u64,
    pub(crate) chain_key: u64,
    graph_key: u64,
    near_camera_key: u64,
    pub(crate) world_camera_key: u64,
    pub(crate) cache_key: u64,
    pub(crate) depth_key: u64,
    depth_srv_key: u64,
    pub(crate) width: u32,
    pub(crate) height: u32,
    cache_count: u32,
    depth_slot: u32,
    taa: u32,
    disable: u32,
    mode: u8,
    intervals: [f32; 4],
    jitter: [f32; 2],
    ratio: [f32; 2],
    near_inverse_transposed: Flat,
    near_camera: DepthCamera,
    pub(crate) world_camera: DepthCamera,
    world_unjittered_camera: DepthCamera,
    pub(crate) render_rect: [i32; 4],
    pub(crate) actual_raster: [f32; 6],
    shadow_raster: [f32; 6],
    normalized_port: [f32; 4],
    shadow_dirty: u32,
    colour_selector: u32,
    depth_selector: u32,
    cube_selector: u32,
    descriptor_width: u32,
    descriptor_height: u32,
    descriptor_read: bool,
    cube_key: u64,
    depth_dsv_key: u64,
    colour_slot: u32,
    bound_colour_slot: u32,
    bound_depth_slot: u32,
    colour_texture_key: u64,
    colour_rtv_key: u64,
    scissor: [i32; 4],
    scissor_enabled: bool,
    default_viewport: u8,
    dynamic_viewport: u8,
    valid: bool,
    raster_valid: bool,
    maps: Maps,
}

#[derive(Clone, Copy, Default)]
struct TextureFacts {
    resource: u64,
    device: u64,
    texture: u32,
    width: u32,
    height: u32,
    format: u32,
    mip_levels: u32,
    array_size: u32,
    sample_count: u32,
    sample_quality: u32,
}

#[derive(Clone, Copy, Default)]
struct ContextFacts {
    device: u64,
    immediate: u32,
    viewport_count: u32,
    viewport_copied: u32,
    viewport: [f32; 6],
}

#[derive(Clone, Copy, Default)]
struct TargetFacts {
    rtv: u64,
    dsv: u64,
    rtv_identity: u64,
    depth: TextureFacts,
    colour: TextureFacts,
    dsv_format: u32,
    dsv_dimension: u32,
    dsv_mip: u32,
    rtv_dimension: u32,
    rtv_mip: u32,
}

#[derive(Clone, Copy, Default)]
struct ScissorFacts {
    enabled: u32,
    count: u32,
    copied: u32,
    rect: [i32; 4],
}

#[derive(Clone, Copy, Default)]
struct ViewFacts {
    resource: u64,
    device: u64,
    format: u32,
    dimension: u32,
    most_detailed_mip: u32,
    mip_levels: u32,
}

/// depth_domains.cpp `NativeDepthMathFailure`: the largest projection error and the last
/// forward-W bound, printed by the refusal leaf.
#[repr(C)]
#[allow(dead_code)] // the W fields are read by the C++ receipt leaf
struct MathFailure {
    index: u32,
    actual: f32,
    expected: f32,
    relative_error: f32,
    w: [f64; 4],
    minimum_w: f64,
    range: f64,
}

unsafe extern "C" {
    fn rsf_fallout4_depth_report_refusal(
        report: Report,
        stage: *const c_char,
        snapshot: &Snapshot,
        detail: *const MathFailure,
        math: u32,
    );
    fn rsf_fallout4_depth_report_lease(
        report: Report,
        snapshot: &Snapshot,
        current: &Snapshot,
        reached: u32,
        passed: u32,
        stage: u32,
        changed: u32,
    );
}

const _: () = {
    use std::mem::offset_of;
    assert!(size_of::<Identity>() == 40);
    assert!(size_of::<DepthCamera>() == 71 * 4);
    assert!(size_of::<Snapshot>() == 1496);
    assert!(offset_of!(Snapshot, native_frame) == 40);
    assert!(offset_of!(Snapshot, width) == 144);
    assert!(offset_of!(Snapshot, mode) == 168);
    assert!(offset_of!(Snapshot, intervals) == 172);
    assert!(offset_of!(Snapshot, near_camera) == 268);
    assert!(offset_of!(Snapshot, render_rect) == 1120);
    assert!(offset_of!(Snapshot, descriptor_read) == 1224);
    assert!(offset_of!(Snapshot, cube_key) == 1232);
    assert!(offset_of!(Snapshot, colour_slot) == 1248);
    assert!(offset_of!(Snapshot, colour_texture_key) == 1264);
    assert!(offset_of!(Snapshot, scissor) == 1280);
    assert!(offset_of!(Snapshot, scissor_enabled) == 1296);
    assert!(offset_of!(Snapshot, raster_valid) == 1300);
    assert!(offset_of!(Snapshot, maps) == 1304);
    assert!(size_of::<MathFailure>() == 64 && offset_of!(MathFailure, w) == 16);
    assert!(offset_of!(MathFailure, range) == 56);
};

const NO_IDENTITY: Identity = Identity::ZERO;

const NO_CAMERA: DepthCamera = DepthCamera {
    view: [0.0; 16],
    projection: [0.0; 16],
    view_projection: [0.0; 16],
    view_projection_unjittered: [0.0; 16],
    origin: [0.0; 3],
    port: [0.0; 4],
};

/// The C++ value-initialized snapshot.
pub(crate) const EMPTY: Snapshot = Snapshot {
    identity: NO_IDENTITY,
    native_frame: 0,
    renderer_key: 0,
    context_key: 0,
    device_context_key: 0,
    device_key: 0,
    window_key: 0,
    chain_key: 0,
    graph_key: 0,
    near_camera_key: 0,
    world_camera_key: 0,
    cache_key: 0,
    depth_key: 0,
    depth_srv_key: 0,
    width: 0,
    height: 0,
    cache_count: 0,
    depth_slot: 0,
    taa: 0,
    disable: 0,
    mode: 0,
    intervals: [0.0; 4],
    jitter: [0.0; 2],
    ratio: [0.0; 2],
    near_inverse_transposed: [0.0; 16],
    near_camera: NO_CAMERA,
    world_camera: NO_CAMERA,
    world_unjittered_camera: NO_CAMERA,
    render_rect: [0; 4],
    actual_raster: [0.0; 6],
    shadow_raster: [0.0; 6],
    normalized_port: [0.0; 4],
    shadow_dirty: 0,
    colour_selector: 0,
    depth_selector: 0,
    cube_selector: 0,
    descriptor_width: 0,
    descriptor_height: 0,
    descriptor_read: false,
    cube_key: 0,
    depth_dsv_key: 0,
    colour_slot: 0,
    bound_colour_slot: 0,
    bound_depth_slot: 0,
    colour_texture_key: 0,
    colour_rtv_key: 0,
    scissor: [0; 4],
    scissor_enabled: false,
    default_viewport: 0,
    dynamic_viewport: 0,
    valid: false,
    raster_valid: false,
    maps: Maps {
        near: [[0.0; 4]; 4],
        world: [[0.0; 4]; 4],
        projection: [[0.0; 4]; 4],
    },
};

const NO_FAILURE: MathFailure = MathFailure {
    index: 0,
    actual: 0.0,
    expected: 0.0,
    relative_error: 0.0,
    w: [0.0; 4],
    minimum_w: 0.0,
    range: 0.0,
};

const NO_DOMAIN: rsf_game_depth_domain = rsf_game_depth_domain {
    classify_min: 0.0,
    classify_max: 0.0,
    endpoint_flags: 0,
    reserved: 0,
    viewport_xywh: [0.0; 4],
    viewport_min_depth: 0.0,
    viewport_max_depth: 0.0,
    source_clip_to_target_clip: [0.0; 16],
};

const NO_DOMAINS: rsf_game_depth_domains = rsf_game_depth_domains {
    struct_size: 0,
    version: 0,
    domain_count: 0,
    clear_enabled: 0,
    clear_value: 0.0,
    reserved: [0; 3],
    domains: [NO_DOMAIN; 2],
};

/// Refusal stages in the C++ order; the lease receipt prints the number.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
enum Stage {
    Unverified,
    Source,
    Tls,
    Owner,
    Dimensions,
    Cache,
    Range,
    CameraOwner,
    DepthOwner,
    CacheDuplicate,
    CacheMissing,
    CameraRead,
    CameraMath,
    WorldCache,
    NearInverse,
    RasterMath,
    PassAssociation,
    TargetCamera,
    DepthFormat,
    DepthView,
    StateChanged,
    ProducerRaster,
    ProducerTarget,
    ProducerShadow,
    ProducerScissor,
    ProjectionMismatch,
    FiniteMap,
    NearW,
    WorldW,
    ProducerSelector,
    ProducerDescriptor,
    ProducerTargetDirty,
    ProducerTargetMap,
    ProducerColour,
    NearBasis,
}

const NAMES: [&CStr; 35] = [
    c"unverified",
    c"source",
    c"tls",
    c"owner",
    c"dimensions",
    c"cache",
    c"range",
    c"camera-owner",
    c"depth-owner",
    c"cache-duplicate",
    c"cache-missing",
    c"camera-read",
    c"camera-math",
    c"world-cache",
    c"near-inverse",
    c"raster-math",
    c"pass-association",
    c"target-camera",
    c"depth-format",
    c"depth-view",
    c"state-changed",
    c"producer-raster",
    c"producer-target",
    c"producer-shadow",
    c"producer-scissor",
    c"projection-mismatch",
    c"finite-map",
    c"near-W",
    c"world-W",
    c"producer-selector",
    c"producer-descriptor",
    c"producer-target-dirty",
    c"producer-target-map",
    c"producer-colour",
    c"near-basis",
];
const _: () = assert!(Stage::NearBasis as usize + 1 == NAMES.len() && NAMES.len() <= 64);

/// Two canonical identities of live objects name the same object.
fn same_object(a: u64, b: u64) -> bool {
    a != 0 && b != 0 && a == b
}

// `!above(a, b)` keeps the C++ `!(a>b)` meaning for NaN.
fn above(value: f32, bound: f32) -> bool {
    value > bound
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

/// `key + offset` with the C++ unsigned wraparound.
fn at(key: u64, offset: usize) -> usize {
    (key as usize).wrapping_add(offset)
}

fn guard<const N: usize>(rva: usize, expected: &[u8; N]) -> bool {
    let mut actual = [0u8; N];
    load(image().wrapping_add(rva), &mut actual) && actual == *expected
}

fn bits<const N: usize>(values: &[f32; N]) -> [u32; N] {
    values.map(f32::to_bits)
}

/// `memcmp` equality of two cameras.
fn same_camera(a: &DepthCamera, b: &DepthCamera) -> bool {
    bits(&a.view) == bits(&b.view)
        && bits(&a.projection) == bits(&b.projection)
        && bits(&a.view_projection) == bits(&b.view_projection)
        && bits(&a.view_projection_unjittered) == bits(&b.view_projection_unjittered)
        && bits(&a.origin) == bits(&b.origin)
        && bits(&a.port) == bits(&b.port)
}

/// A nonzero session, source and submission, and every field equal.
fn same_identity(a: &Identity, b: &Identity) -> bool {
    a.session != 0 && a.source != 0 && a.submission != 0 && a == b
}

/// Reports `stage` once for the process when a callback is supplied. Always refuses.
fn refuse(stage: Stage, snapshot: &Snapshot, report: Report, detail: Option<&MathFailure>) -> bool {
    if report.is_some() {
        let bit = 1u64 << stage as u32;
        if REPORTED.fetch_or(bit, Ordering::Relaxed) & bit == 0 {
            let math = match (detail, stage) {
                (Some(_), Stage::ProjectionMismatch) => 1,
                (Some(_), Stage::NearW | Stage::WorldW) => 2,
                _ => 0,
            };
            let detail = detail.map_or(std::ptr::null(), std::ptr::from_ref);
            unsafe {
                rsf_fallout4_depth_report_refusal(
                    report,
                    NAMES[stage as usize].as_ptr(),
                    snapshot,
                    detail,
                    math,
                )
            };
        }
    }
    false
}

fn refused<T>(
    stage: Stage,
    snapshot: &Snapshot,
    report: Report,
    detail: Option<&MathFailure>,
) -> Option<T> {
    refuse(stage, snapshot, report, detail);
    None
}

/// `matches`: the stored value is finite and agrees with `expected` element by element.
fn matches(value: &Matrix, expected: &Flat) -> bool {
    let mut actual = [0.0; 16];
    camera::store(value, &mut actual)
        && actual
            .iter()
            .zip(expected)
            .all(|(&actual, &expected)| camera::near(actual, expected))
}

/// `XMMatrixTranspose`, a pure shuffle.
fn transpose(m: &Matrix) -> Matrix {
    std::array::from_fn(|row| std::array::from_fn(|column| m[column][row]))
}

fn forward_domain(value: &Matrix, range: f32, detail: &mut MathFailure) -> bool {
    let mut m = [0.0; 16];
    if !camera::store(value, &mut m) || !above(range, 0.0) {
        return false;
    }
    let mut bound = CameraDepthBound {
        coefficients: [0.0; 4],
        minimum: 0.0,
        range: 0.0,
    };
    let forward = camera::depth_domain_forward(&m, range, &mut bound);
    detail.w = bound.coefficients;
    detail.minimum_w = bound.minimum;
    detail.range = bound.range;
    forward
}

fn read_camera(entry: usize, key: u64, jitter: u8, output: &mut DepthCamera) -> bool {
    let (mut actual_key, mut actual_jitter) = (0u64, 0u8);
    load(entry.wrapping_add(0x238), &mut actual_key)
        && actual_key == key
        && load(entry.wrapping_add(0x240), &mut actual_jitter)
        && actual_jitter == jitter
        && load(entry.wrapping_add(0x50), &mut output.view)
        && load(entry.wrapping_add(0x90), &mut output.projection)
        && load(entry.wrapping_add(0xd0), &mut output.view_projection)
        && load(
            entry.wrapping_add(0x110),
            &mut output.view_projection_unjittered,
        )
        && load(entry.wrapping_add(0x210), &mut output.origin)
        && load(at(key, 0x184), &mut output.port)
}

fn valid_camera(value: &DepthCamera) -> bool {
    if value.origin.iter().any(|v| !v.is_finite()) {
        return false;
    }
    // Camera ports remain native normalized facts; producer raster validates their common
    // allocation-space viewport independently of the ratios.
    if value
        .port
        .iter()
        .any(|&v| !v.is_finite() || !(0.0..=1.0).contains(&v))
    {
        return false;
    }
    if !above(value.port[1], value.port[0]) || !above(value.port[2], value.port[3]) {
        return false;
    }
    [
        &value.view,
        &value.projection,
        &value.view_projection,
        &value.view_projection_unjittered,
    ]
    .iter()
    .all(|m| camera::invert(&xm::load(m)).is_some())
        && matches(
            &xm::multiply(&xm::load(&value.view), &xm::load(&value.projection)),
            &value.view_projection,
        )
}

/// The full temporal sample: owner, dimensions, cache, range, camera owners, depth owner and the
/// three unique camera cache entries.
fn sample(cpu: &Identity, out: &mut Snapshot, stage: &mut Stage) -> bool {
    let base = image();
    let state = base.wrapping_add(GRAPHICS_RVA);
    let targets = base.wrapping_add(TARGETS_RVA);
    out.identity = *cpu;
    let mut graph_camera = 0u64;
    *stage = Stage::Tls;
    let Some((context, Some(tls_device_context))) = crate::build_layout::render_context(base, true)
    else {
        return false;
    };
    out.context_key = context as u64;
    *stage = Stage::Owner;
    if context == 0
        || !load(base.wrapping_add(RENDERER_POINTER), &mut out.renderer_key)
        || out.renderer_key != base.wrapping_add(RENDERER_OBJECT) as u64
        || !load(at(out.renderer_key, RENDERER_DEVICE), &mut out.device_key)
        || out.device_key == 0
        || !load(
            at(out.renderer_key, RENDERER_CONTEXT),
            &mut out.device_context_key,
        )
        || out.device_context_key == 0
        || tls_device_context as u64 != out.device_context_key
        || !load(at(out.renderer_key, RENDERER_WINDOW), &mut out.window_key)
        || out.window_key == 0
        || !load(at(out.renderer_key, RENDERER_CHAIN), &mut out.chain_key)
        || out.chain_key == 0
    {
        return false;
    }
    let mut frame = 0u32;
    *stage = Stage::Dimensions;
    if !load(state.wrapping_add(0x98), &mut frame) {
        return false;
    }
    out.native_frame = u64::from(frame);
    if !load(state.wrapping_add(0x78), &mut out.width)
        || !load(state.wrapping_add(0x7c), &mut out.height)
        || out.width == 0
        || out.height == 0
        || out.width > 16384
        || out.height > 16384
    {
        return false;
    }
    *stage = Stage::Cache;
    if !load(state.wrapping_add(0x140), &mut out.cache_key)
        || out.cache_key == 0
        || !load(state.wrapping_add(0x150), &mut out.cache_count)
        || out.cache_count == 0
        || out.cache_count > 256
        || !load(state.wrapping_add(0xac), &mut out.taa)
        || out.taa == 0
        || !load(state.wrapping_add(0xb0), &mut out.disable)
        || out.disable != 0
        || !load(state.wrapping_add(4), &mut out.jitter)
        || !load(targets.wrapping_add(0xfb8), &mut out.ratio)
        || !load(targets.wrapping_add(0xfe4), &mut out.default_viewport)
        || !load(targets.wrapping_add(0xfe5), &mut out.dynamic_viewport)
    {
        return false;
    }
    if out
        .ratio
        .iter()
        .any(|&ratio| !ratio.is_finite() || ratio <= 0.0 || ratio > 1.0)
    {
        return false;
    }
    *stage = Stage::Range;
    if !load(base.wrapping_add(0x3e62250), &mut out.mode)
        || out.mode != 0
        || !load(base.wrapping_add(0x3e62190), &mut out.intervals[0])
        || !load(base.wrapping_add(0x2f97b30), &mut out.intervals[1])
        || !load(base.wrapping_add(0x3e62194), &mut out.intervals[2])
        || !load(base.wrapping_add(0x2f97b34), &mut out.intervals[3])
    {
        return false;
    }
    let r = out.intervals;
    if r.iter().any(|v| !v.is_finite()) {
        return false;
    }
    // Finite here, so the C++ `!(r1>0 && r1<1)` is `r1<=0 || r1>=1`.
    if r[0] != 0.0 || r[1] <= 0.0 || r[1] >= 1.0 || r[2] != r[1] || r[3] != 1.0 {
        return false;
    }
    *stage = Stage::CameraOwner;
    if !load(base.wrapping_add(0x3e622d0), &mut out.near_camera_key)
        || out.near_camera_key == 0
        || !load(base.wrapping_add(0x3e62210), &mut out.world_camera_key)
        || out.world_camera_key == 0
        || !load(base.wrapping_add(0x32dd338), &mut out.graph_key)
        || out.graph_key == 0
        || !load(at(out.graph_key, 0x140), &mut graph_camera)
        || graph_camera != out.world_camera_key
    {
        return false;
    }
    *stage = Stage::DepthOwner;
    if !load(targets.wrapping_add(0xf84 + 4), &mut out.depth_slot) || out.depth_slot >= 13 {
        return false;
    }
    let depth = 0x1d48 + out.depth_slot as usize * 0x98;
    if !load(at(out.renderer_key, depth), &mut out.depth_key)
        || out.depth_key == 0
        || !load(at(out.renderer_key, depth + 0x88), &mut out.depth_srv_key)
        || out.depth_srv_key == 0
        || !load(at(context, 0x2410), &mut out.near_inverse_transposed)
    {
        return false;
    }
    let (mut near_entry, mut world_entry, mut unjittered_entry) = (0usize, 0usize, 0usize);
    for index in 0..out.cache_count {
        let entry = at(out.cache_key, index as usize * 0x250);
        let (mut key, mut jitter) = (0u64, 0u8);
        *stage = Stage::Cache;
        if !load(entry.wrapping_add(0x238), &mut key)
            || !load(entry.wrapping_add(0x240), &mut jitter)
        {
            return false;
        }
        *stage = Stage::CacheDuplicate;
        if key == out.near_camera_key && jitter == 1 {
            if near_entry != 0 {
                return false;
            }
            near_entry = entry;
        }
        if key == out.world_camera_key && jitter == 1 {
            if world_entry != 0 {
                return false;
            }
            world_entry = entry;
        }
        if key == out.world_camera_key && jitter == 0 {
            if unjittered_entry != 0 {
                return false;
            }
            unjittered_entry = entry;
        }
    }
    *stage = Stage::CacheMissing;
    if near_entry == 0 || world_entry == 0 || unjittered_entry == 0 {
        return false;
    }
    *stage = Stage::CameraRead;
    let (near_key, world_key) = (out.near_camera_key, out.world_camera_key);
    read_camera(near_entry, near_key, 1, &mut out.near_camera)
        && read_camera(world_entry, world_key, 1, &mut out.world_camera)
        && read_camera(
            unjittered_entry,
            world_key,
            0,
            &mut out.world_unjittered_camera,
        )
}

fn same(a: &Snapshot, b: &Snapshot) -> bool {
    let keys = |s: &Snapshot| {
        [
            s.native_frame,
            s.renderer_key,
            s.context_key,
            s.device_context_key,
            s.device_key,
            s.window_key,
            s.chain_key,
            s.graph_key,
            s.near_camera_key,
            s.world_camera_key,
            s.cache_key,
            s.depth_key,
            s.depth_srv_key,
        ]
    };
    let counts = |s: &Snapshot| {
        [
            s.width,
            s.height,
            s.cache_count,
            s.depth_slot,
            s.taa,
            s.disable,
        ]
    };
    let flags = |s: &Snapshot| [s.mode, s.default_viewport, s.dynamic_viewport];
    same_identity(&a.identity, &b.identity)
        && keys(a) == keys(b)
        && counts(a) == counts(b)
        && flags(a) == flags(b)
        && bits(&a.intervals) == bits(&b.intervals)
        && bits(&a.jitter) == bits(&b.jitter)
        && bits(&a.ratio) == bits(&b.ratio)
        && bits(&a.near_inverse_transposed) == bits(&b.near_inverse_transposed)
        && same_camera(&a.near_camera, &b.near_camera)
        && same_camera(&a.world_camera, &b.world_camera)
        && same_camera(&a.world_unjittered_camera, &b.world_unjittered_camera)
}

/// Source-clip maps recovered from one admitted sample.
#[repr(C)]
#[derive(Clone, Copy)]
struct Maps {
    near: Matrix,
    world: Matrix,
    projection: Matrix,
}

fn math(s: &Snapshot, stage: &mut Stage, detail: &mut MathFailure) -> Option<Maps> {
    *stage = Stage::CameraMath;
    if !valid_camera(&s.near_camera)
        || !valid_camera(&s.world_camera)
        || !valid_camera(&s.world_unjittered_camera)
    {
        return None;
    }
    if s.jitter.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let w = &s.world_camera;
    let u = &s.world_unjittered_camera;
    *stage = Stage::WorldCache;
    if bits(&w.origin) != bits(&u.origin)
        || !matches(&xm::load(&w.view), &u.view)
        || !matches(&xm::load(&w.view_projection_unjittered), &u.view_projection)
        || !matches(&xm::load(&u.view_projection), &u.view_projection_unjittered)
    {
        return None;
    }
    *stage = Stage::NearBasis;
    if !matches(&xm::load(&s.near_camera.view), &w.view) {
        return None;
    }
    *stage = Stage::NearInverse;
    let inverse_view = camera::invert(&xm::load(&w.view))?;
    let inverse_near_projection = camera::invert(&xm::load(&s.near_camera.projection))?;
    if !matches(
        &transpose(&inverse_near_projection),
        &s.near_inverse_transposed,
    ) {
        return None;
    }
    let inverse_world = camera::invert(&xm::load(&w.view_projection))?;
    let projection = xm::multiply(&inverse_view, &xm::load(&w.view_projection_unjittered));
    *stage = Stage::ProjectionMismatch;
    let mut recovered = [0.0; 16];
    if !camera::store(&projection, &mut recovered) {
        *stage = Stage::FiniteMap;
        return None;
    }
    let mut projection_matches = true;
    for (index, (&actual, &expected)) in recovered.iter().zip(&u.projection).enumerate() {
        let error = (actual - expected).abs() / 1.0_f32.max(actual.abs()).max(expected.abs());
        if error >= detail.relative_error {
            detail.index = index as u32;
            detail.actual = actual;
            detail.expected = expected;
            detail.relative_error = error;
        }
        if !camera::near(actual, expected) {
            projection_matches = false;
        }
    }
    if !projection_matches {
        return None;
    }
    // Native first-person roots supply a separate local camera pose. Describe the composed
    // pixel's view Z, never translate it into physical world space.
    let mut near_values = [0.0; 16];
    *stage = Stage::NearBasis;
    // The direct jitter0 projection has just passed the unchanged recovered-P comparison; use
    // its native structural zeros instead of roundoff in inv(V)*VP.
    if !camera::composed_view_depth(&s.near_camera.projection, &u.projection, &mut near_values) {
        return None;
    }
    let near = xm::load(&near_values);
    let world = xm::multiply(&inverse_world, &xm::load(&w.view_projection_unjittered));
    let mut checked = [0.0; 16];
    *stage = Stage::FiniteMap;
    if !camera::store(&near, &mut checked)
        || !camera::store(&world, &mut checked)
        || !camera::store(&projection, &mut checked)
    {
        return None;
    }
    *stage = Stage::NearW;
    if !forward_domain(&near, s.intervals[1] - s.intervals[0], detail) {
        return None;
    }
    *stage = Stage::WorldW;
    forward_domain(&world, s.intervals[3] - s.intervals[2], detail).then_some(Maps {
        near,
        world,
        projection,
    })
}

/// Producer raster and target admission after prepass: the immediate context on the owning
/// device, one viewport inside the allocation, the native depth DSV and slot0 colour owners, the
/// native shadow selectors, descriptor and physical slot maps, and a containing scissor.
/// `temporal` also requires every camera port to equal the shadow port.
fn producer_raster(s: &mut Snapshot, stage: &mut Stage, temporal: bool) -> bool {
    *stage = Stage::ProducerRaster;
    let context = native::context(s.device_context_key);
    if context.immediate == 0 {
        return false;
    }
    let device = context.device;
    if !same_native(device, s.device_key) {
        return false;
    }
    if context.viewport_count != 1 || context.viewport_copied != 1 {
        return false;
    }
    s.actual_raster = context.viewport;
    if s.actual_raster.iter().any(|v| !v.is_finite()) {
        return false;
    }
    let [x, y, width, height, min_depth, max_depth] = s.actual_raster;
    if x < 0.0
        || y < 0.0
        || width <= 0.0
        || height <= 0.0
        || f64::from(x) + f64::from(width) > f64::from(s.width)
        || f64::from(y) + f64::from(height) > f64::from(s.height)
        || min_depth < 0.0
        || max_depth > 1.0
        || min_depth >= max_depth
    {
        return false;
    }
    // Half-open viewport support evaluated at source texel centers.
    s.render_rect = [
        (f64::from(x) - 0.5).ceil() as i32,
        (f64::from(y) - 0.5).ceil() as i32,
        (f64::from(x) + f64::from(width) - 0.5).ceil() as i32,
        (f64::from(y) + f64::from(height) - 0.5).ceil() as i32,
    ];
    if s.render_rect[2] <= s.render_rect[0] || s.render_rect[3] <= s.render_rect[1] {
        return false;
    }
    *stage = Stage::ProducerTarget;
    let bound = native::targets(s.device_context_key);
    if bound.dsv == 0 {
        return false;
    }
    s.depth_dsv_key = bound.dsv;
    let depth = &bound.depth;
    if !same_native(depth.resource, s.depth_key) || depth.texture == 0 {
        return false;
    }
    if depth.width != s.width
        || depth.height != s.height
        || depth.format != DXGI_FORMAT_R24G8_TYPELESS
        || depth.mip_levels != 1
        || depth.array_size != 1
        || depth.sample_count != 1
        || depth.sample_quality != 0
    {
        return false;
    }
    if bound.dsv_format != DXGI_FORMAT_D24_UNORM_S8_UINT
        || bound.dsv_dimension != D3D11_DSV_DIMENSION_TEXTURE2D
        || bound.dsv_mip != 0
    {
        return false;
    }
    let shadow = at(s.context_key, 0x1b70);
    let targets = image().wrapping_add(TARGETS_RVA);
    *stage = Stage::ProducerSelector;
    let mut cube = 0u32;
    if !load(shadow, &mut s.shadow_dirty)
        || !load(shadow.wrapping_add(0x90), &mut s.shadow_raster)
        || !load(shadow.wrapping_add(0x6d0), &mut s.normalized_port)
        || !load(shadow.wrapping_add(0x84), &mut s.colour_selector)
        || !load(shadow.wrapping_add(0x88), &mut s.depth_selector)
        || !load(shadow.wrapping_add(0x8c), &mut s.cube_selector)
        || !load(shadow.wrapping_add(0x54), &mut cube)
    {
        return false;
    }
    s.cube_key = u64::from(cube);
    if cube != u32::MAX
        || s.depth_selector != 1
        || (s.colour_selector >= 100 && s.colour_selector != u32::MAX)
    {
        return false;
    }
    // Native width/height getters use the logical slot0 selector, not Main's ID.
    // SetCurrentRenderTarget separately maps that ID to the physical OM slot.
    *stage = Stage::ProducerDescriptor;
    let descriptor = if s.colour_selector != u32::MAX {
        targets.wrapping_add(s.colour_selector as usize * 0x20)
    } else {
        targets.wrapping_add(0xc80 + 0x1c)
    };
    if !load(descriptor, &mut s.descriptor_width)
        || !load(descriptor.wrapping_add(4), &mut s.descriptor_height)
    {
        return false;
    }
    s.descriptor_read = true;
    if s.descriptor_width != s.width || s.descriptor_height != s.height {
        return false;
    }
    *stage = Stage::ProducerTargetDirty;
    if s.shadow_dirty & 1 != 0 {
        return false;
    }
    *stage = Stage::ProducerTargetMap;
    if !load(shadow.wrapping_add(0x2c), &mut s.bound_colour_slot)
        || !load(shadow.wrapping_add(0x4c), &mut s.bound_depth_slot)
        || s.bound_depth_slot != s.depth_slot
    {
        return false;
    }
    s.colour_slot = u32::MAX;
    if s.colour_selector != u32::MAX
        && (!load(
            targets.wrapping_add(0xdf4 + s.colour_selector as usize * 4),
            &mut s.colour_slot,
        ) || s.colour_slot >= 101)
    {
        return false;
    }
    if s.bound_colour_slot != s.colour_slot {
        return false;
    }
    *stage = Stage::ProducerColour;
    if s.colour_selector == u32::MAX {
        if bound.rtv != 0 {
            return false;
        }
    } else {
        let (mut native_texture, mut native_rtv) = (0u64, 0u64);
        let target = at(s.renderer_key, 0xa58 + s.colour_slot as usize * 0x30);
        if !load(target, &mut native_texture)
            || native_texture == 0
            || !load(target.wrapping_add(0x10), &mut native_rtv)
            || native_rtv == 0
            || bound.rtv == 0
            || !same_native(bound.rtv_identity, native_rtv)
        {
            return false;
        }
        let colour = &bound.colour;
        if colour.resource == 0
            || colour.texture == 0
            || !same_native(colour.resource, native_texture)
        {
            return false;
        }
        if !same_object(colour.device, device)
            || colour.width != s.descriptor_width
            || colour.height != s.descriptor_height
            || colour.mip_levels != 1
            || colour.array_size != 1
            || colour.sample_count != 1
            || colour.sample_quality != 0
            || bound.rtv_dimension != D3D11_RTV_DIMENSION_TEXTURE2D
            || bound.rtv_mip != 0
        {
            return false;
        }
        s.colour_texture_key = native_texture;
        s.colour_rtv_key = native_rtv;
    }
    *stage = Stage::ProducerShadow;
    if s.shadow_raster.iter().any(|v| !v.is_finite()) {
        return false;
    }
    let world_port = s.world_camera.port;
    if (0..4).any(|i| {
        s.shadow_raster[i] != s.actual_raster[i]
            || (temporal
                && (s.normalized_port[i] != world_port[i]
                    || s.near_camera.port[i] != world_port[i]
                    || s.world_unjittered_camera.port[i] != world_port[i]))
    }) {
        return false;
    }
    let port = s.normalized_port;
    if port
        .iter()
        .any(|&value| !value.is_finite() || !(0.0..=1.0).contains(&value))
    {
        return false;
    }
    if !above(port[1], port[0]) || !above(port[2], port[3]) {
        return false;
    }
    let (allocation_width, allocation_height) = (s.width as f32, s.height as f32);
    if s.default_viewport == 0
        || ((s.ratio[0] < 1.0 || s.ratio[1] < 1.0) && s.dynamic_viewport == 0)
        || x != allocation_width * port[0]
        || y != allocation_height * (1.0 - port[2])
        || width > allocation_width * (port[1] - port[0])
        || height > allocation_height * (port[2] - port[3])
    {
        return false;
    }
    *stage = Stage::ProducerScissor;
    let scissor = native::scissor(s.device_context_key);
    s.scissor_enabled = scissor.enabled != 0;
    if s.scissor_enabled {
        if scissor.count != 1 {
            return false;
        }
        let [left, top, right, bottom] = scissor.rect;
        if scissor.copied != 1
            || left > s.render_rect[0]
            || top > s.render_rect[1]
            || right < s.render_rect[2]
            || bottom < s.render_rect[3]
        {
            return false;
        }
        s.scissor = scissor.rect;
    }
    true
}

/// Only native owner/raster prerequisites. No temporal cache, projection, history or CPU
/// sample-validity predicate is admitted through this boundary.
fn raster_owner(cpu: &Identity, out: &mut Snapshot) -> bool {
    let base = image();
    let state = base.wrapping_add(GRAPHICS_RVA);
    let targets = base.wrapping_add(TARGETS_RVA);
    out.identity = *cpu;
    let mut frame = 0u32;
    let Some((context, Some(tls_device))) = crate::build_layout::render_context(base, true) else {
        return false;
    };
    out.context_key = context as u64;
    if out.context_key == 0 && !load(base.wrapping_add(0x38e1398), &mut out.context_key) {
        return false;
    }
    if out.context_key == 0
        || !load(base.wrapping_add(RENDERER_POINTER), &mut out.renderer_key)
        || out.renderer_key != base.wrapping_add(RENDERER_OBJECT) as u64
        || !load(at(out.renderer_key, RENDERER_DEVICE), &mut out.device_key)
        || out.device_key == 0
        || !load(
            at(out.renderer_key, RENDERER_CONTEXT),
            &mut out.device_context_key,
        )
        || out.device_context_key == 0
        || tls_device as u64 != out.device_context_key
        || !load(at(out.renderer_key, RENDERER_WINDOW), &mut out.window_key)
        || out.window_key == 0
        || !load(at(out.renderer_key, RENDERER_CHAIN), &mut out.chain_key)
        || out.chain_key == 0
        || !load(state.wrapping_add(0x98), &mut frame)
        || !load(state.wrapping_add(0x78), &mut out.width)
        || !load(state.wrapping_add(0x7c), &mut out.height)
        || out.width == 0
        || out.height == 0
        || out.width > 16384
        || out.height > 16384
        || !load(base.wrapping_add(0x32dd338), &mut out.graph_key)
        || out.graph_key == 0
        || !load(at(out.graph_key, 0x140), &mut out.world_camera_key)
        || out.world_camera_key == 0
        || !load(targets.wrapping_add(0xfb8), &mut out.ratio)
        || !load(targets.wrapping_add(0xfe4), &mut out.default_viewport)
        || !load(targets.wrapping_add(0xfe5), &mut out.dynamic_viewport)
        || !load(targets.wrapping_add(0xf84 + 4), &mut out.depth_slot)
        || out.depth_slot >= 13
    {
        return false;
    }
    let depth = 0x1d48 + out.depth_slot as usize * 0x98;
    if !load(at(out.renderer_key, depth), &mut out.depth_key)
        || out.depth_key == 0
        || !load(at(out.renderer_key, depth + 0x88), &mut out.depth_srv_key)
        || out.depth_srv_key == 0
    {
        return false;
    }
    out.native_frame = u64::from(frame);
    out.ratio
        .iter()
        .all(|&v| v.is_finite() && v > 0.0 && v <= 1.0)
}

fn same_raster_owner(a: &Snapshot, b: &Snapshot) -> bool {
    let keys = |s: &Snapshot| {
        [
            s.native_frame,
            s.renderer_key,
            s.context_key,
            s.device_context_key,
            s.device_key,
            s.window_key,
            s.chain_key,
            s.graph_key,
            s.world_camera_key,
            s.depth_key,
            s.depth_srv_key,
        ]
    };
    same_identity(&a.identity, &b.identity)
        && keys(a) == keys(b)
        && [a.width, a.height, a.depth_slot] == [b.width, b.height, b.depth_slot]
}

fn same_raster(a: &Snapshot, b: &Snapshot) -> bool {
    let words = |s: &Snapshot| {
        [
            s.shadow_dirty,
            s.colour_selector,
            s.depth_selector,
            s.cube_selector,
            s.descriptor_width,
            s.descriptor_height,
            s.colour_slot,
            s.bound_colour_slot,
            s.bound_depth_slot,
        ]
    };
    let keys = |s: &Snapshot| {
        [
            s.cube_key,
            s.depth_dsv_key,
            s.colour_texture_key,
            s.colour_rtv_key,
        ]
    };
    a.render_rect == b.render_rect
        && bits(&a.actual_raster) == bits(&b.actual_raster)
        && bits(&a.shadow_raster) == bits(&b.shadow_raster)
        && bits(&a.normalized_port) == bits(&b.normalized_port)
        && a.descriptor_read == b.descriptor_read
        && a.scissor_enabled == b.scissor_enabled
        && a.scissor == b.scissor
        && words(a) == words(b)
        && keys(a) == keys(b)
}

fn verify() -> bool {
    let okay = guard(0x21f38b0, b"\x48\x89\x5c\x24\x08\x48\x89\x6c\x24\x10")
        && guard(0x21f9f69, b"\xe8\x42\x91\xff\xff")
        && guard(0x21f9f73, b"\xe8\x38\x99\xff\xff")
        && guard(
            0x21f3294,
            b"\x48\x8b\x15\x35\xf0\xc6\x01\x48\x8d\x0d\xfe\x3e\xb9\x01\x41\xb0\x01\xe8\x76\x8c\x63\xff",
        )
        && guard(
            0x21f32aa,
            b"\x48\x8b\x15\x5f\xef\xc6\x01\x48\x8d\x0d\xe8\x3e\xb9\x01\x41\xb0\x01\xe8\x60\x8c\x63\xff",
        )
        && guard(
            0x21f32c0,
            b"\x48\x8b\x15\x49\xef\xc6\x01\x48\x8d\x0d\xd2\x3e\xb9\x01\x45\x33\xc0\xe8\x4a\x8c\x63\xff",
        )
        && guard(0x182c02d, b"\xe8\x1e\x17\x00\x00")
        && guard(0x182bf4a, b"\x48\x8d\xb1\x40\x01\x00\x00\x8b\x46\x10")
        && guard(
            0x182bf6a,
            b"\x49\x39\xb8\x38\x02\x00\x00\x75\x09\x41\x38\x98\x40\x02\x00\x00",
        )
        && guard(
            0x182d9d9,
            b"\xe8\x52\x8a\xb0\xfe\x0f\x11\x83\xd0\x00\x00\x00\x0f\x11\x83\x10\x01\x00\x00",
        )
        && guard(
            0x182da16,
            b"\x41\x83\xbe\xac\x00\x00\x00\x00\x0f\x84\xc5\x00\x00\x00\x40\x84\xed\x0f\x84\xbc\x00\x00\x00",
        )
        && guard(0x21f38d1, b"\x44\x8b\x05\x40\xf7\xc7\x01")
        && guard(
            0x21f3920,
            b"\x65\x48\x8b\x04\x25\x58\x00\x00\x00\x41\x8b\xf1\x4a\x8b\x3c\xc0\x49\x8b\x04\x39",
        )
        && guard(
            0x21f3939,
            b"\x48\x8b\x05\x58\xda\x6e\x01\x48\x05\x70\x1b\x00\x00",
        )
        && guard(
            0x21f3b36,
            b"\x0f\x10\x80\x60\x07\x00\x00\x48\x8d\x4c\x24\x40\x0f\x10\x88\x70\x07\x00\x00\x0f\x10\x90\x80\x07\x00\x00\x0f\x10\x98\x90\x07\x00\x00\xe8\xe4\x25\x14\xfe",
        )
        && guard(
            0x21f3b94,
            b"\x0f\x11\x9b\xa0\x08\x00\x00\x0f\x11\xa3\xb0\x08\x00\x00\x0f\x11\x8b\xc0\x08\x00\x00\x0f\x11\x83\xd0\x08\x00\x00",
        )
        && guard(0x21f3db6, b"\xe8\xc5\x7e\x63\xff")
        && guard(
            0x182db0f,
            b"\x89\x83\x10\x02\x00\x00\x8b\x86\xa4\x00\x00\x00\x89\x83\x14\x02\x00\x00\x8b\x86\xa8\x00\x00\x00\x89\x83\x18\x02\x00\x00\x48\x89\xb3\x38\x02\x00\x00",
        )
        && guard(0x182db38, b"\x40\x88\xab\x40\x02\x00\x00")
        && guard(0x21f0e25, b"\x84\xc0\x75\x04\x44\x0f\x28\xc8")
        && guard(0x21f0e2d, b"\xf3\x44\x0f\x11\x0d\xfa\x6c\xda\x00")
        && guard(0x21f0e44, b"\xf3\x0f\x11\x35\x48\x13\xc7\x01")
        && guard(0x183985b, b"\x48\x63\x88\xf4\x1b\x00\x00")
        && guard(
            0x1839868,
            b"\x85\xc9\x78\x0c\x48\x8b\xc1\x48\xc1\xe0\x05\x42\x8b\x04\x00",
        )
        && guard(0x1839bc9, b"\x85\xd2\x75\x07\x44\x89\x83\x84\x00\x00\x00")
        && guard(0x1839bdf, b"\x49\x63\xc0\x8b\xb4\x81\xf4\x0d\x00\x00")
        && guard(
            0x1839c3c,
            b"\x89\x74\xab\x2c\xc7\x43\x54\xff\xff\xff\xff\x89\x7c\xab\x5c\x83\x0b\x01",
        )
        && guard(
            0x1839cb1,
            b"\x48\x63\xc2\x41\x89\x92\x88\x00\x00\x00\x8b\x8c\x81\x84\x0f\x00\x00",
        )
        && guard(
            0x1823db7,
            b"\x48\x63\x47\xd0\x83\xf8\xff\x74\x47\x48\x8d\x0c\x40\x48\x8b\x05\xd5\xd5\x0b\x02\x48\x03\xc9\x83\x3f\x00\x48\x8b\x94\xc8\x68\x0a\x00\x00",
        )
        && guard(0x1823f14, b"\x48\x8b\x01\xff\x90\x08\x01\x00\x00")
        && guard(0x1839878, b"\x48\x63\x88\x88\x00\x00\x00")
        && guard(0x1839883, b"\x48\x6b\xc9\x1c\x42\x8b\x84\x01\x80\x0c\x00\x00")
        && guard(0x18398df, b"\x48\xc1\xe0\x05\x42\x8b\x44\x00\x04")
        && guard(0x181805f, b"\x83\x0b\x02\x89\x83\xa4\x00\x00\x00")
        && guard(
            0x182430d,
            b"\x4c\x8d\x87\x90\x00\x00\x00\xba\x01\x00\x00\x00\x48\x8b\x01\xff\x90\x60\x01\x00\x00",
        );
    VERIFIED.store(okay, Ordering::Release);
    okay
}

fn capture_raster(cpu: &Identity, report: Report) -> Option<Snapshot> {
    let (mut before, mut after) = (EMPTY, EMPTY);
    let mut stage = Stage::Source;
    let admitted = VERIFIED.load(Ordering::Acquire)
        && same_identity(cpu, &current_identity())
        && raster_owner(cpu, &mut before)
        && producer_raster(&mut before, &mut stage, false)
        && raster_owner(cpu, &mut after)
        && producer_raster(&mut after, &mut stage, false)
        && same_raster_owner(&before, &after)
        && same_raster(&before, &after)
        && bits(&before.ratio) == bits(&after.ratio)
        && before.default_viewport == after.default_viewport
        && before.dynamic_viewport == after.dynamic_viewport
        && same_identity(cpu, &current_identity());
    if !admitted {
        return refused(stage, &before, report, None);
    }
    before.raster_valid = true;
    Some(before)
}

pub(crate) fn raster_current(snapshot: &Snapshot) -> bool {
    let mut current = EMPTY;
    VERIFIED.load(Ordering::Acquire)
        && snapshot.raster_valid
        && same_identity(&snapshot.identity, &current_identity())
        && raster_owner(&snapshot.identity, &mut current)
        && same_raster_owner(snapshot, &current)
}

// Output can borrow either the raster-only recovery producer or the temporal producer.
// The three keys are usable only during the current synchronous render callback.
pub(crate) fn resolution_owner(snapshot: &Snapshot) -> Option<(usize, usize, usize)> {
    let current = if snapshot.raster_valid {
        raster_current(snapshot)
    } else {
        snapshot_current(snapshot, None)
    };
    current.then_some((
        snapshot.context_key as usize,
        snapshot.device_context_key as usize,
        snapshot.chain_key as usize,
    ))
}

pub(crate) fn snapshot_current(snapshot: &Snapshot, report: Report) -> bool {
    let mut stage = Stage::Source;
    let mut current = EMPTY;
    let (mut reached, mut passed) = (0u32, 0u32);
    let mut check = |bit: u32, value: bool| {
        reached |= bit;
        if value {
            passed |= bit;
        }
        value
    };
    let okay = check(1, VERIFIED.load(Ordering::Acquire))
        && check(2, snapshot.valid)
        && check(4, same_identity(&snapshot.identity, &current_identity()))
        && check(8, render_source_is_current(&snapshot.identity))
        && check(16, sample(&snapshot.identity, &mut current, &mut stage))
        && check(32, same(snapshot, &current))
        && check(64, render_source_is_current(&snapshot.identity));
    if !okay && report.is_some() {
        let mut changed = 0u32;
        if passed & 16 != 0 {
            if snapshot.mode != current.mode
                || snapshot.default_viewport != current.default_viewport
                || snapshot.dynamic_viewport != current.dynamic_viewport
            {
                changed |= 1;
            }
            if bits(&snapshot.intervals) != bits(&current.intervals) {
                changed |= 2;
            }
            if bits(&snapshot.jitter) != bits(&current.jitter) {
                changed |= 4;
            }
            if bits(&snapshot.ratio) != bits(&current.ratio) {
                changed |= 8;
            }
            if bits(&snapshot.near_inverse_transposed) != bits(&current.near_inverse_transposed) {
                changed |= 16;
            }
            if !same_camera(&snapshot.near_camera, &current.near_camera) {
                changed |= 32;
            }
            if !same_camera(&snapshot.world_camera, &current.world_camera) {
                changed |= 64;
            }
            if !same_camera(
                &snapshot.world_unjittered_camera,
                &current.world_unjittered_camera,
            ) {
                changed |= 128;
            }
        }
        if LEASE_BUDGET.fetch_add(1, Ordering::Relaxed) < 16 {
            unsafe {
                rsf_fallout4_depth_report_lease(
                    report,
                    snapshot,
                    &current,
                    reached,
                    passed,
                    stage as u32,
                    changed,
                )
            };
        }
    }
    okay
}

fn capture(cpu: &Identity, report: Report) -> Option<Snapshot> {
    let (mut before, mut after) = (EMPTY, EMPTY);
    before.identity = *cpu;
    let mut stage = Stage::Unverified;
    let mut detail = NO_FAILURE;
    if !VERIFIED.load(Ordering::Acquire) {
        return refused(stage, &before, report, None);
    }
    if !same_identity(cpu, &current_identity()) || !render_source_is_current(cpu) {
        return refused(Stage::Source, &before, report, None);
    }
    if !sample(cpu, &mut before, &mut stage) || !producer_raster(&mut before, &mut stage, true) {
        return refused(stage, &before, report, Some(&detail));
    }
    let Some(maps) = math(&before, &mut stage, &mut detail) else {
        return refused(stage, &before, report, Some(&detail));
    };
    if !sample(cpu, &mut after, &mut stage) || !producer_raster(&mut after, &mut stage, true) {
        return refused(stage, &after, report, None);
    }
    if !same(&before, &after) || !same_raster(&before, &after) {
        return refused(Stage::StateChanged, &after, report, None);
    }
    if !same_identity(cpu, &current_identity()) || !render_source_is_current(cpu) {
        return refused(Stage::Source, &after, report, None);
    }
    before.maps = maps;
    before.valid = true;
    Some(before)
}

/// The primary SR pass names this snapshot's source, native owners, raster and output.
fn pass_matches(snapshot: &Snapshot, cpu: &Identity, pass: &rsf_game_render_pass) -> bool {
    let frame = &pass.camera;
    let render_width = snapshot.render_rect[2].wrapping_sub(snapshot.render_rect[0]) as u32;
    let render_height = snapshot.render_rect[3].wrapping_sub(snapshot.render_rect[1]) as u32;
    let output_rect = [0, 0, snapshot.width as i32, snapshot.height as i32];
    pass.struct_size as usize >= size_of::<rsf_game_render_pass>()
        && pass.role == RSF_GAME_RENDER_SR
        && pass.flags & RSF_GAME_RENDER_PRIMARY != 0
        && pass.session_id == cpu.session
        && pass.source_frame_id == cpu.source
        && pass.submission_id == cpu.submission
        && pass.scope_id == cpu.submission
        && pass.native_frame == snapshot.native_frame
        && pass.view_key == snapshot.world_camera_key
        && pass.family_key == snapshot.world_camera_key
        && pass.window_key == snapshot.window_key
        && pass.viewport_key == snapshot.window_key
        && pass.rhi_viewport_key == snapshot.window_key
        && pass.depth as u64 == snapshot.depth_key
        && pass.swapchain as u64 == snapshot.chain_key
        && pass.camera_valid != 0
        && frame.struct_size as usize >= size_of_val(frame)
        && frame.abi_version == RSF_GAME_CAMERA_ABI_VERSION
        && frame.depth_inverted == 0
        && frame.render_width == render_width
        && frame.render_height == render_height
        && frame.output_width == snapshot.width
        && frame.output_height == snapshot.height
        && pass.render_rect == snapshot.render_rect
        && pass.output_rect == output_rect
}

fn make(
    snapshot: &Snapshot,
    cpu: &Identity,
    pass: &rsf_game_render_pass,
    report: Report,
) -> Option<rsf_game_depth_domains> {
    if !VERIFIED.load(Ordering::Acquire) {
        return refused(Stage::Unverified, snapshot, report, None);
    }
    if !snapshot.valid
        || !same_identity(&snapshot.identity, cpu)
        || !same_identity(cpu, &current_identity())
        || !render_source_is_current(cpu)
    {
        return refused(Stage::Source, snapshot, report, None);
    }
    if !pass_matches(snapshot, cpu, pass) {
        return refused(Stage::PassAssociation, snapshot, report, None);
    }
    let (mut current, mut confirmed) = (EMPTY, EMPTY);
    let mut stage = Stage::Source;
    if !sample(cpu, &mut current, &mut stage) {
        return refused(stage, &current, report, None);
    }
    if !same(snapshot, &current) {
        return refused(Stage::StateChanged, &current, report, None);
    }
    let maps = &snapshot.maps;
    let mut native_projection = [0.0; 16];
    if !camera::store(&maps.projection, &mut native_projection) {
        return refused(Stage::FiniteMap, snapshot, report, None);
    }
    let [x, y, width, height, ..] = snapshot.actual_raster;
    let raster = CameraRaster {
        xywh: [x, y, width, height],
        crop: snapshot.render_rect,
    };
    let Some(affine) = camera::raster_transform(&raster) else {
        return refused(Stage::RasterMath, snapshot, report, None);
    };
    let affine = xm::load(&affine);
    let projection = xm::multiply(&maps.projection, &affine);
    let near = xm::multiply(&maps.near, &affine);
    let world = xm::multiply(&maps.world, &affine);
    let frame = &pass.camera;
    if !camera::invert(&projection).is_some_and(|inverse| {
        matches(&projection, &frame.view_to_clip) && matches(&inverse, &frame.clip_to_view)
    }) {
        return refused(Stage::TargetCamera, snapshot, report, None);
    }
    let w = &snapshot.world_camera;
    let world_view = xm::multiply(
        &xm::translation(-w.origin[0], -w.origin[1], -w.origin[2]),
        &xm::load(&w.view),
    );
    if !matches(&world_view, &frame.world_to_view)
        || !camera::invert(&world_view)
            .is_some_and(|view_world| matches(&view_world, &frame.view_to_world))
    {
        return refused(Stage::TargetCamera, snapshot, report, None);
    }
    let mut target = [0.0; 16];
    if !camera::store(&projection, &mut target) {
        return refused(Stage::TargetCamera, snapshot, report, None);
    }
    let Some([jitter_x, jitter_y]) =
        camera::projection_jitter(&w.projection, &native_projection, [width, height])
    else {
        return refused(Stage::TargetCamera, snapshot, report, None);
    };
    if !camera::near(frame.jitter_pixels[0], jitter_x)
        || !camera::near(frame.jitter_pixels[1], jitter_y)
    {
        return refused(Stage::TargetCamera, snapshot, report, None);
    }
    let depth = native::texture(pass.depth as u64);
    if depth.format != DXGI_FORMAT_R24G8_TYPELESS
        || depth.width != snapshot.width
        || depth.height != snapshot.height
        || depth.mip_levels != 1
        || depth.array_size != 1
        || depth.sample_count != 1
        || depth.sample_quality != 0
        || !same_native(depth.device, current.device_key)
    {
        return refused(Stage::DepthFormat, snapshot, report, None);
    }
    let view = native::view(current.depth_srv_key);
    if view.format != DXGI_FORMAT_R24_UNORM_X8_TYPELESS
        || view.dimension != D3D11_SRV_DIMENSION_TEXTURE2D
        || view.most_detailed_mip != 0
        || view.mip_levels != 1
        || !same_object(view.resource, depth.resource)
        || !same_object(view.device, depth.device)
    {
        return refused(Stage::DepthView, snapshot, report, None);
    }
    let mut result = NO_DOMAINS;
    result.struct_size = size_of::<rsf_game_depth_domains>() as u32;
    result.version = RSF_DEPTH_DOMAINS_VERSION;
    result.domain_count = 2;
    result.clear_enabled = 1;
    result.clear_value = 1.0;
    for (index, domain) in result.domains.iter_mut().enumerate() {
        let far = index == 1;
        domain.classify_min = if far { snapshot.intervals[1] } else { 0.0 };
        domain.classify_max = if far { 1.0 } else { snapshot.intervals[1] };
        // Explicit reference-aligned ownership policy, not primitive provenance.
        domain.endpoint_flags =
            RSF_DEPTH_DOMAIN_INCLUDE_MAX | if far { 0 } else { RSF_DEPTH_DOMAIN_INCLUDE_MIN };
        domain.viewport_xywh = [x, y, width, height];
        domain.viewport_min_depth = snapshot.intervals[index * 2];
        domain.viewport_max_depth = snapshot.intervals[index * 2 + 1];
        let map = if far { &world } else { &near };
        if !camera::store(map, &mut domain.source_clip_to_target_clip) {
            return refused(Stage::FiniteMap, snapshot, report, None);
        }
    }
    if !sample(cpu, &mut confirmed, &mut stage) {
        return refused(stage, &confirmed, report, None);
    }
    if !same(snapshot, &confirmed) {
        return refused(Stage::StateChanged, &confirmed, report, None);
    }
    if !same_identity(cpu, &current_identity()) || !render_source_is_current(cpu) {
        return refused(Stage::Source, &confirmed, report, None);
    }
    Some(result)
}

/// Checks the expected bytes of every native site the snapshot depends on.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_fallout4_depth_verify() -> bool {
    verify()
}

/// Owner and producer raster only, sampled twice. Never temporal admission.
///
/// # Safety
/// `cpu` is readable and `output` writable, and
/// neither overlaps the other.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_depth_capture_raster(
    cpu: *const Identity,
    output: *mut Snapshot,
    report: Report,
) -> bool {
    let Some(output) = (unsafe { output.as_mut() }) else {
        return false;
    };
    *output = EMPTY;
    let Some(cpu) = (unsafe { cpu.as_ref() }) else {
        return false;
    };
    let cpu = *cpu;
    capture_raster(&cpu, report).is_some_and(|snapshot| {
        *output = snapshot;
        true
    })
}

/// # Safety
/// `snapshot` is readable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_depth_raster_current(snapshot: *const Snapshot) -> bool {
    unsafe { snapshot.as_ref() }.is_some_and(raster_current)
}

/// The temporal snapshot after the original prepass returned at its exact caller.
///
/// # Safety
/// `cpu` is readable and `output` writable, and
/// neither overlaps the other.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_depth_capture(
    cpu: *const Identity,
    output: *mut Snapshot,
    report: Report,
) -> bool {
    let Some(output) = (unsafe { output.as_mut() }) else {
        return false;
    };
    *output = EMPTY;
    let Some(cpu) = (unsafe { cpu.as_ref() }) else {
        return false;
    };
    let cpu = *cpu;
    capture(&cpu, report).is_some_and(|snapshot| {
        *output = snapshot;
        true
    })
}

/// The two composed domains for the primary SR pass. `output` may lie inside `*pass` (the caller
/// passes `pass.depth_domains`), so it is cleared first through the pointer, the pass is read as a
/// copy afterwards, and the result is written only on admission.
///
/// # Safety
/// `snapshot`, `cpu` and `pass` are readable and
/// `output` is writable. Only `output` and `pass` may overlap.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_depth_make(
    snapshot: *const Snapshot,
    cpu: *const Identity,
    pass: *const rsf_game_render_pass,
    output: *mut rsf_game_depth_domains,
    report: Report,
) -> bool {
    if output.is_null() {
        return false;
    }
    unsafe { output.write(NO_DOMAINS) };
    let (Some(snapshot), Some(cpu)) = (unsafe { snapshot.as_ref() }, unsafe { cpu.as_ref() })
    else {
        return false;
    };
    if pass.is_null() {
        return false;
    }
    let (snapshot, cpu, pass) = (*snapshot, *cpu, unsafe { pass.read() });
    match make(&snapshot, &cpu, &pass, report) {
        Some(result) => {
            unsafe { output.write(result) };
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDENTITY: Flat = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];

    fn projection() -> Flat {
        let mut p = [0.0; 16];
        p[0] = 2.0;
        p[5] = 3.0;
        p[10] = 1.01;
        p[11] = 1.0;
        p[14] = -1.01;
        p
    }

    fn fixture() -> Snapshot {
        let p = projection();
        let entry = DepthCamera {
            view: IDENTITY,
            projection: p,
            view_projection: p,
            view_projection_unjittered: p,
            origin: [10.0, -4.0, 2.5],
            port: [0.0, 1.0, 1.0, 0.0],
        };
        let mut snapshot = EMPTY;
        snapshot.near_camera = entry;
        snapshot.world_camera = entry;
        snapshot.world_unjittered_camera = entry;
        snapshot.intervals = [0.0, 0.25, 0.25, 1.0];
        let inverse = camera::invert(&xm::load(&p)).expect("fixture projection inverts");
        assert!(camera::store(
            &transpose(&inverse),
            &mut snapshot.near_inverse_transposed
        ));
        snapshot
    }

    #[test]
    fn stage_numbers_and_names_follow_the_cpp_order() {
        assert_eq!(Stage::Unverified as u32, 0);
        assert_eq!(Stage::StateChanged as u32, 20);
        assert_eq!(Stage::ProjectionMismatch as u32, 25);
        assert_eq!(Stage::NearBasis as u32, 34);
        assert_eq!(NAMES[Stage::NearW as usize], c"near-W");
        assert_eq!(NAMES[Stage::ProducerColour as usize], c"producer-colour");
    }

    #[test]
    fn admitted_cameras_compose_forward_domains() {
        let snapshot = fixture();
        let mut stage = Stage::Source;
        let mut detail = NO_FAILURE;
        let maps = math(&snapshot, &mut stage, &mut detail).expect("fixture admits");
        assert_eq!(stage, Stage::WorldW);
        let mut values = [0.0; 16];
        for map in [&maps.near, &maps.world] {
            assert!(camera::store(map, &mut values));
            for (index, value) in values.iter().enumerate() {
                let expected = if index % 5 == 0 { 1.0 } else { 0.0 };
                assert!((value - expected).abs() < 1e-5, "{index}: {value}");
            }
        }
        assert!(detail.minimum_w > 0.99 && detail.range == 0.75);
    }

    #[test]
    fn refusals_name_their_stage() {
        let mut stage = Stage::Source;
        let mut detail = NO_FAILURE;

        let mut snapshot = fixture();
        snapshot.world_camera.port[1] = 0.0;
        assert!(math(&snapshot, &mut stage, &mut detail).is_none());
        assert_eq!(stage, Stage::CameraMath);

        let mut snapshot = fixture();
        snapshot.world_unjittered_camera.origin[0] = 10.5;
        assert!(math(&snapshot, &mut stage, &mut detail).is_none());
        assert_eq!(stage, Stage::WorldCache);

        let mut snapshot = fixture();
        snapshot.near_inverse_transposed[0] += 0.25;
        assert!(math(&snapshot, &mut stage, &mut detail).is_none());
        assert_eq!(stage, Stage::NearInverse);

        // A world range of zero has no forward bound.
        let mut snapshot = fixture();
        snapshot.intervals = [0.0, 0.25, 0.25, 0.25];
        assert!(math(&snapshot, &mut stage, &mut detail).is_none());
        assert_eq!(stage, Stage::WorldW);
    }

    #[test]
    fn comparisons_are_bytewise_like_memcmp() {
        let a = fixture();
        let mut b = a;
        assert!(same_raster(&a, &b));
        b.actual_raster[0] = -0.0;
        assert!(!same_raster(&a, &b));
        let mut c = a;
        c.world_camera.origin[2] = 2.5;
        assert!(same_camera(&a.world_camera, &c.world_camera));
        // Identity needs a nonzero session, source and submission.
        assert!(!same(&a, &a));
        let mut d = a;
        d.identity = Identity {
            session: 1,
            source: 2,
            submission: 3,
            began_qpc: 4,
            frequency: 5,
        };
        assert!(same(&d, &d) && same_raster_owner(&d, &d));
    }
}
