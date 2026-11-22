// SPDX-License-Identifier: MIT
use crate::com_ref::{self, vtable};
use crate::d3d11_output::{GraphicsCache, texture_desc};
use crate::d3d11_output_platform::{
    rsf_d3d11_output_child_device, rsf_d3d11_output_copy_resource, rsf_d3d11_output_create_srv,
    rsf_d3d11_output_create_texture, rsf_d3d11_output_is_immediate, rsf_d3d11_output_srv_format,
    rsf_d3d11_output_unbind_targets,
};
use crate::dxgi_format::{
    D24_UNORM_S8_UINT, D32_FLOAT, R24_UNORM_X8_TYPELESS, R24G8_TYPELESS, R32_TYPELESS,
};
use crate::graphics_d3d11::{compute_shader, shader_code};
use crate::sys::*;
use rsf_game_sys::{
    RSF_DEPTH_DOMAIN_INCLUDE_MAX, RSF_DEPTH_DOMAIN_INCLUDE_MIN, RSF_DEPTH_DOMAINS_VERSION,
    depth_domains_requested, rsf_game_depth_domain, rsf_game_depth_domains,
};
use std::{
    ffi::{CStr, c_void},
    ptr,
};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCOMPILE_ENABLE_STRICTNESS;
use windows::Win32::Graphics::Direct3D11::*;
use windows::core::{IUnknown, Interface};

type Object = *mut c_void;
const NONE: Object = ptr::null_mut();

// DXGI_FORMAT and D3D11_BIND_FLAG values used by the region kernels.
const RGBA16_UNORM: u32 = 11;
const RGBA16_SNORM: u32 = 13;
const RGB10A2_UNORM: u32 = 24;
const RGBA8_UNORM: u32 = 28;
const RGBA8_SNORM: u32 = 31;
const RG16_UNORM: u32 = 35;
const RG16_SNORM: u32 = 37;
const RG8_UNORM: u32 = 49;
const RG8_SNORM: u32 = 51;
const R16_TYPELESS: u32 = 53;
const R16_FLOAT: u32 = 54;
const D16_UNORM: u32 = 55;
const R16_UNORM: u32 = 56;
const R16_SNORM: u32 = 58;
const R8_UNORM: u32 = 61;
const R8_SNORM: u32 = 63;
const BGRA8_UNORM: u32 = 87;
const BIND_UNORDERED_ACCESS: u32 = 128;

#[derive(Clone, Copy)]
pub(super) struct Region {
    pub(super) bounds: [u32; 4],
    pub(super) extent: [u32; 2],
}

impl Region {
    pub(super) fn new([left, top, right, bottom]: [i32; 4]) -> Option<Self> {
        if left < 0 || top < 0 || right <= left || bottom <= top {
            return None;
        }
        Some(Self {
            bounds: [left as u32, top as u32, right as u32, bottom as u32],
            extent: [(right - left) as u32, (bottom - top) as u32],
        })
    }

    fn origin(self) -> [u32; 2] {
        [self.bounds[0], self.bounds[1]]
    }

    pub(super) fn fits(self, desc: &rsf_d3d11_texture_desc) -> bool {
        desc.samples == 1
            && desc.layers == 1
            && self.bounds[2] <= desc.width
            && self.bounds[3] <= desc.height
    }

    pub(super) fn groups(self) -> [u32; 2] {
        self.extent.map(|size| size.div_ceil(8))
    }
}

#[repr(C)]
#[derive(Default)]
struct DomainConstants {
    range: [f32; 4],
    viewport: [f32; 4],
    transform: [f32; 16],
    flags: [u32; 4],
}

#[repr(C)]
#[derive(Default)]
struct RegionConstants {
    origin: [u32; 2],
    size: [u32; 2],
    count: u32,
    clear_enabled: u32,
    clear_value: f32,
    far_clear: f32,
    domains: [DomainConstants; 2],
}

#[repr(C)]
struct MaskConstants {
    origin: [u32; 2],
    size: [u32; 2],
    weights: [f32; 4],
}

const _: () = {
    assert!(size_of::<DomainConstants>() == 112);
    assert!(std::mem::offset_of!(DomainConstants, flags) == 96);
    assert!(size_of::<RegionConstants>() == 256);
    assert!(std::mem::offset_of!(RegionConstants, domains) == 32);
    assert!(size_of::<MaskConstants>() == 32);
    assert!(std::mem::offset_of!(MaskConstants, weights) == 16);
};

fn all_zero(values: &[f32]) -> bool {
    values.iter().all(|value| value.to_bits() == 0)
}

