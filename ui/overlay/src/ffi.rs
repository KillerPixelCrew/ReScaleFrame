//! C exports for `overlay.h`, with size checks, borrowed output and unwind containment.
//!
//! All calls on a handle require exclusive access. Null and short structures are rejected;
//! checks cannot validate arbitrary foreign addresses. Outputs borrow handle-owned buffers
//! until the next frame or destruction. A caught panic poisons the handle, which must then be
//! destroyed because egui may retain a lock. Aborting panics and allocator aborts are not caught.

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

/// Maximum borrowed UTF-8 string scan. The caller must still supply readable memory.
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
/// Returns null on an ABI mismatch or a caught construction panic. Allocator aborts are not caught.
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
/// Destruction requires exclusive access and invalidates every borrowed output from the handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_overlay_destroy(overlay: *mut RsfOverlay) {
    if overlay.is_null() {
        return;
    }
    // Destruction has no result channel; contain unwinding panics during the drop.
    let _ = catch_unwind(AssertUnwindSafe(|| drop(unsafe { Box::from_raw(overlay) })));
}

/// Lay out one frame.
///
/// # Safety
/// `overlay` must be a live handle. `input` and `stats` must point at structs whose `struct_size`
/// says how large they are. Each pointer must address aligned, readable storage of that size;
/// strings must be readable through their NUL or the 512-byte scan limit. `draw_data` and `intent`
/// may each be null; non-null outputs must be aligned, writable and carry their own `struct_size`.
/// Inputs, outputs and the handle must not alias. No concurrent calls on this handle are allowed.
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
/// Pixels remain borrowed until the next frame or destruction. Zero also covers invalid arguments
/// and a poisoned handle; this export has no separate error result.
///
/// # Safety
/// `overlay` must be a live handle with exclusive access. A non-null `updates` must address aligned,
/// writable room for `max_updates` entries and must not alias the handle or its buffers.
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
/// Free them after drawing the frame that emitted them. Zero also covers an invalid or poisoned call.
///
/// # Safety
/// `overlay` must be a live handle with exclusive access. A non-null `ids` must address aligned,
/// writable room for `max_ids` entries and must not alias the handle or its buffers.
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

/// Validate the complete input/output prefix before forming references or advancing overlay state.
/// Outputs are populated only after a successful safe-core frame.
///
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
    // Read the size before forming a reference to the complete structure.
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
        startup_hint_alpha: input.startup_hint_alpha,
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
            fg_changed: u32::from(produced.fg_changed),
            fg_mode: produced.fg_mode,
            fg_generated: produced.fg_generated,
            fg_backend_changed: u32::from(produced.fg_backend_changed),
            fg_backend: produced.fg_backend,
            reflex_changed: u32::from(produced.reflex_changed),
            reflex_mode: produced.reflex_mode,
            performance_hud_changed: u32::from(produced.performance_hud_changed),
            performance_hud: u32::from(produced.performance_hud),
            frame_limit_changed: u32::from(produced.frame_limit_changed),
            frame_limit_us: produced.frame_limit_us,
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
            backend_changed: u32::from(produced.backend_changed),
            backend: produced.backend,
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
/// `pointer` must be non-null and point at a frame input, stats, draw-data or intent envelope
/// beginning with `uint32_t struct_size`. Only those four bytes are read by this helper.
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

/// Borrow runtime strings and copy scalars without retaining host memory beyond the frame call.
///
/// # Safety
/// `stats` must be a valid struct. Non-null strings must remain readable through their NUL or
/// the 512-byte scan limit, borrowed for this call.
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
        backend: stats.backend,
        requested_backend: stats.requested_backend,
        last_switch_result: stats.last_switch_result,
        generation: crate::model::GenerationStats {
            backend: stats.fg_backend,
            requested_backend: stats.fg_requested_backend,
            backend_choices: stats.fg_backend_choices,
            selection_result: stats.fg_selection_result,
            available: stats.fg_available != 0,
            requested_mode: stats.fg_requested_mode,
            requested_generated: stats.fg_requested_generated,
            effective_mode: stats.fg_effective_mode,
            effective_generated: stats.fg_effective_generated,
            active: stats.fg_active != 0,
            max_generated: stats.fg_max_generated,
            reflex_available: stats.reflex_available != 0,
            requested_reflex: stats.reflex_requested_mode,
            effective_reflex: stats.reflex_effective_mode,
            reason: stats.fg_reason,
            last_result: stats.fg_last_result,
            total_presented: stats.fg_total_presented,
            frame_limit_us: stats.frame_limit_us,
            display_refresh_mhz: stats.display_refresh_mhz,
        },
        application_presented_frames: stats.application_presented_frames,
        counter_clock: [stats.sample_qpc, stats.qpc_frequency],
        show_performance_hud: stats.show_performance_hud != 0,
        fg_present_count_valid: stats.fg_present_count_valid != 0,
    }
}

/// Read a bounded UTF-8 string; invalid UTF-8 becomes a diagnostic label and null becomes `None`.
/// The returned lifetime is chosen by the caller and must not outlive the host allocation.
///
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
    Some(std::str::from_utf8(bytes).unwrap_or("(not valid utf-8)"))
}

/// Rust-side version, pointer, size and output fixtures. Native C layout agreement and deliberate
/// panic recovery require separate validation.
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
            startup_hint_alpha: 0.0,
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
            backend: 1,
            requested_backend: 1,
            last_switch_result: 0,
            fg_available: 0,
            fg_requested_mode: 0,
            fg_requested_generated: 1,
            fg_effective_mode: 0,
            fg_effective_generated: 0,
            fg_active: 0,
            fg_max_generated: 0,
            reflex_available: 0,
            reflex_requested_mode: 0,
            reflex_effective_mode: 0,
            fg_reason: 0,
            fg_last_result: 0,
            fg_total_presented: 0,
            application_presented_frames: 0,
            sample_qpc: 0,
            qpc_frequency: 0,
            show_performance_hud: 0,
            fg_present_count_valid: 0,
            frame_limit_us: 0,
            display_refresh_mhz: 0,
            fg_backend: 0,
            fg_requested_backend: 0,
            fg_backend_choices: 0,
            fg_selection_result: 0,
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
        assert_eq!(RSF_OVERLAY_ABI_VERSION, 9);
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
