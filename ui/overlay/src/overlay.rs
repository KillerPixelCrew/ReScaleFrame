//! Persistent egui state, mouse snapshot translation and owned output buffers for the C renderer.

use egui::{Event, PointerButton, Pos2, RawInput, ViewportId, epaint};

use crate::abi::{
    RSF_OVERLAY_MOUSE_LEFT, RSF_OVERLAY_MOUSE_MIDDLE, RSF_OVERLAY_MOUSE_RIGHT, RsfOverlayDrawCall,
    RsfOverlayVertex,
};
use crate::model::{FrameInput, Intent, Stats, TextureUpdate};
use crate::panel::{self, Controls, Selection};

/// Fixed atlas cap, chosen without querying a GPU. egui may rebuild atlas space at this limit.
const MAX_TEXTURE_SIDE: usize = 2048;

/// Reference display height for the scale heuristic below.
const REFERENCE_HEIGHT: f32 = 1080.0;

/// Persistent panel state and buffers. Consume frame output and texture work before the next frame.
pub struct Overlay {
    context: egui::Context,
    selection: Selection,
    controls: Controls,
    performance: crate::performance::Meter,

    /// Animation time in seconds, advanced from host deltas without reading a platform clock.
    time: f64,
    last_mouse: Option<[f32; 2]>,
    last_buttons: u32,
    /// egui's button history, kept separate from the host's history across hidden frames.
    told_egui_buttons: u32,
    was_visible: bool,

    /// Owned mesh buffers; C borrows them until the next frame or destruction.
    vertices: Vec<RsfOverlayVertex>,
    indices: Vec<u32>,
    calls: Vec<RsfOverlayDrawCall>,

    /// Drainable texture work. Pixel allocations remain live until the next frame or destruction.
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

        // One pass bounds render-thread work and prevents replaying the same click during layout.
        context.options_mut(|options| {
            options.max_passes = std::num::NonZeroUsize::new(1).expect("1 is not zero");
        });