fn empty_domain(d: &rsf_game_depth_domain) -> bool {
    d.endpoint_flags == 0
        && d.reserved == 0
        && all_zero(&[
            d.classify_min,
            d.classify_max,
            d.viewport_min_depth,
            d.viewport_max_depth,
        ])
        && all_zero(&d.viewport_xywh)
        && all_zero(&d.source_clip_to_target_clip)
}

fn nonsingular(transform: &[f32; 16]) -> bool {
    let mut matrix: [[f64; 4]; 4] =
        std::array::from_fn(|row| std::array::from_fn(|col| f64::from(transform[row * 4 + col])));
    for col in 0..4 {
        let mut pivot = col;
        for row in col + 1..4 {
            if matrix[row][col].abs() > matrix[pivot][col].abs() {
                pivot = row;
            }
        }
        let scale = matrix[pivot][col..]
            .iter()
            .fold(0.0_f64, |scale, value| scale.max(value.abs()));
        if !scale.is_finite() || scale == 0.0 || matrix[pivot][col].abs() <= scale * 1e-12 {
            return false;
        }
        matrix.swap(pivot, col);
        let pivot_row = matrix[col];
        for row in &mut matrix[col + 1..] {
            let factor = row[col] / pivot_row[col];
            for j in col + 1..4 {
                row[j] -= factor * pivot_row[j];
            }
        }
    }
    true
}

fn bounded(values: &[f32], limit: f32) -> bool {
    values
        .iter()
        .all(|value| value.is_finite() && value.abs() <= limit)
}

fn unorm_interval(min: f32, max: f32) -> bool {
    min.is_finite() && max.is_finite() && min >= 0.0 && max <= 1.0 && min < max
}

fn valid_projection(c: &DomainConstants, d: &rsf_game_depth_domain, region: Region) -> bool {
    // Bound each affine homogeneous component at the box corners. W must
    // remain positive after accounting for float32 unmapping and dot products.
    let epsilon = f64::from(f32::EPSILON);
    let mut bounds = [[0.0_f64; 2]; 3];
    for (axis, axis_bounds) in bounds[..2].iter_mut().enumerate() {
        let origin = f64::from(d.viewport_xywh[axis]);
        let inverse = f64::from(c.viewport[axis + 2]);
        let start = (f64::from(region.bounds[axis]) + 0.5 - origin) * inverse * 2.0 - 1.0;
        let end = (f64::from(region.bounds[axis + 2]) - 0.5 - origin) * inverse * 2.0 - 1.0;
        let (start, end) = if axis == 1 {
            (-start, -end)
        } else {
            (start, end)
        };
        let error = 16.0
            * epsilon
            * ((f64::from(region.bounds[axis + 2]) + 1.0 + origin.abs()) * inverse * 2.0 + 1.0);
        *axis_bounds = [start.min(end) - error, start.max(end) + error];
    }
    let near = f64::from(c.range[2]);
    let inverse = f64::from(c.range[3]);
    let low_z = (f64::from(d.classify_min) - near) * inverse;
    let high_z = (f64::from(d.classify_max) - near) * inverse;
    let error = 16.0 * epsilon * (f64::from(d.classify_max).abs() + near.abs()) * inverse;
    bounds[2] = [low_z.min(high_z) - error, low_z.max(high_z) + error];

    let mut magnitude = [0.0_f64; 4];
    for (col, component) in magnitude.iter_mut().enumerate() {
        *component = f64::from(c.transform[12 + col]).abs();
        for (row, [lower, upper]) in bounds.into_iter().enumerate() {
            *component +=
                lower.abs().max(upper.abs()) * f64::from(c.transform[row * 4 + col]).abs();
        }
        if !component.is_finite() || *component > 1e20 {
            return false;
        }
    }
    let mut minimum_w = f64::INFINITY;
    for corner in 0..8 {
        let mut w = f64::from(c.transform[15]);
        for (axis, axis_bounds) in bounds.iter().enumerate() {
            w += axis_bounds[(corner >> axis) & 1] * f64::from(c.transform[axis * 4 + 3]);
        }
        minimum_w = minimum_w.min(w);
    }
    let conservative_w = minimum_w - 64.0 * epsilon * magnitude[3];
    conservative_w > 1e-6 && magnitude[2] / conservative_w <= 1e30
}

