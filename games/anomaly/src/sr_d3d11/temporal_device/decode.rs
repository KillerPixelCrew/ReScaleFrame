// SPDX-License-Identifier: MIT
use super::abi::*;
use super::*;
use crate::sr_pass::build_pass;
use crate::sr_stage_abi::rsf_anomaly_sr_jitter_state as Jitter;
use crate::sr_temporal::motion_decode_params;
use rsf_game_sys::rsf_game_render_pass;
use rsf_game_sys::sr_abi::rsf_motion_decode_params;
use std::ptr::{NonNull, null_mut};

struct Decode<'a> {
    handle: NonNull<rsf_motion_decode>,
    api: &'a Graphics,
}

impl Decode<'_> {
    fn new<'a>(d: &Device, api: &'a Graphics, [width, height]: [u32; 2]) -> Decode<'a> {
        let mut handle = null_mut();
        let setup = rsf_motion_decode_setup {
            struct_size: size_of::<rsf_motion_decode_setup>() as u32,
            abi_version: RSF_MOTION_DECODE_ABI_VERSION,
            width,
            height,
            output_format: DXGI_FORMAT_R32G32_FLOAT.0 as u32,
            ..Default::default()
        };
        let status = unsafe { api.decode_create.unwrap()(d.device.as_raw(), &setup, &mut handle) };
        assert_eq!(status, 0);
        Decode {
            handle: NonNull::new(handle).unwrap(),
            api,
        }
    }

    fn run(&self, d: &Device, source: &ID3D11Texture2D, params: &rsf_motion_decode_params) {
        let status = unsafe {
            self.api.decode_run.unwrap()(
                self.handle.as_ptr(),
                d.context.as_raw(),
                source.as_raw(),
                params,
            )
        };
        assert_eq!(status, 0);
    }

    fn texture(&self) -> *mut c_void {
        let raw = unsafe { self.api.decode_texture.unwrap()(self.handle.as_ptr()) };
        assert!(!raw.is_null());
        raw
    }
}

impl Drop for Decode<'_> {
    fn drop(&mut self) {
        unsafe { self.api.decode_destroy.unwrap()(self.handle.as_ptr()) };
    }
}

struct Resolve<'a> {
    handle: NonNull<rsf_motion_resolve>,
    api: &'a Graphics,
}

impl Resolve<'_> {
    fn new<'a>(d: &Device, api: &'a Graphics, [width, height]: [u32; 2]) -> Resolve<'a> {
        let mut handle = null_mut();
        let status =
            unsafe { api.resolve_create.unwrap()(d.device.as_raw(), width, height, &mut handle) };
        assert_ne!(status, 0);
        Resolve {
            handle: NonNull::new(handle).unwrap(),
            api,
        }
    }

    fn run(
        &self,
        d: &Device,
        decode: &Decode<'_>,
        depth: &ID3D11Texture2D,
        pass: &rsf_game_render_pass,
        size: [u32; 2],
        jitter: [f32; 2],
    ) -> Vec<f32> {
        let params = rsf_motion_resolve_params {
            struct_size: size_of::<rsf_motion_resolve_params>() as u32,
            clip_to_previous: pass.camera.clip_to_previous_clip,
            decoded_to_pixels: std::array::from_fn(|i| -(size[i] as f32) * pass.motion_to_uv[i]),
            sentinel: motion_decode_params().invalid_value,
            has_sentinel: u32::from(pass.motion_camera_included == 0),
            depth_layer: null_mut(),
            jitter_pixels: jitter,
        };
        let status = unsafe {
            self.api.resolve_run.unwrap()(
                self.handle.as_ptr(),
                d.context.as_raw(),
                decode.texture(),
                depth.as_raw(),
                &params,
            )
        };
        assert_ne!(status, 0);
        let raw = unsafe { self.api.resolve_motion.unwrap()(self.handle.as_ptr()) };
        let motion = unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }.unwrap();
        read::<u16>(d, motion, 2)
            .into_iter()
            .map(half_to_float)
            .collect()
    }
}

