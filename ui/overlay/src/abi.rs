//! The Rust side of `include/rescaleframe/overlay.h`.
//! Its records are the generated ones shared with the Runtime.
//! The layout assertions pin the contract against the C header on 64-bit targets.

/// Result code returned by the fallible entry points.
pub type RsfOverlayResult = i32;

/// The call did what it was asked to.
pub const RSF_OVERLAY_OK: RsfOverlayResult = 0;
/// A null pointer, or a struct that is shorter than this build's version of it.
pub const RSF_OVERLAY_ERROR_INVALID_ARGUMENT: RsfOverlayResult = -1;
/// The caller compiled against a different `RSF_OVERLAY_ABI_VERSION`.
pub const RSF_OVERLAY_ERROR_ABI_MISMATCH: RsfOverlayResult = -2;
/// A panic was caught at the boundary. The handle is only good for destruction afterwards.
pub const RSF_OVERLAY_ERROR_PANICKED: RsfOverlayResult = -3;

/// Quality level as it crosses the boundary.
pub type RsfOverlayQuality = u32;

/// Render at output resolution.
pub const RSF_OVERLAY_QUALITY_NATIVE: RsfOverlayQuality = 0;
/// Quality preset.
pub const RSF_OVERLAY_QUALITY_QUALITY: RsfOverlayQuality = 1;
/// Balanced preset.
pub const RSF_OVERLAY_QUALITY_BALANCED: RsfOverlayQuality = 2;
/// Performance preset.
pub const RSF_OVERLAY_QUALITY_PERFORMANCE: RsfOverlayQuality = 3;
/// Ultra performance preset.
pub const RSF_OVERLAY_QUALITY_ULTRA_PERFORMANCE: RsfOverlayQuality = 4;

/// Left mouse button, in `rsf_overlay_input::mouse_buttons`.
pub const RSF_OVERLAY_MOUSE_LEFT: u32 = 0x1;
/// Right mouse button.
pub const RSF_OVERLAY_MOUSE_RIGHT: u32 = 0x2;
/// Middle mouse button.
pub const RSF_OVERLAY_MOUSE_MIDDLE: u32 = 0x4;

pub(crate) use rsf_game_sys::{
    RSF_OVERLAY_SAVED_CLEAR, RSF_OVERLAY_SAVED_PASTE, RSF_OVERLAY_SAVED_ROW_LIMIT,
    RSF_OVERLAY_SAVED_SET, RSF_OVERLAY_SAVED_TEXT, RSF_OVERLAY_SAVED_TOGGLE,
};

/// Version of the panel ABI implemented by this build.
pub use rsf_game_sys::RSF_OVERLAY_ABI_VERSION;
/// A host option whose value is 0 or 1.
pub use rsf_game_sys::RSF_OVERLAY_HOST_OPTION_TOGGLE;
/// XeSS Ultra Quality preset.
pub use rsf_game_sys::RSF_OVERLAY_QUALITY_ULTRA_QUALITY;
/// An adapter option borrowed for the frame call.
pub use rsf_game_sys::rsf_overlay_host_option as RsfOverlayHostOption;
/// Settings and actions requested by the panel this frame.
pub use rsf_game_sys::rsf_overlay_intent as RsfOverlayIntent;
/// The frame status record defined in overlay.h.
pub use rsf_game_sys::rsf_overlay_stats as RsfOverlayStats;
pub(crate) use rsf_game_sys::{
    RSF_OVERLAY_FRAME_LIMIT_VRR, RSF_OVERLAY_SR_CHANGE_REFUSED,
    RSF_OVERLAY_SR_QUALITY_STANDARD_MASK,
};

/// One frame of input, in physical pixels.
pub use rsf_game_sys::rsf_overlay_input as RsfOverlayInput;
/// One draw call; indices are call-local and `vertex_offset` is added as `BaseVertexLocation`.
pub use rsf_game_sys::rsf_overlay_draw_call as RsfOverlayDrawCall;
/// The triangles for one frame, owned by the overlay until the next frame call.
pub use rsf_game_sys::rsf_overlay_draw_data as RsfOverlayDrawData;
/// A change to the overlay's texture atlas, borrowed until the next frame call.
pub use rsf_game_sys::rsf_overlay_texture_update as RsfOverlayTextureUpdate;
/// One vertex in egui's layout; `color` is premultiplied RGBA with red in the low byte.
pub use rsf_game_sys::rsf_overlay_vertex as RsfOverlayVertex;
#[cfg(all(test, target_pointer_width = "64"))]
mod tests {
    use super::*;
    use core::mem::offset_of;