fn depth_domain(
    d: &rsf_game_depth_domain,
    region: Region,
    quantum: f64,
) -> Option<DomainConstants> {
    if !unorm_interval(d.classify_min, d.classify_max)
        || d.reserved != 0
        || d.endpoint_flags & !(RSF_DEPTH_DOMAIN_INCLUDE_MIN | RSF_DEPTH_DOMAIN_INCLUDE_MAX) != 0
        || !unorm_interval(d.viewport_min_depth, d.viewport_max_depth)
        || f64::from(d.classify_min) < f64::from(d.viewport_min_depth) - quantum
        || f64::from(d.classify_max) > f64::from(d.viewport_max_depth) + quantum
        || !bounded(&d.viewport_xywh, 1e8)
    {
        return None;
    }

    // Native viewports can be fractional. They must cover the sampled texel
    // centers, with exclusive right and bottom bounds.
    let [x, y, width, height] = d.viewport_xywh;
    let [left, top, right, bottom] = region.bounds.map(f64::from);
    if f64::from(x) > left + 0.5
        || f64::from(y) > top + 0.5
        || width <= 0.0
        || height <= 0.0
        || f64::from(x) + f64::from(width) <= right - 0.5
        || f64::from(y) + f64::from(height) <= bottom - 0.5
    {
        return None;
    }

    let constants = DomainConstants {
        range: [
            d.classify_min,
            d.classify_max,
            d.viewport_min_depth,
            1.0 / (d.viewport_max_depth - d.viewport_min_depth),
        ],
        viewport: [x, y, 1.0 / width, 1.0 / height],
        transform: d.source_clip_to_target_clip,
        flags: [d.endpoint_flags, 0, 0, 0],
    };
    (bounded(&constants.range, 1e20)
        && bounded(&constants.viewport, 1e20)
        && bounded(&constants.transform, 1e20)
        && nonsingular(&constants.transform)
        && valid_projection(&constants, d, region))
    .then_some(constants)
}

fn depth_constants(
    facts: Option<rsf_game_depth_domains>,
    format: u32,
    region: Region,
    inverted: bool,
) -> Option<RegionConstants> {
    let mut constants = RegionConstants {
        origin: region.origin(),
        size: region.extent,
        ..Default::default()
    };
    let Some(facts) = facts.filter(depth_domains_requested) else {
        return Some(constants);
    };
    if facts.struct_size as usize != size_of::<rsf_game_depth_domains>()
        || facts.version != RSF_DEPTH_DOMAINS_VERSION
        || !(1..=2).contains(&facts.domain_count)
        || facts.clear_enabled > 1
        || facts.reserved != [0; 3]
        || !(0.0..=1.0).contains(&facts.clear_value)
        || (facts.clear_enabled == 0 && facts.clear_value != 0.0)
    {
        return None;
    }
    // Only these decoded UNORM formats have the required finite sample bounds.
    let quantum = match rsf_d3d11_output_srv_format(format) {
        R16_UNORM => 1.0 / 65535.0,
        R24_UNORM_X8_TYPELESS => 1.0 / 16777215.0,
        _ => return None,
    };

    let (active, unused) = facts.domains.split_at(facts.domain_count as usize);
    if !unused.iter().all(empty_domain) {
        return None;
    }
    let clear = |value| facts.clear_enabled != 0 && facts.clear_value == value;
    for (i, d) in active.iter().enumerate() {
        let includes_min = d.endpoint_flags & RSF_DEPTH_DOMAIN_INCLUDE_MIN != 0;
        if i == 0 {
            if d.classify_min != 0.0 || (!includes_min && !clear(0.0)) {
                return None;
            }
        } else {
            let previous = &active[i - 1];
            let includes_boundary = previous.endpoint_flags & RSF_DEPTH_DOMAIN_INCLUDE_MAX != 0;
            if previous.classify_max != d.classify_min
                || (includes_boundary && includes_min)
                || (!includes_boundary && !includes_min && !clear(d.classify_min))
            {
                return None;
            }
        }
        if i + 1 == active.len()
            && (d.classify_max != 1.0
                || (d.endpoint_flags & RSF_DEPTH_DOMAIN_INCLUDE_MAX == 0 && !clear(1.0)))
        {
            return None;
        }
        constants.domains[i] = depth_domain(d, region, quantum)?;
    }

    constants.count = facts.domain_count;
    constants.clear_enabled = facts.clear_enabled;
    constants.clear_value = facts.clear_value;
    constants.far_clear = if inverted { 0.0 } else { 1.0 };
    Some(constants)
}

