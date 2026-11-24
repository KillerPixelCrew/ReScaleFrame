// SPDX-License-Identifier: MIT
//! Exercises the actual SR passes and resource creators on hardware or WARP.
use super::targets::{Device as Targets, Error, Target, TargetView, TwinInput};
use super::{DrawViews, Extent, Pass, ROUND_TRIP, UPSCALE};
use crate::sr_fixture_device::Device;
use crate::sr_fixture_readback::rsf_anomaly_sr_fixture_readback;
use crate::sr_tonemap::roundtrip_fixture::{
    UpscaleReadback, rsf_anomaly_sr_float_to_half as half, rsf_anomaly_sr_roundtrip_numerics,
    rsf_anomaly_sr_upscale_numerics,
};
use crate::sr_tonemap::rsf_anomaly_tonemap_forward as tonemap;
use rsf_shader_compiler::{Request, compile_system_checked};
use std::ffi::{CStr, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::null_mut;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::Interface;

const RTV: u32 = D3D11_BIND_RENDER_TARGET.0 as u32;
const SRV: u32 = D3D11_BIND_SHADER_RESOURCE.0 as u32;
const DSV: u32 = D3D11_BIND_DEPTH_STENCIL.0 as u32;
const UAV: u32 = D3D11_BIND_UNORDERED_ACCESS.0 as u32;
const COLOUR: u32 = RTV | SRV;
const UNORM: DXGI_FORMAT = DXGI_FORMAT_R8G8B8A8_UNORM;
const HDR: DXGI_FORMAT = DXGI_FORMAT_R16G16B16A16_FLOAT;
const DEPTH: DXGI_FORMAT = DXGI_FORMAT_D24_UNORM_S8_UINT;
const DEPTH_FORMAT: DXGI_FORMAT = DXGI_FORMAT_R24G8_TYPELESS;
const CLEAR_DEPTH_STENCIL: u32 = D3D11_CLEAR_DEPTH.0 | D3D11_CLEAR_STENCIL.0;

fn extent([width, height]: [u32; 2]) -> Extent {
    Extent { width, height }
}

fn texels([width, height]: [u32; 2]) -> usize {
    width as usize * height as usize
}

unsafe extern "C" fn log(_: *mut c_void, message: *const c_char) {
    if !message.is_null() {
        println!("{}", unsafe { CStr::from_ptr(message) }.to_string_lossy());
    }
}

fn read(
    device: &Device,
    texture: &ID3D11Texture2D,
    size: [u32; 2],
    bytes: usize,
) -> Option<Vec<u8>> {
    let mut output = vec![0; texels(size) * bytes];
    unsafe {
        rsf_anomaly_sr_fixture_readback(
            device.device.as_raw(),
            device.context.as_raw(),
            texture.as_raw(),
            null_mut(),
            output.as_mut_ptr(),
            output.len(),
        )
    }
    .then_some(output)
}

fn round_trip_views(
    srv: &ID3D11ShaderResourceView,
    input: [u32; 2],
    rtv: &ID3D11RenderTargetView,
    output: [u32; 2],
) -> DrawViews {
    let mut views = DrawViews {
        output: extent(output),
        source: extent(input),
        ..Default::default()
    };
    views.targets[0] = rtv.as_raw();
    views.inputs[0] = srv.as_raw();
    views
}

fn run_pass(
    d: &Device,
    pass: &Pass,
    halves: &[u16],
    input: [u32; 2],
    output: [u32; 2],
) -> Option<Vec<u8>> {
    let source = d.data_texture(input, HDR, COLOUR, halves, input[0] * 8)?;
    let target = d.texture(output, UNORM, COLOUR)?;
    let srv = d.srv(&source, None)?;
    let rtv = d.rtv(&target, None)?;
    unsafe {
        d.context
            .ClearRenderTargetView(&rtv, &[0.25, 0.5, 0.75, 1.0])
    };
    let views = round_trip_views(&srv, input, &rtv, output);
    if !pass.run(d.context.as_raw(), ROUND_TRIP, &views) {
        return None;
    }
    read(d, &target, output, 4)
}

struct RunRoundTrip<'a> {
    device: &'a Device,
    pass: &'a Pass,
}

