//! The overlay's state across frames, and the triangles it produces.
//!
//! Safe Rust throughout. It owns the egui context, turns the host's per frame snapshot of the
//! mouse into the events egui expects, and flattens egui's output into the flat buffers the C
//! header describes. The FFI layer above it does nothing but check pointers and copy.

use egui::{Event, PointerButton, Pos2, RawInput, ViewportId, epaint};

use crate::abi::{
    RSF_OVERLAY_MOUSE_LEFT, RSF_OVERLAY_MOUSE_MIDDLE, RSF_OVERLAY_MOUSE_RIGHT, RsfOverlayDrawCall,
    RsfOverlayVertex,
};
use crate::model::{FrameInput, Intent, Stats, TextureUpdate};
use crate::panel::{self, Controls, Selection};

/// Largest texture side the overlay will ask egui for.
///
/// Chosen, not measured. The overlay never sees the device, so it cannot ask what the real limit
/// is; 2048 is egui's own default and is below the guaranteed minimum of every D3D11 feature level
/// the project targets. What this buys is one thing only: no texture handed to the renderer is
/// ever larger than this on either side. Beyond it egui does not shrink anything, it starts reusing
/// atlas space and rebuilds the atlas, which costs a whole-texture update and, briefly, wrong
/// glyphs. Read out of epaint's `TextureAtlas`, not observed here.
const MAX_TEXTURE_SIDE: usize = 2048;

/// Reference display height for the scale heuristic below.
const REFERENCE_HEIGHT: f32 = 1080.0;

/// The overlay, across frames.
pub struct Overlay {
    context: egui::Context,
    selection: Selection,
    controls: Controls,

    /// Monotonic seconds handed to egui for animation. Advanced by the host's delta, because the
    /// overlay has no business reading a clock on a render thread.
    time: f64,
    last_mouse: Option<[f32; 2]>,
    last_buttons: u32,
    /// Buttons egui has been told are down, which is not the same as the buttons the host reports
    /// while the panel is hidden and egui is not being run. See [`Overlay::raw_input`].
    told_egui_buttons: u32,
    was_visible: bool,

    /// Rebuilt every frame. The C side is handed pointers into these and the header promises they
    /// stay valid until the next frame call, which is exactly as long as they live here.
    vertices: Vec<RsfOverlayVertex>,
    indices: Vec<u32>,
    calls: Vec<RsfOverlayDrawCall>,

    /// Texture work from the last frame, drained by the two collection entry points. The pixels
    /// stay owned here until the next frame clears them, so a caller that reads them after the
    /// draw call it belongs to still reads live memory.
    texture_updates: Vec<TextureUpdate>,
    texture_updates_taken: usize,
    textures_to_free: Vec<u64>,
    textures_to_free_taken: usize,
}

impl Default for Overlay {
    fn default() -> Self {
        Self::new()
    }
}

impl Overlay {
    /// Create an overlay with nothing selected and nothing drawn yet.
    #[must_use]
    pub fn new() -> Self {
        let context = egui::Context::default();

        // One layout pass per frame, always. egui otherwise runs a second pass over the same input
        // whenever something asks it to discard the first, which a Grid does the first time it is
        // shown. This runs on a game's render thread, where a predictable single layout is worth
        // more than a grid whose column widths are right one frame earlier, and it means the same
        // click is never presented to the widgets twice.
        context.options_mut(|options| {
            options.max_passes = std::num::NonZeroUsize::new(1).expect("1 is not zero");
        });

        Self {
            context,
            selection: Selection::default(),
            controls: Controls::default(),
            time: 0.0,
            last_mouse: None,
            last_buttons: 0,
            told_egui_buttons: 0,
            was_visible: false,
            vertices: Vec::new(),
            indices: Vec::new(),
            calls: Vec::new(),
            texture_updates: Vec::new(),
            texture_updates_taken: 0,
            textures_to_free: Vec::new(),
            textures_to_free_taken: 0,
        }
    }

