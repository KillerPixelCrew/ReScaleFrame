//! The structures and constants of `include/rescaleframe/overlay.h`, mirrored field for field.
//!
//! Nothing here has behaviour. It exists so the C header has exactly one Rust counterpart, and so
//! a change to the header shows up as a diff in one file rather than as a silently wrong offset in
//! the middle of a game's frame. The layout test in this module pins the offsets the header
//! implies on a 64-bit target.

use core::ffi::c_char;

/// Version of the interface this build implements, matching `RSF_OVERLAY_ABI_VERSION`.
pub const RSF_OVERLAY_ABI_VERSION: u32 = 2;

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

/// What the overlay is told about the session, once per frame. Mirrors `rsf_overlay_stats`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RsfOverlayStats {
    /// `sizeof(rsf_overlay_stats)` as the caller knows it.
    pub struct_size: u32,
    /// Non-zero when a backend has been loaded at all.
    pub backend_loaded: u32,
    /// Non-zero when the driver accepted that backend.
    pub backend_supported: u32,
    /// Vendor name, borrowed for the duration of the call. May be null.
    pub backend_name: *const c_char,
    /// Why the backend is unusable, borrowed for the duration of the call. May be null.
    pub refusal_reason: *const c_char,
    /// Width the scene is rendered at.
    pub render_width: u32,
    /// Height the scene is rendered at.
    pub render_height: u32,
    /// Width the result is presented at.
    pub output_width: u32,
    /// Height the result is presented at.
    pub output_height: u32,
    /// Frames presented since the overlay was created.
    pub frames_presented: u32,
    /// Frames the backend actually evaluated.
    pub frames_evaluated: u32,
    /// Frames the backend refused.
    pub frames_refused: u32,
    /// The backend's own last result code.
    pub last_result: i32,
    /// Non-zero when scene colour was found this frame.
    pub have_scene_color: u32,
    /// Non-zero when depth was found this frame.
    pub have_depth: u32,
    /// Non-zero when motion was found this frame.
    pub have_motion: u32,
    /// Non-zero when an exposure value was found this frame.
    pub have_exposure: u32,
    /// Non-zero when the motion has been decoded into something a backend can consume.
    pub motion_decoded: u32,
    /// Non-zero when the projection carries a jitter.
    pub jitter_active: u32,
    /// The jitter offset in pixels, x then y.
    pub jitter_pixels: [f32; 2],
    /// The quality currently in effect, which is not always the one last clicked.
    pub quality: RsfOverlayQuality,
    /// Non-zero when reconstruction is currently enabled.
    pub enabled: u32,
    /// Non-zero while the debug view is drawing over the frame.
    pub debug_view_on: u32,
    /// Non-zero while the reconstruction is being put into the game's own frame.
    pub reinsert_on: u32,
    /// Non-zero once reinsertion has everything it needs, so the panel can refuse before asking.
    pub reinsert_available: u32,
    /// The render scale in effect, as a percentage, or zero when nothing has set one.
    pub render_scale_percent: u32,
    /// How many frame captures have been written this session.
    pub captures_written: u32,
}

/// What the user asked for, this frame. Mirrors `rsf_overlay_intent`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RsfOverlayIntent {
    /// `sizeof(rsf_overlay_intent)`, written by the overlay.
    pub struct_size: u32,
    /// Non-zero when the quality was changed in this frame.
    pub quality_changed: u32,
    /// The quality the panel now shows as chosen, whether or not it changed.
    pub quality: RsfOverlayQuality,
    /// Non-zero when the enable toggle was changed in this frame.
    pub enabled_changed: u32,
    /// The enable state the panel now shows, whether or not it changed.
    pub enabled: u32,
    /// Non-zero when the user asked for this frame's inputs to be written out.
    pub dump_requested: u32,
    /// Bring the backend up. Separate from `enabled`: starting can fail where choosing cannot.
    pub start_requested: u32,
    /// Non-zero when the debug view was toggled in this frame.
    pub debug_view_changed: u32,
    /// The debug view state the panel now shows.
    pub debug_view: u32,
    /// Non-zero when reinsertion was toggled in this frame.
    pub reinsert_changed: u32,
    /// The reinsertion state the panel now shows.
    pub reinsert: u32,
    /// Apply `scale_percent`. Zero percent is not a request.
    pub scale_requested: u32,
    /// The render scale to apply, as a percentage.
    pub scale_percent: u32,
    /// Non-zero when the user asked for a frame capture.
    pub capture_requested: u32,
}

impl Default for RsfOverlayIntent {
    fn default() -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            quality_changed: 0,
            quality: RSF_OVERLAY_QUALITY_NATIVE,
            enabled_changed: 0,
            enabled: 0,
            dump_requested: 0,
            start_requested: 0,
            debug_view_changed: 0,
            debug_view: 0,
            reinsert_changed: 0,
            reinsert: 0,
            scale_requested: 0,
            scale_percent: 0,
            capture_requested: 0,
        }
    }
}

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
    /// Zero lays out nothing and returns no draw calls, without discarding widget state.
    pub visible: u32,
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
        // 124 bytes of fields, padded to 128 by the eight byte alignment the two pointers impose.
        assert_eq!(size_of::<RsfOverlayStats>(), 128);
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
    }

    #[test]
    fn the_other_structs_match_the_header_layout() {
        assert_eq!(size_of::<RsfOverlayIntent>(), 56);
        assert_eq!(offset_of!(RsfOverlayIntent, start_requested), 24);
        assert_eq!(size_of::<RsfOverlayInput>(), 36);
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
