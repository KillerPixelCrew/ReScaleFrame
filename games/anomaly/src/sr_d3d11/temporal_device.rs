// SPDX-License-Identifier: MIT
//! D3D11 temporal SR fixture, using the live prepare, evaluate and reinsert inputs.
#[path = "temporal_device_abi.rs"]
mod abi;
mod decode;
mod provider;

use self::abi::rsf_anomaly_temporal_fixture_graphics as Graphics;
use super::{DrawViews, Extent, PREPARE, Pass, Platform, REINSERT, TemporalTargets, UPSCALE};
use crate::sr_fixture_device::Device;
use crate::sr_fixture_readback::rsf_anomaly_sr_fixture_readback;
use crate::sr_pass::PassInputs;
use crate::sr_stage::FIRST_PERSON_DEPTH_BITS;
use crate::sr_stage_abi::{rsf_anomaly_sr_camera as Camera, rsf_anomaly_sr_jitter_state as Jitter};
use crate::sr_tonemap::roundtrip_fixture::{half_to_float, rsf_anomaly_sr_float_to_half as half};
use crate::sr_tonemap::{
    rsf_anomaly_tonemap_forward as tonemap, rsf_anomaly_tonemap_inverse as inverse,
    rsf_anomaly_tonemap_unorm8 as unorm8,
};
use std::ffi::{CStr, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::null_mut;
use windows::Win32::Graphics::Direct3D::D3D11_SRV_DIMENSION_TEXTURE2D;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::Interface;

const SRV: u32 = D3D11_BIND_SHADER_RESOURCE.0 as u32;
const RTV: u32 = D3D11_BIND_RENDER_TARGET.0 as u32;
const DSV: u32 = D3D11_BIND_DEPTH_STENCIL.0 as u32;
const HDR: DXGI_FORMAT = DXGI_FORMAT_R16G16B16A16_FLOAT;
const UNORM: DXGI_FORMAT = DXGI_FORMAT_R8G8B8A8_UNORM;
type Matrix = [f32; 16];

fn extent([width, height]: [u32; 2]) -> Extent {
    Extent { width, height }
}
fn texels([w, h]: [u32; 2]) -> usize {
    w as usize * h as usize
}
fn d24(depth: f64) -> u32 {
    (depth * 16777215.).round() as u32
}

unsafe extern "C" fn log(_: *mut c_void, message: *const c_char) {
    if !message.is_null() {
        println!(
            "  | {}",
            unsafe { CStr::from_ptr(message) }.to_string_lossy()
        );
    }
}

trait Word: Sized {
    fn from_bytes(bytes: &[u8]) -> Self;
}
impl Word for u8 {
    fn from_bytes(bytes: &[u8]) -> Self {
        bytes[0]
    }
}
impl Word for u16 {
    fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_le_bytes(bytes.try_into().unwrap())
    }
}
impl Word for f32 {
    fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_le_bytes(bytes.try_into().unwrap())
    }
}

fn read<T: Word>(d: &Device, texture: &ID3D11Texture2D, channels: usize) -> Vec<T> {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    let mut bytes = vec![0; texels([desc.Width, desc.Height]) * channels * size_of::<T>()];
    let ok = unsafe {
        rsf_anomaly_sr_fixture_readback(
            d.device.as_raw(),
            d.context.as_raw(),
            texture.as_raw(),
            null_mut(),
            bytes.as_mut_ptr(),
            bytes.len(),
        )
    };
    assert!(ok, "texture readback");
    bytes
        .chunks_exact(size_of::<T>())
        .map(T::from_bytes)
        .collect()
}

struct Depth {
    srv: ID3D11ShaderResourceView,
    dsv: ID3D11DepthStencilView,
    texture: ID3D11Texture2D,
}
impl Depth {
    fn new(d: &Device, size: [u32; 2], data: &[u32]) -> Self {
        let format = DXGI_FORMAT_R24G8_TYPELESS;
        let input = d
            .data_texture(size, format, SRV, data, size[0] * 4)
            .unwrap();
        let texture = d.texture(size, format, SRV | DSV).unwrap();
        unsafe { d.context.CopyResource(&texture, &input) };
        let dsv = d
            .dsv(
                &texture,
                Some(&D3D11_DEPTH_STENCIL_VIEW_DESC {
                    Format: DXGI_FORMAT_D24_UNORM_S8_UINT,
                    ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D,
                    ..Default::default()
                }),
            )
            .unwrap();
        let srv = d
            .srv(
                &texture,
                Some(&D3D11_SHADER_RESOURCE_VIEW_DESC {
                    Format: DXGI_FORMAT_R24_UNORM_X8_TYPELESS,
                    ViewDimension: D3D11_SRV_DIMENSION_TEXTURE2D,
                    Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture2D: D3D11_TEX2D_SRV {
                            MostDetailedMip: 0,
                            MipLevels: 1,
                        },
                    },
                }),
            )
            .unwrap();
        Self { srv, dsv, texture }
    }
}