    /// The offsets a C compiler produces for these declarations on a 64-bit target. Written out
    /// rather than derived, because deriving them from the Rust structs would test nothing: the
    /// point is that the two agree, and only one of them is in this file.
    #[test]
    fn stats_matches_the_header_layout() {
        // ABI 2 held 124 bytes of fields padded to 128 by the eight byte alignment the two
        // pointers impose, so ABI 3's first appended field lands in that padding at 124 rather
        // than after it. That is exactly why the padding is written down here.
        assert_eq!(size_of::<RsfOverlayStats>(), 440);
        assert_eq!(size_of::<RsfOverlayHostOption>(), 40);
        assert_eq!(offset_of!(RsfOverlayStats, backend_name), 16);
        assert_eq!(offset_of!(RsfOverlayStats, refusal_reason), 24);
        assert_eq!(offset_of!(RsfOverlayStats, render_width), 32);
        assert_eq!(offset_of!(RsfOverlayStats, frames_presented), 48);
        assert_eq!(offset_of!(RsfOverlayStats, last_result), 60);
        assert_eq!(offset_of!(RsfOverlayStats, have_scene_color), 64);
        assert_eq!(offset_of!(RsfOverlayStats, jitter_pixels), 88);
        assert_eq!(offset_of!(RsfOverlayStats, quality), 96);
        assert_eq!(offset_of!(RsfOverlayStats, enabled), 100);
        // Appended in ABI 2. Their offsets are the check that they were appended rather than
        // inserted, which is the one change these structs are not allowed to make.
        assert_eq!(offset_of!(RsfOverlayStats, debug_view_on), 104);
        assert_eq!(offset_of!(RsfOverlayStats, captures_written), 120);
        // Appended in ABI 3, into the padding ABI 2 left behind.
        assert_eq!(offset_of!(RsfOverlayStats, jitter_on), 124);
        assert_eq!(offset_of!(RsfOverlayStats, jitter_available), 128);
        assert_eq!(offset_of!(RsfOverlayStats, backend), 132);
        assert_eq!(offset_of!(RsfOverlayStats, requested_backend), 136);
        assert_eq!(offset_of!(RsfOverlayStats, last_switch_result), 140);
        assert_eq!(offset_of!(RsfOverlayStats, fg_available), 144);
        assert_eq!(offset_of!(RsfOverlayStats, fg_total_presented), 192);
        assert_eq!(
            offset_of!(RsfOverlayStats, application_presented_frames),
            200
        );
        assert_eq!(offset_of!(RsfOverlayStats, show_performance_hud), 224);
        // Appended in ABI 8.
        assert_eq!(offset_of!(RsfOverlayStats, frame_limit_us), 232);
        assert_eq!(offset_of!(RsfOverlayStats, display_refresh_mhz), 236);
        assert_eq!(offset_of!(RsfOverlayStats, fg_backend), 240);
        assert_eq!(offset_of!(RsfOverlayStats, fg_selection_result), 252);
        // Appended in ABI 10.
        assert_eq!(offset_of!(RsfOverlayStats, sr_backend_choices), 256);
        assert_eq!(offset_of!(RsfOverlayStats, dlss_preset), 260);
        assert_eq!(offset_of!(RsfOverlayStats, dlss_preset_available), 264);
        assert_eq!(offset_of!(RsfOverlayStats, fg_mfg_unlock_status), 268);
        assert_eq!(offset_of!(RsfOverlayStats, sr_quality_locked), 272);
        assert_eq!(offset_of!(RsfOverlayStats, host_option_count), 276);
        assert_eq!(offset_of!(RsfOverlayStats, host_options), 280);
        assert_eq!(offset_of!(RsfOverlayStats, sharpening_available), 288);
        assert_eq!(offset_of!(RsfOverlayStats, sharpening_mode), 292);
        assert_eq!(offset_of!(RsfOverlayStats, sharpening_sharpness), 296);
        assert_eq!(offset_of!(RsfOverlayStats, sharpening_mas), 300);
        assert_eq!(
            offset_of!(RsfOverlayStats, sharpening_motion_sharpness),
            304
        );
        assert_eq!(
            offset_of!(RsfOverlayStats, sharpening_motion_threshold),
            308
        );
        assert_eq!(
            offset_of!(RsfOverlayStats, sharpening_motion_scale_limit),
            312
        );
        assert_eq!(
            offset_of!(RsfOverlayStats, sharpening_contrast_enabled),
            316
        );
        assert_eq!(offset_of!(RsfOverlayStats, sharpening_contrast), 320);
        assert_eq!(offset_of!(RsfOverlayStats, sharpening_depth_scale), 324);
        assert_eq!(offset_of!(RsfOverlayStats, sharpening_depth_bias), 328);
        assert_eq!(offset_of!(RsfOverlayStats, sharpening_clamp_output), 332);
        assert_eq!(offset_of!(RsfOverlayStats, sharpening_debug), 336);
        // Appended in ABI 16, into the padding the host option pointer leaves at the end.
        assert_eq!(offset_of!(RsfOverlayStats, ui_preferences), 340);
        assert_eq!(offset_of!(RsfOverlayStats, latency_pacing_owner), 344);
        assert_eq!(offset_of!(RsfOverlayStats, latency_reserved), 348);
        assert_eq!(offset_of!(RsfOverlayStats, latency_paced_frames), 352);
        assert_eq!(offset_of!(RsfOverlayStats, pcl_available), 360);
        assert_eq!(offset_of!(RsfOverlayStats, input_receipts), 368);
        assert_eq!(offset_of!(RsfOverlayStats, pcl_routed), 392);
        assert_eq!(offset_of!(RsfOverlayStats, pcl_failed), 400);
        assert_eq!(offset_of!(RsfOverlayStats, pcl_unmatched), 408);
        assert_eq!(offset_of!(RsfOverlayStats, sr_quality_choices), 416);
        assert_eq!(offset_of!(RsfOverlayStats, sr_change_id), 420);
        assert_eq!(offset_of!(RsfOverlayStats, sr_change_status), 424);
    }