    /// Lay out one frame and report what the user asked for.
    ///
    /// Hiding the overlay skips the layout entirely but keeps reconciling the selection against
    /// the stats, so a change applied while it was hidden is not still shown as pending when it
    /// comes back.
    pub fn frame(&mut self, input: &FrameInput, stats: &Stats<'_>) -> Intent {
        self.vertices.clear();
        self.indices.clear();
        self.calls.clear();
        self.texture_updates.clear();
        self.texture_updates_taken = 0;
        self.textures_to_free.clear();
        self.textures_to_free_taken = 0;

        self.selection.reconcile(stats);

        let mut intent = Intent {
            quality: self.selection.quality(),
            enabled: self.selection.enabled(),
            ..Intent::default()
        };

        let display = [input.display[0] as f32, input.display[1] as f32];
        if !input.visible || display[0] <= 0.0 || display[1] <= 0.0 {
            // Still track the pointer while hidden, so a button pressed and released behind the
            // panel does not arrive as a click the moment it reappears.
            self.last_mouse = Some(input.mouse);
            self.last_buttons = input.mouse_buttons;
            self.was_visible = false;
            self.controls = Controls::default();
            return intent;
        }

        let points_per_pixel = 1.0 / pixels_per_point(display[1]);
        let raw = self.raw_input(input, display, points_per_pixel);

        let selection = &self.selection;
        let controls = &mut self.controls;
        *controls = Controls::default();
        let output = self.context.run_ui(raw, |ui| {
            panel::show(ui.ctx(), selection, stats, &mut intent, controls);
        });

        // Applied after the layout rather than during it, so every widget in the frame reads the
        // same selection: the one the frame started with. A checkbox that wrote back into that
        // state mid-layout would toggle twice if the layout ever ran twice over one set of events.
        self.selection.apply(&intent);
        intent.quality = self.selection.quality();
        intent.enabled = self.selection.enabled();

        let pixels_per_point = output.pixels_per_point;
        self.collect_textures(output.textures_delta);

        let primitives = self.context.tessellate(output.shapes, pixels_per_point);
        self.collect_primitives(&primitives, pixels_per_point, input.display);

        self.was_visible = true;
        intent
    }

    /// The triangles for the last frame.
    #[must_use]
    pub fn vertices(&self) -> &[RsfOverlayVertex] {
        &self.vertices
    }

    /// The indices for the last frame. They are call-local: add a call's `vertex_offset`.
    #[must_use]
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// The draw calls for the last frame, in the order they must be drawn.
    #[must_use]
    pub fn draw_calls(&self) -> &[RsfOverlayDrawCall] {
        &self.calls
    }

    /// Where the panel's controls landed in the last frame. See [`Controls`].
    #[must_use]
    pub fn controls(&self) -> &Controls {
        &self.controls
    }

    /// Take up to `max` texture updates that have not been taken yet.
    ///
    /// Draining rather than repeating, so a caller with a small array can loop until it gets
    /// nothing back. The pixels stay owned by the overlay either way.
    pub fn drain_texture_updates(&mut self, max: usize) -> &[TextureUpdate] {
        let start = self.texture_updates_taken;
        let end = self.texture_updates.len().min(start.saturating_add(max));
        self.texture_updates_taken = end;
        &self.texture_updates[start..end]
    }

    /// Take up to `max` ids of textures the overlay has finished with.
    pub fn drain_textures_to_free(&mut self, max: usize) -> &[u64] {
        let start = self.textures_to_free_taken;
        let end = self.textures_to_free.len().min(start.saturating_add(max));
        self.textures_to_free_taken = end;
        &self.textures_to_free[start..end]
    }