struct Outputs {
    colour: ID3D11Texture2D,
    position: ID3D11Texture2D,
    targets: [ID3D11RenderTargetView; 3],
    depth: Depth,
}
impl Outputs {
    fn new(d: &Device, size: [u32; 2]) -> Self {
        let colour = d.texture(size, UNORM, RTV | SRV).unwrap();
        let position = d.texture(size, HDR, RTV).unwrap();
        let distort = d.texture(size, UNORM, RTV).unwrap();
        let targets = [&colour, &position, &distort].map(|texture| d.rtv(texture, None).unwrap());
        let depth = Depth::new(d, size, &vec![d24(1.); texels(size)]);
        Self {
            colour,
            position,
            targets,
            depth,
        }
    }

    fn reinsert(&self, input: [u32; 2], output: [u32; 2], inputs: [*mut c_void; 5]) -> DrawViews {
        DrawViews {
            output: extent(output),
            source: extent(input),
            targets: self.targets.each_ref().map(Interface::as_raw),
            depth: self.depth.dsv.as_raw(),
            inputs,
            ..Default::default()
        }
    }
}

fn projection(aspect: f32) -> Matrix {
    let (n, f) = (0.2f32, 900.);
    let h = (1.2f32 * 0.5).tan().recip();
    let q = f / (f - n);
    [
        h / aspect,
        0.,
        0.,
        0.,
        0.,
        h,
        0.,
        0.,
        0.,
        0.,
        q,
        1.,
        0.,
        0.,
        -q * n,
        0.,
    ]
}

fn view_at(px: f32) -> Matrix {
    [
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., -px, 0., 0., 1.,
    ]
}

fn camera(view: Matrix, projection: Matrix) -> Camera {
    Camera {
        view,
        projection,
        valid: true,
    }
}

fn mul(v: [f64; 4], m: &Matrix) -> [f64; 4] {
    std::array::from_fn(|col| (0..4).map(|row| v[row] * f64::from(m[row * 4 + col])).sum())
}

fn camera_motion([x, y]: [u32; 2], size: [u32; 2], depth: f64, matrix: &Matrix) -> [f64; 2] {
    let [w, h] = size.map(f64::from);
    let ndc = [
        (f64::from(x) + 0.5) / w * 2. - 1.,
        1. - (f64::from(y) + 0.5) / h * 2.,
    ];
    let prev = mul([ndc[0], ndc[1], depth, 1.], matrix);
    [
        (prev[0] / prev[3] - ndc[0]) * w * 0.5,
        (prev[1] / prev[3] - ndc[1]) * h * -0.5,
    ]
}

fn frame_inputs(
    render: [u32; 2],
    output: [u32; 2],
    current: Camera,
    previous: Camera,
    jitter: Jitter,
) -> PassInputs {
    let mut input = PassInputs::default();
    input.frame.native_frame = jitter.frame;
    input.frame.generation = 1;
    [input.frame.render_width, input.frame.render_height] = render;
    [input.frame.output_width, input.frame.output_height] = output;
    input.frame.current = current;
    input.frame.previous = previous;
    input.frame.jitter = jitter;
    input.frame.frame_seconds = 1. / 60.;
    input
}

fn make_pass(d: &Device, platform: &'static Platform) -> Pass {
    let pass = Pass::new(platform, d.device.as_raw(), UPSCALE, Some(log), null_mut()).unwrap();
    assert!(pass.temporal_ready());
    pass
}