unsafe extern "C" fn run_round_trip(
    owner: *mut c_void,
    input: *const u16,
    width: u32,
    height: u32,
    output: *mut u8,
    ow: u32,
    oh: u32,
) -> bool {
    let rig = unsafe { &*owner.cast::<RunRoundTrip<'_>>() };
    let source = unsafe { std::slice::from_raw_parts(input, texels([width, height]) * 4) };
    let result = run_pass(rig.device, rig.pass, source, [width, height], [ow, oh]);
    if let Some(pixels) = result {
        unsafe { std::ptr::copy_nonoverlapping(pixels.as_ptr(), output, pixels.len()) };
        true
    } else {
        false
    }
}

fn viewport(size: [u32; 2], min: f32, max: f32) -> D3D11_VIEWPORT {
    D3D11_VIEWPORT {
        Width: size[0] as f32,
        Height: size[1] as f32,
        MinDepth: min,
        MaxDepth: max,
        ..Default::default()
    }
}

fn bindings(
    d: &Device,
) -> (
    Option<ID3D11RenderTargetView>,
    Option<ID3D11DepthStencilView>,
) {
    let (mut rtv, mut dsv) = (None, None);
    unsafe {
        d.context
            .OMGetRenderTargets(Some(std::slice::from_mut(&mut rtv)), Some(&mut dsv))
    };
    (rtv, dsv)
}

fn bound_viewport(d: &Device) -> D3D11_VIEWPORT {
    let mut result = D3D11_VIEWPORT::default();
    let mut count = 1;
    unsafe { d.context.RSGetViewports(&mut count, Some(&mut result)) };
    assert_eq!(count, 1);
    result
}

fn state_preserved(d: &Device, pass: &Pass) {
    let size = [64, 64];
    let make_rtv = || {
        d.rtv(&d.texture(size, UNORM, COLOUR).unwrap(), None)
            .unwrap()
    };
    let (bound, target) = (make_rtv(), make_rtv());
    let twin = d.srv(&d.texture(size, HDR, COLOUR).unwrap(), None).unwrap();
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
        BlendEnable: true.into(),
        SrcBlend: D3D11_BLEND_SRC_ALPHA,
        SrcBlendAlpha: D3D11_BLEND_SRC_ALPHA,
        DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
        DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
        BlendOp: D3D11_BLEND_OP_ADD,
        BlendOpAlpha: D3D11_BLEND_OP_ADD,
        RenderTargetWriteMask: 7,
    };
    let mut blend = None;
    unsafe { d.device.CreateBlendState(&desc, Some(&mut blend)) }.unwrap();
    let factor = [0.1, 0.2, 0.3, 0.4];
    let mask = 0xf0f0_f0f0;
    unsafe {
        d.context.OMSetRenderTargets(
            Some(&[Some(bound.clone())]),
            None::<&ID3D11DepthStencilView>,
        );
        d.context
            .PSSetShaderResources(5, Some(&[Some(twin.clone())]));
        d.context
            .RSSetViewports(Some(&[viewport([32, 32], 0.25, 0.75)]));
        d.context
            .OMSetBlendState(blend.as_ref(), Some(&factor), mask);
    }
    assert!(pass.run(
        d.context.as_raw(),
        ROUND_TRIP,
        &round_trip_views(&twin, size, &target, size)
    ));
    assert!(bindings(d).0.as_ref() == Some(&bound));
    let (mut restored, mut shader, mut blend_after) = (None, None, None);
    let (mut factor_after, mut mask_after) = ([0.; 4], 0);
    unsafe {
        d.context
            .PSGetShaderResources(5, Some(std::slice::from_mut(&mut restored)));
        d.context.PSGetShader(&mut shader, None, None);
        d.context.OMGetBlendState(
            Some(&mut blend_after),
            Some(&mut factor_after),
            Some(&mut mask_after),
        );
    }
    assert!(restored.as_ref() == Some(&twin));
    let vp = bound_viewport(d);
    assert_eq!(
        [vp.Width, vp.Height, vp.MinDepth, vp.MaxDepth],
        [32., 32., 0.25, 0.75]
    );
    assert!(blend.is_some() && blend_after == blend && shader.is_none());
    assert_eq!((factor_after, mask_after), (factor, mask));
    unsafe { d.context.ClearState() };
}