    /// Turn the host's snapshot of the mouse into the events egui expects.
    ///
    /// The header hands over a position and a button mask, not events, so the transitions have to
    /// be recovered by comparing against the previous frame.
    fn raw_input(
        &mut self,
        input: &FrameInput,
        display: [f32; 2],
        points_per_pixel: f32,
    ) -> RawInput {
        let mouse = [sanitise(input.mouse[0], 0.0), sanitise(input.mouse[1], 0.0)];
        let position = Pos2::new(mouse[0] * points_per_pixel, mouse[1] * points_per_pixel);

        let mut events = Vec::new();
        // A pointer that has not moved needs no event, except on the frame the panel reappears:
        // egui's idea of where the pointer is may be several frames stale by then.
        if self.last_mouse != Some(mouse) || !self.was_visible {
            events.push(Event::PointerMoved(position));
        }
        // Transitions are recovered against two histories, because they answer two questions. The
        // host's previous mask says whether the user pressed just now; what egui was last told says
        // whether egui is still holding something. They only differ across a spell of being hidden,
        // when the host keeps reporting and egui is not being run.
        let mut told = self.told_egui_buttons;
        for (mask, button) in [
            (RSF_OVERLAY_MOUSE_LEFT, PointerButton::Primary),
            (RSF_OVERLAY_MOUSE_RIGHT, PointerButton::Secondary),
            (RSF_OVERLAY_MOUSE_MIDDLE, PointerButton::Middle),
        ] {
            let was_down = self.last_buttons & mask != 0;
            let is_down = input.mouse_buttons & mask != 0;
            let egui_holds_it = told & mask != 0;
            // Press on an edge the panel was there to see. A button already down when the panel
            // reappears is not a press: the user was not aiming at a panel that was not drawn.
            let pressed = is_down && !was_down && !egui_holds_it;
            // Release whenever egui is holding a button the host says is up, which is what a
            // release that happened while the panel was hidden looks like from here. Without this
            // egui keeps the button down for good: widgets stay in their pressed state and a press
            // that landed on the window drags it as soon as the panel comes back. A late release
            // lands wherever the pointer is now, after the move event above, so it finishes the
            // click only if the pointer never moved while the panel was away.
            let released = !is_down && egui_holds_it;
            if pressed || released {
                events.push(Event::PointerButton {
                    pos: position,
                    button,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
                if pressed {
                    told |= mask;
                } else {
                    told &= !mask;
                }
            }
        }
        self.told_egui_buttons = told;

        let scroll = sanitise(input.scroll_delta, 0.0);
        if scroll != 0.0 {
            events.push(Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, scroll),
                // The header carries one wheel delta and no phase, which is what a mouse wheel
                // produces. egui asks for Move when the phase is unknown.
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            });
        }

        self.last_mouse = Some(mouse);
        self.last_buttons = input.mouse_buttons;

        // The header says a zero delta is survivable, and it is, but not for free: egui drives its
        // fade in from elapsed time, so a clock that never advances leaves the panel permanently
        // part way through appearing. A host that cannot tell us gets an assumed 60Hz rather than
        // a frozen one.
        let delta = sanitise(input.delta_seconds, 0.0).clamp(0.0, 1.0);
        let advance = if delta > 0.0 { delta } else { 1.0 / 60.0 };
        self.time += f64::from(advance);

        let mut raw = RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(display[0] * points_per_pixel, display[1] * points_per_pixel),
            )),
            max_texture_side: Some(MAX_TEXTURE_SIDE),
            time: Some(self.time),
            predicted_dt: advance,
            events,
            focused: true,
            ..RawInput::default()
        };
        if let Some(viewport) = raw.viewports.get_mut(&ViewportId::ROOT) {
            viewport.native_pixels_per_point = Some(1.0 / points_per_pixel);
        }
        raw
    }

    fn collect_textures(&mut self, delta: epaint::textures::TexturesDelta) {
        for (id, patch) in delta.set {
            let epaint::ImageData::Color(image) = &patch.image;
            let [width, height] = image.size;
            // A patch whose pixel count does not match its size would have the renderer walk off
            // the end of the buffer, and an empty one would hand it a dangling pointer, since an
            // empty Vec has no allocation to point at. egui produces neither, so this only fires if
            // egui changes shape under us, and dropping a patch beats a bad memcpy inside a game.
            if width == 0 || height == 0 || width * height != image.pixels.len() {
                continue;
            }
            let mut pixels = Vec::with_capacity(image.pixels.len() * 4);
            for pixel in &image.pixels {
                pixels.extend_from_slice(&pixel.to_array());
            }
            let [x, y] = patch.pos.unwrap_or([0, 0]);
            self.texture_updates.push(TextureUpdate {
                id: texture_id_to_u64(id),
                x: x as u32,
                y: y as u32,
                width: width as u32,
                height: height as u32,
                pixels,
                is_whole_texture: patch.pos.is_none(),
            });
        }
        for id in delta.free {
            self.textures_to_free.push(texture_id_to_u64(id));
        }
    }

    fn collect_primitives(
        &mut self,
        primitives: &[epaint::ClippedPrimitive],
        pixels_per_point: f32,
        display: [u32; 2],
    ) {
        for primitive in primitives {
            let epaint::Primitive::Mesh(mesh) = &primitive.primitive else {
                // The overlay never registers a paint callback, so this arm is unreachable in
                // practice. It is a skip rather than a panic because being wrong about that on a
                // render thread should cost a missing quad, not the game.
                continue;
            };
            if mesh.indices.is_empty() || mesh.vertices.is_empty() {
                continue;
            }
            let Some(clip) = scissor(primitive.clip_rect, pixels_per_point, display) else {
                continue;
            };
            // Overflowing a u32 index would need a quarter of a billion vertices from a debug
            // panel. Stopping is still better than wrapping the offsets.
            if self.vertices.len() + mesh.vertices.len() > u32::MAX as usize
                || self.indices.len() + mesh.indices.len() > u32::MAX as usize
            {
                break;
            }

            let vertex_offset = self.vertices.len() as u32;
            let index_offset = self.indices.len() as u32;
            self.vertices
                .extend(mesh.vertices.iter().map(|vertex| RsfOverlayVertex {
                    // egui tessellates in points; the header asks for physical pixels.
                    x: vertex.pos.x * pixels_per_point,
                    y: vertex.pos.y * pixels_per_point,
                    u: vertex.uv.x,
                    v: vertex.uv.y,
                    color: u32::from_le_bytes(vertex.color.to_array()),
                }));
            self.indices.extend_from_slice(&mesh.indices);
            self.calls.push(RsfOverlayDrawCall {
                index_offset,
                index_count: mesh.indices.len() as u32,
                vertex_offset,
                clip_x: clip[0],
                clip_y: clip[1],
                clip_width: clip[2],
                clip_height: clip[3],
                texture_id: texture_id_to_u64(mesh.texture_id),
            });
        }
    }
}

