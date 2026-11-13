//! The panel itself: what it shows, and what a tap on it means.
//!
//! This is the half worth testing. It needs an egui context and nothing else, so every widget here
//! can be laid out, tapped and asserted on without a device, a swap chain or a game.
//!
//! The layout is built for handheld touch screens first: a full-height sheet with a tab rail, a
//! status strip that says what is in effect, and controls no smaller than 48 points. Nothing needs
//! hover, values are stepped or dragged rather than typed, and a held step button repeats.
//!
//! Counter-derived FPS is shown separately from generation activity. Latency, quality and
//! unmeasured speed improvements are not inferred from a requested mode.

use core::ops::RangeInclusive;

use egui::{
    Align2, Color32, Context, FontId, Id, Label, Painter, Pos2, Rect, Response, RichText, Sense,
    Stroke, StrokeKind, Ui, UiBuilder, Vec2, pos2, vec2,
};

use crate::model::{
    GenerationStats, Intent, MAX_HOST_OPTIONS, PanelSize, Quality, Stats, UiPreferences,
};
use crate::performance::Rates;

/// Requested and not in effect yet, or something to read before believing the rest of the panel.
const AMBER: Color32 = Color32::from_rgb(0xf5, 0xc8, 0x62);
const AMBER_DOT: Color32 = Color32::from_rgb(0xf0, 0xb4, 0x3c);
const AMBER_FILL: Color32 = Color32::from_rgb(0x4a, 0x3a, 0x14);
const AMBER_TEXT: Color32 = Color32::from_rgb(0xff, 0xe2, 0xa3);
const WARN_FILL: Color32 = Color32::from_rgb(0x2e, 0x25, 0x10);
const WARN_LINE: Color32 = Color32::from_rgb(0x6b, 0x54, 0x20);
/// Confirmed by the runtime.
const BLUE: Color32 = Color32::from_rgb(0x4f, 0xa8, 0xff);
const BLUE_TEXT: Color32 = Color32::from_rgb(0x8c, 0xc4, 0xff);
const SELECTED: Color32 = Color32::from_rgb(0x2b, 0x5b, 0x94);
const SELECTED_DIM: Color32 = Color32::from_rgb(0x26, 0x33, 0x45);
const SWITCH_ON: Color32 = Color32::from_rgb(0x3d, 0x8f, 0xe6);
const SWITCH_OFF: Color32 = Color32::from_rgb(0x35, 0x3c, 0x47);
const KNOB: Color32 = Color32::from_rgb(0xcf, 0xd5, 0xdc);
const TEXT: Color32 = Color32::from_rgb(0xee, 0xf1, 0xf4);
const SECTION: Color32 = Color32::from_rgb(0xd6, 0xdb, 0xe1);
/// Detail that is true but not load bearing. 7:1 on the sheet.
const MUTED: Color32 = Color32::from_rgb(0xa9, 0xb1, 0xbc);
const DISABLED: Color32 = Color32::from_rgb(0x5d, 0x65, 0x70);
const GREY_DOT: Color32 = Color32::from_rgb(0x5a, 0x62, 0x6e);
const LINE: Color32 = Color32::from_rgb(0x23, 0x28, 0x31);
const WELL: Color32 = Color32::from_rgb(0x0b, 0x0d, 0x11);
const WELL_LINE: Color32 = Color32::from_rgb(0x26, 0x2c, 0x35);
const BUTTON: Color32 = Color32::from_rgb(0x1d, 0x22, 0x2b);
const BUTTON_LINE: Color32 = Color32::from_rgb(0x2e, 0x35, 0x40);
const PRESSED: Color32 = Color32::from_rgb(0x2a, 0x31, 0x3c);
const TAB_ON: Color32 = Color32::from_rgb(0x22, 0x32, 0x4a);

/// Smallest touch target, in points.
const TARGET: f32 = 48.0;
/// Widest the sheet gets, in points.
const SHEET_WIDTH: f32 = 540.0;
const MARGIN: f32 = 12.0;
const HEADER: f32 = 64.0;
const STRIP: f32 = 76.0;
const FOOTER: f32 = 44.0;
const RAIL: f32 = 84.0;
/// Pacing owners reported by the runtime.
const LATENCY_REFLEX: u32 = 1;
const LATENCY_XELL: u32 = 2;
const LATENCY_ANTILAG2: u32 = 3;
const LATENCY_ANTILAG_VULKAN: u32 = 4;

/// Super-resolution backends by ID. The ID is the bit the host's choices mask uses.
const SR_BACKENDS: [(u32, &str); 6] = [
    (1, "DLSS"),
    (2, "FSR2"),
    (3, "FSR3"),
    (4, "FSR4"),
    (5, "XeSS"),
    (6, "FSR1"),
];

/// Frame generation providers by ID. Zero is Off, and the ID is the bit in the choices mask.
const FG_PROVIDERS: [(u32, &str); 5] = [
    (0, "Off"),
    (1, "DLSS-G"),
    (3, "FSR3"),
    (4, "FSR4"),
    (5, "XeSS"),
];

const DLSS_PRESETS: [(u32, &str); 7] = [
    (0, "Default"),
    (5, "E"),
    (6, "F"),
    (10, "J"),
    (11, "K"),
    (12, "L"),
    (13, "M"),
];

/// Highest multiplier the panel offers a segment for.
const MAX_MULTIPLIER: u32 = 6;

/// One section of the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum Tab {
    #[default]
    Upscale,
    Frames,
    Sharpen,
    Game,
    Display,
}

impl Tab {
    const ALL: [Self; 5] = [
        Self::Upscale,
        Self::Frames,
        Self::Sharpen,
        Self::Game,
        Self::Display,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Upscale => "Upscale",
            Self::Frames => "Frames",
            Self::Sharpen => "Sharpen",
            Self::Game => "Game",
            Self::Display => "Display",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0)
    }
}

/// Panel-local state that the host never sees: the open tab, the layout the user asked for until
/// the host echoes it, and whether the pointer went down outside the sheet.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct View {
    pub(crate) tab: Tab,
    requested_preferences: Option<u32>,
    press_outside: bool,
}

impl View {
    /// Drop a layout request once the host reports it stored.
    pub(crate) fn reconcile(&mut self, stats: &Stats<'_>) {
        if self.requested_preferences == Some(stats.ui_preferences) {
            self.requested_preferences = None;
        }
    }

    /// The layout in use: the user's request until the host has it, then the host's copy. A host
    /// that never stores the word still gets the request for the rest of the session.
    pub(crate) fn preferences(&self, stats: &Stats<'_>) -> UiPreferences {
        UiPreferences::from_bits(self.requested_preferences.unwrap_or(stats.ui_preferences))
    }

    pub(crate) fn apply(&mut self, intent: &Intent) {
        if intent.ui_preferences_changed {
            self.requested_preferences = Some(intent.ui_preferences);
        }
    }

    /// Forget a press in progress. Called while the panel is hidden.
    pub(crate) fn release(&mut self) {
        self.press_outside = false;
    }
}

/// What the panel currently shows as chosen, and what it is waiting to see take effect.
///
/// The quality last tapped and the quality in effect differ whenever a change has been requested
/// and not yet applied. That gap is this struct: `requested` holds the user's ask until the
/// runtime reports it back through the stats, and until then the panel shows the ask and says that
/// it has not landed. Without this the selection would visibly snap back to the old level for
/// however many frames the runtime takes to apply it.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Selection {
    quality: Quality,
    requested_quality: Option<Quality>,
    enabled: bool,
    requested_enabled: Option<bool>,
    requested_backend: Option<u32>,
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

        // A refused switch is reported through `last_switch_result`, and the effective backend is
        // then the one to show.
        if self
            .requested_backend
            .is_some_and(|backend| backend == stats.backend || stats.last_switch_result != 0)
        {
            self.requested_backend = None;
        }
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
        if intent.backend_changed {
            self.requested_backend = Some(intent.backend);
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
/// The C boundary does not pass these on. They exist because the unit tests tap the controls,
/// and tapping a hard coded coordinate would mean every layout change silently stops testing
/// anything: a tap that lands on nothing asserts nothing. Only the open tab's controls are laid
/// out, so the others are `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Controls {
    /// The whole sheet.
    pub sheet: Option<Rect>,
    /// The close button in the header.
    pub close: Option<Rect>,
    /// One button per tab in `Tab::ALL` order, `None` for a tab this host does not offer.
    pub tabs: [Option<Rect>; 5],
    /// One rectangle per quality level, in [`Quality::ALL`] order.
    pub quality: [Option<Rect>; 5],
    /// Default, E, F, J, K, L and M model selectors.
    pub dlss_preset: [Option<Rect>; 7],
    /// The upscaling switch row.
    pub enabled: Option<Rect>,
    /// One control per SR backend, in `SR_BACKENDS` order.
    pub backend: [Option<Rect>; 6],
    /// Off, DLSS-G, FSR3, FSR4 and XeSS generation choices.
    pub fg_backend: [Option<Rect>; 5],
    /// The frame generation switch row.
    pub fg_enabled: Option<Rect>,
    /// Multiplier segments, 2x first.
    pub fg_multiplier: [Option<Rect>; 5],
    /// Off, On and On + Boost.
    pub reflex: [Option<Rect>; 3],
    /// Frame limit step buttons, down then up.
    pub frame_limit: [Option<Rect>; 2],
    /// Frame limit presets: 30, 40, 60, VRR, Off.
    pub frame_limit_preset: [Option<Rect>; 5],
    /// Off, RCAS, depth RCAS, depth DAS, vendor native.
    pub sharpening_mode: [Option<Rect>; 5],
    /// Sharpness slider.
    pub sharpening_strength: Option<Rect>,
    /// MAS switch row, available for custom passes.
    pub sharpening_mas: Option<Rect>,
    /// RCAS contrast switch row.
    pub sharpening_contrast: Option<Rect>,
    /// Depth scale slider, available for depth modes.
    pub sharpening_depth: Option<Rect>,
    /// Debug tint switch row.
    pub sharpening_debug: Option<Rect>,
    /// Host options: the switch row of a toggle, the up button of a number.
    pub host_option: [Option<Rect>; MAX_HOST_OPTIONS],
    /// FPS overlay switch row.
    pub hud: Option<Rect>,
    /// Compact and detailed FPS overlay.
    pub hud_detail: [Option<Rect>; 2],
    /// Panel sizes in [`PanelSize::ALL`] order.
    pub panel_size: [Option<Rect>; 4],
    /// Left and right.
    pub panel_side: [Option<Rect>; 2],
}