fn target_device(d: &Device) -> Targets {
    Targets {
        handle: d.device.as_raw(),
    }
}

fn target_input(texture: Option<&ID3D11Texture2D>, with_uav: bool) -> TwinInput {
    TwinInput {
        texture: texture.map_or(null_mut(), Interface::as_raw),
        rtv: null_mut(),
        srv: null_mut(),
        uav: null_mut(),
        size: Extent::default(),
        format: 0,
        promote: 1,
        with_uav: u32::from(with_uav),
        world: 0,
    }
}

fn twins(d: &Device) {
    let target = || target_device(d);
    let engine = d.texture([320, 200], UNORM, COLOUR).unwrap();
    let (_twin, desc, format) = target().twin(&target_input(Some(&engine), false)).unwrap();
    let view = _twin.view(desc, format);
    assert_eq!(desc.size, extent([320, 200]));
    assert_eq!((desc.format, desc.samples), (HDR.0 as u32, 1));
    let fields = view.fields();
    assert!(
        fields.surface == view.texture
            && fields.texture_surface == fields.surface
            && fields.desc_cache == fields.surface
            && !fields.rtv.is_null()
            && !fields.srv.is_null()
            && fields.uav.is_null()
            && fields.format == 113
    );
    let actual = unsafe { ID3D11Texture2D::from_raw_borrowed(&view.texture) }.unwrap();
    let mut actual_desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { actual.GetDesc(&mut actual_desc) };
    assert_eq!(fields.desc, unsafe {
        std::mem::transmute::<D3D11_TEXTURE2D_DESC, [u8; 44]>(actual_desc)
    });
    let (uav, desc, format) = target().twin(&target_input(Some(&engine), true)).unwrap();
    assert!(!uav.view(desc, format).uav.is_null());
    let wrong = d.texture([64, 64], HDR, COLOUR).unwrap();
    assert_eq!(
        target().twin(&target_input(Some(&wrong), false)).err(),
        Some(Error::Format)
    );
    if let Some(msaa) = d.texture_desc([64, 64], UNORM, COLOUR, 4, None) {
        assert_eq!(
            target().twin(&target_input(Some(&msaa), false)).err(),
            Some(Error::Shape)
        );
    }
    assert_eq!(
        target().twin(&target_input(None, false)).err(),
        Some(Error::NotTexture)
    );
}

fn dsv_desc() -> D3D11_DEPTH_STENCIL_VIEW_DESC {
    D3D11_DEPTH_STENCIL_VIEW_DESC {
        Format: DEPTH,
        ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D,
        ..Default::default()
    }
}

fn debug_layer(d: &Device, pass: &Pass) {
    if d.info.is_none() {
        println!("debug-layer checks skipped: D3D11 SDK layers are not installed");
        return;
    }
    d.debug_errors();
    let size = [128, 128];
    let rtv = |size, format, bind| {
        d.rtv(&d.texture(size, format, bind).unwrap(), None)
            .unwrap()
    };
    let (a, b, half) = (
        rtv(size, HDR, COLOUR),
        rtv(size, UNORM, COLOUR),
        rtv([64, 64], UNORM, RTV),
    );
    let depth = d
        .dsv(
            &d.texture(size, DEPTH_FORMAT, DSV | SRV).unwrap(),
            Some(&dsv_desc()),
        )
        .unwrap();
    let source = d.srv(&d.texture(size, HDR, SRV).unwrap(), None).unwrap();
    unsafe {
        d.context
            .OMSetRenderTargets(Some(&[Some(a.clone()), Some(b)]), &depth);
        d.context.ClearRenderTargetView(&a, &[2.5, 0.5, 0.25, 1.]);
        d.context
            .ClearDepthStencilView(&depth, CLEAR_DEPTH_STENCIL, 1., 0);
    }
    assert!(pass.run(
        d.context.as_raw(),
        ROUND_TRIP,
        &round_trip_views(&source, size, &a, size)
    ));
    assert_eq!(d.debug_errors(), 0, "mixed FP16/UNORM8 MRT with D24S8");
    unsafe {
        d.context
            .OMSetRenderTargets(Some(&[Some(half.clone())]), &depth)
    };
    let bound = bindings(d);
    let mismatch = d.debug_errors();
    println!(
        "half-size RTV with full-size DSV: {mismatch} debug errors, RTV held {}, DSV held {}",
        bound.0.as_ref() == Some(&half),
        bound.1.as_ref() == Some(&depth)
    );
    unsafe { d.context.ClearState() };
}

