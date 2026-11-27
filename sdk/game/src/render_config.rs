// SPDX-License-Identifier: MIT
use crate::rsf_game_render_config;
use crate::sampling::vendor_sampling;

impl rsf_game_render_config {
    /// Initializes the output record before requesting a host render plan.
    pub const fn request() -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            enabled: 0,
            output_width: 0,
            output_height: 0,
            render_width: 0,
            render_height: 0,
            sr_backend: 0,
            jitter_phases: 8,
            lod_bias: 0.0,
        }
    }

    /// A host's plan for `backend` at these extents, with that vendor's jitter phases and LOD
    /// bias (`vendor_sampling`).
    pub fn plan(
        enabled: bool,
        backend: u32,
        [output_width, output_height]: [u32; 2],
        [render_width, render_height]: [u32; 2],
    ) -> Self {
        let sampling = vendor_sampling(backend, render_width, output_width);
        Self {
            enabled: u32::from(enabled),
            output_width,
            output_height,
            render_width,
            render_height,
            sr_backend: backend,
            jitter_phases: sampling.phases,
            lod_bias: sampling.lod_bias,
            ..Self::request()
        }
    }

    /// Returns the host's enabled render extent for the requested output size.
    pub fn checked_render_extent(&self, output: [u32; 2]) -> Option<[u32; 2]> {
        let render = [self.render_width, self.render_height];
        (self.enabled != 0
            && [self.output_width, self.output_height] == output
            && render
                .into_iter()
                .zip(output)
                .all(|(render, output)| (1..=output).contains(&render)))
        .then_some(render)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RSF_SR_DLSS, RSF_SR_SPATIAL};

    #[test]
    fn a_plan_carries_its_backend_and_vendor_sampling() {
        let plan = rsf_game_render_config::plan(true, RSF_SR_DLSS, [3840, 2160], [2560, 1440]);
        assert_eq!((plan.sr_backend, plan.jitter_phases), (RSF_SR_DLSS, 18));
        assert!((plan.lod_bias + 1.585).abs() < 1e-3);
        assert_eq!(plan.checked_render_extent([3840, 2160]), Some([2560, 1440]));
        let spatial =
            rsf_game_render_config::plan(true, RSF_SR_SPATIAL, [3840, 2160], [2560, 1440]);
        assert_eq!(spatial.jitter_phases, 1, "a spatial scaler takes no jitter");
        let native = rsf_game_render_config::plan(false, 0, [1920, 1080], [1920, 1080]);
        assert_eq!((native.jitter_phases, native.lod_bias), (8, 0.0));
        assert_eq!(native.checked_render_extent([1920, 1080]), None);
    }
}
