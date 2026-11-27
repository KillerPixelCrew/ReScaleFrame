// SPDX-License-Identifier: MIT
//! FSR1's render planning, one table for every graphics API that serves the Runtime's EASU pass
//! (the D3D11 SR pipeline and the D3D12 SR path).

/// FSR1's published render scale per quality: native, quality, balanced, performance, ultra
/// performance, ultra quality.
pub(crate) const RATIOS: [f32; 6] = [1.0, 1.5, 1.7, 2.0, 3.0, 1.3];

/// The render extent FSR1 plans for `output` at `quality`; an unknown quality plans native.
pub(crate) fn plan(output: [u32; 2], quality: u32) -> [u32; 2] {
    let ratio = RATIOS.get(quality as usize).copied().unwrap_or(1.0);
    output.map(|extent| ((extent as f32 / ratio) as u32).max(1))
}

#[cfg(test)]
mod tests {
    #[test]
    fn fsr1_plans_its_published_ratios() {
        assert_eq!(super::plan([3840, 2160], 0), [3840, 2160]);
        assert_eq!(super::plan([3840, 2160], 1), [2560, 1440]);
        assert_eq!(super::plan([1280, 720], 2), [752, 423]);
        assert_eq!(super::plan([3840, 2160], 3), [1920, 1080]);
        assert_eq!(super::plan([3840, 2160], 4), [1280, 720]);
        assert_eq!(super::plan([1, 1], 4), [1, 1]);
        assert_eq!(super::plan([1300, 1300], 5), [1000, 1000]);
    }
}