        Self {
            context,
            selection: Selection::default(),
            controls: Controls::default(),
            performance: crate::performance::Meter::default(),
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
    /// Hidden frames still reconcile requested settings and sample rates. They may draw the
    /// noninteractive startup hint or HUD, but produce no control rectangles or mouse intents.
    /// Each call replaces all mesh buffers and undrained texture work from the previous frame.
    pub fn frame(&mut self, input: &FrameInput, stats: &Stats<'_>) -> Intent {
        self.vertices.clear();
        self.indices.clear();
        self.calls.clear();
        self.texture_updates.clear();
        self.texture_updates_taken = 0;
        self.textures_to_free.clear();
        self.textures_to_free_taken = 0;

        self.selection.reconcile(stats);
        let rates = self.performance.sample(stats);

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
            let alpha = sanitise(input.startup_hint_alpha, 0.0).clamp(0.0, 1.0);
            if !input.visible
                && (alpha > 0.0 || stats.show_performance_hud)
                && display[0] > 0.0
                && display[1] > 0.0
            {
                let scale = pixels_per_point(display[1]);
                let mut raw = RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        egui::vec2(display[0] / scale, display[1] / scale),
                    )),
                    max_texture_side: Some(MAX_TEXTURE_SIDE),
                    ..RawInput::default()
                };
                if let Some(viewport) = raw.viewports.get_mut(&ViewportId::ROOT) {
                    viewport.native_pixels_per_point = Some(scale);
                }
                // No pointer events, widgets or cursor while the hint is shown.
                let output = self.context.run_ui(raw, |ui| {
                    if alpha > 0.0 {
                        paint_startup_hint(ui.ctx(), alpha);
                    }
                    if stats.show_performance_hud {
                        panel::performance_hud(ui.ctx(), rates, stats);
                    }
                });
                self.collect_textures(output.textures_delta);
                let primitives = self
                    .context
                    .tessellate(output.shapes, output.pixels_per_point);
                self.collect_primitives(&primitives, output.pixels_per_point, input.display);
            }
            return intent;
        }

        let points_per_pixel = 1.0 / pixels_per_point(display[1]);
        let raw = self.raw_input(input, display, points_per_pixel);

        let selection = &self.selection;
        let controls = &mut self.controls;
        *controls = Controls::default();
        let pointer = Pos2::new(
            sanitise(input.mouse[0], 0.0) * points_per_pixel,
            sanitise(input.mouse[1], 0.0) * points_per_pixel,
        );
        let output = self.context.run_ui(raw, |ui| {
            panel::show(ui.ctx(), selection, stats, &mut intent, controls);
            if stats.show_performance_hud {
                panel::performance_hud(ui.ctx(), rates, stats);
            }
            paint_cursor(ui.ctx(), pointer);
        });

        // Commit after layout so all widgets read the same starting selection.
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
        // Host history detects new presses; egui history detects releases missed while hidden.
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
            // Release stale egui holds at the current pointer position after the move event.
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

        // Advance animations by an assumed 60 Hz when the host provides no valid positive delta.
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

    /// Copy atlas pixels into owned RGBA patches and preserve egui's upload/free ordering.
    fn collect_textures(&mut self, delta: epaint::textures::TexturesDelta) {
        for (id, patch) in delta.set {
            let epaint::ImageData::Color(image) = &patch.image;
            let [width, height] = image.size;
            // Reject malformed patches before a native renderer can copy beyond the allocation.
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

    /// Flatten meshes in paint order, converting points to physical pixels and retaining local
    /// indices. Skip unsupported callbacks and clipped meshes; stop before u32 offsets overflow.
    fn collect_primitives(
        &mut self,
        primitives: &[epaint::ClippedPrimitive],
        pixels_per_point: f32,
        display: [u32; 2],
    ) {
        for primitive in primitives {
            let epaint::Primitive::Mesh(mesh) = &primitive.primitive else {
                // The native ABI has no paint callback representation.
                continue;
            };
            if mesh.indices.is_empty() || mesh.vertices.is_empty() {
                continue;
            }
            let Some(clip) = scissor(primitive.clip_rect, pixels_per_point, display) else {
                continue;
            };
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

/// Paint the host-timed startup hint without interactive widgets.
fn paint_startup_hint(ctx: &egui::Context, alpha: f32) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("rsf_startup_hint"),
    ));
    let text = painter.layout_no_wrap(
        "ReScaleFrame · Press Insert for settings".to_owned(),
        egui::FontId::proportional(18.0),
        egui::Color32::WHITE.gamma_multiply(alpha),
    );
    let position = Pos2::new(24.0, 24.0);
    let rect = egui::Rect::from_min_size(position, text.size() + egui::vec2(24.0, 16.0));
    painter.rect_filled(
        rect,
        6.0,
        egui::Color32::from_black_alpha(200).gamma_multiply(alpha),
    );
    painter.galley(
        position + egui::vec2(12.0, 8.0),
        text,
        egui::Color32::WHITE.gamma_multiply(alpha),
    );
}

/// Draw a pointer because AC7 hides the system cursor.
fn paint_cursor(ctx: &egui::Context, position: Pos2) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("rsf_overlay_cursor"),
    ));
    // Keep the cursor convex for egui's polygon tessellator.
    let points = vec![
        position,
        position + egui::vec2(0.0, 17.0),
        position + egui::vec2(12.0, 12.0),
    ];
    painter.add(egui::Shape::convex_polygon(
        points,
        egui::Color32::WHITE,
        egui::Stroke::new(1.0, egui::Color32::BLACK),
    ));
}

/// Scale from display height: 1x through 1080 pixels, capped at 3x. No host DPI is supplied.
fn pixels_per_point(display_height: f32) -> f32 {
    (display_height / REFERENCE_HEIGHT).clamp(1.0, 3.0)
}

/// Reserve bit 63 for user textures so their IDs cannot alias managed atlas IDs.
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