/// Where the sheet goes this frame, in points.
pub(crate) fn sheet_rect(screen: Vec2, preferences: UiPreferences) -> Rect {
    let width = SHEET_WIDTH.min(screen.x - 2.0 * MARGIN).max(1.0);
    let height = (screen.y - 2.0 * MARGIN).max(1.0);
    let x = if preferences.right {
        (screen.x - MARGIN - width).max(0.0)
    } else {
        MARGIN
    };
    Rect::from_min_size(pos2(x, MARGIN), vec2(width, height))
}

/// Everything one frame of the panel reads.
pub(crate) struct Frame<'a, 'b> {
    pub(crate) selection: &'a Selection,
    pub(crate) stats: &'a Stats<'b>,
    pub(crate) rates: Rates,
    pub(crate) preferences: UiPreferences,
}

/// Lay out one frame of the panel, recording what the user did into `intent`.
///
/// `selection` is read only: what the user asked for goes into `intent` and is folded back into
/// the selection by the caller once the frame is over. `view` is panel-local and changes here.
pub(crate) fn show(
    ctx: &Context,
    frame: &Frame<'_, '_>,
    view: &mut View,
    intent: &mut Intent,
    controls: &mut Controls,
) {
    let stats = frame.stats;
    let screen = ctx.content_rect().size();
    let sheet = sheet_rect(screen, frame.preferences);

    let offered: Vec<Tab> = Tab::ALL
        .into_iter()
        .filter(|tab| match tab {
            Tab::Sharpen => stats.backend_choices != 0 && stats.sharpening_available,
            Tab::Game => stats.host_options.iter().any(Option::is_some),
            _ => true,
        })
        .collect();
    if !offered.contains(&view.tab) {
        view.tab = Tab::Upscale;
    }

    egui::Area::new(Id::new("rsf-sheet"))
        .fixed_pos(sheet.min)
        .constrain(false)
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            let (sheet, _) = ui.allocate_exact_size(sheet.size(), Sense::hover());
            controls.sheet = Some(sheet);
            let painter = ui.painter().clone();
            painter.rect_filled(
                sheet,
                18.0,
                Color32::from_rgba_unmultiplied(15, 17, 21, 242),
            );
            painter.rect_stroke(
                sheet,
                18.0,
                Stroke::new(1.0, Color32::from_rgb(0x2a, 0x30, 0x39)),
                StrokeKind::Inside,
            );

            let header = Rect::from_min_size(sheet.min, vec2(sheet.width(), HEADER));
            let strip = Rect::from_min_size(header.left_bottom(), vec2(sheet.width(), STRIP));
            let footer = Rect::from_min_max(
                pos2(sheet.left(), (sheet.bottom() - FOOTER).max(strip.bottom())),
                sheet.max,
            );
            let middle = Rect::from_min_max(strip.left_bottom(), footer.right_top());
            let rail = Rect::from_min_size(middle.min, vec2(RAIL, middle.height()));
            let body = Rect::from_min_max(
                pos2(rail.right() + 16.0, middle.top() + 14.0),
                pos2(middle.right() - 14.0, middle.bottom() - 6.0),
            );

            draw_header(ui, &painter, header, intent, controls);
            painter.hline(sheet.x_range(), header.bottom(), Stroke::new(1.0, LINE));
            draw_strip(ui, &painter, strip, frame, view);
            painter.hline(sheet.x_range(), strip.bottom(), Stroke::new(1.0, LINE));
            painter.vline(rail.right(), middle.y_range(), Stroke::new(1.0, LINE));
            draw_rail(ui, &painter, rail, &offered, view, controls);
            painter.hline(sheet.x_range(), footer.top(), Stroke::new(1.0, LINE));
            painter.text(
                pos2(footer.left() + 18.0, footer.center().y),
                Align2::LEFT_CENTER,
                "Insert or a three-finger tap opens and closes. Tap outside to close.",
                FontId::proportional(12.5),
                MUTED,
            );

            if body.width() > 40.0 && body.height() > 40.0 {
                let tab = view.tab;
                ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt(tab)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.spacing_mut().item_spacing = vec2(8.0, 10.0);
                            match tab {
                                Tab::Upscale => upscale_tab(ui, frame, intent, controls),
                                Tab::Frames => frames_tab(ui, stats, intent, controls),
                                Tab::Sharpen => sharpen_tab(ui, stats, intent, controls),
                                Tab::Game => game_tab(ui, stats, intent, controls),
                                Tab::Display => display_tab(ui, frame, intent, controls),
                            }
                            ui.add_space(12.0);
                        });
                });
            }
        });

    // A tap that starts and ends outside the sheet closes it. Both ends are checked so a scroll
    // drag that wanders off the edge is not a close.
    let (pressed, released, position) = ctx.input(|input| {
        (
            input.pointer.primary_pressed(),
            input.pointer.primary_released(),
            input.pointer.interact_pos(),
        )
    });
    let outside = position.is_some_and(|point| !sheet.contains(point));
    if pressed {
        view.press_outside = outside;
    }
    if released {
        if view.press_outside && outside {
            intent.close_requested = true;
        }
        view.press_outside = false;
    }
}

fn draw_header(
    ui: &Ui,
    painter: &Painter,
    header: Rect,
    intent: &mut Intent,
    controls: &mut Controls,
) {
    let icon = Rect::from_center_size(
        pos2(header.left() + 30.0, header.center().y),
        vec2(20.0, 20.0),
    );
    let stroke = Stroke::new(2.0, BLUE);
    painter.rect_stroke(
        Rect::from_min_size(icon.min, vec2(9.0, 9.0)),
        1.5,
        stroke,
        StrokeKind::Middle,
    );
    painter.line(
        vec![
            icon.min + vec2(12.0, 0.0),
            icon.right_top(),
            icon.max,
            icon.min + vec2(7.0, 20.0),
            icon.min + vec2(7.0, 13.0),
        ],
        stroke,
    );
    painter.text(
        pos2(header.left() + 50.0, header.center().y),
        Align2::LEFT_CENTER,
        "ReScaleFrame",
        FontId::proportional(18.0),
        TEXT,
    );
    let close = Rect::from_center_size(
        pos2(header.right() - 10.0 - TARGET / 2.0, header.center().y),
        Vec2::splat(TARGET),
    );
    let response = ui.interact(close, Id::new("rsf-close"), Sense::click());
    paint_button(painter, close, response.is_pointer_button_down_on(), true);
    paint_glyph(painter, close, Glyph::Close, TEXT);
    controls.close = Some(close);
    if response.clicked() {
        intent.close_requested = true;
    }
}

/// The three cells at the top: what is in effect, not what was last tapped.
fn draw_strip(ui: &Ui, painter: &Painter, strip: Rect, frame: &Frame<'_, '_>, view: &mut View) {
    let stats = frame.stats;
    let cells = [
        (Tab::Upscale, "UPSCALING", upscaling_cell(frame)),
        (Tab::Frames, "FRAME GEN", generation_cell(&stats.generation)),
        (Tab::Display, "FRAMES / S", rates_cell(frame.rates)),
    ];
    let width = strip.width() / cells.len() as f32;
    for (index, (tab, label, cell)) in cells.into_iter().enumerate() {
        let rect = Rect::from_min_size(
            strip.min + vec2(width * index as f32, 0.0),
            vec2(width, strip.height()),
        );
        let response = ui.interact(rect, Id::new(("rsf-cell", index)), Sense::click());
        if response.is_pointer_button_down_on() {
            painter.rect_filled(rect.shrink(1.0), 0.0, BUTTON);
        }
        if index > 0 {
            painter.vline(rect.left(), rect.y_range(), Stroke::new(1.0, LINE));
        }
        let left = rect.left() + 14.0;
        let clip = painter.with_clip_rect(rect.shrink2(vec2(6.0, 0.0)));
        clip.text(
            pos2(left, rect.top() + 12.0),
            Align2::LEFT_TOP,
            label,
            FontId::proportional(10.5),
            MUTED,
        );
        clip.text(
            pos2(left, rect.top() + 29.0),
            Align2::LEFT_TOP,
            &cell.value,
            FontId::proportional(15.0),
            TEXT,
        );
        clip.circle_filled(pos2(left + 4.0, rect.top() + 59.0), 4.0, cell.dot);
        clip.text(
            pos2(left + 14.0, rect.top() + 59.0),
            Align2::LEFT_CENTER,
            &cell.sub,
            FontId::proportional(12.0),
            cell.sub_color,
        );
        if response.clicked() {
            view.tab = tab;
        }
    }
}

struct Cell {
    value: String,
    sub: String,
    dot: Color32,
    sub_color: Color32,
}

