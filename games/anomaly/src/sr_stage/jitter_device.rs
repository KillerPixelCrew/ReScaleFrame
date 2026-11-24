// SPDX-License-Identifier: MIT
//! Draw the stock motion writer with production projection jitter and PS b13.
use super::math::{JitterSequence, jitter_to_uv, next_jitter};
use crate::motion::{PixelInputs, rsf_anomaly_motion_pack_pixel};
use crate::sr_fixture_device::Device;
use crate::sr_fixture_readback::rsf_anomaly_sr_fixture_readback;
use crate::sr_swap::{engine_mul, jitter_projection};
use rsf_shader_compiler::{Request, compile_system_checked};
use std::ffi::CStr;
use std::mem::MaybeUninit;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::{Interface, s};

const WIDTH: u32 = 96;
const HEIGHT: u32 = 54;
const OUTPUT: [u32; 2] = [144, 81];
const PIXELS: usize = (WIDTH * HEIGHT) as usize;
type Matrix = [f32; 16];
type Result<T> = std::result::Result<T, String>;

const SOURCE: &str = concat!(
    r"
cbuffer Fixture : register(b0)
{
    row_major float4x4 current_full;
    row_major float4x4 previous_full;
};
struct VSOut
{
    float4 position : SV_Position;
    float4 previous_clip : TEXCOORD12;
};
VSOut vs_main(float3 p : POSITION)
{
    VSOut o;
    o.position = mul(float4(p, 1), current_full);
    o.previous_clip = mul(float4(p, 1), previous_full);
    return o;
}
",
    include_str!("../../shaders/core/rsf_core_pixel.h"),
    r"
float4 ps_main(VSOut i) : SV_Target0
{
    return rsf_encode_motion(i.position, i.previous_clip, 1);
}
"
);

fn check(ok: bool, message: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(message.into()) }
}

fn created<T>(status: windows::core::Result<()>, output: Option<T>, name: &str) -> Result<T> {
    status.map_err(|err| format!("{name}: {err}"))?;
    output.ok_or_else(|| format!("{name}: no interface returned"))
}

fn compile_shader(entry: &CStr, target: &CStr) -> Result<Vec<u8>> {
    compile_system_checked(Request {
        source: SOURCE.as_bytes(),
        name: c"rsf_sr_jitter_fixture.hlsl",
        entry,
        target,
        flags: (1 << 11) | (1 << 15), // strictness and optimization level 3
    })
    .map_err(|reason| format!("{}: {reason}", entry.to_string_lossy()))
}

struct Fixture {
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    layout: ID3D11InputLayout,
    vertices: Option<ID3D11Buffer>,
    matrices: Option<ID3D11Buffer>,
    pixel: Option<ID3D11Buffer>,
    target: ID3D11Texture2D,
    staging: ID3D11Texture2D,
    rtv: Option<ID3D11RenderTargetView>,
    raster: ID3D11RasterizerState,
}

