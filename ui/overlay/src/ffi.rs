//! The C entry points of `overlay.h`, and nothing else.
//!
//! Every function here checks its pointers, converts, calls into the safe core, and copies the
//! answer back out. The rules it follows:
//!
//! * A panic never leaves this file. Unwinding into a game's render thread is undefined behaviour,
//!   so each entry point catches and reports `RSF_OVERLAY_ERROR_PANICKED` instead. Once that has
//!   happened the handle is marked and refuses everything but destruction, because a panic out of
//!   the middle of egui leaves its context locked and calling it again would deadlock the render
//!   thread rather than crash it.
//! * `catch_unwind` only works while panics unwind. A profile built with `panic = "abort"` would
//!   abort the game instead, and nothing in this file can prevent that.
//! * Every struct is checked against the size this build compiled, and a shorter one is refused.
//!   That includes the two output structs, so a caller has to fill in `struct_size` on those too.
//! * Nothing is allocated for the caller. The pointers handed back point into buffers the handle
//!   owns and refills on the next frame call.

use std::ffi::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::null_mut;

use crate::abi::{
    RSF_OVERLAY_ABI_VERSION, RSF_OVERLAY_ERROR_INVALID_ARGUMENT, RSF_OVERLAY_ERROR_PANICKED,
    RSF_OVERLAY_OK, RsfOverlayDrawData, RsfOverlayInput, RsfOverlayIntent, RsfOverlayResult,
    RsfOverlayStats, RsfOverlayTextureUpdate,
};
use crate::model::{FrameInput, Quality, Stats};
use crate::overlay::Overlay;

/// Longest borrowed string the overlay will read.
///
/// The header says these are NUL terminated and borrowed for the call. A missing terminator is the
/// caller's bug, but scanning without a bound turns it into a walk across the game's address
/// space, so the scan stops here and takes what it has.
const MAX_BORROWED_STRING: usize = 512;

/// The handle behind `rsf_overlay*`.
///
/// Opaque to C, which only ever sees the pointer.
pub struct RsfOverlay {
    overlay: Overlay,
    /// Set once a panic has been caught. Every entry point but destruction then refuses.
    poisoned: bool,
}

/// Create the overlay.
///
/// Returns null on an ABI mismatch, or if construction panicked or could not allocate.
#[unsafe(no_mangle)]
pub extern "C" fn rsf_overlay_create(abi_version: u32) -> *mut RsfOverlay {
    if abi_version != RSF_OVERLAY_ABI_VERSION {
        return null_mut();
    }
    match catch_unwind(|| {
        Box::into_raw(Box::new(RsfOverlay {
            overlay: Overlay::new(),
            poisoned: false,
        }))
    }) {
        Ok(handle) => handle,
        Err(_) => null_mut(),
    }
}

/// Destroy an overlay created by [`rsf_overlay_create`].
///
/// Valid on a handle that has reported `RSF_OVERLAY_ERROR_PANICKED`, and the only thing that is.
///
/// # Safety
/// `overlay` must be null, or a handle from [`rsf_overlay_create`] that has not been destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_overlay_destroy(overlay: *mut RsfOverlay) {
    if overlay.is_null() {
        return;
    }
    // Dropping egui's context runs other people's code, and this function cannot report anything,
    // so a panic here has to stop at the boundary the same as anywhere else.
    let _ = catch_unwind(AssertUnwindSafe(|| drop(unsafe { Box::from_raw(overlay) })));
}

/// Lay out one frame.
///
/// # Safety
/// `overlay` must be a live handle. `input` and `stats` must point at structs whose `struct_size`
/// says how large they are, and their strings must be NUL terminated. `draw_data` and `intent` may
/// each be null; a non-null one must be writable and carry its own `struct_size`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_overlay_frame(
    overlay: *mut RsfOverlay,
    input: *const RsfOverlayInput,
    stats: *const RsfOverlayStats,
    draw_data: *mut RsfOverlayDrawData,
    intent: *mut RsfOverlayIntent,
) -> RsfOverlayResult {
    let Some(handle) = (unsafe { overlay.as_mut() }) else {
        return RSF_OVERLAY_ERROR_INVALID_ARGUMENT;
    };
    if handle.poisoned {
        return RSF_OVERLAY_ERROR_PANICKED;
    }

    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        frame_inner(&mut handle.overlay, input, stats, draw_data, intent)
    }));
    match result {
        Ok(code) => code,
        Err(_) => {
            handle.poisoned = true;
            RSF_OVERLAY_ERROR_PANICKED
        }
    }
}