fn upscaling_cell(frame: &Frame<'_, '_>) -> Cell {
    let stats = frame.stats;
    let selection = frame.selection;
    if stats.backend_choices == 0 {
        return Cell {
            value: "Not offered".to_owned(),
            sub: "No SR in this game".to_owned(),
            dot: GREY_DOT,
            sub_color: MUTED,
        };
    }
    if stats.refusal_reason.is_some() {
        return Cell {
            value: sr_name(stats.backend).to_owned(),
            sub: "Unavailable".to_owned(),
            dot: AMBER_DOT,
            sub_color: AMBER,
        };
    }
    let pending = selection.requested_quality;
    if let Some(enabled) = selection.requested_enabled {
        return Cell {
            value: if stats.enabled {
                format!("{} · {}", sr_name(stats.backend), quality_label(stats))
            } else {
                "Off".to_owned()
            },
            sub: format!("Turning {}", on_off(enabled)),
            dot: AMBER_DOT,
            sub_color: AMBER,
        };
    }
    if !stats.enabled {
        return Cell {
            value: "Off".to_owned(),
            sub: "Native rendering".to_owned(),
            dot: GREY_DOT,
            sub_color: MUTED,
        };
    }
    let value = format!("{} · {}", sr_name(stats.backend), quality_label(stats));
    if let Some(level) = pending {
        return Cell {
            value,
            sub: format!("Applying {}", level.label()),
            dot: AMBER_DOT,
            sub_color: AMBER,
        };
    }
    let [rw, rh] = stats.render_size;
    let [ow, oh] = stats.output_size;
    let sub = if rw > 0 && rh > 0 && ow > 0 && oh > 0 {
        format!("{rw}×{rh} → {ow}×{oh}")
    } else {
        "On".to_owned()
    };
    Cell {
        value,
        sub,
        dot: BLUE,
        sub_color: MUTED,
    }
}

fn generation_cell(generation: &GenerationStats) -> Cell {
    let name = fg_backend_name(generation.backend);
    if generation.active {
        Cell {
            value: format!("{name} {}×", generation.effective_generated + 1),
            sub: "Generating".to_owned(),
            dot: BLUE,
            sub_color: MUTED,
        }
    } else if generation.requested_mode != 0 && generation.backend != 0 {
        Cell {
            value: format!("{name} requested"),
            sub: "Not generating yet".to_owned(),
            dot: AMBER_DOT,
            sub_color: AMBER,
        }
    } else {
        Cell {
            value: "Off".to_owned(),
            sub: if generation.backend == 0 {
                "No provider".to_owned()
            } else {
                "Switched off".to_owned()
            },
            dot: GREY_DOT,
            sub_color: MUTED,
        }
    }
}

fn rates_cell(rates: Rates) -> Cell {
    let format_rate =
        |rate: Option<f64>| rate.map_or_else(|| "--".to_owned(), |v| format!("{v:.0}"));
    Cell {
        value: format!(
            "{} → {}",
            format_rate(rates.rendered),
            format_rate(rates.presented)
        ),
        sub: "rendered → presented".to_owned(),
        dot: GREY_DOT,
        sub_color: MUTED,
    }
}

fn draw_rail(
    ui: &Ui,
    painter: &Painter,
    rail: Rect,
    offered: &[Tab],
    view: &mut View,
    controls: &mut Controls,
) {
    for (slot, tab) in offered.iter().enumerate() {
        let rect = Rect::from_min_size(
            pos2(rail.left() + 10.0, rail.top() + 10.0 + slot as f32 * 72.0),
            vec2(64.0, 64.0),
        );
        if rect.bottom() > rail.bottom() {
            break;
        }
        let response = ui.interact(rect, Id::new(("rsf-tab", tab.index())), Sense::click());
        let current = view.tab == *tab;
        if current {
            painter.rect_filled(rect, 12.0, TAB_ON);
        } else if response.is_pointer_button_down_on() {
            painter.rect_filled(rect, 12.0, PRESSED);
        }
        let color = if current { Color32::WHITE } else { MUTED };
        paint_tab_icon(
            painter,
            *tab,
            pos2(rect.center().x, rect.top() + 24.0),
            color,
        );
        painter.text(
            pos2(rect.center().x, rect.bottom() - 13.0),
            Align2::CENTER_CENTER,
            tab.label(),
            FontId::proportional(11.5),
            color,
        );
        controls.tabs[tab.index()] = Some(rect);
        if response.clicked() {
            view.tab = *tab;
        }
    }
}

