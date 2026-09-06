//! The panel itself: what it shows, and what a click on it means.
//!
//! This is the half worth testing. It needs an egui context and nothing else, so every widget here
//! can be laid out, clicked and asserted on without a device, a swap chain or a game.
//!
//! What it deliberately does not show: a frame rate gain, a latency figure, or a quality score.
//! The project cannot measure any of the three yet, and a status panel that implies otherwise is
//! worse than no panel.

use egui::{Color32, Context, Grid, Rect, RichText, Ui, Window};

use crate::model::{Intent, Quality, Stats};

/// An input that was found this frame.
const FOUND: Color32 = Color32::from_rgb(0x6c, 0xd0, 0x70);
/// An input that was not found, or a backend that refused.
const MISSING: Color32 = Color32::from_rgb(0xff, 0x6e, 0x5c);
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
    /// The dump button.
    pub dump: Option<Rect>,
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
    ui.separator();
    resolution_section(ui, stats);
    ui.separator();
    inputs_section(ui, stats);
    ui.separator();
    counters_section(ui, stats);
    ui.separator();

    let dump = ui.button("Dump this frame's inputs");
    controls.dump = Some(dump.rect);
    if dump.clicked() {
        intent.dump_requested = true;
    }

    ui.label(
        RichText::new("Counters only. This build measures no frame time and no latency.")
            .small()
            .color(MUTED),
    );
}

