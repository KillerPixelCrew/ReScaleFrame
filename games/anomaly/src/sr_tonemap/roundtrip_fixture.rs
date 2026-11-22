// SPDX-License-Identifier: MIT
//! CPU reference cases driven through the production D3D11 round-trip pass.
use super::{
    rsf_anomaly_tonemap_forward as forward, rsf_anomaly_tonemap_inverse as inverse,
    rsf_anomaly_tonemap_unorm8 as unorm8,
};
use std::ffi::c_void;

fn half_to_float(half: u16) -> f32 {
    let sign = u32::from(half & 0x8000) << 16;
    let exponent = u32::from((half >> 10) & 0x1f);
    let mantissa = u32::from(half & 0x3ff);
    if exponent == 0 && mantissa != 0 {
        // The smallest half-precision subnormal is exactly 2^-24.
        return (mantissa as f32 / 16_777_216.0).copysign(f32::from_bits(sign));
    }
    let bits = match exponent {
        0 => sign,
        31 => sign | 0x7f80_0000 | (mantissa << 13),
        _ => sign | ((exponent + 112) << 23) | (mantissa << 13),
    };
    f32::from_bits(bits)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_float_to_half(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let magnitude = bits & 0x7fff_ffff;
    if magnitude >= 0x7f80_0000 {
        return sign | 0x7c00 | if magnitude > 0x7f80_0000 { 0x200 } else { 0 };
    }
    if magnitude >= 0x477f_f000 {
        return sign | 0x7c00;
    }
    if magnitude < 0x3880_0000 {
        return sign | (value.abs() * 16_777_216.0).round_ties_even() as u16;
    }
    let rebased = magnitude - (112 << 23);
    let rounded = rebased + 0xfff + ((rebased >> 13) & 1);
    sign | (rounded >> 13) as u16
}

type Run = unsafe extern "C" fn(*mut c_void, *const u16, u32, u32, *mut u8, u32, u32) -> bool;

struct Pass {
    owner: *mut c_void,
    run: Run,
}

impl Pass {
    fn evaluate(&self, data: &[[u16; 4]], input: [u32; 2], output: [u32; 2]) -> Vec<u8> {
        let [w, h] = input;
        let [ow, oh] = output;
        assert_eq!(data.len(), (w * h) as usize);
        let mut bytes = vec![0; (ow * oh * 4) as usize];
        assert!(unsafe {
            (self.run)(
                self.owner,
                data.as_ptr().cast(),
                w,
                h,
                bytes.as_mut_ptr(),
                ow,
                oh,
            )
        });
        bytes
    }
}

fn every_half(pass: &Pass) {
    let input: Vec<_> = (0..=u16::MAX).map(|bits| [bits; 4]).collect();
    let output = pass.evaluate(&input, [256, 256], [256, 256]);
    let (mut worst, mut exact, mut nans) = (0_i32, 0, 0);
    for (bits, pixel) in (0..=u16::MAX).zip(output.chunks_exact(4)) {
        let y = half_to_float(bits);
        let expected = unorm8(f64::from(y));
        nans += usize::from(y.is_nan());
        for &value in pixel {
            let diff = (i32::from(value) - expected).abs();
            worst = worst.max(diff);
            exact += usize::from(diff == 0);
        }
    }
    println!(
        "every FP16 pattern: {} channels, {exact} exact, worst {worst} code value(s), {nans} NaN patterns",
        output.len()
    );
    assert!(worst <= 1);
}

fn stock_curve(pass: &Pass) {
    const MID: usize = 256 * 256 / 2;
    let x: Vec<_> = (0..MID * 2)
        .map(|i| {
            if i < MID {
                2.0 * i as f64 / MID as f64
            } else {
                let start = 2.0_f64.log10();
                10.0_f64.powf(start + (4.0 - start) * (i - MID) as f64 / (MID - 1) as f64)
            }
        })
        .collect();
    let input: Vec<_> = x
        .iter()
        .map(|&x| {
            let y = rsf_anomaly_sr_float_to_half(forward(x) as f32);
            [y, y, y, rsf_anomaly_sr_float_to_half(1.0)]
        })
        .collect();
    let output = pass.evaluate(&input, [256, 256], [256, 256]);
    let (mut worst, mut exact, mut worst_x) = (0, 0, 0.0);
    for (&x, pixel) in x.iter().zip(output.chunks_exact(4)) {
        let diff = (i32::from(pixel[0]) - unorm8(forward(x))).abs();
        if diff > worst {
            worst = diff;
            worst_x = x;
        }
        exact += usize::from(diff == 0);
        assert_eq!(pixel[3], 255);
    }
    println!(
        "stock curve x in [0, 1e4]: {} samples, {exact} exact, worst {worst} code value(s) at x={worst_x:.6}",
        x.len()
    );
    assert!(worst <= 1);

    let mut value = 1e-6;
    let mut worst_inverse = 0.0_f64;
    while value <= 20.0 {
        worst_inverse = worst_inverse.max((inverse(forward(value)) - value).abs() / value);
        value *= 1.01;
    }
    println!("float64 f^-1(f(x)) relative error up to x=20: {worst_inverse:.3e}");
    assert!(worst_inverse < 1e-12);
}

fn nearest_mapping(pass: &Pass) {
    let input: Vec<_> = (1..=16)
        .map(|i| {
            let value = forward(inverse(f64::from(i) / 17.0));
            [
                rsf_anomaly_sr_float_to_half(value as f32),
                0,
                0,
                rsf_anomaly_sr_float_to_half(1.0),
            ]
        })
        .collect();
    let output = pass.evaluate(&input, [4, 4], [8, 8]);
    for (i, pixel) in output.chunks_exact(4).enumerate() {
        let source = (i / 8 / 2) * 4 + (i % 8 / 2);
        assert!((i32::from(pixel[0]) - unorm8((source + 1) as f64 / 17.0)).abs() <= 1);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_roundtrip_numerics(owner: *mut c_void, run: Run) -> i32 {
    let pass = Pass { owner, run };
    [every_half, stock_curve, nearest_mapping]
        .into_iter()
        .filter(|exercise| std::panic::catch_unwind(|| exercise(&pass)).is_err())
        .count() as i32
}

#[repr(C)]
struct UpscaleReadback {
    input: [u32; 2],
    output: [u32; 2],
    source_colour: *const u16,
    source_position: *const u16,
    source_distort: *const u8,
    colour: *const u8,
    position: *const u8,
    distort: *const u8,
    depth: *const u8,
    twin_depth: *const u8,
}

const _: () = assert!(size_of::<UpscaleReadback>() == 80);

fn bilinear([cx, cy]: [f64; 2], [rw, rh]: [usize; 2], sample: impl Fn(usize) -> f64) -> f64 {
    let (bx, by) = (cx.floor(), cy.floor());
    let (wx, wy) = (cx - bx, cy - by);
    let clamp = |coordinate: f64, n| coordinate.clamp(0.0, (n - 1) as f64) as usize;
    let (x0, x1) = (clamp(bx, rw), clamp(bx + 1.0, rw));
    let (y0, y1) = (clamp(by, rh), clamp(by + 1.0, rh));
    let (tl, tr) = (sample(y0 * rw + x0), sample(y0 * rw + x1));
    let (bl, br) = (sample(y1 * rw + x0), sample(y1 * rw + x1));
    let top = tl + (tr - tl) * wx;
    let bottom = bl + (br - bl) * wx;
    top + (bottom - top) * wy
}

// The device harness validates the size of every span before lending the record.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_sr_upscale_numerics(readback: &UpscaleReadback) -> i32 {
    let result = std::panic::catch_unwind(|| {
        let [rw, rh] = readback.input.map(|n| n as usize);
        let [ow, oh] = readback.output.map(|n| n as usize);
        let source = rw * rh;
        let output = ow * oh;
        let bytes = |ptr, length| unsafe { std::slice::from_raw_parts(ptr, length) };
        let in_colour = unsafe { std::slice::from_raw_parts(readback.source_colour, source * 4) };
        let in_position =
            unsafe { std::slice::from_raw_parts(readback.source_position, source * 4) };
        let in_distort = bytes(readback.source_distort, source * 4);
        let colour = bytes(readback.colour, output * 4);
        let position = bytes(readback.position, output * 8);
        let distort = bytes(readback.distort, output * 4);
        let depth = bytes(readback.depth, output * 4);
        let twin_depth = bytes(readback.twin_depth, source * 4);
        let (
            mut worst_colour,
            mut worst_distort,
            mut alpha_bad,
            mut position_bad,
            mut depth_bad,
            mut stencil_bad,
        ) = (0_i32, 0_i32, 0, 0, 0, 0);
        for y in 0..oh {
            for x in 0..ow {
                let o = y * ow + x;
                let s = (y * rh / oh).min(rh - 1) * rw + (x * rw / ow).min(rw - 1);
                let cx = (x as f64 + 0.5) * rw as f64 / ow as f64 - 0.5;
                let cy = (y as f64 + 0.5) * rh as f64 / oh as f64 - 0.5;
                for c in 0..3 {
                    let (mut best, mut best_d) = (255_i32, 255_i32);
                    // Allow float division to land on either side of a source centre.
                    for e in [0.0, 1e-4, -1e-4] {
                        let xy = [cx + e, cy + e];
                        let size = [rw, rh];
                        let linear = bilinear(xy, size, |i| {
                            let y = half_to_float(in_colour[i * 4 + c]);
                            inverse(f64::from(y.clamp(0.0, 65504.0)))
                        });
                        let d = bilinear(xy, size, |i| f64::from(in_distort[i * 4 + c]) / 255.0);
                        best = best
                            .min((i32::from(colour[o * 4 + c]) - unorm8(forward(linear))).abs());
                        best_d = best_d.min((i32::from(distort[o * 4 + c]) - unorm8(d)).abs());
                    }
                    worst_colour = worst_colour.max(best);
                    worst_distort = worst_distort.max(best_d);
                }
                let a = half_to_float(in_colour[s * 4 + 3]);
                alpha_bad +=
                    usize::from((i32::from(colour[o * 4 + 3]) - unorm8(f64::from(a))).abs() > 1);
                for c in 0..4 {
                    let start = o * 8 + c * 2;
                    let got = u16::from_le_bytes(position[start..start + 2].try_into().unwrap());
                    position_bad += usize::from(got != in_position[s * 4 + c]);
                }
                // Standard D24S8 stores depth in the low three bytes.
                depth_bad += usize::from(depth[o * 4..o * 4 + 3] != twin_depth[s * 4..s * 4 + 3]);
                stencil_bad += usize::from(depth[o * 4 + 3] != 0);
            }
        }
        println!(
            "upscale {rw}x{rh} -> {ow}x{oh}: colour worst {worst_colour} code(s) vs float64 bilinear in f^-1 space, generic1 worst {worst_distort}, alpha mismatches {alpha_bad}, position bit mismatches {position_bad}, depth mismatches {depth_bad}, stencil not cleared {stencil_bad}"
        );
        assert!(worst_colour <= 1 && worst_distort <= 1);
        assert_eq!(
            (alpha_bad, position_bad, depth_bad, stencil_bad),
            (0, 0, 0, 0)
        );
    });
    i32::from(result.is_err())
}
