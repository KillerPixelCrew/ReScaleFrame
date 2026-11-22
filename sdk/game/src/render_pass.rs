// SPDX-License-Identifier: MIT
use crate::rsf_game_render_pass;
use core::ptr::null_mut;

impl rsf_game_render_pass {
    /// Copies frame identity, camera and other scalar facts for use after the callback lease.
    #[must_use]
    pub fn without_resources(&self) -> Self {
        Self {
            color_input: null_mut(),
            color_output: null_mut(),
            depth: null_mut(),
            motion: null_mut(),
            exposure: null_mut(),
            color_output_readable: null_mut(),
            ui_input: null_mut(),
            swapchain: null_mut(),
            scene_surface: null_mut(),
            sampled_texture: null_mut(),
            native_frame_context: null_mut(),
            temporal_mask: null_mut(),
            ..*self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_pass_clears_every_borrowed_pointer_and_preserves_identity() {
        let borrowed = core::ptr::dangling_mut::<u8>().cast();
        // The ABI record has integer/float fields and raw pointers, all valid when zeroed.
        let mut pass: rsf_game_render_pass = unsafe { core::mem::zeroed() };
        pass.color_input = borrowed;
        pass.color_output = borrowed;
        pass.depth = borrowed;
        pass.motion = borrowed;
        pass.exposure = borrowed;
        pass.color_output_readable = borrowed;
        pass.ui_input = borrowed;
        pass.swapchain = borrowed;
        pass.scene_surface = borrowed;
        pass.sampled_texture = borrowed;
        pass.native_frame_context = borrowed;
        pass.temporal_mask = borrowed;
        pass.native_frame = 37;
        pass.source_frame_id = 91;
        pass.resource_generation = 4;
        pass.camera_valid = 1;
        pass.camera.vertical_fov_radians = 0.66;

        let retained = pass.without_resources();
        let pointers = [
            retained.color_input,
            retained.color_output,
            retained.depth,
            retained.motion,
            retained.exposure,
            retained.color_output_readable,
            retained.ui_input,
            retained.swapchain,
            retained.scene_surface,
            retained.sampled_texture,
            retained.native_frame_context,
            retained.temporal_mask,
        ];
        assert!(pointers.iter().all(|pointer| pointer.is_null()));
        assert_eq!(pass.native_frame_context, borrowed);
        assert_eq!(retained.native_frame, 37);
        assert_eq!(retained.source_frame_id, 91);
        assert_eq!(retained.resource_generation, 4);
        assert_eq!(retained.camera_valid, 1);
        assert_eq!(retained.camera.vertical_fov_radians, 0.66);
    }
}