/// Replace nonfinite host values before they reach layout or vertex output.
fn sanitise(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn performance_hud_draws_without_opening_the_interactive_panel() {
        let mut overlay = Overlay::new();
        let stats = Stats {
            show_performance_hud: true,
            ..Stats::default()
        };
        let input = FrameInput {
            visible: false,
            display: [1280, 720],
            ..FrameInput::default()
        };
        for _ in 0..3 {
            assert!(overlay.frame(&input, &stats).is_idle());
        }
        assert!(!overlay.vertices.is_empty());
        assert_eq!(overlay.controls, Controls::default());
        assert!(!overlay.was_visible);
    }
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
    fn startup_hint_draws_without_opening_controls_or_accepting_clicks() {
        let mut overlay = Overlay::new();
        let stats = stats_with_backend();
        let mut input = FrameInput {
            visible: false,
            startup_hint_alpha: 1.0,
            mouse_buttons: RSF_OVERLAY_MOUSE_LEFT,
            ..visible_input()
        };
        for _ in 0..3 {
            assert!(overlay.frame(&input, &stats).is_idle());
        }
        assert!(!overlay.draw_calls().is_empty());
        assert!(overlay.controls().enabled.is_none());
        input.startup_hint_alpha = 0.0;
        assert!(overlay.frame(&input, &stats).is_idle());
        assert!(overlay.draw_calls().is_empty());
    }

    #[test]
    fn a_backend_click_requests_once_and_keeps_the_effective_selection() {
        let mut overlay = Overlay::new();
        let mut stats = stats_with_backend();
        settle(&mut overlay, &stats);
        let target = overlay.controls().backend[2].expect("FSR3 control was laid out");
        let [press, release] = click(&mut overlay, &stats, target);
        assert!(!press.backend_changed);
        assert!(release.backend_changed);
        assert_eq!(release.backend, 3);
        assert!(!idle_frame(&mut overlay, &stats).backend_changed);
        stats.requested_backend = 3;
        stats.last_switch_result = -7;
        assert!(!idle_frame(&mut overlay, &stats).backend_changed);
        stats.backend = 3;
        stats.last_switch_result = 0;
        assert!(!idle_frame(&mut overlay, &stats).backend_changed);
    }

    #[test]
    fn generation_provider_selection_is_separate_and_persisted() {
        let mut overlay = Overlay::new();
        let mut stats = stats_with_backend();
        stats.generation.backend = 1;
        stats.generation.requested_backend = 1;
        stats.generation.backend_choices = 0x3b;
        stats.generation.available = true;
        settle(&mut overlay, &stats);
        let target = overlay.controls().fg_backend[2].expect("FSR3 generation choice was laid out");
        let [press, release] = click(&mut overlay, &stats, target);
        assert!(!press.fg_backend_changed);
        assert!(release.fg_backend_changed);
        assert_eq!(release.fg_backend, 3);
        assert!(!release.backend_changed);
        assert!(!release.fg_changed);
        stats.generation.requested_backend = 3;
        assert!(!idle_frame(&mut overlay, &stats).fg_backend_changed);
        assert_eq!(stats.generation.backend, 1);
        let off = overlay.controls().fg_backend[0].expect("Off generation choice was laid out");
        let [_, release] = click(&mut overlay, &stats, off);
        assert!(release.fg_backend_changed);
        assert_eq!(release.fg_backend, 0);
    }

    #[test]
    fn an_unimplemented_generation_provider_cannot_be_selected() {
        let mut overlay = Overlay::new();
        let mut stats = stats_with_backend();
        stats.generation.backend_choices = 0x39; // Unity has no native DLSS-G presentation yet.
        settle(&mut overlay, &stats);
        let target = overlay.controls().fg_backend[1].expect("Disabled DLSS-G choice was laid out");
        let [press, release] = click(&mut overlay, &stats, target);
        assert!(!press.fg_backend_changed);
        assert!(!release.fg_backend_changed);
    }

    #[test]
    fn supported_mfg_multiplier_maps_to_generated_frames() {
        let mut overlay = Overlay::new();
        let mut stats = stats_with_backend();
        stats.generation.available = true;
        stats.generation.backend = 5;
        stats.generation.requested_backend = 5;
        stats.generation.requested_mode = 1;
        stats.generation.requested_generated = 1;
        stats.generation.max_generated = 3;
        settle(&mut overlay, &stats);
        let track = overlay
            .controls()
            .fg_multiplier
            .expect("MFG track exists for supported hardware");
        let right = egui::Rect::from_center_size(
            egui::pos2(track.right() - 2.0, track.center().y),
            egui::vec2(1.0, 1.0),
        );
        let [press, release] = click(&mut overlay, &stats, right);
        let changed = if press.fg_changed { press } else { release };
        assert!(changed.fg_changed);
        assert_eq!(changed.fg_generated, 3);
        assert_eq!(changed.fg_mode, 1);
        stats.generation.max_generated = 1;
        idle_frame(&mut overlay, &stats);
        assert!(overlay.controls().fg_multiplier.is_some());
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
    fn a_preset_can_be_selected_before_enabling_dlss() {
        let mut overlay = Overlay::new();
        let stats = Stats::default();
        settle(&mut overlay, &stats);
        let target = quality_rect(&overlay, Quality::UltraPerformance);
        let [press, release] = click(&mut overlay, &stats, target);
        assert_eq!(
            [press, release]
                .iter()
                .filter(|i| i.quality_changed)
                .count(),
            1
        );
        assert_eq!(release.quality, Quality::UltraPerformance);
        assert!(!release.enabled_changed);
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
    fn the_consumer_panel_has_only_enable_and_presets() {
        let mut overlay = Overlay::new();
        let stats = stats_with_backend();
        settle(&mut overlay, &stats);
        assert!(overlay.controls().enabled.is_some());
        assert!(overlay.controls().quality.iter().all(Option::is_some));
        let intent = idle_frame(&mut overlay, &stats);
        assert!(!intent.dump_requested && !intent.capture_requested);
        assert!(!intent.reinsert_changed && !intent.jitter_changed && !intent.debug_view_changed);
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