impl Drop for Resolve<'_> {
    fn drop(&mut self) {
        unsafe { self.api.resolve_destroy.unwrap()(self.handle.as_ptr()) };
    }
}

fn pass(size: [u32; 2], current: Camera, previous: Camera, jitter: Jitter) -> rsf_game_render_pass {
    // Only descriptors and camera math are exercised here. Distinct nonnull placeholders are never submitted.
    let resources = [0u8; 4];
    let mut input = frame_inputs(size, size, current, previous, jitter);
    [input.color, input.color_output, input.depth, input.motion] =
        std::array::from_fn(|i| std::ptr::from_ref(&resources[i]).cast_mut().cast());
    build_pass(&input).unwrap_or_else(|error| panic!("SR pass refused: {}", error.name()))
}

fn uv_reference([u, v, valid, _]: [f32; 4]) -> Option<[f32; 2]> {
    (valid > 0.5).then_some([u, v])
}

pub(super) fn decode_case(d: &Device, api: &Graphics) {
    let [w, h] = [8, 2];
    let stored = [
        [0., 0., 1., 0.],
        [0.01, -0.02, 1., 0.],
        [0., 0., 0., 0.],
        [0.3, 0.3, 0., 0.],
        [0., 0., 0.5, 0.],
        [0., 0., 0.75, 0.],
        [0.5, 0.25, 1., 0.],
        [0., 0., 0.25, 0.],
    ];
    let halves: Vec<_> = stored
        .into_iter()
        .cycle()
        .take((w * h) as usize)
        .flat_map(|value| value.map(half))
        .collect();
    let size = [w, h];
    let source = d.data_texture(size, HDR, SRV, &halves, w * 8).unwrap();
    let decode = Decode::new(d, api, size);
    let params = motion_decode_params();
    decode.run(d, &source, &params);
    let raw = decode.texture();
    let texture = unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }.unwrap();
    let bytes = read::<f32>(d, texture, 2);
    assert_eq!(bytes.len(), (w * h * 2) as usize);
    let (mut zero, mut sentinels) = (0, 0);
    for (i, sample) in halves.as_chunks::<4>().0.iter().enumerate() {
        let expected = uv_reference(sample.map(half_to_float));
        let got = &bytes[i * 2..i * 2 + 2];
        assert_eq!(got, expected.unwrap_or([params.invalid_value; 2]));
        if i < w as usize {
            zero += usize::from(expected == Some([0.; 2]));
            sentinels += usize::from(expected.is_none());
        }
    }
    assert_eq!((zero, sentinels), (2, 4));
    let mut legacy = params;
    legacy.validity_enabled = 0;
    legacy.zero_means_unwritten = 1;
    decode.run(d, &source, &legacy);
    let values = read::<f32>(d, texture, 2);
    assert_eq!(values.len(), (w * h * 2) as usize);
    assert_eq!(
        [values[0], values[6]],
        [params.invalid_value, half_to_float(half(0.3))]
    );
    legacy.struct_size = (std::mem::offset_of!(rsf_motion_decode_params, zero_means_unwritten)
        + size_of::<u32>()) as u32;
    decode.run(d, &source, &legacy);
    decode.run(d, &source, &params);
    let depth = d
        .data_texture(
            size,
            DXGI_FORMAT_R32_FLOAT,
            SRV,
            &vec![0.9f32; (w * h) as usize],
            w * 4,
        )
        .unwrap();
    let proj = projection(w as f32 / h as f32);
    let jitter = Jitter {
        applied: true,
        render_width: w,
        render_height: h,
        ..Default::default()
    };
    let pass = pass(
        size,
        camera(view_at(0.), proj),
        camera(view_at(0.5), proj),
        jitter,
    );
    let got = Resolve::new(d, api, size).run(d, &decode, &depth, &pass, size, [0.; 2]);
    assert_eq!(got.len(), (w * h * 2) as usize);
    for (i, value) in got.as_chunks::<2>().0.iter().enumerate() {
        let x = i as u32 % w;
        let y = i as u32 / w;
        let expected = match uv_reference(stored[x as usize]) {
            Some([u, v]) => [-f64::from(u) * f64::from(w), -f64::from(v) * f64::from(h)],
            None => camera_motion([x, y], size, 0.9, &pass.camera.clip_to_previous_clip),
        };
        for axis in 0..2 {
            assert!(
                (f64::from(value[axis]) - expected[axis]).abs()
                    <= 2e-3 * expected[axis].abs().max(1.),
                "resolve ({x},{y}) {value:?}, expected {expected:?}"
            );
        }
        if x == 0 {
            assert_eq!(*value, [0.; 2]);
        }
        if x == 2 {
            assert!(value[0].abs() > 0.05);
        }
    }
}