fn controls_section(
    ui: &mut Ui,
    selection: &Selection,
    stats: &Stats<'_>,
    intent: &mut Intent,
    controls: &mut Controls,
) {
    let mut enabled = selection.enabled;
    let toggle = ui.checkbox(&mut enabled, "Enabled");
    controls.enabled = Some(toggle.rect);
    if toggle.changed() {
        intent.enabled = enabled;
        intent.enabled_changed = true;
    }

    backend_line(ui, stats);

    // Picking a level with no backend behind it does nothing at all, which looks like a bug in the
    // level rather than an absent backend. Grey it out and say which it is.
    ui.horizontal_wrapped(|ui| {
        ui.scope(|ui| {
            if !stats.backend_loaded {
                ui.disable();
            }
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

        if !stats.backend_loaded {
            ui.label(RichText::new("no backend loaded").color(WARN));
        }
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

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn backend_line(ui: &mut Ui, stats: &Stats<'_>) {
    ui.horizontal_wrapped(|ui| {
        ui.label("Backend");
        if stats.backend_loaded {
            let name = stats.backend_name.unwrap_or("unnamed");
            if stats.backend_supported {
                ui.label(RichText::new(name).strong().color(FOUND));
            } else {
                ui.label(RichText::new(name).strong().color(MISSING));
                ui.label(RichText::new("not supported").color(MISSING));
            }
        } else {
            ui.label(RichText::new("none loaded").color(MISSING));
        }
    });

    // A refusal reason is worth reading whether or not a backend was loaded: "none loaded" and why
    // it could not be are two different pieces of the same answer.
    if let Some(reason) = stats.refusal_reason
        && !stats.backend_supported
    {
        ui.label(RichText::new(reason).color(MISSING));
    }
}

fn resolution_section(ui: &mut Ui, stats: &Stats<'_>) {
    Grid::new("rsf_resolution")
        .num_columns(2)
        .spacing([12.0, 3.0])
        .show(ui, |ui| {
            ui.label("Render");
            ui.label(RichText::new(size_text(stats.render)).monospace());
            ui.end_row();

            ui.label("Output");
            ui.label(RichText::new(size_text(stats.output)).monospace());
            ui.end_row();

            // The ratio is what a quality level actually means, so it comes from the two sizes
            // rather than from the selected level. If they disagree, the sizes are the truth.
            ui.label("Ratio");
            match stats.render_scale() {
                Some([x, y]) => {
                    ui.label(RichText::new(format!("{x:.3} x {y:.3} render/output")).monospace());
                }
                None => {
                    ui.label(RichText::new("unknown").color(MUTED));
                }
            }
            ui.end_row();
        });
}

fn inputs_section(ui: &mut Ui, stats: &Stats<'_>) {
    Grid::new("rsf_inputs")
        .num_columns(2)
        .spacing([12.0, 3.0])
        .show(ui, |ui| {
            found_row(ui, "Scene colour", stats.have_scene_color, None);
            found_row(ui, "Depth", stats.have_depth, None);

            // Motion being present and motion being usable are different questions, and this
            // project has already answered the second one wrongly by assuming it from the first.
            let motion_note = if stats.motion_decoded {
                Some(("decoded", FOUND))
            } else {
                Some(("not decoded", WARN))
            };
            found_row(ui, "Motion", stats.have_motion, motion_note);

            found_row(ui, "Exposure", stats.have_exposure, None);

            // Same again for jitter: it was assumed active here long before it was checked.
            ui.label("Jitter");
            if stats.jitter_active {
                ui.label(
                    RichText::new(format!(
                        "ACTIVE  {:+.3}, {:+.3} px",
                        stats.jitter_pixels[0], stats.jitter_pixels[1]
                    ))
                    .monospace()
                    .color(FOUND),
                );
            } else {
                ui.label(RichText::new("NONE").monospace().strong().color(MISSING));
            }
            ui.end_row();
        });
}

/// One input row. Colour and word both carry the state, because a colour alone is hard to read at
/// a glance and this is meant to be readable while flying.
fn found_row(ui: &mut Ui, label: &str, found: bool, note: Option<(&str, Color32)>) {
    ui.label(label);
    ui.horizontal(|ui| {
        if found {
            ui.label(RichText::new("FOUND").monospace().strong().color(FOUND));
        } else {
            ui.label(RichText::new("MISSING").monospace().strong().color(MISSING));
        }
        if let Some((text, color)) = note
            && found
        {
            ui.label(RichText::new(text).color(color));
        }
    });
    ui.end_row();
}

fn counters_section(ui: &mut Ui, stats: &Stats<'_>) {
    Grid::new("rsf_counters")
        .num_columns(2)
        .spacing([12.0, 3.0])
        .show(ui, |ui| {
            ui.label("Presented");
            ui.label(RichText::new(stats.frames_presented.to_string()).monospace());
            ui.end_row();

            ui.label("Evaluated");
            let evaluated = match stats.evaluated_fraction() {
                Some(fraction) => format!(
                    "{} ({:.1}% of presented)",
                    stats.frames_evaluated,
                    fraction * 100.0
                ),
                None => stats.frames_evaluated.to_string(),
            };
            ui.label(RichText::new(evaluated).monospace());
            ui.end_row();

            ui.label("Refused");
            if stats.frames_refused == 0 {
                ui.label(RichText::new("0").monospace());
            } else {
                ui.label(
                    RichText::new(stats.frames_refused.to_string())
                        .monospace()
                        .color(MISSING),
                );
            }
            ui.end_row();

            // The backend's own code, shown only once something has been refused, and in hex as
            // well because that is how vendor SDKs write them down.
            if stats.frames_refused > 0 {
                ui.label("Last result");
                ui.label(
                    RichText::new(format!(
                        "{} (0x{:08X})",
                        stats.last_result, stats.last_result as u32
                    ))
                    .monospace()
                    .color(MISSING),
                );
                ui.end_row();
            }
        });
}

fn size_text(size: [u32; 2]) -> String {
    if size[0] == 0 || size[1] == 0 {
        "unknown".to_owned()
    } else {
        format!("{} x {}", size[0], size[1])
    }
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
        let said = words(&Selection::default(), &stats).to_lowercase();

        // The project measures none of these, so nothing on the panel may imply it does. This is
        // the honesty rule as a test rather than as a review comment.
        for forbidden in ["fps", "faster", "gain", "boost", "speedup", "score"] {
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
