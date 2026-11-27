// SPDX-License-Identifier: MIT
use crate::{RSF_SR_DLSS, RSF_SR_FSR2, RSF_SR_FSR3, RSF_SR_FSR4, RSF_SR_SPATIAL, RSF_SR_XESS};

// Both precisions preserve the arithmetic used by their projection convention.
macro_rules! halton {
    ($name:ident, $float:ty) => {
        pub fn $name(mut index: u32, base: u32) -> $float {
            if base < 2 {
                return 0.0;
            }
            let mut value = 0.0;
            let mut fraction = 1.0;
            while index != 0 {
                fraction /= base as $float;
                value += fraction * (index % base) as $float;
                index /= base;
            }
            value
        }
    };
}

halton!(halton_f32, f32);
halton!(halton_f64, f64);

/// The jitter phase count and texture LOD bias a backend's vendor publishes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VendorSampling {
    /// Halton(2,3) samples before the sequence repeats; 1 means no jitter.
    pub phases: u32,
    /// Added to the engine's texture sampler LOD bias.
    pub lod_bias: f32,
}

/// The vendor rule for `backend` rendering `render_width` pixels upscaled to `output_width`, one
/// implementation for every integration (rule5-audit F5). With n = output / render per dimension:
///
/// - FSR2, FSR3, FSR4: `int(8 n^2)` phases from the widths (ffxFsr3UpscalerGetJitterPhaseCount),
///   bias `log2(render / output) - 1` (FidelityFX super-resolution-upscaler.md, "mipBias").
/// - XeSS: at least `8 n^2` phases, bias `log2(render / output)` (XeSS-SR developer guide: 72 phases
///   and -1.585 at Ultra performance, -1.202 at the 2.3x Performance preset).
/// - DLSS: `8 n^2` phases, bias `log2(render / output) - 1` (DLSS programming guide, mip-map bias
///   and jitter phase sections; the -1 confirmed here from integration notes, not NVIDIA's text).
/// - FSR1 and the engine's spatial scaler: no history, so no jitter; bias `log2(render / output)`.
///
/// No backend, an unknown one, or a render extent above the output: 8 phases and no bias.
pub fn vendor_sampling(backend: u32, render_width: u32, output_width: u32) -> VendorSampling {
    const NONE: VendorSampling = VendorSampling {
        phases: 8,
        lod_bias: 0.0,
    };
    if render_width == 0 || render_width > output_width {
        return NONE;
    }
    let ratio = f64::from(output_width) / f64::from(render_width);
    let base = 8.0 * ratio * ratio;
    let at_least = (base - 1e-9).ceil().max(8.0) as u32;
    let scale = (f64::from(render_width) / f64::from(output_width)).log2();
    let (phases, lod_bias) = match backend {
        RSF_SR_FSR2 | RSF_SR_FSR3 | RSF_SR_FSR4 => ((base as u32).max(8), scale - 1.0),
        RSF_SR_DLSS => (at_least, scale - 1.0),
        RSF_SR_XESS => (at_least, scale),
        RSF_SR_SPATIAL => (1, scale),
        _ => return NONE,
    };
    VendorSampling {
        phases,
        lod_bias: lod_bias as f32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(backend: u32, render: u32) -> (u32, f32) {
        let value = vendor_sampling(backend, render, 3840);
        (value.phases, (value.lod_bias * 1000.0).round() / 1000.0)
    }

    #[test]
    fn fidelityfx_matches_its_published_table() {
        // super-resolution-upscaler.md: Quality 18, Balanced 23, Performance 32, Ultra performance 72.
        for backend in [RSF_SR_FSR2, RSF_SR_FSR3, RSF_SR_FSR4] {
            assert_eq!(at(backend, 2560), (18, -1.585));
            assert_eq!(at(backend, 2259).0, 23);
            assert_eq!(at(backend, 1920), (32, -2.0));
            assert_eq!(at(backend, 1280), (72, -2.585));
            assert_eq!(at(backend, 3840), (8, -1.0));
        }
    }

    #[test]
    fn xess_matches_its_guide_examples() {
        assert_eq!(at(RSF_SR_XESS, 1280), (72, -1.585));
        // The XeSS 1.3 Performance preset renders 1/2.3 of the output.
        let performance = vendor_sampling(RSF_SR_XESS, 1000, 2300);
        assert_eq!((performance.lod_bias * 1000.0).round(), -1202.0);
        assert_eq!(at(RSF_SR_XESS, 2259).0, 24);
    }

    #[test]
    fn dlss_spatial_and_no_backend() {
        assert_eq!(at(RSF_SR_DLSS, 2560), (18, -1.585));
        assert_eq!(at(RSF_SR_DLSS, 2259).0, 24);
        assert_eq!(at(RSF_SR_SPATIAL, 2560), (1, -0.585));
        for backend in [0, 7, 40] {
            assert_eq!(at(backend, 2560), (8, 0.0));
        }
        assert_eq!(vendor_sampling(RSF_SR_DLSS, 0, 3840).phases, 8);
        assert_eq!(vendor_sampling(RSF_SR_DLSS, 4000, 3840).lod_bias, 0.0);
    }
}