fn paint_tab_icon(painter: &Painter, tab: Tab, centre: Pos2, color: Color32) {
    let stroke = Stroke::new(2.0, color);
    let at = |x: f32, y: f32| centre + vec2(x, y);
    match tab {
        Tab::Upscale => {
            painter.rect_stroke(
                Rect::from_min_max(at(-10.0, 2.0), at(-2.0, 10.0)),
                1.5,
                stroke,
                StrokeKind::Middle,
            );
            painter.rect_stroke(
                Rect::from_min_max(at(-4.0, -10.0), at(10.0, 4.0)),
                1.5,
                stroke,
                StrokeKind::Middle,
            );
        }
        Tab::Frames => {
            painter.rect_stroke(
                Rect::from_min_max(at(-11.0, -7.0), at(-2.0, 7.0)),
                1.5,
                stroke,
                StrokeKind::Middle,
            );
            painter.rect_stroke(
                Rect::from_min_max(at(2.0, -7.0), at(11.0, 7.0)),
                1.5,
                stroke,
                StrokeKind::Middle,
            );
        }
        Tab::Sharpen => {
            painter.add(egui::Shape::closed_line(
                vec![
                    at(0.0, -10.0),
                    at(3.0, -3.0),
                    at(10.0, 0.0),
                    at(3.0, 3.0),
                    at(0.0, 10.0),
                    at(-3.0, 3.0),
                    at(-10.0, 0.0),
                    at(-3.0, -3.0),
                ],
                stroke,
            ));
        }
        Tab::Game => {
            painter.rect_stroke(
                Rect::from_min_max(at(-11.0, -6.0), at(11.0, 7.0)),
                4.0,
                stroke,
                StrokeKind::Middle,
            );
            painter.line_segment([at(-7.0, 0.5), at(-1.0, 0.5)], stroke);
            painter.line_segment([at(-4.0, -2.5), at(-4.0, 3.5)], stroke);
            painter.circle_filled(at(4.0, -1.0), 1.4, color);
            painter.circle_filled(at(7.0, 2.0), 1.4, color);
        }
        Tab::Display => {
            painter.rect_stroke(
                Rect::from_min_max(at(-11.0, -9.0), at(11.0, 5.0)),
                1.5,
                stroke,
                StrokeKind::Middle,
            );
            painter.line_segment([at(0.0, 5.0), at(0.0, 10.0)], stroke);
            painter.line_segment([at(-5.0, 10.0), at(5.0, 10.0)], stroke);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Tabs

fn upscale_tab(ui: &mut Ui, frame: &Frame<'_, '_>, intent: &mut Intent, controls: &mut Controls) {
    let stats = frame.stats;
    let selection = frame.selection;
    if let Some(reason) = stats.refusal_reason {
        banner(ui, "Upscaling unavailable", reason);
    }
    if stats.last_switch_result != 0 {
        banner(
            ui,
            "Requested configuration refused",
            &format!(
                "The settings in effect stay as they were. Runtime result: {}.",
                stats.last_switch_result
            ),
        );
    }
    // A zero mask means the host offers no super resolution backend: there is nothing to enable or
    // choose, so the upscaling controls are not drawn rather than shown as a switch that cannot work.
    if stats.backend_choices == 0 {
        empty_card(
            ui,
            "No upscaling in this game",
            "Upscaling: none offered by this host. The other sections still apply.",
        );
        return;
    }

    let detail = match selection.requested_enabled {
        // Without this the switch reads on while the runtime reports reconstruction off, which is
        // the panel claiming a state that is not in effect.
        Some(requested) => Some((
            format!(
                "requested {}, still {} in effect",
                on_off(requested),
                on_off(stats.enabled)
            ),
            AMBER,
        )),
        None => Some((
            "Renders the scene smaller and reconstructs the output".to_owned(),
            MUTED,
        )),
    };
    let (tapped, rect) = switch_row(
        ui,
        "sr-enabled",
        "Upscaling",
        detail,
        selection.enabled,
        true,
    );
    controls.enabled = Some(rect);
    if tapped {
        intent.enabled = !selection.enabled;
        intent.enabled_changed = true;
    }

    heading(ui, "Backend");
    let shown_backend = selection.requested_backend.unwrap_or(stats.backend);
    let items: Vec<Segment> = SR_BACKENDS
        .iter()
        .map(|(id, name)| Segment {
            label: (*name).to_owned(),
            sub: None,
            enabled: stats.backend_loaded && stats.backend_choices & (1 << id) != 0,
            mark: if *id == shown_backend {
                if selection.requested_backend.is_some() {
                    Mark::Pending
                } else {
                    Mark::On
                }
            } else {
                Mark::Off
            },
        })
        .collect();
    let (chosen, rects) = segments(ui, "sr-backend", &items, 6);
    store(&mut controls.backend, &rects);
    if let Some(index) = chosen {
        let backend = SR_BACKENDS[index].0;
        if backend != stats.backend || selection.requested_backend.is_some() {
            intent.backend_changed = true;
            intent.backend = backend;
        }
    }
    let missing: Vec<&str> = SR_BACKENDS
        .iter()
        .filter(|(id, _)| stats.backend_choices & (1 << id) == 0)
        .map(|(_, name)| *name)
        .collect();
    if !stats.backend_loaded {
        note(ui, "No backend is loaded yet.", MUTED);
    } else if !missing.is_empty() {
        note(
            ui,
            format!("Not offered here: {}.", missing.join(", ")),
            MUTED,
        );
    }

    // Presets can be saved while disabled; startup applies the selected level.
    let render = stats.render_size;
    let title = if render[0] > 0 && render[1] > 0 {
        format!("Quality · renders {}×{}", render[0], render[1])
    } else {
        "Quality".to_owned()
    };
    heading(ui, &title);
    let items: Vec<Segment> = Quality::ALL
        .iter()
        .map(|level| Segment {
            label: if *level == Quality::Native && stats.backend == 1 {
                "DLAA".to_owned()
            } else if *level == Quality::UltraPerformance {
                "Ultra".to_owned()
            } else if *level == Quality::Performance {
                "Perf".to_owned()
            } else {
                level.label().to_owned()
            },
            sub: None,
            enabled: !stats.sr_quality_locked,
            mark: if *level == selection.quality {
                if selection.requested_quality.is_some() {
                    Mark::Pending
                } else {
                    Mark::On
                }
            } else {
                Mark::Off
            },
        })
        .collect();
    let (chosen, rects) = segments(ui, "sr-quality", &items, 5);
    store(&mut controls.quality, &rects);
    if let Some(index) = chosen {
        let level = Quality::ALL[index];
        if level != selection.quality {
            intent.quality = level;
            intent.quality_changed = true;
        }
    }
    if stats.sr_quality_locked {
        note(ui, "Input resolution is controlled by the game", MUTED);
    }
    match (selection.requested_quality, stats.quality) {
        (Some(requested), effect) => {
            let in_effect = match effect {
                Some(level) => level.label().to_owned(),
                None => format!("unknown ({})", stats.quality_raw),
            };
            pending_note(
                ui,
                &format!(
                    "requested {}, still {} in effect",
                    requested.label(),
                    in_effect
                ),
            );
        }
        (None, None) => note(
            ui,
            format!(
                "runtime reports quality {}, which this build does not know",
                stats.quality_raw
            ),
            AMBER,
        ),
        (None, Some(_)) => {}
    }

    if stats.dlss_preset_available {
        heading(ui, "DLSS model");
        let dlss = stats.backend == 1;
        let items: Vec<Segment> = DLSS_PRESETS
            .iter()
            .map(|(id, label)| Segment {
                label: (*label).to_owned(),
                sub: None,
                enabled: dlss,
                mark: if *id == stats.dlss_preset {
                    Mark::On
                } else {
                    Mark::Off
                },
            })
            .collect();
        let (chosen, rects) = segments(ui, "dlss-preset", &items, 7);
        store(&mut controls.dlss_preset, &rects);
        if let Some(index) = chosen {
            let preset = DLSS_PRESETS[index].0;
            if preset != stats.dlss_preset {
                intent.dlss_preset = preset;
                intent.dlss_preset_changed = true;
            }
        }
        note(
            ui,
            if dlss {
                "Requested model; NVIDIA driver overrides may apply. E/F are legacy presets."
            } else {
                "Applies when DLSS is the backend."
            },
            MUTED,
        );
    }
}

fn frames_tab(ui: &mut Ui, stats: &Stats<'_>, intent: &mut Intent, controls: &mut Controls) {
    let generation = stats.generation;
    let runtime_switching = generation.backend_choices & 0x8000_0000 != 0;
    if generation.last_result != 0 {
        banner(
            ui,
            "Frame generation request refused",
            &format!(
                "The provider in effect is kept. Runtime result: {}.",
                generation.last_result
            ),
        );
    }

    heading(ui, "Provider");
    let pending = generation.requested_backend != generation.backend;
    let items: Vec<Segment> = FG_PROVIDERS
        .iter()
        .map(|(id, name)| Segment {
            label: (*name).to_owned(),
            sub: None,
            enabled: generation.backend_choices & (1 << id) != 0,
            mark: if *id == generation.requested_backend {
                if pending { Mark::Pending } else { Mark::On }
            } else {
                Mark::Off
            },
        })
        .collect();
    let (chosen, rects) = segments(ui, "fg-provider", &items, 5);
    store(&mut controls.fg_backend, &rects);
    if let Some(index) = chosen {
        let id = FG_PROVIDERS[index].0;
        if id != generation.requested_backend {
            intent.fg_backend_changed = true;
            intent.fg_backend = id;
        }
    }
    if pending {
        let (lead, tail) = match (runtime_switching, generation.selection_result == 0) {
            (true, true) => ("Applying", " at the next frame."),
            (true, false) => ("Requested", "."),
            (false, _) => ("Saved:", ". Awaiting the presentation owner."),
        };
        pending_note(
            ui,
            &format!(
                "{lead} {}{tail}",
                fg_backend_name(generation.requested_backend)
            ),
        );
    }
    let missing: Vec<&str> = FG_PROVIDERS
        .iter()
        .filter(|(id, _)| *id != 0 && generation.backend_choices & (1 << id) == 0)
        .map(|(_, name)| *name)
        .collect();
    let mut line = if runtime_switching {
        "Provider changes apply during play.".to_owned()
    } else {
        "GPU compatibility is checked at startup.".to_owned()
    };
    if !missing.is_empty() {
        line.push_str(&format!(" Not offered here: {}.", missing.join(", ")));
    }
    note(ui, line, MUTED);
    if generation.selection_result != 0 {
        note(
            ui,
            if runtime_switching {
                "Provider change failed; current provider retained."
            } else {
                "Could not save the provider choice."
            },
            AMBER,
        );
    }

    let usable = generation.available && !pending;
    let status = if !generation.available {
        (
            "Select a supported provider to enable frame generation".to_owned(),
            MUTED,
        )
    } else if generation.active {
        (
            format!(
                "Generating {} frame(s) per rendered frame",
                generation.effective_generated
            ),
            BLUE_TEXT,
        )
    } else if generation.requested_mode != 0 {
        (waiting_reason(generation.reason).to_owned(), AMBER)
    } else {
        ("Off".to_owned(), MUTED)
    };
    let on = generation.requested_mode != 0;
    let (tapped, rect) = switch_row(
        ui,
        "fg-enabled",
        "Frame generation",
        Some(status),
        on,
        usable,
    );
    controls.fg_enabled = Some(rect);
    if tapped {
        intent.fg_changed = true;
        intent.fg_mode = u32::from(!on);
        intent.fg_generated = generation.requested_generated.max(1);
    }

    heading(ui, "Multiplier");
    if generation.backend == 1 {
        match generation.mfg_unlock_status {
            1 => note(ui, "RTX 40 MFG unlock ready", MUTED),
            2 => note(
                ui,
                "RTX 40 MFG unlock unavailable for the loaded runtime",
                MUTED,
            ),
            _ => {}
        }
    }
    let maximum = generation.max_generated.saturating_add(1).max(2);
    let current = generation
        .requested_generated
        .max(1)
        .saturating_add(1)
        .clamp(2, maximum);
    let top = maximum.clamp(4, MAX_MULTIPLIER);
    let items: Vec<Segment> = (2..=top)
        .map(|multiplier| Segment {
            label: format!("{multiplier}×"),
            sub: None,
            enabled: usable && multiplier <= maximum && generation.max_generated >= 1,
            mark: if multiplier == current {
                Mark::On
            } else {
                Mark::Off
            },
        })
        .collect();
    let columns = items.len();
    let (chosen, rects) = segments(ui, "fg-multiplier", &items, columns);
    store(&mut controls.fg_multiplier, &rects);
    if let Some(index) = chosen {
        let multiplier = 2 + index as u32;
        if multiplier != current {
            intent.fg_changed = true;
            intent.fg_mode = generation.requested_mode;
            intent.fg_generated = multiplier - 1;
        }
    }
    if generation.available && generation.max_generated <= 1 {
        note(
            ui,
            "This provider reports a maximum of 2x on the current GPU.",
            MUTED,
        );
    }

    separator(ui);
    if matches!(
        generation.latency_owner,
        LATENCY_ANTILAG2 | LATENCY_ANTILAG_VULKAN
    ) {
        let technology = if generation.latency_owner == LATENCY_ANTILAG_VULKAN {
            "AMD Vulkan Anti-Lag"
        } else {
            "AMD Anti-Lag 2"
        };
        heading(ui, technology);
        note(
            ui,
            format!(
                "{}: {} driver updates",
                technology, generation.latency_paced_frames
            ),
            MUTED,
        );
        latency_toggle(ui, &generation, intent, controls);
        note(
            ui,
            "AMD driver support is required. Boost maps to On.",
            MUTED,
        );
    } else if generation.latency_owner == LATENCY_XELL {
        heading(ui, "Intel XeLL");
        let status = if generation.latency_paced_frames == 0 {
            "Intel XeLL: waiting for paced frames".to_owned()
        } else {
            format!(
                "Intel XeLL active, {} frames paced",
                generation.latency_paced_frames
            )
        };
        note(ui, status, MUTED);
        latency_toggle(ui, &generation, intent, controls);
        if generation.requested_reflex == 0 && generation.effective_mode != 0 {
            note(ui, "XeSS FG requires On while generating frames.", AMBER);
        }
        note(ui, "Boost maps to On for XeLL.", MUTED);
    } else {
        heading(ui, "Reflex");
        if generation.backend == 1 && generation.effective_reflex != generation.requested_reflex {
            note(
                ui,
                "DLSS-G requires Reflex On while generating frames.",
                AMBER,
            );
        }
        let items: Vec<Segment> = [(0, "Off"), (1, "On"), (2, "On + Boost")]
            .iter()
            .map(|(id, label)| Segment {
                label: (*label).to_owned(),
                sub: None,
                enabled: generation.reflex_available,
                mark: if *id == generation.requested_reflex {
                    Mark::On
                } else {
                    Mark::Off
                },
            })
            .collect();
        let (chosen, rects) = segments(ui, "reflex", &items, 3);
        store(&mut controls.reflex, &rects);
        if let Some(index) = chosen
            && index as u32 != generation.requested_reflex
        {
            intent.reflex_changed = true;
            intent.reflex_mode = index as u32;
        }
        if !generation.reflex_available {
            note(ui, "Reflex is not reported available here.", MUTED);
        }

        if generation.latency_owner == LATENCY_REFLEX && generation.latency_paced_frames > 0 {
            note(
                ui,
                format!(
                    "Reflex active, {} frames paced",
                    generation.latency_paced_frames
                ),
                MUTED,
            );
        }
    }

    frame_limit(ui, &generation, intent, controls);
}

fn latency_toggle(
    ui: &mut Ui,
    generation: &GenerationStats,
    intent: &mut Intent,
    controls: &mut Controls,
) {
    let items: Vec<Segment> = ["Off", "On"]
        .iter()
        .enumerate()
        .map(|(id, label)| Segment {
            label: (*label).to_owned(),
            sub: None,
            enabled: generation.reflex_available,
            mark: if (generation.requested_reflex != 0) == (id != 0) {
                Mark::On
            } else {
                Mark::Off
            },
        })
        .collect();
    let (chosen, rects) = segments(ui, "latency_toggle", &items, 2);
    store(&mut controls.reflex, &rects);
    if let Some(index) = chosen
        && (generation.requested_reflex != 0) != (index != 0)
    {
        intent.reflex_changed = true;
        intent.reflex_mode = index as u32;
    }
}

/// Reflex's own limiter. The value is rendered frames, so generation multiplies it. The panel
/// receives the pointer only: the value is stepped, never typed.
fn frame_limit(
    ui: &mut Ui,
    generation: &GenerationStats,
    intent: &mut Intent,
    controls: &mut Controls,
) {
    heading(ui, "Frame limit");
    let usable = generation.reflex_available;
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
    let fps = if stored == 0 {
        0.0
    } else {
        1.0e6 / f64::from(stored)
    };
    let mut requested: Option<f64> = None;

    let (commit, rects) = ui
        .add_enabled_ui(usable, |ui| {
            stepper(
                ui,
                "frame-limit",
                fps,
                [fps > 0.0, fps < 500.0],
                |value, direction| {
                    if direction > 0.0 {
                        if value < 1.0 {
                            30.0
                        } else {
                            (value.round() + 1.0).min(500.0)
                        }
                    } else if value <= 10.0 {
                        0.0
                    } else {
                        value.round() - 1.0
                    }
                },
                |value| {
                    if value < 1.0 {
                        "Off".to_owned()
                    } else {
                        format!("{value:.1} fps")
                    }
                },
            )
        })
        .inner;
    controls.frame_limit = rects.map(Some);
    if let Some(value) = commit {
        requested = Some(if value < 1.0 { 0.0 } else { 1.0e6 / value });
    }

    // The cap OptiScaler calculates for VRR: presented frames 0.3 ms slower than the refresh
    // interval, here converted to the rendered frames this limit counts.
    let vrr = presents * (refresh_us + 300.0);
    let presets: [(&str, f64, bool); 5] = [
        ("30", 1.0e6 / 30.0, true),
        ("40", 1.0e6 / 40.0, true),
        ("60", 1.0e6 / 60.0, true),
        ("VRR", vrr, refresh_us > 0.0),
        ("Off", 0.0, true),
    ];
    let items: Vec<Segment> = presets
        .iter()
        .map(|(label, interval, enabled)| Segment {
            label: (*label).to_owned(),
            sub: None,
            enabled: usable && *enabled,
            mark: if *enabled && stored == interval.round() as u32 {
                Mark::On
            } else {
                Mark::Off
            },
        })
        .collect();
    let (chosen, rects) = segments(ui, "frame-limit-presets", &items, 5);
    store(&mut controls.frame_limit_preset, &rects);
    if let Some(index) = chosen {
        requested = Some(presets[index].1);
    }
    if let Some(interval) = requested {
        // Saturating float to integer conversion; the range above keeps it small.
        let interval = interval.round() as u32;
        if interval != stored {
            intent.frame_limit_changed = true;
            intent.frame_limit_us = interval;
        }
    }
    let line = if stored == 0 {
        if refresh_us > 0.0 {
            format!(
                "Limits rendered frames, before generation. VRR caps presented frames just under {:.0} Hz.",
                1.0e6 / refresh_us
            )
        } else {
            "Limits rendered frames, before generation.".to_owned()
        }
    } else {
        let rendered = 1.0e6 / f64::from(stored);
        format!(
            "{rendered:.1} fps rendered, up to {:.1} presented",
            rendered * presents
        )
    };
    note(ui, line, MUTED);
}

fn sharpen_tab(ui: &mut Ui, stats: &Stats<'_>, intent: &mut Intent, controls: &mut Controls) {
    let mut options = stats.sharpening;
    let mut changed = false;
    let vendor = matches!(stats.backend, 2..=4);

    heading(ui, "Mode");
    let items: Vec<Segment> = ["Off", "RCAS", "Depth\nRCAS", "Depth\nDAS", "Vendor"]
        .iter()
        .enumerate()
        .map(|(index, label)| Segment {
            label: (*label).to_owned(),
            sub: None,
            enabled: index != 4 || vendor,
            mark: if options.mode == index as u32 {
                Mark::On
            } else {
                Mark::Off
            },
        })
        .collect();
    let (chosen, rects) = segments(ui, "sharpen-mode", &items, 5);
    store(&mut controls.sharpening_mode, &rects);
    if let Some(index) = chosen
        && options.mode != index as u32
    {
        options.mode = index as u32;
        changed = true;
    }
    if !vendor {
        note(ui, "Vendor sharpening: FSR only", MUTED);
    }

    if options.mode != 0 {
        let (apply, rect) = ui
            .add_enabled_ui(options.mode != 4 || vendor, |ui| {
                slider_row(ui, "Sharpness", &mut options.sharpness, 0.0..=1.0, 2)
            })
            .inner;
        changed |= apply;
        controls.sharpening_strength = Some(rect);
    }
    if matches!(options.mode, 1..=3) {
        let (apply, rect) = toggle(
            ui,
            "Motion adaptive sharpening",
            "Changes sharpness for moving pixels",
            &mut options.mas,
        );
        changed |= apply;
        controls.sharpening_mas = Some(rect);
        if options.mas != 0 {
            changed |= slider_row(
                ui,
                "Motion sharpness",
                &mut options.motion_sharpness,
                -1.0..=1.0,
                2,
            )
            .0;
            changed |= slider_row(
                ui,
                "Motion threshold (pixels)",
                &mut options.motion_threshold,
                0.0..=100.0,
                1,
            )
            .0;
            changed |= slider_row(
                ui,
                "Motion scale limit (pixels)",
                &mut options.motion_scale_limit,
                0.01..=100.0,
                2,
            )
            .0;
        }
        if options.mode == 1 {
            let (apply, rect) = toggle(
                ui,
                "Contrast adaptation",
                "RCAS only",
                &mut options.contrast_enabled,
            );
            changed |= apply;
            controls.sharpening_contrast = Some(rect);
            if options.contrast_enabled != 0 {
                changed |= slider_row(ui, "Contrast", &mut options.contrast, 0.0..=2.0, 2).0;
            }
        }
        if matches!(options.mode, 2..=3) {
            if !stats.sharpening_depth_available {
                note(
                    ui,
                    "Depth-aware: needs depth; using RCAS until depth is available",
                    AMBER,
                );
            }
            ui.add_enabled_ui(stats.sharpening_depth_available, |ui| {
                let (apply, rect) =
                    slider_row(ui, "Depth scale", &mut options.depth_scale, 0.0..=600.0, 1);
                changed |= apply;
                controls.sharpening_depth = Some(rect);
                changed |= slider_row(ui, "Depth bias", &mut options.depth_bias, 0.0..=1.0, 3).0;
                changed |= toggle(
                    ui,
                    "Clamp decoded output to 0..1",
                    "",
                    &mut options.clamp_output,
                )
                .0;
            });
        }
    }
    if matches!(options.mode, 1..=3) {
        let usable = options.mode != 1 || options.mas != 0;
        let mut debug = options.debug;
        let (tapped, rect) = switch_row(
            ui,
            "sharpen-debug",
            "Sharpening debug tint",
            Some((
                if usable {
                    "Colours the sharpened pixels".to_owned()
                } else {
                    "MAS debug: enable Motion Adaptive Sharpening".to_owned()
                },
                MUTED,
            )),
            debug != 0,
            usable,
        );
        if tapped {
            debug = u32::from(debug == 0);
            options.debug = debug;
            changed = true;
        }
        controls.sharpening_debug = Some(rect);
    }
    if changed {
        intent.sharpening_changed = true;
        intent.sharpening = options;
    }
}

/// Options the host defines. The panel knows nothing about what they do.
fn game_tab(ui: &mut Ui, stats: &Stats<'_>, intent: &mut Intent, controls: &mut Controls) {
    note(
        ui,
        "Options supplied by the game integration. Each applies while the game runs unless it says otherwise.",
        MUTED,
    );
    for (index, option) in stats.host_options.iter().enumerate() {
        let Some(option) = option else { continue };
        let detail = option
            .restart
            .then(|| ("Applies at next start".to_owned(), BLUE_TEXT));
        let mut chosen = None;
        if option.toggle {
            let (tapped, rect) = switch_row(
                ui,
                ("host", index),
                option.label,
                detail,
                option.value != 0,
                true,
            );
            controls.host_option[index] = Some(rect);
            if tapped {
                chosen = Some(i32::from(option.value == 0));
            }
        } else {
            let divisor = f64::from(option.divisor.max(1));
            let digits = decimals(option.divisor);
            let [minimum, maximum, step] = option.range;
            ui.add(Label::new(RichText::new(option.label).size(15.5).color(TEXT)).wrap());
            if let Some((text, color)) = &detail {
                note(ui, text.as_str(), *color);
            }
            let value = f64::from(option.value);
            let (commit, rects) = stepper(
                ui,
                ("host-number", index),
                value,
                [option.value > minimum, option.value < maximum],
                |current, direction| {
                    (current + direction * f64::from(step))
                        .clamp(f64::from(minimum), f64::from(maximum))
                },
                |current| format!("{:.digits$}", current / divisor),
            );
            controls.host_option[index] = Some(rects[1]);
            if let Some(value) = commit {
                chosen = Some(value.round() as i32);
            }
        }
        separator(ui);
        if let Some(value) = chosen
            && value != option.value
        {
            intent.host_option_changed = true;
            intent.host_option_index = index as u32;
            intent.host_option_value = value;
        }
    }
}

fn display_tab(ui: &mut Ui, frame: &Frame<'_, '_>, intent: &mut Intent, controls: &mut Controls) {
    let stats = frame.stats;
    let mut preferences = frame.preferences;
    let mut layout_changed = false;

    let (tapped, rect) = switch_row(
        ui,
        "hud",
        "FPS overlay",
        Some((
            "Counts rendered and presented frames. The overlay measures no latency.".to_owned(),
            MUTED,
        )),
        stats.show_performance_hud,
        true,
    );
    controls.hud = Some(rect);
    if tapped {
        intent.performance_hud_changed = true;
        intent.performance_hud = !stats.show_performance_hud;
    }

    heading(ui, "Overlay detail");
    let items: Vec<Segment> = ["Compact", "Detailed"]
        .iter()
        .enumerate()
        .map(|(index, label)| Segment {
            label: (*label).to_owned(),
            sub: None,
            enabled: stats.show_performance_hud,
            mark: if preferences.detailed_hud == (index == 1) {
                Mark::On
            } else {
                Mark::Off
            },
        })
        .collect();
    let (chosen, rects) = segments(ui, "hud-detail", &items, 2);
    store(&mut controls.hud_detail, &rects);
    if let Some(index) = chosen
        && preferences.detailed_hud != (index == 1)
    {
        preferences.detailed_hud = index == 1;
        layout_changed = true;
    }

    heading(ui, "Panel size");
    let items: Vec<Segment> = PanelSize::ALL
        .iter()
        .map(|size| Segment {
            label: size.label().to_owned(),
            sub: None,
            enabled: true,
            mark: if *size == preferences.size {
                Mark::On
            } else {
                Mark::Off
            },
        })
        .collect();
    let (chosen, rects) = segments(ui, "panel-size", &items, 4);
    store(&mut controls.panel_size, &rects);
    if let Some(index) = chosen
        && PanelSize::ALL[index] != preferences.size
    {
        preferences.size = PanelSize::ALL[index];
        layout_changed = true;
    }
    note(
        ui,
        "Auto follows the screen height. Larger sizes help on small handheld screens.",
        MUTED,
    );

    heading(ui, "Panel side");
    let items: Vec<Segment> = ["Left", "Right"]
        .iter()
        .enumerate()
        .map(|(index, label)| Segment {
            label: (*label).to_owned(),
            sub: None,
            enabled: true,
            mark: if preferences.right == (index == 1) {
                Mark::On
            } else {
                Mark::Off
            },
        })
        .collect();
    let (chosen, rects) = segments(ui, "panel-side", &items, 2);
    store(&mut controls.panel_side, &rects);
    if let Some(index) = chosen
        && preferences.right != (index == 1)
    {
        preferences.right = index == 1;
        layout_changed = true;
    }

    if layout_changed {
        intent.ui_preferences_changed = true;
        intent.ui_preferences = preferences.to_bits();
    }
}

// ---------------------------------------------------------------------------------------------
// Widgets. Every interactive one is at least `TARGET` points in both directions.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Off,
    On,
    /// Asked for and not in effect yet.
    Pending,
}

struct Segment {
    label: String,
    sub: Option<String>,
    enabled: bool,
    mark: Mark,
}

/// A row of equal buttons in a well, one of which is usually chosen. Returns the tapped index and
/// every button's rectangle.
fn segments(
    ui: &mut Ui,
    salt: impl egui::AsIdSalt,
    items: &[Segment],
    columns: usize,
) -> (Option<usize>, Vec<Rect>) {
    if items.is_empty() {
        return (None, Vec::new());
    }
    let columns = columns.clamp(1, items.len());
    let rows = items.len().div_ceil(columns);
    let tall = items
        .iter()
        .any(|item| item.sub.is_some() || item.label.contains('\n'));
    let cell_height = if tall { 58.0 } else { TARGET };
    let (pad, gap) = (4.0, 4.0);
    let width = ui.available_width();
    let height = pad * 2.0 + rows as f32 * cell_height + (rows - 1) as f32 * gap;
    let (outer, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let base = ui.id().with(salt);
    let painter = ui.painter();
    painter.rect_filled(outer, 12.0, WELL);
    painter.rect_stroke(outer, 12.0, Stroke::new(1.0, WELL_LINE), StrokeKind::Inside);
    let cell_width = (width - pad * 2.0 - gap * (columns - 1) as f32) / columns as f32;
    let mut chosen = None;
    let mut rects = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let (row, column) = (index / columns, index % columns);
        let rect = Rect::from_min_size(
            outer.min
                + vec2(
                    pad + column as f32 * (cell_width + gap),
                    pad + row as f32 * (cell_height + gap),
                ),
            vec2(cell_width, cell_height),
        );
        let enabled = item.enabled && ui.is_enabled();
        let response = ui.interact(
            rect,
            base.with(index),
            if enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        let pressed = enabled && response.is_pointer_button_down_on();
        let (fill, text) = match (item.mark, enabled) {
            (Mark::Pending, _) => (AMBER_FILL, AMBER_TEXT),
            (Mark::On, true) => (SELECTED, Color32::WHITE),
            (Mark::On, false) => (SELECTED_DIM, MUTED),
            (Mark::Off, true) if pressed => (PRESSED, TEXT),
            (Mark::Off, true) => (Color32::TRANSPARENT, Color32::from_rgb(0xd2, 0xd7, 0xde)),
            (Mark::Off, false) => (Color32::TRANSPARENT, DISABLED),
        };
        let painter = ui.painter();
        if fill != Color32::TRANSPARENT {
            painter.rect_filled(rect, 9.0, fill);
        }
        if item.mark == Mark::Pending {
            painter.rect_stroke(rect, 9.0, Stroke::new(2.0, AMBER_DOT), StrokeKind::Inside);
        }
        let clip = painter.with_clip_rect(rect);
        match &item.sub {
            Some(sub) => {
                clip.text(
                    rect.center() - vec2(0.0, 8.0),
                    Align2::CENTER_CENTER,
                    &item.label,
                    FontId::proportional(14.5),
                    text,
                );
                clip.text(
                    rect.center() + vec2(0.0, 11.0),
                    Align2::CENTER_CENTER,
                    sub,
                    FontId::monospace(11.0),
                    text.gamma_multiply(0.8),
                );
            }
            None => {
                clip.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    &item.label,
                    FontId::proportional(if item.label.contains('\n') {
                        13.5
                    } else {
                        14.5
                    }),
                    text,
                );
            }
        }
        if enabled && response.clicked() {
            chosen = Some(index);
        }
        rects.push(rect);
    }
    (chosen, rects)
}

/// A full-width row with a title, an optional detail line and a switch. The whole row is the
/// target, not only the switch. Returns whether it was tapped, and the row.
fn switch_row(
    ui: &mut Ui,
    salt: impl egui::AsIdSalt,
    title: &str,
    detail: Option<(String, Color32)>,
    on: bool,
    enabled: bool,
) -> (bool, Rect) {
    let enabled = enabled && ui.is_enabled();
    let width = ui.available_width();
    let text_width = (width - 56.0 - 14.0).max(80.0);
    let inner = ui.horizontal(|ui| {
        ui.set_min_height(56.0);
        ui.vertical(|ui| {
            ui.set_width(text_width);
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.add(
                Label::new(RichText::new(title).size(15.5).color(if enabled {
                    TEXT
                } else {
                    DISABLED
                }))
                .wrap(),
            );
            if let Some((text, color)) = detail
                && !text.is_empty()
            {
                ui.add(Label::new(RichText::new(text).size(13.0).color(color)).wrap());
            }
        });
        ui.allocate_exact_size(vec2(56.0, 34.0), Sense::hover()).0
    });
    let row = inner.response.rect;
    let response = ui.interact(
        row,
        ui.id().with(salt),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let painter = ui.painter();
    if enabled && response.is_pointer_button_down_on() {
        painter.rect_filled(row.expand(4.0), 10.0, Color32::from_white_alpha(8));
    }
    let switch = inner.inner;
    let track = if on && enabled {
        SWITCH_ON
    } else if on {
        SELECTED_DIM
    } else {
        SWITCH_OFF
    };
    painter.rect_filled(switch, 17.0, track);
    let knob_x = if on {
        switch.right() - 17.0
    } else {
        switch.left() + 17.0
    };
    painter.circle_filled(
        pos2(knob_x, switch.center().y),
        13.0,
        if !enabled {
            DISABLED
        } else if on {
            Color32::WHITE
        } else {
            KNOB
        },
    );
    (enabled && response.clicked(), row)
}

/// A switch row bound to a 0/1 field. Returns whether the field changed, and the row.
fn toggle(ui: &mut Ui, title: &str, detail: &str, value: &mut u32) -> (bool, Rect) {
    let detail = (!detail.is_empty()).then(|| (detail.to_owned(), MUTED));
    let (tapped, rect) = switch_row(ui, title, title, detail, *value != 0, true);
    if tapped {
        *value = u32::from(*value == 0);
    }
    (tapped, rect)
}

#[derive(Debug, Clone, Copy)]
enum Glyph {
    Minus,
    Plus,
    Close,
}

fn paint_button(painter: &Painter, rect: Rect, pressed: bool, enabled: bool) {
    painter.rect_filled(rect, 12.0, if pressed { PRESSED } else { BUTTON });
    painter.rect_stroke(
        rect,
        12.0,
        Stroke::new(1.0, if enabled { BUTTON_LINE } else { WELL_LINE }),
        StrokeKind::Inside,
    );
}

fn paint_glyph(painter: &Painter, rect: Rect, glyph: Glyph, color: Color32) {
    let c = rect.center();
    let stroke = Stroke::new(2.4, color);
    match glyph {
        Glyph::Minus => {
            painter.line_segment([c - vec2(7.0, 0.0), c + vec2(7.0, 0.0)], stroke);
        }
        Glyph::Plus => {
            painter.line_segment([c - vec2(7.0, 0.0), c + vec2(7.0, 0.0)], stroke);
            painter.line_segment([c - vec2(0.0, 7.0), c + vec2(0.0, 7.0)], stroke);
        }
        Glyph::Close => {
            painter.line_segment([c - vec2(6.0, 6.0), c + vec2(6.0, 6.0)], stroke);
            painter.line_segment([c + vec2(-6.0, 6.0), c + vec2(6.0, -6.0)], stroke);
        }
    }
}

/// A held step button. Created on press, repeated while held, committed on release, so a host is
/// asked to apply one value per hold rather than one per repeat.
#[derive(Debug, Clone, Copy)]
struct Hold {
    shown: f64,
    next: f64,
    direction: f64,
}

/// Initial delay before a held button repeats, and the repeat interval, in seconds.
const REPEAT_DELAY: f64 = 0.45;
const REPEAT_INTERVAL: f64 = 0.07;

/// Run the hold logic for a down/up pair. Returns the value to show and, on release, the value
/// to apply.
fn hold_buttons(
    ui: &Ui,
    id: Id,
    buttons: [&Response; 2],
    value: f64,
    next: &dyn Fn(f64, f64) -> f64,
) -> (f64, Option<f64>) {
    let now = ui.input(|input| input.time);
    let mut hold: Option<Hold> = ui.data(|data| data.get_temp(id));
    for (direction, response) in [(-1.0, buttons[0]), (1.0, buttons[1])] {
        if !response.is_pointer_button_down_on() {
            continue;
        }
        match &mut hold {
            None => {
                hold = Some(Hold {
                    shown: next(value, direction),
                    next: now + REPEAT_DELAY,
                    direction,
                });
            }
            Some(held) if held.direction == direction && now >= held.next => {
                held.shown = next(held.shown, direction);
                held.next = now + REPEAT_INTERVAL;
            }
            Some(_) => {}
        }
    }
    let down = buttons
        .iter()
        .any(|response| response.is_pointer_button_down_on());
    let mut commit = None;
    if !down && let Some(held) = hold.take() {
        commit = Some(held.shown);
    }
    ui.data_mut(|data| match hold {
        Some(held) => {
            data.insert_temp(id, held);
        }
        None => {
            data.remove::<Hold>(id);
        }
    });
    (hold.map_or(value, |held| held.shown), commit)
}

/// `[−] value [+]` across the full width. `next(value, ±1.0)` gives the neighbouring value.
/// Returns the value to apply once a press or hold ends, and the two buttons.
fn stepper(
    ui: &mut Ui,
    salt: impl egui::AsIdSalt,
    value: f64,
    can: [bool; 2],
    next: impl Fn(f64, f64) -> f64,
    text: impl Fn(f64) -> String,
) -> (Option<f64>, [Rect; 2]) {
    let id = ui.id().with(salt);
    let enabled = ui.is_enabled();
    let width = ui.available_width();
    let (row, _) = ui.allocate_exact_size(vec2(width, TARGET), Sense::hover());
    let minus = Rect::from_min_size(row.min, Vec2::splat(TARGET));
    let plus = Rect::from_min_size(pos2(row.right() - TARGET, row.top()), Vec2::splat(TARGET));
    let value_box = Rect::from_min_max(
        pos2(minus.right() + 8.0, row.top()),
        pos2(plus.left() - 8.0, row.bottom()),
    );
    // Drag as well as click: a click-only target stops counting as held after egui's long-press
    // limit, which would end a repeat while the finger is still down.
    let sense = |allowed: bool| {
        if allowed && enabled {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        }
    };
    let down = ui.interact(minus, id.with("down"), sense(can[0]));
    let up = ui.interact(plus, id.with("up"), sense(can[1]));
    let (shown, commit) = hold_buttons(ui, id.with("hold"), [&down, &up], value, &next);
    let painter = ui.painter();
    for (rect, response, glyph, allowed) in [
        (minus, &down, Glyph::Minus, can[0]),
        (plus, &up, Glyph::Plus, can[1]),
    ] {
        let usable = allowed && enabled;
        paint_button(
            painter,
            rect,
            usable && response.is_pointer_button_down_on(),
            usable,
        );
        paint_glyph(painter, rect, glyph, if usable { TEXT } else { DISABLED });
    }
    painter.rect_filled(value_box, 12.0, WELL);
    painter.rect_stroke(
        value_box,
        12.0,
        Stroke::new(1.0, WELL_LINE),
        StrokeKind::Inside,
    );
    painter.text(
        value_box.center(),
        Align2::CENTER_CENTER,
        text(shown),
        FontId::monospace(16.0),
        if enabled { TEXT } else { DISABLED },
    );
    (commit, [minus, plus])
}

/// Title and value, then `[−] slider [+]`. The slider applies when the drag ends, the buttons when
/// the press ends, so a host is never asked to rebuild on every pixel. Returns whether `value`
/// changed, and the slider's own rectangle.
fn slider_row(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: RangeInclusive<f32>,
    digits: usize,
) -> (bool, Rect) {
    let id = ui.id().with(label);
    let enabled = ui.is_enabled();
    let initial = *value;
    let dragging: Option<f32> = ui.data(|data| data.get_temp(id));
    let (low, high) = (*range.start(), *range.end());
    let step = f64::from(high - low) / 50.0;
    let header = ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(14.0).strong().color(if enabled {
            SECTION
        } else {
            DISABLED
        }));
        ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover())
            .0
    });
    let width = ui.available_width();
    let (row, _) = ui.allocate_exact_size(vec2(width, TARGET), Sense::hover());
    let minus = Rect::from_min_size(row.min, Vec2::splat(TARGET));
    let plus = Rect::from_min_size(pos2(row.right() - TARGET, row.top()), Vec2::splat(TARGET));
    let track = Rect::from_min_max(
        pos2(minus.right() + 12.0, row.top()),
        pos2(plus.left() - 12.0, row.bottom()),
    );
    let sense = if enabled {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    };
    let down = ui.interact(minus, id.with("down"), sense);
    let up = ui.interact(plus, id.with("up"), sense);
    let (stepped, commit) = hold_buttons(
        ui,
        id.with("hold"),
        [&down, &up],
        f64::from(dragging.unwrap_or(initial)),
        &|current, direction| (current + direction * step).clamp(f64::from(low), f64::from(high)),
    );

    let mut shown = stepped as f32;
    let response = ui
        .scope_builder(UiBuilder::new().max_rect(track), |ui| {
            ui.spacing_mut().slider_width = track.width();
            ui.spacing_mut().interact_size.y = TARGET;
            ui.add(
                egui::Slider::new(&mut shown, range)
                    .show_value(false)
                    .trailing_fill(true),
            )
        })
        .inner;
    if response.dragged() {
        ui.data_mut(|data| data.insert_temp(id, shown));
    }
    let mut apply = response.drag_stopped() || (response.changed() && !response.dragged());
    if apply {
        ui.data_mut(|data| data.remove::<f32>(id));
        *value = shown;
    }
    if let Some(stepped) = commit {
        *value = stepped as f32;
        apply = true;
    }

    let painter = ui.painter();
    painter.text(
        header.inner.right_center(),
        Align2::RIGHT_CENTER,
        format!("{shown:.digits$}"),
        FontId::monospace(14.0),
        if enabled { TEXT } else { DISABLED },
    );
    for (rect, response, glyph) in [(minus, &down, Glyph::Minus), (plus, &up, Glyph::Plus)] {
        paint_button(
            painter,
            rect,
            enabled && response.is_pointer_button_down_on(),
            enabled,
        );
        paint_glyph(painter, rect, glyph, if enabled { TEXT } else { DISABLED });
    }
    (apply && *value != initial, response.rect)
}

fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(text).size(13.5).strong().color(SECTION));
}

fn note(ui: &mut Ui, text: impl Into<String>, color: Color32) {
    ui.add(Label::new(RichText::new(text.into()).size(13.0).color(color)).wrap());
}

fn pending_note(ui: &mut Ui, text: &str) {
    ui.horizontal_wrapped(|ui| {
        let chip =
            ui.painter()
                .layout_no_wrap("Applying".to_owned(), FontId::proportional(12.0), AMBER);
        let (rect, _) = ui.allocate_exact_size(chip.size() + vec2(18.0, 8.0), Sense::hover());
        ui.painter()
            .rect_filled(rect, 11.0, Color32::from_rgb(0x3f, 0x32, 0x14));
        ui.painter().galley(rect.min + vec2(9.0, 4.0), chip, AMBER);
        ui.label(RichText::new(text).size(13.0).color(AMBER));
    });
}

fn banner(ui: &mut Ui, title: &str, detail: &str) {
    egui::Frame::new()
        .fill(WARN_FILL)
        .stroke(Stroke::new(1.0, WARN_LINE))
        .corner_radius(12.0)
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(RichText::new(title).size(14.5).strong().color(AMBER_TEXT));
            ui.add(
                Label::new(
                    RichText::new(detail)
                        .size(13.0)
                        .color(Color32::from_rgb(0xe8, 0xd6, 0xab)),
                )
                .wrap(),
            );
        });
}

