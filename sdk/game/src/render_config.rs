// SPDX-License-Identifier: MIT
use crate::rsf_game_render_config;

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