const THIN: [u32; 2] = [96, 48];
const FOV: f64 = 1.2;
const NEAR: f64 = 0.2;
const FAR: f64 = 900.;
const SKY: usize = 0;
const GROUND: usize = 1;
const POLE: usize = 2;
const MOVER: usize = 3;

#[derive(Clone, Copy)]
struct Texel {
    surface: usize,
    depth: f64,
    truth: [f64; 2],
}

fn yaw_view([cx, cy, cz]: [f64; 3], yaw: f64) -> Matrix {
    let (s, c) = yaw.sin_cos();
    [
        c as f32,
        0.,
        s as f32,
        0.,
        0.,
        1.,
        0.,
        0.,
        -s as f32,
        0.,
        c as f32,
        0.,
        (-cx * c + cz * s) as f32,
        -cy as f32,
        (-cx * s - cz * c) as f32,
        1.,
    ]
}

fn rasterize([jx, jy]: [f64; 2], previous: &Matrix, proj: &Matrix) -> Vec<Texel> {
    let [w, h] = THIN.map(f64::from);
    let y_scale = (FOV / 2.).tan().recip();
    let x_scale = y_scale / (w / h);
    let q = FAR / (FAR - NEAR);
    (0..texels(THIN))
        .map(|i| {
            let x = (i % THIN[0] as usize) as f64 + 0.5 - jx;
            let y = (i / THIN[0] as usize) as f64 + 0.5 - jy;
            let dx = (x / w * 2. - 1.) / x_scale;
            let dy = (1. - y / h * 2.) / y_scale;
            let mut surface = SKY;
            let mut best = 1e30;
            let mut take = |kind, depth| {
                if depth > 0. && depth < best {
                    surface = kind;
                    best = depth;
                }
            };
            if dy < 0. {
                take(GROUND, -1.6 / dy);
            }
            if (dx * 14. - 0.9).abs() < 0.15 && dy * 14. > -1.6 && dy * 14. < 6. {
                take(POLE, 14.);
            }
            if (dx * 6. + 1.2).abs() < 0.5 && dy * 6. > -1.6 && dy * 6. < 1.8 {
                take(MOVER, 6.);
            }
            let z = if surface == SKY { 1e6 } else { best };
            let depth = if surface == SKY { 1. } else { q - q * NEAR / z };
            let point = [
                dx * z - if surface == MOVER { 0.6 } else { 0. },
                dy * z,
                z,
                1.,
            ];
            let clip = mul(mul(point, previous), proj);
            Texel {
                surface,
                depth,
                truth: [
                    (clip[0] / clip[3] * 0.5 + 0.5) * w - x,
                    (0.5 - clip[1] / clip[3] * 0.5) * h - y,
                ],
            }
        })
        .collect()
}