fn hash(mut value: u32) -> u32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^ (value >> 16)
}

const FILL: &str = r"
float4 vs(uint id : SV_VertexID) : SV_Position { float2 uv = float2((id << 1) & 2, id & 2); return float4(uv * float2(2, -2) + float2(-1, 1), 0, 1); }
float ps(float4 p : SV_Position) : SV_Depth { uint2 q = uint2(p.xy); uint v = q.x * 7919u + q.y * 104729u + 17u; v ^= v >> 13; v *= 0x5bd1e995u; v ^= v >> 15; return float(v & 0xffffffu) / 16777215.0; }
";

fn fill_depth(d: &Device, dsv: &ID3D11DepthStencilView, size: [u32; 2]) {
    let shader = |entry, target| {
        compile_system_checked(Request {
            source: FILL.as_bytes(),
            name: c"fill_depth",
            entry,
            target,
            flags: 1 << 11,
        })
        .expect("depth fill shader")
    };
    let (mut vs, mut ps, mut state) = (None, None, None);
    let desc = D3D11_DEPTH_STENCIL_DESC {
        DepthEnable: true.into(),
        DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ALL,
        DepthFunc: D3D11_COMPARISON_ALWAYS,
        ..Default::default()
    };
    unsafe {
        d.device
            .CreateVertexShader(
                &shader(c"vs", c"vs_5_0"),
                None::<&ID3D11ClassLinkage>,
                Some(&mut vs),
            )
            .unwrap();
        d.device
            .CreatePixelShader(
                &shader(c"ps", c"ps_5_0"),
                None::<&ID3D11ClassLinkage>,
                Some(&mut ps),
            )
            .unwrap();
        d.device
            .CreateDepthStencilState(&desc, Some(&mut state))
            .unwrap();
        d.context
            .ClearDepthStencilView(dsv, CLEAR_DEPTH_STENCIL, 1., 0x33);
        d.context.OMSetRenderTargets(None, dsv);
        d.context.OMSetDepthStencilState(state.as_ref(), 0);
        d.context
            .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        d.context.VSSetShader(vs.as_ref(), None);
        d.context.PSSetShader(ps.as_ref(), None);
        d.context.RSSetViewports(Some(&[viewport(size, 0., 1.)]));
        d.context.Draw(3, 0);
        d.context.ClearState();
    }
}

struct Rig {
    input: [u32; 2],
    output: [u32; 2],
    colour: Vec<u16>,
    position: Vec<u16>,
    distort: Vec<u8>,
    inputs: [ID3D11ShaderResourceView; 3],
    output_textures: [ID3D11Texture2D; 4],
    targets: [ID3D11RenderTargetView; 3],
    depth_target: ID3D11DepthStencilView,
    depth: Target,
    depth_desc: super::Texture,
}

