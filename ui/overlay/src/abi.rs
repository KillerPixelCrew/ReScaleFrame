//! The Rust side of `include/rescaleframe/overlay.h`.
//! The frame status, option and intent use generated records shared with the Runtime.
//! The layout assertions pin the contract against the C header on 64-bit targets.

#[path = "../../../sdk/game/src/sharpening_defaults.rs"]
pub(crate) mod sharpening_defaults;

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

#[path = "wire.rs"]
mod wire;
pub(crate) use wire::{
    RSF_OVERLAY_SAVED_CLEAR, RSF_OVERLAY_SAVED_PASTE, RSF_OVERLAY_SAVED_ROW_LIMIT,
    RSF_OVERLAY_SAVED_SET, RSF_OVERLAY_SAVED_TEXT, RSF_OVERLAY_SAVED_TOGGLE,
};

/// Version of the panel ABI implemented by this build.
pub use wire::RSF_OVERLAY_ABI_VERSION;
/// XeSS Ultra Quality preset.
pub use wire::RSF_OVERLAY_QUALITY_ULTRA_QUALITY;
/// An adapter option borrowed for the frame call.
pub use wire::rsf_overlay_host_option as RsfOverlayHostOption;
/// Settings and actions requested by the panel this frame.
pub use wire::rsf_overlay_intent as RsfOverlayIntent;
/// The frame status record defined in overlay.h.
pub use wire::rsf_overlay_stats as RsfOverlayStats;
pub(crate) use wire::{
    RSF_OVERLAY_FRAME_LIMIT_VRR, RSF_OVERLAY_SR_CHANGE_REFUSED,
    RSF_OVERLAY_SR_QUALITY_STANDARD_MASK,
};

/// One frame of input, in physical pixels. Mirrors `rsf_overlay_input`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RsfOverlayInput {
    /// `sizeof(rsf_overlay_input)` as the caller knows it.
    pub struct_size: u32,
    /// Mouse x in the presented image's coordinates.
    pub mouse_x: f32,
    /// Mouse y in the presented image's coordinates.
    pub mouse_y: f32,
    /// Held mouse buttons, as `RSF_OVERLAY_MOUSE_*` bits.
    pub mouse_buttons: u32,
    /// Wheel movement since the previous frame.
    pub scroll_delta: f32,
    /// Width of the presented image.
    pub display_width: u32,
    /// Height of the presented image.
    pub display_height: u32,
    /// Seconds since the previous frame.
    pub delta_seconds: f32,
    /// Zero draws only the optional startup hint, without discarding widget state.
    pub visible: u32,
    /// Host-timed startup hint opacity, clamped to 0..1. Appended in ABI 5.
    pub startup_hint_alpha: f32,
}

/// One vertex, in the layout egui produces. Mirrors `rsf_overlay_vertex`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RsfOverlayVertex {
    /// Position x, in physical pixels.
    pub x: f32,
    /// Position y, in physical pixels.
    pub y: f32,
    /// Texture coordinate u.
    pub u: f32,
    /// Texture coordinate v.
    pub v: f32,
    /// Premultiplied RGBA, red in the low byte, so the bytes in memory read R, G, B, A on a
    /// little-endian target. That is what `DXGI_FORMAT_R8G8B8A8_UNORM` expects.
    pub color: u32,
}

/// One draw call. Mirrors `rsf_overlay_draw_call`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RsfOverlayDrawCall {
    /// First index of this call in the shared index buffer.
    pub index_offset: u32,
    /// How many indices to draw.
    pub index_count: u32,
    /// Value to add to every index, as `BaseVertexLocation` does. Indices are call-local.
    pub vertex_offset: u32,
    /// Scissor rectangle x, in physical pixels.
    pub clip_x: u32,
    /// Scissor rectangle y, in physical pixels.
    pub clip_y: u32,
    /// Scissor rectangle width, in physical pixels.
    pub clip_width: u32,
    /// Scissor rectangle height, in physical pixels.
    pub clip_height: u32,
    /// Which texture to bind, matching an id from a texture update.
    pub texture_id: u64,
}

/// The triangles for one frame. Mirrors `rsf_overlay_draw_data`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RsfOverlayDrawData {
    /// `sizeof(rsf_overlay_draw_data)`, written by the overlay.
    pub struct_size: u32,
    /// Vertices, owned by the overlay and valid until the next frame call. Null when empty.
    pub vertices: *const RsfOverlayVertex,
    /// How many vertices.
    pub vertex_count: u32,
    /// Indices, owned by the overlay and valid until the next frame call. Null when empty.
    pub indices: *const u32,
    /// How many indices.
    pub index_count: u32,
    /// Draw calls, owned by the overlay and valid until the next frame call. Null when empty.
    pub calls: *const RsfOverlayDrawCall,
    /// How many draw calls.
    pub call_count: u32,
}

impl Default for RsfOverlayDrawData {
    fn default() -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            vertices: core::ptr::null(),
            vertex_count: 0,
            indices: core::ptr::null(),
            index_count: 0,
            calls: core::ptr::null(),
            call_count: 0,
        }
    }
}

/// A change to the overlay's texture atlas. Mirrors `rsf_overlay_texture_update`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RsfOverlayTextureUpdate {
    /// Which texture this patches.
    pub id: u64,
    /// Destination x of the patch.
    pub x: u32,
    /// Destination y of the patch.
    pub y: u32,
    /// Patch width.
    pub width: u32,
    /// Patch height.
    pub height: u32,
    /// Tightly packed RGBA bytes, `width * height * 4` of them, borrowed until the next frame call.
    pub pixels: *const u8,
    /// Non-zero when the destination has to be created or recreated at this size first.
    pub is_whole_texture: u32,
}

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