fn mask_view_format(format: u32) -> Option<u32> {
    let view = rsf_d3d11_output_srv_format(format);
    // Numeric views preserve the channel semantics of the weighted sum.
    matches!(
        view,
        RSF_D3D11_FORMAT_R32_FLOAT
            | RSF_D3D11_FORMAT_RG32_FLOAT
            | RSF_D3D11_FORMAT_RGBA32_FLOAT
            | R16_FLOAT
            | RSF_D3D11_FORMAT_RG16_FLOAT
            | RSF_D3D11_FORMAT_RGBA16_FLOAT
            | R16_UNORM
            | RG16_UNORM
            | RGBA16_UNORM
            | R8_UNORM
            | RG8_UNORM
            | RGBA8_UNORM
            | R16_SNORM
            | RG16_SNORM
            | RGBA16_SNORM
            | R8_SNORM
            | RG8_SNORM
            | RGBA8_SNORM
            | RGB10A2_UNORM
            | BGRA8_UNORM
    )
    .then_some(view)
}

enum Kernel {
    Depth,
    Color,
    Mask,
}

pub(super) unsafe fn compile_compute(
    device: Object,
    source: &[u8],
    name: &CStr,
    flags: u32,
) -> Option<IUnknown> {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device) }?;
    let code = shader_code(source, name, c"main", c"cs_5_0", flags).ok()?;
    let shader = compute_shader(device, &code)?;
    unsafe { com_ref::owned(shader.into_raw()) }
}

impl Kernel {
    unsafe fn compile(self, device: Object) -> Option<IUnknown> {
        let (source, name): (&[u8], &CStr) = match self {
            Self::Depth => (
                include_bytes!("composition_shaders/depth_region.hlsl"),
                c"NativeDepthRegion",
            ),
            Self::Color => (
                include_bytes!("composition_shaders/color_region.hlsl"),
                c"NativeColorRegion",
            ),
            Self::Mask => (
                include_bytes!("composition_shaders/temporal_mask_region.hlsl"),
                c"NativeTemporalMaskRegion",
            ),
        };
        unsafe { compile_compute(device, source, name, D3DCOMPILE_ENABLE_STRICTNESS) }
    }
}

struct Dispatch {
    shader: Object,
    constants: Object,
    views: [Object; 3],
    targets: [Object; 2],
    view_count: u32,
    target_count: u32,
    groups: [u32; 2],
}

pub(super) unsafe fn device_identity(device: Object) -> Option<IUnknown> {
    let mut identity = NONE;
    let status = unsafe {
        (vtable::<IUnknown>(&device).QueryInterface)(device, &IUnknown::IID, &mut identity)
    };
    unsafe { com_ref::from_result(status, identity) }
}

pub(super) unsafe fn create_constants(device: Object, size: usize) -> Option<IUnknown> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: size as u32,
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        ..Default::default()
    };
    let mut buffer = NONE;
    let status = unsafe {
        (vtable::<ID3D11Device>(&device).CreateBuffer)(device, &desc, ptr::null(), &mut buffer)
    };
    unsafe { com_ref::from_result(status, buffer) }
}

unsafe fn create_uav(device: Object, texture: Object) -> Option<IUnknown> {
    let mut view = NONE;
    let status = unsafe {
        (vtable::<ID3D11Device>(&device).CreateUnorderedAccessView)(
            device,
            texture,
            ptr::null(),
            &mut view,
        )
    };
    unsafe { com_ref::from_result(status, view) }
}

pub(super) unsafe fn crop(
    context: Object,
    target: Object,
    source: Object,
    &[left, top, right, bottom]: &[u32; 4],
) {
    let bounds = D3D11_BOX {
        left,
        top,
        right,
        bottom,
        front: 0,
        back: 1,
    };
    unsafe {
        (vtable::<ID3D11DeviceContext>(&context).CopySubresourceRegion)(
            context, target, 0, 0, 0, 0, source, 0, &bounds,
        )
    };
}

pub(super) unsafe fn update_constants<T>(context: Object, buffer: Object, data: &T) {
    unsafe {
        (vtable::<ID3D11DeviceContext>(&context).UpdateSubresource)(
            context,
            buffer,
            0,
            ptr::null(),
            ptr::from_ref(data).cast(),
            0,
            0,
        )
    };
}

unsafe fn dispatch(context: Object, pass: &Dispatch) {
    let api = unsafe { vtable::<ID3D11DeviceContext>(&context) };
    unsafe {
        (api.CSSetShader)(context, pass.shader, ptr::null(), 0);
        (api.CSSetShaderResources)(context, 0, pass.view_count, pass.views.as_ptr());
        (api.CSSetUnorderedAccessViews)(
            context,
            0,
            pass.target_count,
            pass.targets.as_ptr(),
            ptr::null(),
        );
        if !pass.constants.is_null() {
            (api.CSSetConstantBuffers)(context, 0, 1, &pass.constants);
        }
        (api.Dispatch)(context, pass.groups[0], pass.groups[1], 1);
        (api.CSSetShaderResources)(context, 0, pass.view_count, [NONE; 3].as_ptr());
        (api.CSSetUnorderedAccessViews)(
            context,
            0,
            pass.target_count,
            [NONE; 2].as_ptr(),
            ptr::null(),
        );
    }
}