impl Fixture {
    fn create(device: &ID3D11Device) -> Result<Self> {
        let vs_code = compile_shader(c"vs_main", c"vs_5_0")?;
        let ps_code = compile_shader(c"ps_main", c"ps_5_0")?;
        let (mut vs, mut ps, mut layout) = (None, None, None);
        let vs = created(
            unsafe {
                device.CreateVertexShader(&vs_code, None::<&ID3D11ClassLinkage>, Some(&mut vs))
            },
            vs,
            "VS",
        )?;
        let ps = created(
            unsafe {
                device.CreatePixelShader(&ps_code, None::<&ID3D11ClassLinkage>, Some(&mut ps))
            },
            ps,
            "PS",
        )?;
        let element = D3D11_INPUT_ELEMENT_DESC {
            SemanticName: s!("POSITION"),
            Format: DXGI_FORMAT_R32G32B32_FLOAT,
            InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
            ..Default::default()
        };
        let layout = created(
            unsafe { device.CreateInputLayout(&[element], &vs_code, Some(&mut layout)) },
            layout,
            "layout",
        )?;
        let buffer = |size, bind: D3D11_BIND_FLAG, data| {
            let desc = D3D11_BUFFER_DESC {
                ByteWidth: size,
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: bind.0 as u32,
                ..Default::default()
            };
            let mut output = None;
            created(
                unsafe { device.CreateBuffer(&desc, data, Some(&mut output)) },
                output,
                "buffer",
            )
        };
        // Static plane tilted in depth, covering the full view.
        let quad: [[f32; 3]; 6] = [
            [-50., -30., 10.],
            [-50., 30., 40.],
            [50., -30., 10.],
            [50., -30., 10.],
            [-50., 30., 40.],
            [50., 30., 40.],
        ];
        let init = D3D11_SUBRESOURCE_DATA {
            pSysMem: quad.as_ptr().cast(),
            ..Default::default()
        };
        let vertices = buffer(
            size_of_val(&quad) as u32,
            D3D11_BIND_VERTEX_BUFFER,
            Some(&init as *const _),
        )?;
        let matrices = buffer(128, D3D11_BIND_CONSTANT_BUFFER, None)?;
        let pixel = buffer(
            crate::motion::PIXEL_BYTES as u32,
            D3D11_BIND_CONSTANT_BUFFER,
            None,
        )?;
        let mut desc = D3D11_TEXTURE2D_DESC {
            Width: WIDTH,
            Height: HEIGHT,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            ..Default::default()
        };
        let mut target = None;
        let target = created(
            unsafe { device.CreateTexture2D(&desc, None, Some(&mut target)) },
            target,
            "motion target",
        )?;
        let mut rtv = None;
        let rtv = created(
            unsafe { device.CreateRenderTargetView(&target, None, Some(&mut rtv)) },
            rtv,
            "rtv",
        )?;
        desc.BindFlags = 0;
        desc.Usage = D3D11_USAGE_STAGING;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let mut staging = None;
        let staging = created(
            unsafe { device.CreateTexture2D(&desc, None, Some(&mut staging)) },
            staging,
            "staging",
        )?;
        let desc = D3D11_RASTERIZER_DESC {
            FillMode: D3D11_FILL_SOLID,
            CullMode: D3D11_CULL_NONE,
            DepthClipEnable: true.into(),
            ..Default::default()
        };
        let mut raster = None;
        let raster = created(
            unsafe { device.CreateRasterizerState(&desc, Some(&mut raster)) },
            raster,
            "raster",
        )?;
        Ok(Self {
            vs,
            ps,
            layout,
            vertices: Some(vertices),
            matrices: Some(matrices),
            pixel: Some(pixel),
            target,
            staging,
            rtv: Some(rtv),
            raster,
        })
    }

    fn draw(
        &self,
        device: &Device,
        current: Matrix,
        previous: Matrix,
        jitter: [f32; 4],
    ) -> Result<Vec<[f32; 3]>> {
        let ctx = &device.context;
        let matrices = [current, previous];
        let mut bytes = MaybeUninit::uninit();
        check(
            rsf_anomaly_motion_pack_pixel(
                &PixelInputs {
                    viewport: [0., 0., WIDTH as f32, HEIGHT as f32],
                    jitter,
                },
                &mut bytes,
            ),
            "pack_pixel",
        )?;
        let bytes = unsafe { bytes.assume_init() };
        let viewport = D3D11_VIEWPORT {
            Width: WIDTH as f32,
            Height: HEIGHT as f32,
            MaxDepth: 1.,
            ..Default::default()
        };
        unsafe {
            ctx.UpdateSubresource(
                self.matrices.as_ref().unwrap(),
                0,
                None,
                matrices.as_ptr().cast(),
                0,
                0,
            );
            ctx.UpdateSubresource(
                self.pixel.as_ref().unwrap(),
                0,
                None,
                bytes.as_ptr().cast(),
                0,
                0,
            );
            ctx.ClearState();
            ctx.ClearRenderTargetView(self.rtv.as_ref().unwrap(), &[-7.; 4]);
            ctx.IASetInputLayout(&self.layout);
            ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            ctx.IASetVertexBuffers(0, 1, Some(&self.vertices), Some(&12), Some(&0));
            ctx.VSSetShader(&self.vs, None);
            ctx.VSSetConstantBuffers(0, Some(std::slice::from_ref(&self.matrices)));
            ctx.PSSetShader(&self.ps, None);
            ctx.PSSetConstantBuffers(13, Some(std::slice::from_ref(&self.pixel)));
            ctx.RSSetState(&self.raster);
            ctx.RSSetViewports(Some(&[viewport]));
            ctx.OMSetRenderTargets(
                Some(std::slice::from_ref(&self.rtv)),
                None::<&ID3D11DepthStencilView>,
            );
            ctx.Draw(6, 0);
        }
        let mut pixels = vec![[0u16; 4]; PIXELS];
        check(
            unsafe {
                rsf_anomaly_sr_fixture_readback(
                    device.device.as_raw(),
                    ctx.as_raw(),
                    self.target.as_raw(),
                    self.staging.as_raw(),
                    pixels.as_mut_ptr().cast(),
                    size_of_val(pixels.as_slice()),
                )
            },
            "readback",
        )?;
        check(device.debug_errors() == 0, "debug-layer errors")?;
        Ok(pixels
            .into_iter()
            .map(|[u, v, valid, _]| [u, v, valid].map(half))
            .collect())
    }
}