fn resolve_thin(
    d: &Device,
    api: &Graphics,
    texels: &[Texel],
    pass: &rsf_game_render_pass,
    write_mover: bool,
    jitter: [f32; 2],
) -> Vec<f32> {
    let [w, h] = THIN;
    let halves: Vec<_> = texels
        .iter()
        .flat_map(|texel| {
            if texel.surface == POLE || (texel.surface == MOVER && write_mover) {
                [
                    half((-texel.truth[0] / f64::from(w)) as f32),
                    half((-texel.truth[1] / f64::from(h)) as f32),
                    half(1.),
                    0,
                ]
            } else {
                [0; 4]
            }
        })
        .collect();
    let depths: Vec<_> = texels.iter().map(|texel| d24(texel.depth)).collect();
    let source = d.data_texture(THIN, HDR, SRV, &halves, w * 8).unwrap();
    let depth = Depth::new(d, THIN, &depths);
    let decode = Decode::new(d, api, THIN);
    decode.run(d, &source, &motion_decode_params());
    Resolve::new(d, api, THIN).run(d, &decode, &depth.texture, pass, THIN, jitter)
}

pub(super) fn thin_case(d: &Device, api: &Graphics) {
    let [w, h] = THIN;
    let [jx, jy] = [0.3125, -0.2037037];
    let previous = yaw_view([-0.35, 0.02, -0.1], -0.0105);
    let proj = projection(w as f32 / h as f32);
    let jitter = Jitter {
        applied: true,
        previous_applied: true,
        render_width: w,
        render_height: h,
        current_x: jx as f32,
        current_y: jy as f32,
        previous_x: -0.4375,
        previous_y: 0.2592593,
        ..Default::default()
    };
    let pass = pass(
        THIN,
        camera(view_at(0.), proj),
        camera(previous, proj),
        jitter,
    );
    let model = rasterize([jx, jy], &previous, &proj);
    let mut counts = [0; 4];
    for texel in &model {
        counts[texel.surface] += 1;
    }
    assert!(
        counts[SKY] > 100
            && counts[GROUND] > 100
            && counts[POLE] >= 12
            && counts[POLE] <= 2 * h
            && counts[MOVER] > 50
    );
    for pass_jitter in [false, true] {
        let jitter = if pass_jitter {
            pass.camera.jitter_pixels
        } else {
            [0.; 2]
        };
        let written = resolve_thin(d, api, &model, &pass, true, jitter);
        let unwritten = resolve_thin(d, api, &model, &pass, false, jitter);
        assert_eq!(
            (written.len(), unwritten.len()),
            (model.len() * 2, model.len() * 2)
        );
        let mut worst = [0f64; 4];
        let (mut edge, mut mover_miss, mut steps) = (0f64, f64::INFINITY, 0);
        let error = |values: &[f32], i: usize, truth: [f64; 2]| {
            (f64::from(values[2 * i]) - truth[0]).hypot(f64::from(values[2 * i + 1]) - truth[1])
        };
        for (i, texel) in model.iter().enumerate() {
            assert!(written[2 * i].is_finite() && written[2 * i + 1].is_finite());
            worst[texel.surface] = worst[texel.surface].max(error(&written, i, texel.truth));
            if texel.surface == MOVER {
                mover_miss = mover_miss.min(error(&unwritten, i, texel.truth));
            }
            if texel.surface == POLE {
                let x = (i % w as usize) as i32;
                for next in [x - 1, x + 1]
                    .into_iter()
                    .filter(|&x| x >= 0 && x < w as i32)
                {
                    let j = i / w as usize * w as usize + next as usize;
                    if matches!(model[j].surface, POLE | MOVER) {
                        continue;
                    }
                    let delta = |axis| {
                        f64::from(written[2 * i + axis] - written[2 * j + axis])
                            - (texel.truth[axis] - model[j].truth[axis])
                    };
                    edge = edge.max(delta(0).hypot(delta(1)));
                    steps += 1;
                }
            }
        }
        let background = if pass_jitter { 0.01 } else { 0.05 };
        println!(
            "thin (jitter passed: {pass_jitter}): {worst:?} px, pole step {edge:.4} px over {steps} edges, unwritten mover misses by {mover_miss:.2} px"
        );
        assert!(worst[POLE] < 0.01 && worst[MOVER] < 0.01);
        assert!(worst[GROUND] < background && worst[SKY] < background + 0.02);
        assert!(steps > 20 && edge < 0.03 && mover_miss > 2.);
    }
}