pub(super) struct SourceView {
    view: IUnknown,
    texture: IUnknown,
    format: u32,
}

impl SourceView {
    pub(super) fn matching(&self, texture: Object, format: u32) -> Option<Object> {
        (self.texture.as_raw() == texture && self.format == format).then(|| self.view.as_raw())
    }

    pub(super) unsafe fn get(
        cache: &mut Option<Self>,
        device: Object,
        texture: Object,
        format: u32,
    ) -> Option<Object> {
        if let Some(view) = cache
            .as_ref()
            .and_then(|entry| entry.matching(texture, format))
        {
            return Some(view);
        }
        let view = unsafe { com_ref::owned(rsf_d3d11_output_create_srv(device, texture, format)) }?;
        // Keep the allocation alive so its identity cannot be reused while cached.
        let texture = unsafe { com_ref::retain(texture) };
        Some(
            cache
                .insert(Self {
                    view,
                    texture,
                    format,
                })
                .view
                .as_raw(),
        )
    }
}

fn readable_desc(mut source: rsf_d3d11_texture_desc, format: u32) -> rsf_d3d11_texture_desc {
    source.usage = RSF_D3D11_USAGE_DEFAULT;
    source.cpu_access_flags = 0;
    source.misc_flags = 0;
    source.bind_flags = RSF_D3D11_BIND_SHADER_RESOURCE;
    source.format = format;
    source
}

unsafe fn readable_copy(
    device: Object,
    cached: &mut Option<IUnknown>,
    view: &mut Option<SourceView>,
    desc: &rsf_d3d11_texture_desc,
) -> Option<Object> {
    if let Some(texture) = cached.as_ref() {
        let previous = unsafe { texture_desc(texture.as_raw()) };
        if (
            previous.width,
            previous.height,
            previous.format,
            previous.mips,
        ) == (desc.width, desc.height, desc.format, desc.mips)
        {
            return Some(texture.as_raw());
        }
    }
    *view = None;
    *cached = None;
    let texture = unsafe { com_ref::owned(rsf_d3d11_output_create_texture(device, desc)) }?;
    Some(cached.insert(texture).as_raw())
}

unsafe fn output_texture(
    device: Object,
    extent: [u32; 2],
    format: u32,
    unordered: bool,
) -> Option<IUnknown> {
    let desc = rsf_d3d11_texture_desc {
        width: extent[0],
        height: extent[1],
        mips: 1,
        layers: 1,
        format,
        samples: 1,
        usage: RSF_D3D11_USAGE_DEFAULT,
        bind_flags: RSF_D3D11_BIND_SHADER_RESOURCE
            | if unordered { BIND_UNORDERED_ACCESS } else { 0 },
        ..Default::default()
    };
    unsafe { com_ref::owned(rsf_d3d11_output_create_texture(device, &desc)) }
}

struct RegionShaders {
    depth: IUnknown,
    color: IUnknown,
    constants: IUnknown,
}

impl RegionShaders {
    unsafe fn new(device: Object) -> Option<Self> {
        Some(Self {
            depth: unsafe { Kernel::Depth.compile(device) }?,
            color: unsafe { Kernel::Color.compile(device) }?,
            constants: unsafe { create_constants(device, size_of::<RegionConstants>()) }?,
        })
    }
}

#[derive(PartialEq, Eq)]
struct RegionKey {
    extent: [u32; 2],
    color_format: u32,
    motion_format: u32,
}

struct RegionOutput {
    color: IUnknown,
    depth: IUnknown,
    motion: IUnknown,
    depth_target: IUnknown,
    color_target: IUnknown,
    key: RegionKey,
}

impl RegionOutput {
    unsafe fn new(device: Object, key: RegionKey) -> Option<Self> {
        let color =
            unsafe { output_texture(device, key.extent, RSF_D3D11_FORMAT_RGBA16_FLOAT, true) }?;
        let depth =
            unsafe { output_texture(device, key.extent, RSF_D3D11_FORMAT_R32_FLOAT, true) }?;
        let motion = unsafe { output_texture(device, key.extent, key.motion_format, false) }?;
        let depth_target = unsafe { create_uav(device, depth.as_raw()) }?;
        let color_target = unsafe { create_uav(device, color.as_raw()) }?;
        Some(Self {
            color,
            depth,
            motion,
            depth_target,
            color_target,
            key,
        })
    }
}

