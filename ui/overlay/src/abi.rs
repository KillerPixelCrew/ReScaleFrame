//! The Rust side of `include/rescaleframe/overlay.h`.
//! Its records are the generated ones shared with the Runtime.

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

/// One draw call; indices are call-local and `vertex_offset` is added as `BaseVertexLocation`.
pub use rsf_game_sys::rsf_overlay_draw_call as RsfOverlayDrawCall;
/// The triangles for one frame, owned by the overlay until the next frame call.
pub use rsf_game_sys::rsf_overlay_draw_data as RsfOverlayDrawData;
/// One frame of input, in physical pixels.
pub use rsf_game_sys::rsf_overlay_input as RsfOverlayInput;
/// A change to the overlay's texture atlas, borrowed until the next frame call.
pub use rsf_game_sys::rsf_overlay_texture_update as RsfOverlayTextureUpdate;
/// One vertex in egui's layout; `color` is premultiplied RGBA with red in the low byte.
pub use rsf_game_sys::rsf_overlay_vertex as RsfOverlayVertex;

/// Struct sizes of the 64-bit layout, pinned so a change to `overlay.h` has to be made on purpose:
/// every struct is append-only, so any change moves the size. The field offsets are asserted by
/// bindgen's layout checks in `rsf-game-sys`.
#[cfg(all(test, target_pointer_width = "64"))]
#[test]
fn struct_sizes_match_the_header() {
    assert_eq!(size_of::<RsfOverlayStats>(), 448);
    assert_eq!(size_of::<RsfOverlayIntent>(), 216);
    assert_eq!(size_of::<RsfOverlayHostOption>(), 40);
    assert_eq!(size_of::<RsfOverlayInput>(), 40);
    assert_eq!(size_of::<RsfOverlayVertex>(), 20);
    assert_eq!(size_of::<RsfOverlayDrawCall>(), 40);
    assert_eq!(size_of::<RsfOverlayDrawData>(), 56);
    assert_eq!(size_of::<RsfOverlayTextureUpdate>(), 40);
}