use rsf_test_support::device::half_to_f32 as half;

fn run(device: &Device) -> Result<()> {
    check(
        device.level.0 >= D3D_FEATURE_LEVEL_11_0.0,
        "feature level 11",
    )?;
    let fixture = Fixture::create(&device.device)?;
    let [ow, oh] = OUTPUT;
    let (near, far) = (0.2f32, 1000.);
    let h = (1.2f32 * 0.5).tan().recip();
    let (w, q) = (h / (ow as f32 / oh as f32), far / (far - near));
    let projection = [
        w,
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
        -q * near,
        0.,
    ];
    let view = [
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ];
    let full = |x, y| {
        jitter_projection(&projection, x, y, WIDTH, HEIGHT)
            .map(|matrix| engine_mul(&matrix, &view))
            .ok_or("jittered projection")
    };
    let mut sequence = JitterSequence::new();
    let mut state = Default::default();
    let (mut previous_full, mut previous_sample) = ([0.; 16], [0.; 2]);
    let (mut worst, mut control, mut pan, mut pixels) = (0f64, 0f64, 0f64, 0);
    let tolerance = 2e-4;
    for frame in 0..=18 {
        let sample = sequence.next([WIDTH, HEIGHT, ow, oh], 0, true);
        check(sample.phase_count == 18, "18 phases at scale 0.67")?;
        state = next_jitter(&state, false, true, &sample, WIDTH, HEIGHT, sequence.reset);
        let current = full(sample.x, sample.y)?;
        if frame > 0 {
            let uv = jitter_to_uv(&state);
            check(
                uv.previous_u == previous_sample[0] / WIDTH as f32
                    && uv.previous_v == previous_sample[1] / HEIGHT as f32,
                "previous UV",
            )?;
            let jitter = [uv.current_u, uv.current_v, uv.previous_u, uv.previous_v];
            for [u, v, valid] in fixture.draw(device, current, previous_full, jitter)? {
                check(valid == 1., "every pixel covered and valid")?;
                worst = worst.max(f64::from(u.abs().max(v.abs())));
                pixels += 1;
            }
            let du = f64::from(uv.current_u) - f64::from(uv.previous_u);
            let dv = f64::from(uv.current_v) - f64::from(uv.previous_v);
            check(
                du.abs().max(dv.abs()) > 5. * tolerance,
                "consecutive samples differ visibly",
            )?;
            for [u, v, _] in fixture.draw(device, current, previous_full, [0.; 4])? {
                control = control.max((f64::from(u) - du).abs().max((f64::from(v) - dv).abs()));
            }
            let panned = full(previous_sample[0] + 3., previous_sample[1] - 2.)?;
            for [u, v, _] in fixture.draw(device, current, panned, jitter)? {
                pan = pan.max(
                    (f64::from(u) + 3. / f64::from(WIDTH))
                        .abs()
                        .max((f64::from(v) - 2. / f64::from(HEIGHT)).abs()),
                );
            }
        }
        previous_full = current;
        previous_sample = [sample.x, sample.y];
    }
    println!(
        "static scene with jitter: {pixels} pixels over 18 frames, worst |motion| {worst:.2e} UV"
    );
    println!("control without b13 jitter: {control:.2e} UV; pan: {pan:.2e} UV");
    check(pixels == 18 * PIXELS, "pixel count")?;
    check(
        worst < tolerance,
        "static scene under jitter has zero motion",
    )?;
    check(
        control < tolerance,
        "control shows exactly the jitter difference",
    )?;
    check(pan < tolerance, "pan survives the jitter removal")
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_jitter_device_fixture() -> i32 {
    let Some(device) = Device::hardware_or_warp(false) else {
        println!("SKIP: no D3D11 device");
        return 77;
    };
    match run(&device) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("FAILED {error}");
            1
        }
    }
}
