//! The panel itself: what it shows, and what a click on it means.
//!
//! This is the half worth testing. It needs an egui context and nothing else, so every widget here
//! can be laid out, clicked and asserted on without a device, a swap chain or a game.
//!
//! Counter-derived FPS is shown separately from generation activity. Latency, quality and
//! unmeasured speed improvements are not inferred from a requested mode.

use egui::{Color32, Context, Rect, RichText, Ui, Window};

use crate::model::{Intent, Quality, Stats};

/// Present but not in the state it needs to be in, and anything the user should read before
/// believing the rest of the panel.
const WARN: Color32 = Color32::from_rgb(0xe8, 0xb3, 0x3a);
/// Detail that is true but not load bearing.
const MUTED: Color32 = Color32::from_rgb(0x9a, 0x9a, 0x9a);

/// What the panel currently shows as chosen, and what it is waiting to see take effect.
///
/// The header points out that the quality last clicked and the quality in effect differ whenever a
/// change has been requested and not yet applied. That gap is this struct: `requested` holds the
/// user's ask until the runtime reports it back through the stats, and until then the panel shows
/// the ask and says that it has not landed. Without this the selection would visibly snap back to
/// the old level for however many frames the runtime takes to apply it.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Selection {
    quality: Quality,
    requested_quality: Option<Quality>,
    enabled: bool,
    requested_enabled: Option<bool>,
}

impl Selection {
    /// Reconcile with what the runtime says is in effect. Called once per frame, whether or not
    /// the panel is visible, so a change applied while it was hidden is not shown as still pending.
    pub(crate) fn reconcile(&mut self, stats: &Stats<'_>) {
        if self.requested_quality == stats.quality {
            self.requested_quality = None;
        }
        self.quality = self
            .requested_quality
            .or(stats.quality)
            .unwrap_or(self.quality);

        if self.requested_enabled == Some(stats.enabled) {
            self.requested_enabled = None;
        }
        self.enabled = self.requested_enabled.unwrap_or(stats.enabled);
    }

    /// Take on what the user asked for in a laid out frame.
    ///
    /// Applied after the layout rather than inside it, so every pass over one frame sees the same
    /// starting point. See the call site in [`crate::Overlay::frame`].
    pub(crate) fn apply(&mut self, intent: &Intent) {
        if intent.quality_changed {
            self.quality = intent.quality;
            self.requested_quality = Some(intent.quality);
        }
        if intent.enabled_changed {
            self.enabled = intent.enabled;
            self.requested_enabled = Some(intent.enabled);
        }
    }

    /// The level the panel shows as chosen.
    pub(crate) fn quality(&self) -> Quality {
        self.quality
    }

    /// Whether the panel shows reconstruction as on.
    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }
}

/// Where the panel's interactive controls ended up, in egui points.
///
/// The C boundary does not pass these on. They exist because the unit tests click the controls,
/// and clicking a hard coded coordinate would mean every layout change silently stops testing
/// anything: a click that lands on nothing asserts nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Controls {
    /// One rectangle per quality level, in [`Quality::ALL`] order. `None` when the panel was not
    /// laid out this frame, or the window was collapsed.
    pub quality: [Option<Rect>; 5],
    /// The enable toggle.
    pub enabled: Option<Rect>,
    /// One control per SR backend.
    pub backend: [Option<Rect>; 5],
    /// Off, DLSS-G, FSR3, FSR4 and XeSS generation choices.
    pub fg_backend: [Option<Rect>; 5],
    /// Track of the hardware-supported frame generation multiplier slider.
    pub fg_multiplier: Option<Rect>,
}

/// Lay out one frame of the panel, recording what the user did into `intent`.
///
/// `selection` is read only: what the user asked for goes into `intent` and is folded back into
/// the selection by the caller once the frame is over.
pub(crate) fn show(
    ctx: &Context,
    selection: &Selection,
    stats: &Stats<'_>,
    intent: &mut Intent,
    controls: &mut Controls,
) {
    Window::new("ReScaleFrame")
        .default_pos([24.0, 24.0])
        .default_width(340.0)
        .resizable(false)
        // The first frame after this window appears, egui lays it out invisibly to learn how big
        // it is, so that frame produces no triangles. That is worth knowing when the renderer on
        // the other side of the header draws its first frame and sees nothing.
        .show(ctx, |ui| {
            body(ui, selection, stats, intent, controls);
        });
}