fn empty_card(ui: &mut Ui, title: &str, detail: &str) {
    egui::Frame::new()
        .stroke(Stroke::new(1.0, BUTTON_LINE))
        .corner_radius(12.0)
        .inner_margin(16.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.label(RichText::new(title).size(15.0).strong().color(TEXT));
            ui.add(Label::new(RichText::new(detail).size(13.0).color(MUTED)).wrap());
        });
}

fn separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, Stroke::new(1.0, LINE));
}

fn store<const N: usize>(slots: &mut [Option<Rect>; N], rects: &[Rect]) {
    for (slot, rect) in slots.iter_mut().zip(rects) {
        *slot = Some(*rect);
    }
}

fn decimals(divisor: u32) -> usize {
    let mut digits = 0;
    let mut remaining = divisor.max(1);
    while remaining >= 10 {
        remaining /= 10;
        digits += 1;
    }
    digits
}

fn sr_name(backend: u32) -> &'static str {
    SR_BACKENDS
        .iter()
        .find(|(id, _)| *id == backend)
        .map_or("SR", |(_, name)| name)
}

fn quality_label(stats: &Stats<'_>) -> String {
    match stats.quality {
        Some(Quality::Native) if stats.backend == 1 => "DLAA".to_owned(),
        Some(level) => level.label().to_owned(),
        None => format!("level {}", stats.quality_raw),
    }
}