/// How large the panel should be drawn.
///
/// Chosen rather than measured, and the one number here that is pure taste: a panel laid out at
/// one point per pixel is unreadable on a 4K display, so it scales with the display height and
/// stops at 3x. There is no way to ask the host for a preferred scale in ABI version 1.
fn pixels_per_point(display_height: f32) -> f32 {
    (display_height / REFERENCE_HEIGHT).clamp(1.0, 3.0)
}

/// egui's texture ids as one number.
///
/// The overlay only ever produces managed ids, so the high bit is never actually set today. It is
/// still spelled out, because collapsing the two kinds onto the same numbers would make a user
/// texture silently alias the font atlas the day one appears.
fn texture_id_to_u64(id: epaint::TextureId) -> u64 {
    match id {
        epaint::TextureId::Managed(value) => value & !USER_TEXTURE_BIT,
        epaint::TextureId::User(value) => (value & !USER_TEXTURE_BIT) | USER_TEXTURE_BIT,
    }
}

const USER_TEXTURE_BIT: u64 = 1 << 63;

/// Turn egui's clip rectangle in points into a scissor rectangle in whole physical pixels.
///
/// Returns `None` for a rectangle with no area, which a renderer would otherwise turn into a
/// scissor that clips everything away.
fn scissor(clip: egui::Rect, pixels_per_point: f32, display: [u32; 2]) -> Option<[u32; 4]> {
    let width = display[0] as f32;
    let height = display[1] as f32;
    // Casting a float to an integer saturates in Rust, and NaN becomes zero, so a rectangle egui
    // could not compute becomes an empty one rather than a huge one.
    let min_x = (clip.min.x * pixels_per_point).round().clamp(0.0, width) as u32;
    let min_y = (clip.min.y * pixels_per_point).round().clamp(0.0, height) as u32;
    let max_x = (clip.max.x * pixels_per_point).round().clamp(0.0, width) as u32;
    let max_y = (clip.max.y * pixels_per_point).round().clamp(0.0, height) as u32;
    if max_x <= min_x || max_y <= min_y {
        return None;
    }
    Some([min_x, min_y, max_x - min_x, max_y - min_y])
}