fn body(
    ui: &mut Ui,
    selection: &Selection,
    stats: &Stats<'_>,
    intent: &mut Intent,
    controls: &mut Controls,
) {
    controls_section(ui, selection, stats, intent, controls);
    if let Some(reason) = stats.refusal_reason {
        ui.label(RichText::new(reason).color(WARN));
    }
    ui.separator();
    ui.label(RichText::new("Insert to close").small().color(MUTED));
}

fn controls_section(
    ui: &mut Ui,
    selection: &Selection,
    stats: &Stats<'_>,
    intent: &mut Intent,
    controls: &mut Controls,
) {
    let mut performance = stats.show_performance_hud;
    if ui.checkbox(&mut performance, "Show FPS overlay").changed() {
        intent.performance_hud_changed = true;
        intent.performance_hud = performance;
    }
    let mut enabled = selection.enabled;
    let toggle = ui.checkbox(&mut enabled, "Enable upscaling");
    controls.enabled = Some(toggle.rect);
    if toggle.changed() {
        intent.enabled = enabled;
        intent.enabled_changed = true;
    }

    ui.add_enabled_ui(stats.backend_loaded, |ui| {
        ui.horizontal_wrapped(|ui| {
            let mut backend = stats.backend;
            for (index, (id, name)) in [
                (1, "DLSS"),
                (2, "FSR2"),
                (3, "FSR3"),
                (4, "FSR4"),
                (5, "XeSS"),
            ]
            .iter()
            .enumerate()
            {
                let response = ui.radio_value(&mut backend, *id, *name);
                controls.backend[index] = Some(response.rect);
            }
            if backend != stats.backend {
                intent.backend_changed = true;
                intent.backend = backend;
            }
            if ui.radio(stats.backend == 6, "FSR1").clicked() {
                intent.backend_changed = true;
                intent.backend = 6;
            }
        });
    });
    if stats.last_switch_result != 0 {
        ui.label(
            RichText::new("Requested configuration refused; check runtime status").color(WARN),
        );
    }

    ui.separator();
    let generation = stats.generation;
    let runtime_switching = generation.backend_choices & 0x8000_0000 != 0;
    ui.label("Frame generation provider");
    let mut fg_backend = generation.requested_backend;
    ui.horizontal_wrapped(|ui| {
        for (index, (id, name)) in [
            (0, "Off"),
            (1, "DLSS-G"),
            (3, "FSR3"),
            (4, "FSR4"),
            (5, "XeSS"),
        ]
        .into_iter()
        .enumerate()
        {
            let allowed = generation.backend_choices & (1 << id) != 0;
            let response = ui.add_enabled(allowed, egui::RadioButton::new(fg_backend == id, name));
            controls.fg_backend[index] = Some(response.rect);
            if response.clicked() && fg_backend != id {
                fg_backend = id;
                intent.fg_backend_changed = true;
                intent.fg_backend = id;
            }
        }
    });
    let pending = fg_backend != generation.backend;
    if pending {
        ui.label(
            RichText::new(format!(
                "{} {}{}",
                if runtime_switching {
                    if generation.selection_result == 0 {
                        "Applying"
                    } else {
                        "Requested"
                    }
                } else {
                    "Saved:"
                },
                fg_backend_name(fg_backend),
                if runtime_switching {
                    if generation.selection_result == 0 {
                        " at the next frame."
                    } else {
                        "."
                    }
                } else {
                    ". Awaiting the presentation owner."
                }
            ))
            .color(MUTED),
        );
    } else if generation.available {
        ui.label(
            RichText::new(format!(
                "Current provider: {}",
                fg_backend_name(generation.backend)
            ))
            .small()
            .color(MUTED),
        );
    }
    ui.label(
        RichText::new(if runtime_switching {
            "Provider changes apply during play."
        } else {
            "GPU compatibility is checked at startup."
        })
        .small()
        .color(MUTED),
    );
    if generation.selection_result != 0 {
        ui.label(
            RichText::new(if runtime_switching {
                "Provider change failed; current provider retained."
            } else {
                "Could not save the provider choice."
            })
            .color(MUTED),
        );
    }
    ui.add_enabled_ui(generation.available && !pending, |ui| {
        let mut mode = generation.requested_mode;
        let mut count = generation.requested_generated.max(1);
        let mut enabled = mode != 0;
        if ui.checkbox(&mut enabled, "Frame Generation").changed() {
            mode = u32::from(enabled);
            intent.fg_changed = true;
        }
        {
            let maximum = generation.max_generated.saturating_add(1).max(2);
            let mut multiplier = count.saturating_add(1).clamp(2, maximum);
            let response = ui.add_enabled(
                generation.max_generated > 1,
                egui::Slider::new(&mut multiplier, 2..=maximum)
                    .text("Frame generation multiplier")
                    .custom_formatter(|value, _| format!("{value:.0}x")),
            );
            controls.fg_multiplier = Some(Rect::from_min_max(
                response.rect.min,
                egui::pos2(
                    response.rect.left() + ui.spacing().slider_width,
                    response.rect.bottom(),
                ),
            ));
            if response.changed() {
                count = multiplier - 1;
            }
            intent.fg_changed |= response.changed();
            if generation.available && generation.max_generated == 1 {
                ui.label(
                    RichText::new("This provider reports a maximum of 2x on the current GPU.")
                        .small()
                        .color(MUTED),
                );
            }
        }
        if intent.fg_changed {
            intent.fg_mode = mode;
            intent.fg_generated = count;
        }
    });
    if !generation.available {
        ui.label(
            RichText::new("Select a supported provider to enable frame generation")
                .small()
                .color(MUTED),
        );
    } else if generation.active {
        ui.label(format!(
            "Generating {} frame(s) per rendered frame",
            generation.effective_generated
        ));
    } else if generation.requested_mode != 0 {
        let reason = match generation.reason {
            1 => "Waiting for a matching scene and window",
            2 => "Waiting for reconstruction inputs",
            3 => "Waiting for a complete frame submission",
            4 => "Waiting for flight, hangar, or briefing",
            5 => "Suspended for a camera cut",
            6 => "Current VSync mode is unsupported",
            _ => "Waiting for SDK-confirmed generation",
        };
        ui.label(RichText::new(reason).color(MUTED));
    }
    ui.add_enabled_ui(generation.reflex_available, |ui| {
        if generation.backend == 1 && generation.effective_reflex != generation.requested_reflex {
            ui.label(
                RichText::new("DLSS-G requires Reflex On while generating frames.")
                    .small()
                    .color(MUTED),
            );
        }
        ui.horizontal(|ui| {
            ui.label("Reflex");
            let mut mode = generation.requested_reflex;
            for (id, name) in [(0, "Off"), (1, "On"), (2, "On + Boost")] {
                ui.radio_value(&mut mode, id, name);
            }
            if mode != generation.requested_reflex {
                intent.reflex_changed = true;
                intent.reflex_mode = mode;
            }
        });
        // Reflex's own limiter. The value is rendered frames, so generation multiplies it.
        // The panel receives the mouse only: the value is dragged, never typed.
        let presents = if generation.requested_mode != 0 {
            f64::from(generation.requested_generated.max(1) + 1)
        } else {
            1.0
        };
        let refresh_us = if generation.display_refresh_mhz > 0 {
            1.0e9 / f64::from(generation.display_refresh_mhz)
        } else {
            0.0
        };
        let stored = generation.frame_limit_us;
        ui.horizontal(|ui| {
            ui.label("Frame limit");
            let mut fps = if stored == 0 {
                0.0
            } else {
                1.0e6 / f64::from(stored)
            };
            let dragged = ui
                .add(
                    egui::DragValue::new(&mut fps)
                        .range(0.0..=500.0)
                        .speed(0.25)
                        .custom_formatter(|value, _| {
                            if value < 1.0 {
                                "Off".to_owned()
                            } else {
                                format!("{value:.1} fps")
                            }
                        }),
                )
                .changed();
            let mut requested = None;
            if dragged {
                requested = Some(if fps < 1.0 { 0.0 } else { 1.0e6 / fps });
            }
            // The cap OptiScaler calculates for VRR: presented frames 0.3 ms slower than the
            // refresh interval, here converted to the rendered frames this limit counts.
            if ui
                .add_enabled(refresh_us > 0.0, egui::Button::new("VRR"))
                .clicked()
            {
                requested = Some(presents * (refresh_us + 300.0));
            }
            if ui.button("Off").clicked() {
                requested = Some(0.0);
            }
            if let Some(interval) = requested {
                // Saturating float to integer conversion; the range above keeps it small.
                let interval = interval.round() as u32;
                if interval != stored {
                    intent.frame_limit_changed = true;
                    intent.frame_limit_us = interval;
                }
            }
        });
        let note = if stored == 0 {
            if refresh_us > 0.0 {
                format!(
                    "Drag the value. VRR caps presented frames just under {:.0} Hz.",
                    1.0e6 / refresh_us
                )
            } else {
                "Drag the value. It limits rendered frames, before generation.".to_owned()
            }
        } else {
            let rendered = 1.0e6 / f64::from(stored);
            format!(
                "{rendered:.1} fps rendered, up to {:.1} presented",
                rendered * presents
            )
        };
        ui.label(RichText::new(note).small().color(MUTED));
    });
    if generation.last_result != 0 {
        ui.label(
            RichText::new(format!(
                "Frame generation request refused ({})",
                generation.last_result
            ))
            .color(WARN),
        );
    }

    // Presets can be saved while disabled; startup applies the selected level.
    ui.horizontal_wrapped(|ui| {
        ui.scope(|ui| {
            let mut choice = selection.quality;
            for (index, level) in Quality::ALL.iter().enumerate() {
                let response = ui.radio_value(&mut choice, *level, level.label());
                controls.quality[index] = Some(response.rect);
            }
            if choice != selection.quality {
                intent.quality = choice;
                intent.quality_changed = true;
            }
        });
    });

    match (selection.requested_quality, stats.quality) {
        (Some(requested), effect) => {
            let in_effect = match effect {
                Some(level) => level.label().to_owned(),
                None => format!("unknown ({})", stats.quality_raw),
            };
            ui.label(
                RichText::new(format!(
                    "requested {}, still {} in effect",
                    requested.label(),
                    in_effect
                ))
                .color(WARN),
            );
        }
        (None, None) => {
            ui.label(
                RichText::new(format!(
                    "runtime reports quality {}, which this build does not know",
                    stats.quality_raw
                ))
                .color(WARN),
            );
        }
        (None, Some(_)) => {}
    }

    // The same gap as above, for the toggle. Without this the box stays ticked while the runtime
    // reports reconstruction off, which is the panel claiming a state that is not in effect, and it
    // stays that way for as long as the runtime never applies it. Reconcile has already cleared the
    // request by the time the two agree, so a request still here is one that has not landed.
    if let Some(requested) = selection.requested_enabled {
        ui.label(
            RichText::new(format!(
                "requested {}, still {} in effect",
                on_off(requested),
                on_off(stats.enabled)
            ))
            .color(WARN),
        );
    }
}