impl Rig {
    fn new(d: &Device, input: [u32; 2], output: [u32; 2], colour: Option<&[u16]>) -> Self {
        let count = texels(input);
        let colour = colour.map_or_else(
            || {
                (0..count)
                    .flat_map(|i| {
                        let rgb = std::array::from_fn::<_, 3, _>(|channel| {
                            let linear =
                                6. * f64::from(hash((i * 4 + channel) as u32) % 10000) / 9999.;
                            half(tonemap(linear) as f32)
                        });
                        [
                            rgb[0],
                            rgb[1],
                            rgb[2],
                            half((hash(i as u32) % 256) as f32 / 255.),
                        ]
                    })
                    .collect()
            },
            <[u16]>::to_vec,
        );
        let position: Vec<_> = (0..count * 4)
            .map(|i| (hash(i as u32 + 99) & 0x7bff) as u16)
            .collect();
        let distort: Vec<_> = (0..count * 4).map(|i| hash(i as u32 + 7) as u8).collect();
        let source = [
            d.data_texture(input, HDR, COLOUR, &colour, input[0] * 8),
            d.data_texture(input, HDR, COLOUR, &position, input[0] * 8),
            d.data_texture(input, UNORM, COLOUR, &distort, input[0] * 4),
        ];
        let inputs = source.map(|texture| d.srv(&texture.unwrap(), None).unwrap());
        let output_textures = [UNORM, HDR, UNORM, DEPTH].map(|format| {
            d.texture(output, format, if format == DEPTH { DSV } else { COLOUR })
                .unwrap()
        });
        let targets = std::array::from_fn(|i| d.rtv(&output_textures[i], None).unwrap());
        let depth_target = d.dsv(&output_textures[3], None).unwrap();
        let (depth, depth_desc) = target_device(d)
            .depth(depth_target.as_raw(), extent(output), extent(input))
            .unwrap();
        let raw = depth.view(depth_desc, 0).dsv;
        let depth_dsv = unsafe { ID3D11DepthStencilView::from_raw_borrowed(&raw) }.unwrap();
        fill_depth(d, depth_dsv, input);
        unsafe {
            for target in &targets {
                d.context
                    .ClearRenderTargetView(target, &[0.5, 0.25, 0.75, 0.125]);
            }
            d.context
                .ClearDepthStencilView(&depth_target, CLEAR_DEPTH_STENCIL, 0.5, 0x5a);
        }
        Self {
            input,
            output,
            colour,
            position,
            distort,
            inputs,
            output_textures,
            targets,
            depth_target,
            depth,
            depth_desc,
        }
    }

    fn depth(&self) -> TargetView {
        self.depth.view(self.depth_desc, 0)
    }

    fn views(&self) -> DrawViews {
        let [colour, position, distort] = self.inputs.each_ref().map(Interface::as_raw);
        DrawViews {
            output: extent(self.output),
            source: extent(self.input),
            targets: self.targets.each_ref().map(Interface::as_raw),
            depth: self.depth_target.as_raw(),
            inputs: [colour, position, distort, self.depth().srv, null_mut()],
            ..Default::default()
        }
    }

    fn run(&self, d: &Device, pass: &Pass) -> [Vec<u8>; 5] {
        assert!(pass.run(d.context.as_raw(), UPSCALE, &self.views()));
        let [a, b, c, e] = std::array::from_fn(|i| {
            read(d, &self.output_textures[i], self.output, [4, 8, 4, 4][i]).unwrap()
        });
        let raw = self.depth().texture;
        let depth = unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }.unwrap();
        [a, b, c, e, read(d, depth, self.input, 4).unwrap()]
    }
}

fn check_upscale(d: &Device, pass: &Pass, input: [u32; 2], output: [u32; 2]) {
    let rig = Rig::new(d, input, output, None);
    let [colour, position, distort, depth, twin_depth] = rig.run(d, pass);
    let record = UpscaleReadback {
        input,
        output,
        source_colour: rig.colour.as_ptr(),
        source_position: rig.position.as_ptr(),
        source_distort: rig.distort.as_ptr(),
        colour: colour.as_ptr(),
        position: position.as_ptr(),
        distort: distort.as_ptr(),
        depth: depth.as_ptr(),
        twin_depth: twin_depth.as_ptr(),
    };
    assert_eq!(unsafe { rsf_anomaly_sr_upscale_numerics(&record) }, 0);
}