struct MaskShader {
    shader: IUnknown,
    constants: IUnknown,
}

impl MaskShader {
    unsafe fn new(device: Object) -> Option<Self> {
        Some(Self {
            shader: unsafe { Kernel::Mask.compile(device) }?,
            constants: unsafe { create_constants(device, size_of::<MaskConstants>()) }?,
        })
    }
}

pub(super) struct MaskTarget {
    pub(super) texture: IUnknown,
    pub(super) view: IUnknown,
}

impl MaskTarget {
    pub(super) unsafe fn new(device: Object, extent: [u32; 2]) -> Option<Self> {
        let texture = unsafe { output_texture(device, extent, RSF_D3D11_FORMAT_R32_FLOAT, true) }?;
        let view = unsafe { create_uav(device, texture.as_raw()) }?;
        Some(Self { texture, view })
    }
}

struct MaskOutput {
    extent: [u32; 2],
    targets: [MaskTarget; 2],
}

#[derive(Default)]
struct TemporalMask {
    shader: Option<MaskShader>,
    output: Option<MaskOutput>,
    readable: Option<IUnknown>,
    source: Option<SourceView>,
    hints: [Option<SourceView>; 2],
}

struct Resources {
    shaders: Option<RegionShaders>,
    output: Option<RegionOutput>,
    readable_depth: Option<IUnknown>,
    depth_source: Option<SourceView>,
    color_source: Option<SourceView>,
    temporal: TemporalMask,
    device: IUnknown,
}

// SAFETY: only D3D11 resources are retained. The caller owns the execution
// stream and keeps borrowed outputs live by serializing use and release.
unsafe impl Send for Resources {}

static RESOURCES: GraphicsCache<Option<Resources>> = GraphicsCache::new(None);

fn for_device(cache: &mut Option<Resources>, device: IUnknown) -> &mut Resources {
    if cache
        .as_ref()
        .is_some_and(|entry| entry.device.as_raw() != device.as_raw())
    {
        *cache = None;
    }
    cache.get_or_insert_with(|| Resources {
        shaders: None,
        output: None,
        readable_depth: None,
        depth_source: None,
        color_source: None,
        temporal: TemporalMask::default(),
        device,
    })
}

unsafe fn identity(device: Object) -> Option<IUnknown> {
    unsafe { device_identity(device) }
}

unsafe fn child_device(child: Object) -> Option<IUnknown> {
    unsafe { com_ref::owned(rsf_d3d11_output_child_device(child)) }
}

unsafe fn prepare(
    context: Object,
    inputs: [Object; 3],
    region: Region,
    facts: Option<rsf_game_depth_domains>,
    inverted: bool,
) -> Option<[Object; 3]> {
    let [color, depth, motion] = inputs;
    let device = unsafe { child_device(context) }?;
    if unsafe { rsf_d3d11_output_is_immediate(context) } == 0 {
        return None;
    }
    let device_identity = unsafe { identity(device.as_raw()) }?;
    for input in inputs {
        let owner = unsafe { child_device(input) }?;
        if unsafe { identity(owner.as_raw()) }?.as_raw() != device_identity.as_raw() {
            return None;
        }
    }
    let [color_desc, depth_desc, motion_desc] = inputs.map(|input| unsafe { texture_desc(input) });
    if [&color_desc, &depth_desc, &motion_desc]
        .into_iter()
        .any(|desc| desc.mips != 1 || !region.fits(desc))
    {
        return None;
    }
    let constants = depth_constants(facts, depth_desc.format, region, inverted)?;

    RESOURCES.with(|cache| {
        let r = for_device(cache, device);
        let device = r.device.as_raw();
        let shaders = match &r.shaders {
            Some(shaders) => shaders,
            None => r.shaders.insert(unsafe { RegionShaders::new(device) }?),
        };
        let key = RegionKey {
            extent: region.extent,
            color_format: color_desc.format,
            motion_format: motion_desc.format,
        };
        let output = match &r.output {
            Some(output) if output.key == key => output,
            _ => r.output.insert(unsafe { RegionOutput::new(device, key) }?),
        };

        let copy_depth = depth_desc.bind_flags & RSF_D3D11_BIND_SHADER_RESOURCE == 0;
        let readable = if copy_depth {
            let format = match depth_desc.format {
                D16_UNORM => R16_TYPELESS,
                D24_UNORM_S8_UINT => R24G8_TYPELESS,
                D32_FLOAT => R32_TYPELESS,
                format => format,
            };
            let desc = readable_desc(depth_desc, format);
            unsafe { readable_copy(device, &mut r.readable_depth, &mut r.depth_source, &desc) }?
        } else {
            depth
        };
        let depth_view = unsafe {
            SourceView::get(
                &mut r.depth_source,
                device,
                readable,
                rsf_d3d11_output_srv_format(depth_desc.format),
            )
        };
        let color_view = unsafe {
            SourceView::get(
                &mut r.color_source,
                device,
                color,
                rsf_d3d11_output_srv_format(color_desc.format),
            )
        };

        let groups = region.groups();
        let depth_pass = Dispatch {
            shader: shaders.depth.as_raw(),
            constants: shaders.constants.as_raw(),
            views: [depth_view?, NONE, NONE],
            targets: [output.depth_target.as_raw(), NONE],
            view_count: 1,
            target_count: 1,
            groups,
        };
        let color_pass = Dispatch {
            shader: shaders.color.as_raw(),
            constants: NONE, // Depth bound the shared buffer to b0.
            views: [color_view?, NONE, NONE],
            targets: [output.color_target.as_raw(), NONE],
            ..depth_pass
        };
        unsafe {
            rsf_d3d11_output_unbind_targets(context);
            if copy_depth {
                rsf_d3d11_output_copy_resource(context, readable, depth);
            }
            crop(context, output.motion.as_raw(), motion, &region.bounds);
            update_constants(context, shaders.constants.as_raw(), &constants);
            dispatch(context, &depth_pass);
            dispatch(context, &color_pass);
        }
        Some([
            output.color.as_raw(),
            output.depth.as_raw(),
            output.motion.as_raw(),
        ])
    })?
}