/// Collect texture atlas changes produced by the last frame.
///
/// Writes up to `max_updates` entries and returns how many were written. Entries are handed out
/// once, so a caller with a small array calls again until it gets zero back.
///
/// # Safety
/// `overlay` must be a live handle, and `updates` must point at room for `max_updates` entries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_overlay_texture_updates(
    overlay: *mut RsfOverlay,
    updates: *mut RsfOverlayTextureUpdate,
    max_updates: u32,
) -> u32 {
    let Some(handle) = (unsafe { overlay.as_mut() }) else {
        return 0;
    };
    if handle.poisoned || updates.is_null() || max_updates == 0 {
        return 0;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        let taken = handle.overlay.drain_texture_updates(max_updates as usize);
        for (index, update) in taken.iter().enumerate() {
            let entry = RsfOverlayTextureUpdate {
                id: update.id,
                x: update.x,
                y: update.y,
                width: update.width,
                height: update.height,
                pixels: update.pixels.as_ptr(),
                is_whole_texture: u32::from(update.is_whole_texture),
            };
            unsafe { updates.add(index).write(entry) };
        }
        taken.len() as u32
    }));
    match result {
        Ok(written) => written,
        Err(_) => {
            handle.poisoned = true;
            0
        }
    }
}

/// Collect the ids of textures the overlay has finished with. Drains like
/// [`rsf_overlay_texture_updates`].
///
/// # Safety
/// `overlay` must be a live handle, and `ids` must point at room for `max_ids` entries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_overlay_textures_to_free(
    overlay: *mut RsfOverlay,
    ids: *mut u64,
    max_ids: u32,
) -> u32 {
    let Some(handle) = (unsafe { overlay.as_mut() }) else {
        return 0;
    };
    if handle.poisoned || ids.is_null() || max_ids == 0 {
        return 0;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        let taken = handle.overlay.drain_textures_to_free(max_ids as usize);
        for (index, id) in taken.iter().enumerate() {
            unsafe { ids.add(index).write(*id) };
        }
        taken.len() as u32
    }));
    match result {
        Ok(written) => written,
        Err(_) => {
            handle.poisoned = true;
            0
        }
    }
}

/// # Safety
/// As [`rsf_overlay_frame`].
unsafe fn frame_inner(
    overlay: &mut Overlay,
    input: *const RsfOverlayInput,
    stats: *const RsfOverlayStats,
    draw_data: *mut RsfOverlayDrawData,
    intent: *mut RsfOverlayIntent,
) -> RsfOverlayResult {
    if input.is_null() || stats.is_null() {
        return RSF_OVERLAY_ERROR_INVALID_ARGUMENT;
    }
    // Size first, reference second. A caller with a shorter struct has fewer bytes than this build
    // expects, and a reference to the whole thing would already be invalid by the time anything
    // got to look at the size that says so.
    if unsafe { !fits(input) || !fits(stats) } {
        return RSF_OVERLAY_ERROR_INVALID_ARGUMENT;
    }
    // The two outputs are optional, but a short one is refused rather than partly filled.
    if !draw_data.is_null() && unsafe { !fits(draw_data.cast_const()) } {
        return RSF_OVERLAY_ERROR_INVALID_ARGUMENT;
    }
    if !intent.is_null() && unsafe { !fits(intent.cast_const()) } {
        return RSF_OVERLAY_ERROR_INVALID_ARGUMENT;
    }
    let input = unsafe { &*input };
    let stats = unsafe { &*stats };

    let frame_input = FrameInput {
        mouse: [input.mouse_x, input.mouse_y],
        mouse_buttons: input.mouse_buttons,
        scroll_delta: input.scroll_delta,
        display: [input.display_width, input.display_height],
        delta_seconds: input.delta_seconds,
        visible: input.visible != 0,
    };
    let frame_stats = unsafe { borrow_stats(stats) };
    let produced = overlay.frame(&frame_input, &frame_stats);

    if let Some(out) = unsafe { intent.as_mut() } {
        *out = RsfOverlayIntent {
            struct_size: size_of::<RsfOverlayIntent>() as u32,
            quality_changed: u32::from(produced.quality_changed),
            quality: produced.quality.to_abi(),
            enabled_changed: u32::from(produced.enabled_changed),
            enabled: u32::from(produced.enabled),
            dump_requested: u32::from(produced.dump_requested),
            start_requested: u32::from(produced.start_requested),
            debug_view_changed: u32::from(produced.debug_view_changed),
            debug_view: u32::from(produced.debug_view),
            reinsert_changed: u32::from(produced.reinsert_changed),
            reinsert: u32::from(produced.reinsert),
            scale_requested: u32::from(produced.scale_requested),
            scale_percent: produced.scale_percent,
            capture_requested: u32::from(produced.capture_requested),
            jitter_changed: u32::from(produced.jitter_changed),
            jitter: u32::from(produced.jitter),
        };
    }
    if let Some(out) = unsafe { draw_data.as_mut() } {
        // Null with a count of zero when there is nothing to draw, rather than a pointer into an
        // empty buffer that a renderer might feel entitled to read.
        let vertices = overlay.vertices();
        let indices = overlay.indices();
        let calls = overlay.draw_calls();
        *out = RsfOverlayDrawData {
            struct_size: size_of::<RsfOverlayDrawData>() as u32,
            vertices: slice_ptr(vertices),
            vertex_count: vertices.len() as u32,
            indices: slice_ptr(indices),
            index_count: indices.len() as u32,
            calls: slice_ptr(calls),
            call_count: calls.len() as u32,
        };
    }
    RSF_OVERLAY_OK
}