fn identity(d: &Device, upscale: &Pass, roundtrip: &Pass) {
    let size = [256, 256];
    let halves: Vec<_> = (0..=u16::MAX).flat_map(|value| [value; 4]).collect();
    let rig = Rig::new(d, size, size, Some(&halves));
    let [colour, position, distort, depth, twin_depth] = rig.run(d, upscale);
    assert_eq!(colour, run_pass(d, roundtrip, &halves, size, size).unwrap());
    assert_eq!(distort, rig.distort);
    assert!(
        position
            .chunks_exact(2)
            .map(|bytes| u16::from_le_bytes(bytes.try_into().unwrap()))
            .eq(rig.position)
    );
    assert!(
        depth
            .chunks_exact(4)
            .zip(twin_depth.chunks_exact(4))
            .all(|(a, b)| a[..3] == b[..3])
    );
}

fn upscale_state_and_refusals(d: &Device, pass: &Pass) {
    let (input, output) = ([32, 18], [48, 27]);
    let rig = Rig::new(d, input, output, None);
    let bound = d.rtv(&d.texture(input, UNORM, RTV).unwrap(), None).unwrap();
    let depth = rig.depth().dsv;
    let dsv = unsafe { ID3D11DepthStencilView::from_raw_borrowed(&depth) }.unwrap();
    unsafe {
        d.context
            .OMSetRenderTargets(Some(&[Some(bound.clone())]), dsv);
        d.context.RSSetViewports(Some(&[viewport(input, 0., 0.02)]));
    }
    let views = rig.views();
    assert!(pass.run(d.context.as_raw(), UPSCALE, &views));
    let (rtv_after, dsv_after) = bindings(d);
    assert!(rtv_after.as_ref() == Some(&bound) && dsv_after.as_ref() == Some(dsv));
    let vp = bound_viewport(d);
    assert_eq!(
        [vp.Width, vp.Height, vp.MinDepth, vp.MaxDepth],
        [32., 18., 0., 0.02]
    );
    unsafe {
        super::native::rsf_anomaly_sr_set_full_viewport(d.context.as_raw(), output[0], output[1])
    };
    let vp = bound_viewport(d);
    assert_eq!(
        [vp.Width, vp.Height, vp.MinDepth, vp.MaxDepth],
        [48., 27., 0., 1.]
    );
    unsafe { d.context.ClearState() };
    assert!(!pass.run(
        d.context.as_raw(),
        UPSCALE,
        &DrawViews {
            depth: null_mut(),
            ..views
        }
    ));
    assert!(!pass.run(
        d.context.as_raw(),
        UPSCALE,
        &DrawViews {
            source: extent([64, 18]),
            ..views
        }
    ));
}