fn prepare(
    d: &Device,
    pass: &Pass,
    size: [u32; 2],
    targets: &TemporalTargets,
    scene: [&ID3D11ShaderResourceView; 3],
    first_person: bool,
) {
    let [colour, depth, motion] = scene.map(Interface::as_raw);
    let target = targets.view();
    let views = DrawViews {
        output: extent(size),
        targets: [target.colour_rtv, target.motion_rtv, null_mut()],
        inputs: [colour, depth, motion, null_mut(), null_mut()],
        first_person_depth: FIRST_PERSON_DEPTH_BITS,
        first_person_zero: u32::from(first_person),
        ..Default::default()
    };
    assert!(pass.run(d.context.as_raw(), PREPARE, &views));
}

fn prepare_case(d: &Device, platform: &'static Platform) {
    let (size, output) = ([16, 8], [24, 12]);
    let [w, h] = size;
    let pass = make_pass(d, platform);
    let device = d.device.as_raw();
    let format = HDR.0 as u32;
    let targets =
        TemporalTargets::new(platform, device, extent(size), extent(output), format).unwrap();
    assert!(targets.matches(device, extent(size), extent(output), format));
    assert!(!targets.matches(device, extent([w + 1, h]), extent(output), format));
    let mut depths = Vec::new();
    let mut colour = Vec::new();
    let mut motion = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let linear = 0.05 + 0.4 * f64::from(x) + 0.3 * f64::from(y);
            for channel in 0..3 {
                colour.push(half(
                    tonemap(linear * (1. + 0.1 * f64::from(channel))) as f32
                ));
            }
            colour.push(half(0.25));
            let valid = y < h / 2;
            motion.extend([
                half(if valid { 0.001 * x as f32 } else { 0.3 }),
                half(if valid { -0.002 * y as f32 } else { 0.3 }),
                half(if valid { 1. } else { 0. }),
                0,
            ]);
            depths.push(d24(if x < w / 4 { 0.01 } else { 0.6 }));
        }
    }
    let twin = d
        .data_texture(size, HDR, SRV | RTV, &colour, w * 8)
        .unwrap();
    let twin_srv = d.srv(&twin, None).unwrap();
    let mv = d
        .data_texture(size, HDR, SRV | RTV, &motion, w * 8)
        .unwrap();
    let mv_srv = d.srv(&mv, None).unwrap();
    let depth = Depth::new(d, size, &depths);
    for first_person in [true, false] {
        prepare(
            d,
            &pass,
            size,
            &targets,
            [&twin_srv, &depth.srv, &mv_srv],
            first_person,
        );
        let target = targets.view();
        let colour_tex = unsafe { ID3D11Texture2D::from_raw_borrowed(&target.colour) }.unwrap();
        let motion_tex = unsafe { ID3D11Texture2D::from_raw_borrowed(&target.motion) }.unwrap();
        let out_colour = read::<u16>(d, colour_tex, 4);
        let out_motion = read::<u16>(d, motion_tex, 4);
        assert_eq!(
            (out_colour.len(), out_motion.len()),
            (colour.len(), motion.len())
        );
        let mut worst = 0f64;
        for i in 0..texels(size) {
            for c in 0..3 {
                let expected = inverse(f64::from(half_to_float(colour[i * 4 + c])));
                let actual = f64::from(half_to_float(out_colour[i * 4 + c]));
                assert!(actual.is_finite());
                worst = worst.max((actual - expected).abs() / expected.max(1e-3));
            }
            let (x, y) = (i as u32 % w, i as u32 / w);
            let got = out_motion[i * 4..i * 4 + 3]
                .iter()
                .copied()
                .map(half_to_float);
            let expected = if y < h / 2 {
                [
                    half_to_float(motion[i * 4]),
                    half_to_float(motion[i * 4 + 1]),
                    1.,
                ]
            } else {
                [0., 0., u8::from(first_person && x < w / 4) as f32]
            };
            assert!(
                got.eq(expected) && out_motion[i * 4 + 3] == 0,
                "prepared motion ({x}, {y})"
            );
        }
        println!("prepare (first person {first_person}): relative colour error {worst:.2e}");
        assert!(worst < 2e-3);
    }
    let [ow, oh] = output;
    let reconstructed: Vec<_> = (0..ow * oh)
        .flat_map(|i| {
            let x = 0.02f32 + 0.11 * (i % ow) as f32 + 0.05 * (i / ow) as f32;
            [half(x), half(x), half(x), half(1.)]
        })
        .collect();
    let sr = d
        .data_texture(output, HDR, SRV, &reconstructed, ow * 8)
        .unwrap();
    let sr_srv = d.srv(&sr, None).unwrap();
    let position = d
        .data_texture(size, HDR, SRV, &vec![half(3.); texels(size) * 4], w * 8)
        .unwrap();
    let position_srv = d.srv(&position, None).unwrap();
    let distort = d
        .data_texture(size, UNORM, SRV, &vec![0x8080_8080u32; texels(size)], w * 4)
        .unwrap();
    let distort_srv = d.srv(&distort, None).unwrap();
    let outputs = Outputs::new(d, output);
    let mut views = outputs.reinsert(
        size,
        output,
        [
            twin_srv.as_raw(),
            position_srv.as_raw(),
            distort_srv.as_raw(),
            depth.srv.as_raw(),
            sr_srv.as_raw(),
        ],
    );
    assert!(pass.run(d.context.as_raw(), REINSERT, &views));
    views.inputs[4] = null_mut();
    assert!(!pass.run(d.context.as_raw(), REINSERT, &views));
    let out = read::<u8>(d, &outputs.colour, 4);
    assert_eq!(out.len(), texels(output) * 4);
    let mut worst = 0i32;
    for (i, pixel) in out.as_chunks::<4>().0.iter().enumerate() {
        let expected = unorm8(tonemap(f64::from(half_to_float(reconstructed[i * 4]))));
        worst = worst.max((i32::from(pixel[0]) - expected).abs());
        assert_eq!(i32::from(pixel[3]), unorm8(0.25));
    }
    println!("reinsert: worst UNORM8 difference {worst}");
    let pos = read::<u16>(d, &outputs.position, 4);
    assert_eq!(pos.len(), texels(output) * 4);
    assert_eq!(half_to_float(pos[0]), 3.);
    assert!(worst <= 1);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_temporal_device_fixture(
    platform: &'static Platform,
    graphics: &Graphics,
    argc: i32,
    argv: *const *const c_char,
) -> i32 {
    // The C launcher lends its process arguments and the real linked graphics exports.
    let args = unsafe { crate::ffi::borrow_span(argv, argc.max(0) as usize) };
    let argument = |index| args.get(index).map(|&arg| unsafe { CStr::from_ptr(arg) });
    let selected = argument(1).map_or(b"".as_slice(), CStr::to_bytes);
    let root = argument(2)
        .map(|path| path.to_string_lossy().into_owned().into())
        .or_else(|| std::env::var_os("RSF_SR_VENDOR_ROOT"));
    let mut ran = 0;
    let mut skipped = 0;
    let mut failed = 0;
    if matches!(selected, b"" | b"decode" | b"thin" | b"prepare") {
        let Some(d) = Device::hardware(0x10de, true).or_else(|| Device::hardware(0, true)) else {
            println!("SKIP: no D3D11 hardware device");
            return 77;
        };
        let cases: [(&[u8], &dyn Fn()); 3] = [
            (b"decode", &|| decode::decode_case(&d, graphics)),
            (b"thin", &|| decode::thin_case(&d, graphics)),
            (b"prepare", &|| prepare_case(&d, platform)),
        ];
        for (name, case) in cases {
            if selected.is_empty() || selected == name {
                ran += 1;
                failed += i32::from(catch_unwind(AssertUnwindSafe(case)).is_err());
                unsafe { d.context.ClearState() };
            }
        }
        let errors = d.debug_errors();
        println!("debug layer errors: {errors}");
        failed += i32::from(errors != 0);
    }
    if matches!(selected, b"" | b"provider") {
        if let Some(root) = root.filter(|path| !path.is_empty()) {
            match catch_unwind(AssertUnwindSafe(|| provider::run(platform, &root))) {
                Ok(true) => ran += 1,
                Ok(false) => skipped += 1,
                Err(_) => {
                    ran += 1;
                    failed += 1;
                }
            }
        } else {
            println!("SKIP provider: pass <vendor root> or set RSF_SR_VENDOR_ROOT");
            skipped += 1;
        }
    }
    if ran == 0 && skipped == 0 {
        eprintln!("unknown case");
        return 2;
    }
    println!("{ran} case(s), {skipped} skipped, {failed} failure(s)");
    if failed != 0 {
        1
    } else if ran != 0 {
        0
    } else {
        77
    }
}