/// Whether a caller's struct says it is at least as long as this build's version of it.
///
/// Longer is fine and means the caller compiled against a later header: fields are only ever
/// appended, so everything this build knows about is still where it expects it.
///
/// # Safety
/// `pointer` must be non-null and point at a struct that begins with its own `uint32_t
/// struct_size`, which every struct in `overlay.h` does. Only those four bytes are read.
unsafe fn fits<T>(pointer: *const T) -> bool {
    let declared = unsafe { pointer.cast::<u32>().read_unaligned() };
    declared as usize >= size_of::<T>()
}

fn slice_ptr<T>(slice: &[T]) -> *const T {
    if slice.is_empty() {
        std::ptr::null()
    } else {
        slice.as_ptr()
    }
}

/// # Safety
/// `stats` must be a valid struct with NUL terminated strings, borrowed for this call.
unsafe fn borrow_stats(stats: &RsfOverlayStats) -> Stats<'_> {
    Stats {
        backend_loaded: stats.backend_loaded != 0,
        backend_supported: stats.backend_supported != 0,
        backend_name: unsafe { borrow_str(stats.backend_name) },
        refusal_reason: unsafe { borrow_str(stats.refusal_reason) },
        render: [stats.render_width, stats.render_height],
        output: [stats.output_width, stats.output_height],
        frames_presented: stats.frames_presented,
        frames_evaluated: stats.frames_evaluated,
        frames_refused: stats.frames_refused,
        last_result: stats.last_result,
        have_scene_color: stats.have_scene_color != 0,
        have_depth: stats.have_depth != 0,
        have_motion: stats.have_motion != 0,
        have_exposure: stats.have_exposure != 0,
        motion_decoded: stats.motion_decoded != 0,
        jitter_active: stats.jitter_active != 0,
        jitter_pixels: stats.jitter_pixels,
        quality: Quality::from_abi(stats.quality),
        quality_raw: stats.quality,
        enabled: stats.enabled != 0,
        debug_view_on: stats.debug_view_on != 0,
        reinsert_on: stats.reinsert_on != 0,
        reinsert_available: stats.reinsert_available != 0,
        render_scale_percent: stats.render_scale_percent,
        captures_written: stats.captures_written,
        jitter_gate_on: stats.jitter_on != 0,
        jitter_gate_available: stats.jitter_available != 0,
    }
}

/// # Safety
/// `pointer` must be null, or point at bytes that are readable until a NUL or
/// [`MAX_BORROWED_STRING`] bytes, whichever comes first.
unsafe fn borrow_str<'a>(pointer: *const c_char) -> Option<&'a str> {
    if pointer.is_null() {
        return None;
    }
    let mut length = 0;
    while length < MAX_BORROWED_STRING && unsafe { *pointer.add(length) } != 0 {
        length += 1;
    }
    let bytes = unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), length) };
    // A vendor name with a stray byte in it is still worth showing, and refusing the whole frame
    // over one would be a strange way to report it.
    Some(std::str::from_utf8(bytes).unwrap_or("(not valid utf-8)"))
}