fn fg_backend_name(backend: u32) -> &'static str {
    match backend {
        1 => "DLSS-G",
        3 => "FSR3",
        4 => "FSR4",
        5 => "XeSS",
        _ => "Off",
    }
}

pub(crate) fn performance_hud(ctx: &Context, rates: crate::performance::Rates, stats: &Stats<'_>) {
    let format_rate =
        |rate: Option<f64>| rate.map_or_else(|| "--".to_owned(), |value| format!("{value:.1}"));
    egui::Area::new(egui::Id::new("rsf-performance"))
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 12.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(Color32::from_black_alpha(190))
                .inner_margin(9.0)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("Rendered FPS   {}", format_rate(rates.rendered)))
                            .monospace(),
                    );
                    ui.label(
                        RichText::new(format!("Presented FPS  {}", format_rate(rates.presented)))
                            .monospace(),
                    );
                    let generation = stats.generation;
                    let state = if generation.active {
                        format!("FG active  {}x", generation.effective_generated + 1)
                    } else if generation.requested_mode == 0 {
                        "FG off".to_owned()
                    } else if !generation.available {
                        "FG unavailable".to_owned()
                    } else {
                        "FG suspended".to_owned()
                    };
                    ui.label(RichText::new(state).small().color(if generation.active {
                        Color32::LIGHT_GREEN
                    } else {
                        MUTED
                    }));
                });
        });
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::epaint::Shape;

    /// Everything the panel wrote this frame, joined, so a test can assert on what it says and not
    /// only on what it returns. Read out of the shapes before they become triangles, which is the
    /// last point where the text is still text.
    fn words(selection: &Selection, stats: &Stats<'_>) -> String {
        fn collect(shape: &Shape, into: &mut String) {
            match shape {
                Shape::Text(text) => {
                    into.push_str(text.galley.text());
                    into.push('\n');
                }
                Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, into);
                    }
                }
                _ => {}
            }
        }

        let ctx = Context::default();
        let mut text = String::new();
        // Twice, because egui lays a new window out once invisibly to learn how big it is and that
        // pass paints nothing.
        for _ in 0..2 {
            let mut intent = Intent::default();
            let mut controls = Controls::default();
            let output = ctx.run_ui(egui::RawInput::default(), |ui| {
                show(ui.ctx(), selection, stats, &mut intent, &mut controls);
            });
            text.clear();
            for clipped in &output.shapes {
                collect(&clipped.shape, &mut text);
            }
        }
        text
    }

    /// A selection that has asked for something the stats do not yet report.
    fn asked_for(stats: &Stats<'_>, intent: Intent) -> Selection {
        let mut selection = Selection::default();
        selection.reconcile(stats);
        selection.apply(&intent);
        selection
    }

    #[test]
    fn a_request_that_has_not_landed_is_said_out_loud() {
        let stats = Stats {
            backend_loaded: true,
            backend_supported: true,
            backend_name: Some("Test backend"),
            enabled: false,
            ..Stats::default()
        };

        let asked_to_enable = asked_for(
            &stats,
            Intent {
                enabled: true,
                enabled_changed: true,
                ..Intent::default()
            },
        );
        assert!(
            words(&asked_to_enable, &stats).contains("requested on, still off in effect"),
            "a ticked box the runtime has not acted on has to say so"
        );

        let asked_for_balanced = asked_for(
            &stats,
            Intent {
                quality: Quality::Balanced,
                quality_changed: true,
                ..Intent::default()
            },
        );
        assert!(
            words(&asked_for_balanced, &stats)
                .contains("requested Balanced, still Native in effect")
        );
    }

    #[test]
    fn what_the_panel_will_not_say() {
        let stats = Stats {
            backend_loaded: true,
            backend_supported: true,
            backend_name: Some("Test backend"),
            frames_presented: 240,
            frames_evaluated: 238,
            ..Stats::default()
        };
        // "On + Boost" names the SDK's Reflex option, rather than a measured speed claim.
        let said = words(&Selection::default(), &stats)
            .to_lowercase()
            .replace("on + boost", "on");

        // Counter-derived FPS is available. Unmeasured improvements and quality claims remain
        // forbidden; requested generation alone must never invent a measured FPS gain.
        for forbidden in ["faster", "gain", "boost", "speedup", "score"] {
            assert!(
                !said.contains(forbidden),
                "the panel said {forbidden:?}, which this build cannot measure"
            );
        }
        // Latency and frame time may be named, but only to say they are not measured. Checked per
        // line, because a word list alone cannot tell a claim from a denial of one.
        for line in said.lines() {
            if line.contains("latency") || line.contains("frame time") {
                assert!(
                    line.contains("measures no"),
                    "the panel line {line:?} names something this build cannot measure"
                );
            }
        }
    }
}
