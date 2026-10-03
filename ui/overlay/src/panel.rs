//! The panel itself: what it shows, and what a click on it means.
//!
//! This is the half worth testing. It needs an egui context and nothing else, so every widget here
//! can be laid out, clicked and asserted on without a device, a swap chain or a game.
//!
//! What it deliberately does not show: a frame rate gain, a latency figure, or a quality score.
//! The project cannot measure any of the three yet, and a status panel that implies otherwise is
//! worse than no panel.

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