/// Smoke tests for the boundary itself.
///
/// The panel's behaviour is tested through the safe core, which needs none of this. What is left
/// to check here is the boundary's own rules: the version gate, the pointer checks, the struct size
/// checks, and that a frame actually reaches the buffers a renderer would read. It is still Rust
/// calling Rust, so it says nothing about whether a C compiler agrees with these layouts.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::{
        RSF_OVERLAY_ERROR_ABI_MISMATCH, RSF_OVERLAY_QUALITY_BALANCED, RsfOverlayDrawCall,
        RsfOverlayVertex,
    };
    use std::ptr::null;

    const EMPTY_UPDATE: RsfOverlayTextureUpdate = RsfOverlayTextureUpdate {
        id: 0,
        x: 0,
        y: 0,
        width: 0,
        height: 0,
        pixels: null(),
        is_whole_texture: 0,
    };

    fn input() -> RsfOverlayInput {
        RsfOverlayInput {
            struct_size: size_of::<RsfOverlayInput>() as u32,
            mouse_x: 1.0,
            mouse_y: 1.0,
            mouse_buttons: 0,
            scroll_delta: 0.0,
            display_width: 1280,
            display_height: 720,
            delta_seconds: 1.0 / 60.0,
            visible: 1,
        }
    }

    fn stats() -> RsfOverlayStats {
        RsfOverlayStats {
            struct_size: size_of::<RsfOverlayStats>() as u32,
            backend_loaded: 1,
            backend_supported: 1,
            backend_name: c"Test backend".as_ptr(),
            refusal_reason: null(),
            render_width: 1024,
            render_height: 576,
            output_width: 2048,
            output_height: 1152,
            frames_presented: 100,
            frames_evaluated: 99,
            frames_refused: 1,
            last_result: -3,
            have_scene_color: 1,
            have_depth: 1,
            have_motion: 1,
            have_exposure: 0,
            motion_decoded: 0,
            jitter_active: 1,
            jitter_pixels: [0.25, -0.25],
            quality: RSF_OVERLAY_QUALITY_BALANCED,
            enabled: 1,
            debug_view_on: 0,
            reinsert_on: 0,
            reinsert_available: 0,
            render_scale_percent: 50,
            captures_written: 0,
            jitter_on: 1,
            jitter_available: 1,
        }
    }

    #[test]
    fn a_mismatched_abi_version_gets_no_handle() {
        assert!(rsf_overlay_create(RSF_OVERLAY_ABI_VERSION + 1).is_null());
        assert!(rsf_overlay_create(0).is_null());
        // And destroying nothing is allowed, so a caller can clean up unconditionally.
        unsafe { rsf_overlay_destroy(std::ptr::null_mut()) };
    }

    #[test]
    fn a_frame_fills_in_both_outputs() {
        let overlay = rsf_overlay_create(RSF_OVERLAY_ABI_VERSION);
        assert!(!overlay.is_null());

        let input = input();
        let stats = stats();
        let mut draw = RsfOverlayDrawData::default();
        let mut intent = RsfOverlayIntent::default();
        // More than one, because egui lays a new window out invisibly to size it first.
        for _ in 0..3 {
            let code =
                unsafe { rsf_overlay_frame(overlay, &input, &stats, &mut draw, &mut intent) };
            assert_eq!(code, RSF_OVERLAY_OK);
        }

        assert_eq!(draw.struct_size, size_of::<RsfOverlayDrawData>() as u32);
        assert!(draw.call_count > 0);
        assert!(!draw.vertices.is_null() && !draw.indices.is_null() && !draw.calls.is_null());

        // Walk what a renderer would walk, and check the offsets land inside the buffers.
        let calls = unsafe { std::slice::from_raw_parts(draw.calls, draw.call_count as usize) };
        let vertices: &[RsfOverlayVertex] =
            unsafe { std::slice::from_raw_parts(draw.vertices, draw.vertex_count as usize) };
        let indices =
            unsafe { std::slice::from_raw_parts(draw.indices, draw.index_count as usize) };
        for call in calls {
            let end = call.index_offset as usize + call.index_count as usize;
            assert!(end <= indices.len());
            for index in &indices[call.index_offset as usize..end] {
                assert!((call.vertex_offset as usize + *index as usize) < vertices.len());
            }
        }
        assert_eq!(size_of::<RsfOverlayDrawCall>(), 40);

        assert_eq!(intent.struct_size, size_of::<RsfOverlayIntent>() as u32);
        assert_eq!(intent.quality, RSF_OVERLAY_QUALITY_BALANCED);
        assert_eq!(intent.quality_changed, 0);
        assert_eq!(intent.enabled, 1);

        unsafe { rsf_overlay_destroy(overlay) };
    }

    #[test]
    fn the_font_atlas_arrives_through_the_collection_call() {
        let overlay = rsf_overlay_create(RSF_OVERLAY_ABI_VERSION);
        let input = input();
        let stats = stats();
        assert_eq!(
            unsafe {
                rsf_overlay_frame(
                    overlay,
                    &input,
                    &stats,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            },
            RSF_OVERLAY_OK
        );

        let mut updates = [EMPTY_UPDATE; 8];
        let written = unsafe {
            rsf_overlay_texture_updates(overlay, updates.as_mut_ptr(), updates.len() as u32)
        };
        assert!(written > 0, "the font atlas has to arrive somehow");
        for update in &updates[..written as usize] {
            assert!(!update.pixels.is_null());
            let expected = update.width as usize * update.height as usize * 4;
            let pixels = unsafe { std::slice::from_raw_parts(update.pixels, expected) };
            assert_eq!(pixels.len(), expected);
        }
        // Handed out once each.
        assert_eq!(
            unsafe {
                rsf_overlay_texture_updates(overlay, updates.as_mut_ptr(), updates.len() as u32)
            },
            0
        );

        let mut ids = [0u64; 4];
        // Nothing has been freed yet, but asking must be safe on any frame.
        assert_eq!(
            unsafe { rsf_overlay_textures_to_free(overlay, ids.as_mut_ptr(), ids.len() as u32) },
            0
        );

        unsafe { rsf_overlay_destroy(overlay) };
    }

    #[test]
    fn null_and_short_structs_are_refused() {
        let overlay = rsf_overlay_create(RSF_OVERLAY_ABI_VERSION);
        let input = input();
        let stats = stats();
        let null_out = (std::ptr::null_mut(), std::ptr::null_mut());

        assert_eq!(
            unsafe {
                rsf_overlay_frame(std::ptr::null_mut(), &input, &stats, null_out.0, null_out.1)
            },
            RSF_OVERLAY_ERROR_INVALID_ARGUMENT
        );
        assert_eq!(
            unsafe { rsf_overlay_frame(overlay, null(), &stats, null_out.0, null_out.1) },
            RSF_OVERLAY_ERROR_INVALID_ARGUMENT
        );
        assert_eq!(
            unsafe { rsf_overlay_frame(overlay, &input, null(), null_out.0, null_out.1) },
            RSF_OVERLAY_ERROR_INVALID_ARGUMENT
        );

        let short = RsfOverlayInput {
            struct_size: 4,
            ..input
        };
        assert_eq!(
            unsafe { rsf_overlay_frame(overlay, &short, &stats, null_out.0, null_out.1) },
            RSF_OVERLAY_ERROR_INVALID_ARGUMENT
        );

        // Including the outputs: an unset struct_size is a caller that has not been told.
        let mut draw = RsfOverlayDrawData {
            struct_size: 0,
            ..RsfOverlayDrawData::default()
        };
        assert_eq!(
            unsafe { rsf_overlay_frame(overlay, &input, &stats, &mut draw, null_out.1) },
            RSF_OVERLAY_ERROR_INVALID_ARGUMENT
        );

        // A caller from a later header, whose structs are longer, is not refused.
        let longer = RsfOverlayStats {
            struct_size: stats.struct_size + 64,
            ..stats
        };
        assert_eq!(
            unsafe { rsf_overlay_frame(overlay, &input, &longer, null_out.0, null_out.1) },
            RSF_OVERLAY_OK
        );

        unsafe { rsf_overlay_destroy(overlay) };
    }

    #[test]
    fn the_error_codes_are_the_ones_the_header_defines() {
        assert_eq!(RSF_OVERLAY_OK, 0);
        assert_eq!(RSF_OVERLAY_ERROR_INVALID_ARGUMENT, -1);
        assert_eq!(RSF_OVERLAY_ERROR_ABI_MISMATCH, -2);
        assert_eq!(RSF_OVERLAY_ERROR_PANICKED, -3);
        assert_eq!(RSF_OVERLAY_ABI_VERSION, 3);
    }

    #[test]
    fn a_hidden_frame_reports_nothing_to_draw() {
        let overlay = rsf_overlay_create(RSF_OVERLAY_ABI_VERSION);
        let stats = stats();
        let hidden = RsfOverlayInput {
            visible: 0,
            ..input()
        };
        let mut draw = RsfOverlayDrawData::default();
        assert_eq!(
            unsafe { rsf_overlay_frame(overlay, &hidden, &stats, &mut draw, std::ptr::null_mut()) },
            RSF_OVERLAY_OK
        );
        assert_eq!(draw.call_count, 0);
        assert!(draw.vertices.is_null() && draw.indices.is_null() && draw.calls.is_null());
        unsafe { rsf_overlay_destroy(overlay) };
    }
}