fn world_twins(d: &Device) {
    let input = [96, 54];
    let render = [64, 36];
    let targets = || target_device(d);
    let ssao = d
        .texture(input, DXGI_FORMAT_R16_FLOAT, COLOUR | UAV)
        .unwrap();
    let rtv = d.rtv(&ssao, None).unwrap();
    let srv = d.srv(&ssao, None).unwrap();
    let mut uav: Option<ID3D11UnorderedAccessView> = None;
    unsafe {
        d.device
            .CreateUnorderedAccessView(&ssao, None, Some(&mut uav))
    }
    .unwrap();
    let uav = uav.unwrap();
    let engine = TwinInput {
        texture: ssao.as_raw(),
        rtv: rtv.as_raw(),
        srv: srv.as_raw(),
        uav: uav.as_raw(),
        size: extent(render),
        format: 111,
        with_uav: 1,
        promote: 0,
        world: 1,
    };
    let (twin, desc, format) = targets().twin(&engine).unwrap();
    let fields = twin.view(desc, format).fields();
    assert_eq!(
        (desc.size, desc.format, fields.format, fields.width),
        (extent(render), DXGI_FORMAT_R16_FLOAT.0 as u32, 111, 64)
    );
    assert!(
        desc.bindings & UAV != 0 && !fields.uav.is_null() && fields.desc_cache == fields.surface
    );
    let mut actual = D3D11_TEXTURE2D_DESC::default();
    let raw = fields.surface;
    unsafe {
        ID3D11Texture2D::from_raw_borrowed(&raw)
            .unwrap()
            .GetDesc(&mut actual)
    };
    assert_eq!(fields.desc, unsafe {
        std::mem::transmute::<D3D11_TEXTURE2D_DESC, [u8; 44]>(actual)
    });

    let texture = d
        .texture(input, DXGI_FORMAT_R8G8B8A8_TYPELESS, COLOUR)
        .unwrap();
    let rtv_desc = D3D11_RENDER_TARGET_VIEW_DESC {
        Format: UNORM,
        ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D,
        ..Default::default()
    };
    let srv_desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: UNORM,
        ViewDimension: D3D11_SRV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
            Texture2D: D3D11_TEX2D_SRV {
                MostDetailedMip: 0,
                MipLevels: 1,
            },
        },
    };
    let rtv = d.rtv(&texture, Some(&rtv_desc)).unwrap();
    let srv = d.srv(&texture, Some(&srv_desc)).unwrap();
    let mut albedo = TwinInput {
        texture: texture.as_raw(),
        rtv: rtv.as_raw(),
        srv: srv.as_raw(),
        uav: null_mut(),
        size: extent(render),
        format: 21,
        with_uav: 0,
        promote: 0,
        world: 1,
    };
    let (kept, desc, format) = targets().twin(&albedo).unwrap();
    let view = kept.view(desc, format);
    assert!(!view.rtv.is_null() && !view.srv.is_null() && view.uav.is_null());
    let mut desc = D3D11_RENDER_TARGET_VIEW_DESC::default();
    unsafe {
        ID3D11RenderTargetView::from_raw_borrowed(&view.rtv)
            .unwrap()
            .GetDesc(&mut desc)
    };
    assert_eq!((desc.Format, format), (UNORM, 21));
    albedo.promote = 1;
    let (_promoted, desc, format) = targets().twin(&albedo).unwrap();
    assert_eq!(
        (desc.format, desc.size.width, format),
        (HDR.0 as u32, 64, 113)
    );
    assert_eq!(
        targets()
            .twin(&TwinInput {
                promote: 1,
                ..engine
            })
            .err(),
        Some(Error::Format)
    );
    albedo.promote = 0;
    albedo.size.width = 0;
    assert_eq!(targets().twin(&albedo).err(), Some(Error::Shape));
}

fn depth_twins(d: &Device) {
    let (output, render) = ([96, 54], [64, 36]);
    let dsv = d
        .dsv(&d.texture(output, DEPTH, DSV).unwrap(), None)
        .unwrap();
    let depth = |source, output| target_device(d).depth(source, extent(output), extent(render));
    let (twin, desc) = depth(dsv.as_raw(), output).unwrap();
    let view = twin.view(desc, 0);
    assert_eq!(
        (desc.size, desc.format, view.size),
        (extent(render), DEPTH_FORMAT.0 as u32, extent(render))
    );
    let (mut dsv_desc, mut srv_desc) = (
        D3D11_DEPTH_STENCIL_VIEW_DESC::default(),
        D3D11_SHADER_RESOURCE_VIEW_DESC::default(),
    );
    unsafe {
        ID3D11DepthStencilView::from_raw_borrowed(&view.dsv)
            .unwrap()
            .GetDesc(&mut dsv_desc);
        ID3D11ShaderResourceView::from_raw_borrowed(&view.srv)
            .unwrap()
            .GetDesc(&mut srv_desc);
    }
    assert_eq!(
        (dsv_desc.Format, srv_desc.Format),
        (DEPTH, DXGI_FORMAT_R24_UNORM_X8_TYPELESS)
    );
    assert_eq!(depth(dsv.as_raw(), [128, 54]).err(), Some(Error::Depth));
    let d32 = d
        .dsv(
            &d.texture(output, DXGI_FORMAT_D32_FLOAT, DSV).unwrap(),
            None,
        )
        .unwrap();
    assert_eq!(depth(d32.as_raw(), output).err(), Some(Error::Depth));
    if let Some(msaa) = d.texture_desc(output, DEPTH, DSV, 4, None) {
        assert_eq!(
            depth(d.dsv(&msaa, None).unwrap().as_raw(), output).err(),
            Some(Error::Depth)
        );
    }
    assert_eq!(depth(null_mut(), output).err(), Some(Error::NotTexture));
}