fn fg_backend_name(backend: u32) -> &'static str {
    FG_PROVIDERS
        .iter()
        .find(|(id, _)| *id == backend)
        .map_or("Off", |(_, name)| name)
}

fn waiting_reason(reason: u32) -> &'static str {
    match reason {
        1 => "Waiting for a matching scene and window",
        2 => "Waiting for reconstruction inputs",
        3 => "Waiting for a complete frame submission",
        4 => "Waiting for flight, hangar, or briefing",
        5 => "Suspended for a camera cut",
        6 => "Current VSync mode is unsupported",
        8 => "Waiting for a HUD-less scene color",
        _ => "Waiting for SDK-confirmed generation",
    }
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

// ---------------------------------------------------------------------------------------------
// The FPS overlay

/// Generation state as the compact HUD words it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HudState {
    Active(u32),
    Off,
    Unavailable,
    Suspended,
}

/// What the detailed HUD adds: the SR state and the frame limit.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct HudDetail {
    pub(crate) sr: String,
    pub(crate) limit: String,
}

impl HudDetail {
    pub(crate) fn of(stats: &Stats<'_>) -> Self {
        let sr = if stats.backend_choices == 0 || !stats.enabled {
            "off".to_owned()
        } else {
            format!("{} {}", sr_name(stats.backend), quality_label(stats))
        };
        let limit = match stats.generation.frame_limit_us {
            0 => "off".to_owned(),
            interval => format!("{:.0} fps", 1.0e6 / f64::from(interval)),
        };
        Self { sr, limit }
    }
}