    #[test]
    fn the_other_structs_match_the_header_layout() {
        assert_eq!(size_of::<RsfOverlayIntent>(), 216);
        assert_eq!(offset_of!(RsfOverlayIntent, sr_change_id), 200);
        assert_eq!(offset_of!(RsfOverlayIntent, close_requested), 188);
        assert_eq!(offset_of!(RsfOverlayIntent, ui_preferences_changed), 192);
        assert_eq!(offset_of!(RsfOverlayIntent, ui_preferences), 196);
        assert_eq!(offset_of!(RsfOverlayIntent, dlss_preset_changed), 116);
        assert_eq!(offset_of!(RsfOverlayIntent, host_option_changed), 124);
        assert_eq!(offset_of!(RsfOverlayIntent, sharpening_changed), 136);
        assert_eq!(offset_of!(RsfOverlayIntent, sharpening_mode), 140);
        assert_eq!(offset_of!(RsfOverlayIntent, sharpening_sharpness), 144);
        assert_eq!(offset_of!(RsfOverlayIntent, sharpening_mas), 148);
        assert_eq!(
            offset_of!(RsfOverlayIntent, sharpening_motion_sharpness),
            152
        );
        assert_eq!(
            offset_of!(RsfOverlayIntent, sharpening_motion_threshold),
            156
        );
        assert_eq!(
            offset_of!(RsfOverlayIntent, sharpening_motion_scale_limit),
            160
        );
        assert_eq!(
            offset_of!(RsfOverlayIntent, sharpening_contrast_enabled),
            164
        );
        assert_eq!(offset_of!(RsfOverlayIntent, sharpening_contrast), 168);
        assert_eq!(offset_of!(RsfOverlayIntent, sharpening_depth_scale), 172);
        assert_eq!(offset_of!(RsfOverlayIntent, sharpening_depth_bias), 176);
        assert_eq!(offset_of!(RsfOverlayIntent, sharpening_clamp_output), 180);
        assert_eq!(offset_of!(RsfOverlayIntent, sharpening_debug), 184);

        assert_eq!(offset_of!(RsfOverlayIntent, dlss_preset), 120);
        assert_eq!(offset_of!(RsfOverlayIntent, fg_backend_changed), 108);
        assert_eq!(offset_of!(RsfOverlayIntent, frame_limit_changed), 100);
        assert_eq!(offset_of!(RsfOverlayIntent, fg_changed), 72);
        assert_eq!(offset_of!(RsfOverlayIntent, start_requested), 24);
        assert_eq!(offset_of!(RsfOverlayIntent, jitter_changed), 56);
        assert_eq!(offset_of!(RsfOverlayIntent, backend_changed), 64);
        assert_eq!(offset_of!(RsfOverlayIntent, backend), 68);
        assert_eq!(size_of::<RsfOverlayInput>(), 40);
        assert_eq!(offset_of!(RsfOverlayInput, startup_hint_alpha), 36);
        assert_eq!(size_of::<RsfOverlayVertex>(), 20);
        assert_eq!(offset_of!(RsfOverlayVertex, color), 16);

        assert_eq!(size_of::<RsfOverlayDrawCall>(), 40);
        assert_eq!(offset_of!(RsfOverlayDrawCall, texture_id), 32);

        assert_eq!(size_of::<RsfOverlayDrawData>(), 56);
        assert_eq!(offset_of!(RsfOverlayDrawData, vertices), 8);
        assert_eq!(offset_of!(RsfOverlayDrawData, indices), 24);
        assert_eq!(offset_of!(RsfOverlayDrawData, calls), 40);

        assert_eq!(size_of::<RsfOverlayTextureUpdate>(), 40);
        assert_eq!(offset_of!(RsfOverlayTextureUpdate, pixels), 24);
        assert_eq!(offset_of!(RsfOverlayTextureUpdate, is_whole_texture), 32);
    }
}