unsafe fn temporal_mask(
    context: Object,
    [mask, reactive, bias]: [Object; 3],
    region: Region,
    weights: [f32; 4],
) -> Option<[Object; 2]> {
    if !weights.iter().all(|weight| weight.is_finite()) {
        return None;
    }
    let device = unsafe { child_device(context) }?;
    for input in [mask, reactive, bias]
        .into_iter()
        .filter(|input| !input.is_null())
    {
        // The temporal-mask path historically requires the same device interface.
        if unsafe { child_device(input) }?.as_raw() != device.as_raw() {
            return None;
        }
    }
    let source = unsafe { texture_desc(mask) };
    if !region.fits(&source) {
        return None;
    }
    let format = mask_view_format(source.format)?;
    for input in [reactive, bias]
        .into_iter()
        .filter(|input| !input.is_null())
    {
        let desc = unsafe { texture_desc(input) };
        if [desc.width, desc.height] != region.extent
            || desc.samples != 1
            || desc.layers != 1
            || desc.format != RSF_D3D11_FORMAT_R32_FLOAT
        {
            return None;
        }
    }

    RESOURCES.with(|cache| {
        let r = for_device(cache, device);
        let device = r.device.as_raw();
        let temporal = &mut r.temporal;
        let shader = match &temporal.shader {
            Some(shader) => shader,
            None => temporal.shader.insert(unsafe { MaskShader::new(device) }?),
        };
        let output = match &temporal.output {
            Some(output) if output.extent == region.extent => output,
            _ => temporal.output.insert(MaskOutput {
                extent: region.extent,
                targets: [unsafe { MaskTarget::new(device, region.extent) }?, unsafe {
                    MaskTarget::new(device, region.extent)
                }?],
            }),
        };

        // Release output bindings before copying output-only masks to an SRV allocation.
        unsafe { rsf_d3d11_output_unbind_targets(context) };
        let readable = if source.bind_flags & RSF_D3D11_BIND_SHADER_RESOURCE == 0 {
            let desc = readable_desc(source, source.format);
            let copy = unsafe {
                readable_copy(device, &mut temporal.readable, &mut temporal.source, &desc)
            }?;
            unsafe { rsf_d3d11_output_copy_resource(context, copy, mask) };
            copy
        } else {
            mask
        };

        let mut views = [NONE; 3];
        views[0] = unsafe { SourceView::get(&mut temporal.source, device, readable, format) }?;
        for (i, input) in [reactive, bias].into_iter().enumerate() {
            let cached = &mut temporal.hints[i];
            if input.is_null() {
                *cached = None;
            } else {
                views[i + 1] =
                    unsafe { SourceView::get(cached, device, input, RSF_D3D11_FORMAT_R32_FLOAT) }?;
            }
        }
        let constants = MaskConstants {
            origin: region.origin(),
            size: region.extent,
            weights,
        };
        let pass = Dispatch {
            shader: shader.shader.as_raw(),
            constants: shader.constants.as_raw(),
            views,
            targets: output.targets.each_ref().map(|target| target.view.as_raw()),
            view_count: 3,
            target_count: 2,
            groups: region.groups(),
        };
        unsafe {
            update_constants(context, shader.constants.as_raw(), &constants);
            dispatch(context, &pass);
        }
        Some(
            output
                .targets
                .each_ref()
                .map(|target| target.texture.as_raw()),
        )
    })?
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_native_regions_depth_valid(
    facts: *const rsf_game_depth_domains,
    format: u32,
    rect: *const [i32; 4],
) -> bool {
    let Some(region) = unsafe { rect.as_ref() }.copied().and_then(Region::new) else {
        return false;
    };
    depth_constants(unsafe { facts.as_ref() }.copied(), format, region, false).is_some()
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_native_regions_prepare(
    context: Object,
    color: Object,
    depth: Object,
    motion: Object,
    rect: *const [i32; 4],
    out_color: *mut Object,
    out_depth: *mut Object,
    out_motion: *mut Object,
    facts: *const rsf_game_depth_domains,
    inverted: bool,
) -> bool {
    let inputs = [color, depth, motion];
    if context.is_null()
        || inputs.iter().any(|input| input.is_null())
        || out_color.is_null()
        || out_depth.is_null()
        || out_motion.is_null()
    {
        return false;
    }
    let Some(region) = unsafe { rect.as_ref() }.copied().and_then(Region::new) else {
        return false;
    };
    let Some([color, depth, motion]) =
        (unsafe { prepare(context, inputs, region, facts.as_ref().copied(), inverted) })
    else {
        return false;
    };
    unsafe {
        out_color.write(color);
        out_depth.write(depth);
        out_motion.write(motion);
    }
    true
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_native_regions_temporal_mask(
    context: Object,
    mask: Object,
    rect: *const [i32; 4],
    weights: *const [f32; 4],
    reactive: Object,
    bias: Object,
    out_reactive: *mut Object,
    out_bias: *mut Object,
) -> bool {
    if context.is_null() || mask.is_null() || out_reactive.is_null() || out_bias.is_null() {
        return false;
    }
    let Some(region) = unsafe { rect.as_ref() }.copied().and_then(Region::new) else {
        return false;
    };
    let Some(weights) = unsafe { weights.as_ref() }.copied() else {
        return false;
    };
    let Some([reactive, bias]) =
        (unsafe { temporal_mask(context, [mask, reactive, bias], region, weights) })
    else {
        return false;
    };
    unsafe {
        out_reactive.write(reactive);
        out_bias.write(bias);
    }
    true
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_native_regions_release() {
    RESOURCES.with(|cache| *cache = None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_layout_distinguishes_negative_zero_from_absent_bytes() {
        let rect = [2, 3, 10, 13];
        let region = Region::new(rect).unwrap();
        // Every field in the generated C layout is numeric and permits zero bits.
        let mut facts: rsf_game_depth_domains = unsafe { std::mem::zeroed() };
        let valid = |facts, format| unsafe { rsf_native_regions_depth_valid(facts, format, &rect) };
        assert!(valid(&facts, RSF_D3D11_FORMAT_R32_FLOAT));
        facts.clear_value = -0.0;
        assert!(!valid(&facts, D16_UNORM));

        facts.struct_size = size_of::<rsf_game_depth_domains>() as u32;
        facts.version = RSF_DEPTH_DOMAINS_VERSION;
        facts.domain_count = 1;
        facts.domains[0] = rsf_game_depth_domain {
            classify_min: -0.0,
            classify_max: 1.0,
            endpoint_flags: RSF_DEPTH_DOMAIN_INCLUDE_MIN | RSF_DEPTH_DOMAIN_INCLUDE_MAX,
            reserved: 0,
            viewport_xywh: [2.5, 3.5, 8.0, 10.0],
            viewport_min_depth: 0.0,
            viewport_max_depth: 1.0,
            source_clip_to_target_clip: [
                1.0, -0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
        };
        assert!(valid(&facts, D16_UNORM));
        assert!(valid(&facts, D24_UNORM_S8_UINT));
        assert!(!valid(&facts, D32_FLOAT));
        let constants = depth_constants(Some(facts), D16_UNORM, region, true).unwrap();
        assert_eq!(constants.clear_value.to_bits(), (-0.0_f32).to_bits());
        assert_eq!(
            constants.domains[0].range[0].to_bits(),
            (-0.0_f32).to_bits()
        );
        assert_eq!(
            constants.domains[0].transform[1].to_bits(),
            (-0.0_f32).to_bits()
        );
        assert_eq!(constants.far_clear, 0.0);
        facts.domains[1].viewport_xywh[0] = -0.0;
        assert!(!valid(&facts, D16_UNORM));
    }
}