fn debug_layer_stage2(d: &Device, pass: &Pass) {
    if d.info.is_none() {
        println!("Stage 2 debug-layer checks skipped: D3D11 SDK layers are not installed");
        return;
    }
    d.debug_errors();
    let rig = Rig::new(d, [64, 36], [96, 54], None);
    assert!(pass.run(d.context.as_raw(), UPSCALE, &rig.views()));
    assert_eq!(d.debug_errors(), 0, "mixed seam MRT with SV_Depth");
    let depth = rig.depth().dsv;
    let dsv = unsafe { ID3D11DepthStencilView::from_raw_borrowed(&depth) }.unwrap();
    unsafe {
        d.context
            .OMSetRenderTargets(Some(&[Some(rig.targets[0].clone())]), dsv)
    };
    assert_eq!(
        d.debug_errors(),
        1,
        "output-size RTV with render-size DSV requires a stand-in"
    );
    let scratch = |source| target_device(d).scratch(source, extent(rig.input));
    let (target, desc) = scratch(rig.targets[0].as_raw()).unwrap();
    assert_eq!(
        (desc.size, desc.format),
        (extent(rig.input), UNORM.0 as u32)
    );
    let view = target.view(desc, 0);
    let rtv = unsafe { ID3D11RenderTargetView::from_raw_borrowed(&view.rtv) }.unwrap();
    unsafe {
        d.context
            .OMSetRenderTargets(Some(&[Some(rtv.clone())]), dsv)
    };
    assert_eq!(d.debug_errors(), 0);
    let (bound_rtv, bound_dsv) = bindings(d);
    assert!(bound_rtv.as_ref() == Some(rtv) && bound_dsv.as_ref() == Some(dsv));
    assert_eq!(scratch(null_mut()).err(), Some(Error::NotTexture));
    unsafe { d.context.ClearState() };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_roundtrip_device_fixture() -> i32 {
    let Some(d) = Device::hardware_or_warp(true) else {
        println!("SKIP: no D3D11 hardware or WARP device");
        return 77;
    };
    let make_pass = |kind| Pass::new(d.device.as_raw(), kind, Some(log), null_mut());
    let (Some(roundtrip), Some(upscale)) = (make_pass(ROUND_TRIP), make_pass(UPSCALE)) else {
        eprintln!("FAIL: SR pass creation");
        return 1;
    };
    let mut rig = RunRoundTrip {
        device: &d,
        pass: &roundtrip,
    };
    let mut failures = rsf_anomaly_sr_roundtrip_numerics(
        (&mut rig as *mut RunRoundTrip<'_>).cast(),
        run_round_trip,
    );
    let mut case = |name, run: &dyn Fn()| {
        if catch_unwind(AssertUnwindSafe(run)).is_err() {
            eprintln!("FAIL: {name}");
            failures += 1;
        }
        unsafe { d.context.ClearState() };
    };
    case("round-trip state", &|| state_preserved(&d, &roundtrip));
    case("round-trip twins", &|| twins(&d));
    case("round-trip debug layer", &|| debug_layer(&d, &roundtrip));
    case("scale 1 identity", &|| identity(&d, &upscale, &roundtrip));
    for (input, output) in [
        ([64, 36], [128, 72]),
        ([64, 36], [96, 54]),
        ([65, 41], [97, 61]),
    ] {
        case("upscale numerics", &|| {
            check_upscale(&d, &upscale, input, output)
        });
    }
    case("upscale state and refusals", &|| {
        upscale_state_and_refusals(&d, &upscale)
    });
    case("world twins", &|| world_twins(&d));
    case("depth twins", &|| depth_twins(&d));
    case("upscale debug layer", &|| debug_layer_stage2(&d, &upscale));
    println!("{failures} failure(s)");
    i32::from(failures != 0)
}