/// The compact HUD's text. Rates change about twice a second, so the strings are rebuilt only when
/// the rates, the generation state or the detail differ from the ones they were built for.
#[derive(Default)]
pub(crate) struct HudCache {
    key: Option<(Rates, HudState, Option<HudDetail>)>,
    rows: Vec<(&'static str, String)>,
    active: bool,
}

impl HudCache {
    pub(crate) fn update(
        &mut self,
        rates: Rates,
        generation: &GenerationStats,
        detail: Option<HudDetail>,
    ) {
        let state = if generation.active {
            HudState::Active(generation.effective_generated + 1)
        } else if generation.requested_mode == 0 {
            HudState::Off
        } else if !generation.available {
            HudState::Unavailable
        } else {
            HudState::Suspended
        };
        let key = (rates, state, detail);
        if self.key.as_ref() == Some(&key) {
            return;
        }
        let format_rate =
            |rate: Option<f64>| rate.map_or_else(|| "--".to_owned(), |value| format!("{value:.1}"));
        let mut rows = vec![
            ("Rendered", format_rate(rates.rendered)),
            ("Presented", format_rate(rates.presented)),
            (
                "FG",
                match state {
                    HudState::Active(multiplier) => format!("active {multiplier}x"),
                    HudState::Off => "off".to_owned(),
                    HudState::Unavailable => "unavailable".to_owned(),
                    HudState::Suspended => "suspended".to_owned(),
                },
            ),
        ];
        if let Some(detail) = &key.2 {
            rows.push(("SR", detail.sr.clone()));
            rows.push(("Limit", detail.limit.clone()));
        }
        self.rows = rows;
        self.active = matches!(state, HudState::Active(_));
        self.key = Some(key);
    }
}

pub(crate) fn performance_hud(ctx: &Context, hud: &HudCache) {
    egui::Area::new(Id::new("rsf-performance"))
        .anchor(Align2::RIGHT_TOP, vec2(-14.0, 14.0))
        .interactable(false)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(Color32::from_black_alpha(185))
                .corner_radius(10.0)
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    egui::Grid::new("rsf-performance-grid")
                        .num_columns(2)
                        .spacing(vec2(16.0, 3.0))
                        .show(ui, |ui| {
                            for (key, value) in &hud.rows {
                                ui.label(RichText::new(*key).monospace().size(13.0).color(MUTED));
                                let color = if *key == "FG" && hud.active {
                                    BLUE_TEXT
                                } else {
                                    TEXT
                                };
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new(value)
                                                .monospace()
                                                .size(13.0)
                                                .color(color),
                                        );
                                    },
                                );
                                ui.end_row();
                            }
                        });
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::epaint::Shape;

    /// Everything the panel wrote this frame, joined, so a test can assert on what it says and not
    /// only on what it returns. Read out of the shapes before they become triangles, which is the
    /// last point where the text is still text.
    fn words(selection: &Selection, stats: &Stats<'_>, tab: Tab) -> String {
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
        let mut view = View {
            tab,
            ..View::default()
        };
        // Twice, because egui lays a new area out once invisibly to learn how big it is and that
        // pass paints nothing.
        for _ in 0..2 {
            let mut intent = Intent::default();
            let mut controls = Controls::default();
            let frame = Frame {
                selection,
                stats,
                rates: Rates::default(),
                preferences: UiPreferences::default(),
            };
            let output = ctx.run_ui(egui::RawInput::default(), |ui| {
                show(ui.ctx(), &frame, &mut view, &mut intent, &mut controls);
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
    fn amd_antilag_reports_driver_work_without_claiming_measured_latency() {
        let mut stats = Stats::default();
        stats.generation.available = true;
        stats.generation.reflex_available = true;
        stats.generation.latency_owner = LATENCY_ANTILAG2;
        stats.generation.latency_paced_frames = 120;
        let text = words(&Selection::default(), &stats, Tab::Frames);
        assert!(text.contains("AMD Anti-Lag 2"));
        assert!(text.contains("120 driver updates"));
        assert!(text.contains("Off") && text.contains("On"));
        assert!(!text.contains("On + Boost"));
        stats.generation.latency_owner = LATENCY_ANTILAG_VULKAN;
        let text = words(&Selection::default(), &stats, Tab::Frames);
        assert!(text.contains("AMD Vulkan Anti-Lag"));
        assert!(!text.contains("AMD Anti-Lag 2"));
    }

    #[test]
    fn a_request_that_has_not_landed_is_said_out_loud() {
        let stats = Stats {
            backend_loaded: true,
            backend_choices: 0x3e,
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
            words(&asked_to_enable, &stats, Tab::Upscale)
                .contains("requested on, still off in effect"),
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
            words(&asked_for_balanced, &stats, Tab::Upscale)
                .contains("requested Balanced, still Native in effect")
        );
    }

    #[test]
    fn what_the_panel_will_not_say() {
        let mut stats = Stats {
            backend_loaded: true,
            backend_choices: 0x3e,
            sharpening_available: true,
            show_performance_hud: true,
            ..Stats::default()
        };
        stats.generation.available = true;
        stats.generation.reflex_available = true;
        for tab in Tab::ALL {
            // "On + Boost" names the SDK's Reflex option, rather than a measured speed claim.
            let said = words(&Selection::default(), &stats, tab)
                .to_lowercase()
                .replace("on + boost", "on");

            // Counter-derived FPS is available. Unmeasured improvements and quality claims remain
            // forbidden; requested generation alone must never invent a measured FPS gain.
            for forbidden in ["faster", "gain", "boost", "speedup", "score"] {
                assert!(
                    !said.contains(forbidden),
                    "the {tab:?} tab said {forbidden:?}, which this build cannot measure"
                );
            }
            // Latency and frame time may be named, but only to say they are not measured. Checked
            // per line, because a word list alone cannot tell a claim from a denial of one.
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

    #[test]
    fn the_sheet_fits_small_screens_and_follows_the_side() {
        let deck = vec2(1280.0, 800.0);
        let left = sheet_rect(deck, UiPreferences::default());
        assert_eq!(left.left(), MARGIN);
        assert!(left.right() < deck.x && left.bottom() <= deck.y - MARGIN + 0.5);
        let right = sheet_rect(
            deck,
            UiPreferences {
                right: true,
                ..UiPreferences::default()
            },
        );
        assert!((right.right() - (deck.x - MARGIN)).abs() < 0.5);
        // A screen narrower than the sheet keeps the margins rather than spilling off the edge.
        let narrow = sheet_rect(vec2(400.0, 300.0), UiPreferences::default());
        assert!(narrow.right() <= 400.0 - MARGIN + 0.5);
    }
}