/// Replace a value the host could not produce with one the overlay can lay out from.
///
/// A NaN mouse position propagates into egui's layout and comes back as NaN vertices, which is a
/// renderer's problem several thousand triangles later.
fn sanitise(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Quality;

    /// A display that maps points to pixels one to one, so a test can click a rectangle the panel
    /// reported without converting anything.
    const DISPLAY: [u32; 2] = [1280, 720];

    fn visible_input() -> FrameInput {
        FrameInput {
            display: DISPLAY,
            ..FrameInput::default()
        }
    }

    fn stats_with_backend() -> Stats<'static> {
        Stats {
            backend_loaded: true,
            backend_supported: true,
            backend_name: Some("Test backend"),
            render: [1024, 576],
            output: [2048, 1152],
            frames_presented: 240,
            frames_evaluated: 238,
            have_scene_color: true,
            have_depth: true,
            have_motion: true,
            motion_decoded: true,
            jitter_active: true,
            jitter_pixels: [0.25, -0.125],
            ..Stats::default()
        }
    }

    /// Lay out a frame with the mouse parked away from everything.
    fn idle_frame(overlay: &mut Overlay, stats: &Stats<'_>) -> Intent {
        overlay.frame(
            &FrameInput {
                mouse: [1.0, 1.0],
                ..visible_input()
            },
            stats,
        )
    }

    /// Run the overlay until its layout has settled.
    ///
    /// egui lays a window out once invisibly to learn how big it is, so the first frame after the
    /// panel appears produces no triangles and control rectangles that are not final yet. That is
    /// egui's behaviour rather than this crate's, and it costs one frame of a game that is
    /// rendering continuously, but a test that clicks a rectangle has to wait for it.
    fn settle(overlay: &mut Overlay, stats: &Stats<'_>) {
        for _ in 0..3 {
            idle_frame(overlay, stats);
        }
    }

    /// Press and release over `target`, which is a rectangle the panel reported last frame.
    /// egui completes a click on the release, so this is two frames.
    fn click(overlay: &mut Overlay, stats: &Stats<'_>, target: egui::Rect) -> [Intent; 2] {
        let centre = target.center();
        let press = overlay.frame(
            &FrameInput {
                mouse: [centre.x, centre.y],
                mouse_buttons: RSF_OVERLAY_MOUSE_LEFT,
                ..visible_input()
            },
            stats,
        );
        let release = overlay.frame(
            &FrameInput {
                mouse: [centre.x, centre.y],
                mouse_buttons: 0,
                ..visible_input()
            },
            stats,
        );
        [press, release]
    }

    fn quality_rect(overlay: &Overlay, level: Quality) -> egui::Rect {
        let index = Quality::ALL
            .iter()
            .position(|candidate| *candidate == level)
            .expect("every level is in ALL");
        overlay.controls().quality[index].expect("the panel laid out its quality selector")
    }

    #[test]
    fn a_click_on_a_quality_level_changes_the_intent_exactly_once() {
        let mut overlay = Overlay::new();
        let stats = stats_with_backend();
        settle(&mut overlay, &stats);

        let target = quality_rect(&overlay, Quality::Balanced);
        let [press, release] = click(&mut overlay, &stats, target);
        let changes = [press, release]
            .iter()
            .filter(|intent| intent.quality_changed)
            .count();
        assert_eq!(changes, 1, "one click is one change");
        assert_eq!(release.quality, Quality::Balanced);

        // The runtime has not applied it yet, and the panel must not keep asking for it every
        // frame while it waits.
        for _ in 0..5 {
            let intent = idle_frame(&mut overlay, &stats);
            assert!(!intent.quality_changed);
            assert_eq!(intent.quality, Quality::Balanced);
        }
    }

    #[test]
    fn the_selection_survives_being_hidden() {
        let mut overlay = Overlay::new();
        let stats = stats_with_backend();
        settle(&mut overlay, &stats);
        let target = quality_rect(&overlay, Quality::Performance);
        click(&mut overlay, &stats, target);

        for _ in 0..10 {
            let intent = overlay.frame(
                &FrameInput {
                    visible: false,
                    ..visible_input()
                },
                &stats,
            );
            assert_eq!(intent.quality, Quality::Performance);
            assert!(intent.is_idle());
            assert!(overlay.draw_calls().is_empty());
        }

        let intent = idle_frame(&mut overlay, &stats);
        assert_eq!(intent.quality, Quality::Performance);
        assert!(!intent.quality_changed);
        assert!(!overlay.draw_calls().is_empty(), "the panel came back");
    }

    #[test]
    fn a_click_on_the_enable_toggle_changes_the_intent_exactly_once() {
        let mut overlay = Overlay::new();
        let stats = stats_with_backend();
        settle(&mut overlay, &stats);

        let target = overlay
            .controls()
            .enabled
            .expect("the panel laid out its enable toggle");
        let [press, release] = click(&mut overlay, &stats, target);
        let changes = [press, release]
            .iter()
            .filter(|intent| intent.enabled_changed)
            .count();
        assert_eq!(changes, 1);
        assert!(
            release.enabled,
            "the stats said disabled, so a click enables"
        );

        for _ in 0..5 {
            let intent = idle_frame(&mut overlay, &stats);
            assert!(!intent.enabled_changed);
            assert!(intent.enabled);
        }
    }

    #[test]
    fn a_quality_level_cannot_be_picked_without_a_backend() {
        let mut overlay = Overlay::new();
        let stats = Stats {
            refusal_reason: Some("no backend was loaded for this device"),
            ..Stats::default()
        };
        settle(&mut overlay, &stats);

        let target = quality_rect(&overlay, Quality::UltraPerformance);
        let [press, release] = click(&mut overlay, &stats, target);
        assert!(press.is_idle() && release.is_idle());
        assert_eq!(release.quality, Quality::Native);

        // And the panel is not simply ignoring every click: the controls that are supposed to work
        // in this state still do, which is what makes the assertion above about the greyed out
        // selector rather than about a click that landed on nothing.
        let dump = overlay
            .controls()
            .dump
            .expect("the panel laid out its dump button");
        let [_, release] = click(&mut overlay, &stats, dump);
        assert!(release.dump_requested);
    }

    #[test]
    fn missing_inputs_and_unknown_values_still_lay_out() {
        let mut overlay = Overlay::new();
        let mut stats = Stats {
            frames_presented: 12,
            frames_refused: 12,
            last_result: -2_005_270_522,
            refusal_reason: Some("device does not support the backend"),
            backend_loaded: true,
            ..Stats::default()
        };
        // Nothing found, nothing decoded, no jitter, no sizes, and a quality level from a runtime
        // newer than this build.
        stats.set_quality(9);

        settle(&mut overlay, &stats);
        let intent = idle_frame(&mut overlay, &stats);
        assert!(intent.is_idle());
        assert!(!overlay.draw_calls().is_empty());
        assert!(
            overlay
                .vertices()
                .iter()
                .all(|vertex| vertex.x.is_finite() && vertex.y.is_finite())
        );
    }

    #[test]
    fn the_first_frame_produces_a_whole_font_atlas() {
        let mut overlay = Overlay::new();
        idle_frame(&mut overlay, &stats_with_backend());

        let updates = overlay.drain_texture_updates(16).to_vec();
        assert!(!updates.is_empty(), "the font atlas has to arrive somehow");
        for update in &updates {
            assert_eq!(
                update.pixels.len(),
                update.width as usize * update.height as usize * 4
            );
        }
        assert!(updates.iter().any(|update| update.is_whole_texture));
        // Drained, not repeated.
        assert!(overlay.drain_texture_updates(16).is_empty());
    }

    #[test]
    fn texture_updates_drain_in_batches() {
        let mut overlay = Overlay::new();
        idle_frame(&mut overlay, &stats_with_backend());

        let mut total = 0;
        loop {
            let batch = overlay.drain_texture_updates(1).len();
            if batch == 0 {
                break;
            }
            total += batch;
            assert_eq!(batch, 1);
        }
        assert!(total >= 1);
    }

    #[test]
    fn draw_calls_stay_inside_the_buffers_they_index() {
        let mut overlay = Overlay::new();
        settle(&mut overlay, &stats_with_backend());

        assert!(!overlay.draw_calls().is_empty());
        for call in overlay.draw_calls() {
            let start = call.index_offset as usize;
            let end = start + call.index_count as usize;
            assert!(end <= overlay.indices().len());
            for index in &overlay.indices()[start..end] {
                let vertex = call.vertex_offset as usize + *index as usize;
                assert!(vertex < overlay.vertices().len());
            }
            assert!(call.clip_x + call.clip_width <= DISPLAY[0]);
            assert!(call.clip_y + call.clip_height <= DISPLAY[1]);
            assert!(call.clip_width > 0 && call.clip_height > 0);
        }
    }

    #[test]
    fn a_zero_sized_display_draws_nothing_instead_of_panicking() {
        let mut overlay = Overlay::new();
        let intent = overlay.frame(
            &FrameInput {
                display: [0, 0],
                ..visible_input()
            },
            &stats_with_backend(),
        );
        assert!(intent.is_idle());
        assert!(overlay.draw_calls().is_empty());
    }

    #[test]
    fn a_nan_mouse_position_does_not_reach_the_vertices() {
        let mut overlay = Overlay::new();
        let stats = stats_with_backend();
        overlay.frame(
            &FrameInput {
                mouse: [f32::NAN, f32::INFINITY],
                delta_seconds: f32::NAN,
                ..visible_input()
            },
            &stats,
        );
        assert!(
            overlay
                .vertices()
                .iter()
                .all(|vertex| vertex.x.is_finite() && vertex.y.is_finite())
        );
    }

    #[test]
    fn the_panel_follows_the_runtime_when_it_reports_a_different_level() {
        let mut overlay = Overlay::new();
        let mut stats = stats_with_backend();
        idle_frame(&mut overlay, &stats);

        // Something other than this panel changed the level, so the panel shows what is in effect.
        stats.set_quality(Quality::UltraPerformance.to_abi());
        let intent = idle_frame(&mut overlay, &stats);
        assert_eq!(intent.quality, Quality::UltraPerformance);
        assert!(!intent.quality_changed, "following is not asking");
    }

    #[test]
    fn a_request_stops_pending_once_the_runtime_reports_it() {
        let mut overlay = Overlay::new();
        let mut stats = stats_with_backend();
        settle(&mut overlay, &stats);
        let target = quality_rect(&overlay, Quality::Quality);
        click(&mut overlay, &stats, target);

        stats.set_quality(Quality::Quality.to_abi());
        let intent = idle_frame(&mut overlay, &stats);
        assert_eq!(intent.quality, Quality::Quality);

        // And a later change by something else is followed rather than fought over.
        stats.set_quality(Quality::Native.to_abi());
        let intent = idle_frame(&mut overlay, &stats);
        assert_eq!(intent.quality, Quality::Native);
    }

    #[test]
    fn the_dump_button_asks_once() {
        let mut overlay = Overlay::new();
        let stats = stats_with_backend();
        settle(&mut overlay, &stats);

        let target = overlay
            .controls()
            .dump
            .expect("the panel laid out its dump button");
        let [press, release] = click(&mut overlay, &stats, target);
        assert_eq!(
            [press, release]
                .iter()
                .filter(|intent| intent.dump_requested)
                .count(),
            1
        );
        assert!(!idle_frame(&mut overlay, &stats).dump_requested);
    }

    #[test]
    fn a_button_released_while_hidden_is_not_a_click_when_it_returns() {
        let mut overlay = Overlay::new();
        let stats = stats_with_backend();
        settle(&mut overlay, &stats);
        let target = quality_rect(&overlay, Quality::Balanced);
        let centre = target.center();

        // Pressed over the control while hidden, released while hidden.
        for buttons in [RSF_OVERLAY_MOUSE_LEFT, 0] {
            overlay.frame(
                &FrameInput {
                    mouse: [centre.x, centre.y],
                    mouse_buttons: buttons,
                    visible: false,
                    ..visible_input()
                },
                &stats,
            );
        }

        let intent = overlay.frame(
            &FrameInput {
                mouse: [centre.x, centre.y],
                ..visible_input()
            },
            &stats,
        );
        assert!(intent.is_idle());
    }

    /// The other half of the case above: the press happened while the panel was visible, so egui
    /// saw it, and the release happened while it was hidden, so egui did not. egui is left holding
    /// a button nobody is pressing, which shows as widgets stuck in their pressed colour and, if
    /// the press landed on the window itself, a panel that follows the mouse the moment it returns.
    ///
    /// Asserted against egui's own pointer state rather than a symptom, because the symptom
    /// depends on where the press landed and the wrong state does not.
    #[test]
    fn a_press_the_panel_saw_is_released_even_if_it_was_hidden_for_it() {
        let mut overlay = Overlay::new();
        let stats = stats_with_backend();
        settle(&mut overlay, &stats);
        let target = quality_rect(&overlay, Quality::Balanced);
        let centre = target.center();

        overlay.frame(
            &FrameInput {
                mouse: [centre.x, centre.y],
                mouse_buttons: RSF_OVERLAY_MOUSE_LEFT,
                ..visible_input()
            },
            &stats,
        );
        assert!(
            overlay.context.input(|input| input.pointer.primary_down()),
            "the press has to reach egui, or this test proves nothing"
        );

        // Hidden, and released out of sight.
        overlay.frame(
            &FrameInput {
                mouse: [centre.x, centre.y],
                mouse_buttons: RSF_OVERLAY_MOUSE_LEFT,
                visible: false,
                ..visible_input()
            },
            &stats,
        );
        overlay.frame(
            &FrameInput {
                mouse: [centre.x, centre.y],
                visible: false,
                ..visible_input()
            },
            &stats,
        );

        // Back, with the mouse somewhere else and no button held.
        overlay.frame(
            &FrameInput {
                mouse: [centre.x + 40.0, centre.y + 60.0],
                ..visible_input()
            },
            &stats,
        );
        assert!(
            !overlay.context.input(|input| input.pointer.primary_down()),
            "egui is still holding a button the host says is up"
        );
    }

    /// A host that always reports a zero frame time is allowed by the header, and it used to leave
    /// the panel stuck part way through egui's fade in, drawn at a fraction of its opacity for as
    /// long as the game ran. Driving it against a host with a working clock has to reach the same
    /// pixels.
    #[test]
    fn a_host_with_no_clock_still_gets_a_finished_panel() {
        let stats = stats_with_backend();
        let mut timed = Overlay::new();
        let mut untimed = Overlay::new();

        for _ in 0..10 {
            timed.frame(
                &FrameInput {
                    mouse: [1.0, 1.0],
                    ..visible_input()
                },
                &stats,
            );
            untimed.frame(
                &FrameInput {
                    mouse: [1.0, 1.0],
                    delta_seconds: 0.0,
                    ..visible_input()
                },
                &stats,
            );
        }

        assert!(!timed.vertices().is_empty());
        assert_eq!(timed.vertices(), untimed.vertices());
        assert_eq!(timed.draw_calls(), untimed.draw_calls());
    }

    #[test]
    fn the_scale_heuristic_stays_within_its_bounds() {
        assert_eq!(pixels_per_point(1080.0), 1.0);
        assert_eq!(pixels_per_point(720.0), 1.0);
        assert_eq!(pixels_per_point(2160.0), 2.0);
        assert_eq!(pixels_per_point(100_000.0), 3.0);
    }

    #[test]
    fn an_empty_scissor_is_dropped_rather_than_drawn() {
        let display = [100, 100];
        assert_eq!(
            scissor(
                egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(10.0, 10.0)),
                1.0,
                display
            ),
            Some([0, 0, 10, 10])
        );
        assert_eq!(
            scissor(
                egui::Rect::from_min_max(Pos2::new(5.0, 5.0), Pos2::new(5.0, 20.0)),
                1.0,
                display
            ),
            None
        );
        // Beyond the display, and inverted, both come back as nothing to draw.
        assert_eq!(
            scissor(
                egui::Rect::from_min_max(Pos2::new(200.0, 200.0), Pos2::new(300.0, 300.0)),
                1.0,
                display
            ),
            None
        );
        assert_eq!(
            scissor(
                egui::Rect::from_min_max(Pos2::new(f32::NAN, 0.0), Pos2::new(f32::NAN, 10.0)),
                1.0,
                display
            ),
            None
        );
    }
}
