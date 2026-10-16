//! The ribbon: tabs of grouped commands, as in Word and WPS.

use eframe::egui::{self, vec2, Color32, FontFamily, FontId, Rect, RichText, Sense, Stroke};

use crate::app::{Action, Dialog, Zoom};
use crate::i18n::I18n;
use crate::icons::{self, Icon};
use crate::theme::{self, Palette};
use crate::worker::{
    Align, CaretFormat, EditState, FormatChange, Insert, PageChange, Request, StyleEntry,
    TableCommand, FONT_SIZES,
};

/// Ribbon tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RibbonTab {
    Home,
    Insert,
    Layout,
    Review,
    View,
    Recognition,
}

impl RibbonTab {
    pub(crate) const ALL: [Self; 6] = [
        Self::Home,
        Self::Insert,
        Self::Layout,
        Self::Review,
        Self::View,
        Self::Recognition,
    ];

    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Home => "ribbon.home",
            Self::Insert => "ribbon.insert",
            Self::Layout => "ribbon.layout",
            Self::Review => "ribbon.review",
            Self::View => "ribbon.view",
            Self::Recognition => "ribbon.recognition",
        }
    }
}

/// What the ribbon needs to know to draw itself.
pub(crate) struct RibbonState<'a> {
    pub(crate) i18n: &'a I18n,
    pub(crate) palette: Palette,
    pub(crate) format: &'a CaretFormat,
    pub(crate) styles: &'a [StyleEntry],
    pub(crate) state: &'a EditState,
    /// A document tab is active and ready.
    pub(crate) document: bool,
    pub(crate) navigation: bool,
    pub(crate) report: bool,
    pub(crate) dark: bool,
    pub(crate) zoom: f32,
    pub(crate) system_fonts: bool,
    /// Last colours picked, for the split buttons.
    pub(crate) font_color: [u8; 3],
}

/// Fonts the family box offers: the names documents use, each drawn by its
/// bundled metric-compatible face.
const FONT_FAMILIES: &[&str] = &[
    "Calibri",
    "Cambria",
    "Times New Roman",
    "Arial",
    "Courier New",
    "Georgia",
    "Segoe UI",
    "Liberation Serif",
    "Liberation Sans",
];

/// Ribbon body height, labels included.
const BODY_HEIGHT: f32 = 92.0;
/// Height of a group's command area.
const GROUP_HEIGHT: f32 = 70.0;

/// The ribbon tab strip; returns whether the File button was clicked.
pub(crate) fn tab_strip(
    ui: &mut egui::Ui,
    tab: &mut RibbonTab,
    i18n: &I18n,
    palette: Palette,
) -> bool {
    let mut file = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        let file_button = egui::Button::new(
            RichText::new(i18n.t("ribbon.file"))
                .color(Color32::WHITE)
                .size(13.5),
        )
        .fill(theme::ACCENT)
        .corner_radius(3)
        .min_size(vec2(64.0, 24.0));
        if ui.add(file_button).clicked() {
            file = true;
        }
        ui.add_space(6.0);
        for candidate in RibbonTab::ALL {
            let selected = *tab == candidate;
            let text = RichText::new(i18n.t(candidate.key())).size(13.5);
            let text = if selected {
                text.color(theme::ACCENT)
                    .family(FontFamily::Name(theme::BOLD.into()))
            } else {
                text.color(palette.tab_text)
            };
            let response = ui.add(
                egui::Button::new(text)
                    .frame(false)
                    .min_size(vec2(0.0, 24.0)),
            );
            if selected {
                let rect = response.rect;
                ui.painter().line_segment(
                    [
                        egui::pos2(rect.left() + 4.0, rect.bottom()),
                        egui::pos2(rect.right() - 4.0, rect.bottom()),
                    ],
                    Stroke::new(2.5, theme::ACCENT),
                );
            }
            if response.clicked() {
                *tab = candidate;
            }
        }
    });
    file
}

/// The ribbon body for `tab`.
pub(crate) fn body(
    ui: &mut egui::Ui,
    tab: RibbonTab,
    state: &RibbonState<'_>,
    actions: &mut Vec<Action>,
) {
    let width = ui.available_width();
    ui.allocate_ui(vec2(width, BODY_HEIGHT), |ui| {
        ui.set_min_size(vec2(width, BODY_HEIGHT));
        egui::ScrollArea::horizontal()
            .id_salt("ribbon-scroll")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                    match tab {
                        RibbonTab::Home => home(ui, state, actions),
                        RibbonTab::Insert => insert(ui, state, actions),
                        RibbonTab::Layout => layout(ui, state, actions),
                        RibbonTab::Review => review(ui, state, actions),
                        RibbonTab::View => view(ui, state, actions),
                        RibbonTab::Recognition => recognition(ui, state, actions),
                    }
                });
            });
    });
}

/// A labelled group of commands with a separator after it.
fn group(ui: &mut egui::Ui, label: &str, palette: Palette, add: impl FnOnce(&mut egui::Ui)) {
    let response = ui
        .vertical(|ui| {
            let content = ui
                .horizontal_top(|ui| {
                    ui.set_height(GROUP_HEIGHT);
                    add(ui);
                })
                .response
                .rect;
            let galley = ui.painter().layout_no_wrap(
                label.to_owned(),
                FontId::proportional(11.0),
                palette.group_label,
            );
            let width = content.width().max(galley.size().x);
            let (rect, _) = ui.allocate_exact_size(vec2(width, 16.0), Sense::hover());
            ui.painter().galley(
                egui::pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 1.0),
                galley,
                palette.group_label,
            );
        })
        .response;
    let rect = response.rect;
    ui.add_space(4.0);
    let x = rect.right() + 3.0;
    ui.painter().line_segment(
        [
            egui::pos2(x, rect.top() + 4.0),
            egui::pos2(x, rect.bottom() - 2.0),
        ],
        Stroke::new(1.0, palette.ribbon_line),
    );
    ui.add_space(6.0);
}

/// A large icon over a label.
fn big(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    tip: &str,
    enabled: bool,
    palette: Palette,
) -> bool {
    let natural = ui
        .painter()
        .layout_no_wrap(label.to_owned(), FontId::proportional(12.0), Color32::WHITE)
        .size()
        .x;
    // One line when it fits in a reasonable width, two lines otherwise.
    let width = if natural + 10.0 <= 96.0 {
        (natural + 10.0).max(48.0)
    } else {
        (natural / 2.0 + 18.0).clamp(48.0, 96.0)
    };
    let (rect, response) = ui.allocate_exact_size(vec2(width, GROUP_HEIGHT), Sense::click());
    let response = response.on_hover_text(tip);
    paint_button_back(ui, rect, &response, false, enabled, palette);
    let icon_rect = Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + 21.0),
        vec2(30.0, 30.0),
    );
    let color = text_color(ui, enabled);
    icons::paint(ui.painter(), icon_rect, icon, color, accent(enabled));
    let galley = ui.painter().layout(
        label.to_owned(),
        FontId::proportional(12.0),
        color,
        width - 4.0,
    );
    let pos = egui::pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 40.0);
    ui.painter().galley(pos, galley, color);
    enabled && response.clicked()
}

/// A small icon beside a label.
fn small(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    tip: &str,
    enabled: bool,
    palette: Palette,
) -> bool {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::proportional(12.5),
        text_color(ui, enabled),
    );
    let width = 26.0 + galley.size().x + 6.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, 22.0), Sense::click());
    let response = response.on_hover_text(tip);
    paint_button_back(ui, rect, &response, false, enabled, palette);
    let icon_rect = Rect::from_min_size(rect.left_top() + vec2(4.0, 3.0), vec2(16.0, 16.0));
    icons::paint(
        ui.painter(),
        icon_rect,
        icon,
        text_color(ui, enabled),
        accent(enabled),
    );
    ui.painter().galley(
        egui::pos2(rect.left() + 24.0, rect.center().y - galley.size().y / 2.0),
        galley,
        text_color(ui, enabled),
    );
    enabled && response.clicked()
}

/// A square glyph toggle (B, I, U…).
#[allow(clippy::too_many_arguments)]
fn glyph(
    ui: &mut egui::Ui,
    text: &str,
    family: FontFamily,
    on: bool,
    tip: &str,
    enabled: bool,
    palette: Palette,
    decorate: impl FnOnce(&egui::Painter, Rect, Color32),
) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
    let response = response.on_hover_text(tip);
    paint_button_back(ui, rect, &response, on, enabled, palette);
    let color = text_color(ui, enabled);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        FontId::new(15.0, family),
        color,
    );
    decorate(ui.painter(), rect, color);
    enabled && response.clicked()
}

/// A narrow ▾ button that opens a menu.
fn dropdown(
    ui: &mut egui::Ui,
    tip: &str,
    enabled: bool,
    palette: Palette,
    add: impl FnOnce(&mut egui::Ui),
) {
    let (rect, response) = ui.allocate_exact_size(vec2(14.0, 24.0), Sense::click());
    let response = response.on_hover_text(tip);
    paint_button_back(ui, rect, &response, false, enabled, palette);
    let c = rect.center();
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            c + vec2(-3.5, -1.5),
            c + vec2(3.5, -1.5),
            c + vec2(0.0, 2.5),
        ],
        text_color(ui, enabled),
        Stroke::NONE,
    ));
    if enabled {
        egui::Popup::menu(&response).show(add);
    }
}

/// A square icon toggle (alignment).
fn icon_toggle(
    ui: &mut egui::Ui,
    icon: Icon,
    on: bool,
    tip: &str,
    enabled: bool,
    palette: Palette,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
    let response = response.on_hover_text(tip);
    paint_button_back(ui, rect, &response, on, enabled, palette);
    icons::paint(
        ui.painter(),
        rect.shrink(4.0),
        icon,
        text_color(ui, enabled),
        accent(enabled),
    );
    enabled && response.clicked()
}

fn paint_button_back(
    ui: &egui::Ui,
    rect: Rect,
    response: &egui::Response,
    on: bool,
    enabled: bool,
    palette: Palette,
) {
    let fill = if !enabled {
        None
    } else if response.is_pointer_button_down_on() || on {
        Some(palette.pressed)
    } else if response.hovered() {
        Some(palette.hover)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, 3.0, fill);
        if on {
            ui.painter().rect_stroke(
                rect,
                3.0,
                Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.6)),
                egui::StrokeKind::Inside,
            );
        }
    }
}

fn text_color(ui: &egui::Ui, enabled: bool) -> Color32 {
    if enabled {
        ui.visuals().text_color()
    } else {
        ui.visuals().weak_text_color()
    }
}

fn accent(enabled: bool) -> Color32 {
    if enabled {
        theme::ACCENT
    } else {
        Color32::from_gray(150)
    }
}

fn doc(actions: &mut Vec<Action>, request: Request) {
    actions.push(Action::Doc(request));
}

#[allow(clippy::too_many_lines)]
fn home(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    let t = |key| s.i18n.t(key);
    let on = s.document;
    let editable = on && s.state.read_only.is_none();
    let p = s.palette;
    group(ui, t("group.clipboard"), p, |ui| {
        if big(ui, Icon::Paste, t("cmd.paste"), t("tip.paste"), editable, p) {
            actions.push(Action::Paste);
        }
        ui.vertical(|ui| {
            if small(ui, Icon::Cut, t("cmd.cut"), t("tip.cut"), editable, p) {
                doc(actions, Request::Copy { cut: true });
            }
            if small(ui, Icon::Copy, t("cmd.copy"), t("tip.copy"), on, p) {
                doc(actions, Request::Copy { cut: false });
            }
            if small(
                ui,
                Icon::SelectAll,
                t("cmd.select_all"),
                t("tip.select_all"),
                on,
                p,
            ) {
                doc(actions, Request::SelectAll);
            }
        });
    });
    group(ui, t("group.font"), p, |ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.add_enabled_ui(editable, |ui| {
                    let current = s.format.font.clone().unwrap_or_default();
                    egui::ComboBox::from_id_salt("font-family")
                        .width(132.0)
                        .selected_text(if current.is_empty() {
                            t("font.default").to_owned()
                        } else {
                            current.clone()
                        })
                        .show_ui(ui, |ui| {
                            for family in FONT_FAMILIES {
                                if ui.selectable_label(current == *family, *family).clicked() {
                                    doc(
                                        actions,
                                        Request::Format(FormatChange::Font((*family).to_owned())),
                                    );
                                }
                            }
                        })
                        .response
                        .on_hover_text(t("tip.font"));
                    let size = s.format.size;
                    egui::ComboBox::from_id_salt("font-size")
                        .width(52.0)
                        .selected_text(size.map_or_else(String::new, format_size))
                        .show_ui(ui, |ui| {
                            for candidate in FONT_SIZES {
                                let selected =
                                    size.is_some_and(|size| (size - candidate).abs() < 0.01);
                                if ui
                                    .selectable_label(selected, format_size(*candidate))
                                    .clicked()
                                {
                                    doc(actions, Request::Format(FormatChange::Size(*candidate)));
                                }
                            }
                        })
                        .response
                        .on_hover_text(t("tip.size"));
                });
                if glyph(
                    ui,
                    "A",
                    FontFamily::Proportional,
                    false,
                    t("tip.grow"),
                    editable,
                    p,
                    |painter, rect, color| {
                        triangle(painter, rect, color, true);
                    },
                ) {
                    doc(actions, Request::Format(FormatChange::Grow));
                }
                if glyph(
                    ui,
                    "A",
                    FontFamily::Proportional,
                    false,
                    t("tip.shrink"),
                    editable,
                    p,
                    |painter, rect, color| {
                        triangle(painter, rect, color, false);
                    },
                ) {
                    doc(actions, Request::Format(FormatChange::Shrink));
                }
                if glyph(
                    ui,
                    "",
                    FontFamily::Proportional,
                    false,
                    t("tip.clear_format"),
                    editable,
                    p,
                    |painter, rect, color| {
                        icons::paint(painter, rect.shrink(3.0), Icon::Clear, color, theme::ACCENT);
                    },
                ) {
                    doc(actions, Request::Format(FormatChange::Clear));
                }
            });
            ui.horizontal(|ui| {
                let bold = FontFamily::Name(theme::BOLD.into());
                let italic = FontFamily::Name(theme::ITALIC.into());
                if glyph(
                    ui,
                    "B",
                    bold,
                    s.format.bold,
                    t("tip.bold"),
                    editable,
                    p,
                    |_, _, _| {},
                ) {
                    doc(actions, Request::Format(FormatChange::Bold));
                }
                if glyph(
                    ui,
                    "I",
                    italic,
                    s.format.italic,
                    t("tip.italic"),
                    editable,
                    p,
                    |_, _, _| {},
                ) {
                    doc(actions, Request::Format(FormatChange::Italic));
                }
                if glyph(
                    ui,
                    "U",
                    FontFamily::Proportional,
                    s.format.underline,
                    t("tip.underline"),
                    editable,
                    p,
                    |painter, rect, color| {
                        painter.line_segment(
                            [
                                rect.left_bottom() + vec2(7.0, -5.0),
                                rect.right_bottom() + vec2(-7.0, -5.0),
                            ],
                            Stroke::new(1.2, color),
                        );
                    },
                ) {
                    doc(actions, Request::Format(FormatChange::Underline));
                }
                if glyph(
                    ui,
                    "S",
                    FontFamily::Proportional,
                    s.format.strike,
                    t("tip.strike"),
                    editable,
                    p,
                    |painter, rect, color| {
                        painter.line_segment(
                            [
                                rect.left_center() + vec2(6.0, 0.5),
                                rect.right_center() + vec2(-6.0, 0.5),
                            ],
                            Stroke::new(1.2, color),
                        );
                    },
                ) {
                    doc(actions, Request::Format(FormatChange::Strike));
                }
                if glyph(
                    ui,
                    "x²",
                    FontFamily::Proportional,
                    s.format.superscript,
                    t("tip.superscript"),
                    editable,
                    p,
                    |_, _, _| {},
                ) {
                    doc(actions, Request::Format(FormatChange::Superscript));
                }
                if glyph(
                    ui,
                    "x₂",
                    FontFamily::Proportional,
                    s.format.subscript,
                    t("tip.subscript"),
                    editable,
                    p,
                    |_, _, _| {},
                ) {
                    doc(actions, Request::Format(FormatChange::Subscript));
                }
                ui.add_space(4.0);
                let [r, g, b] = s.font_color;
                let chosen = Color32::from_rgb(r, g, b);
                if glyph(
                    ui,
                    "A",
                    FontFamily::Proportional,
                    false,
                    t("tip.font_color"),
                    editable,
                    p,
                    |painter, rect, _| {
                        painter.rect_filled(
                            Rect::from_min_size(
                                rect.left_bottom() + vec2(5.0, -6.0),
                                vec2(14.0, 3.5),
                            ),
                            0.0,
                            chosen,
                        );
                    },
                ) {
                    doc(
                        actions,
                        Request::Format(FormatChange::Color(Some(s.font_color))),
                    );
                }
                dropdown(ui, t("tip.font_color_more"), editable, p, |ui| {
                    color_menu(ui, s, actions);
                });
                if glyph(
                    ui,
                    "ab",
                    FontFamily::Proportional,
                    false,
                    t("tip.highlight"),
                    editable,
                    p,
                    |painter, rect, _| {
                        painter.rect_filled(
                            Rect::from_min_size(
                                rect.left_bottom() + vec2(4.0, -6.0),
                                vec2(16.0, 3.5),
                            ),
                            0.0,
                            Color32::from_rgb(255, 230, 0),
                        );
                    },
                ) {
                    doc(
                        actions,
                        Request::Format(FormatChange::Highlight(Some(
                            strict_ooxml::model::values::Highlight::Yellow,
                        ))),
                    );
                }
                dropdown(ui, t("tip.highlight"), editable, p, |ui| {
                    highlight_menu(ui, s, actions);
                });
            });
        });
    });
    group(ui, t("group.paragraph"), p, |ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                for (icon, align, tip) in [
                    (Icon::AlignLeft, Align::Left, "tip.align_left"),
                    (Icon::AlignCenter, Align::Center, "tip.align_center"),
                    (Icon::AlignRight, Align::Right, "tip.align_right"),
                    (Icon::Justify, Align::Justify, "tip.justify"),
                ] {
                    let current = s.format.align.unwrap_or(Align::Left) == align && on;
                    if icon_toggle(ui, icon, current, t(tip), editable, p) {
                        doc(actions, Request::Align(align));
                    }
                }
            });
            ui.horizontal(|ui| {
                if small(
                    ui,
                    Icon::PageBreak,
                    t("cmd.page_break"),
                    t("tip.page_break"),
                    editable,
                    p,
                ) {
                    doc(actions, Request::Insert(Insert::PageBreak));
                }
            });
        });
    });
    if !s.styles.is_empty() {
        group(ui, t("group.styles"), p, |ui| {
            style_gallery(ui, s, actions);
        });
    }
    group(ui, t("group.editing"), p, |ui| {
        ui.vertical(|ui| {
            if small(ui, Icon::Find, t("cmd.find"), t("tip.find"), on, p) {
                actions.push(Action::Open(Dialog::Find { replace: false }));
            }
            if small(
                ui,
                Icon::Replace,
                t("cmd.replace"),
                t("tip.replace"),
                editable,
                p,
            ) {
                actions.push(Action::Open(Dialog::Find { replace: true }));
            }
            if small(
                ui,
                Icon::SelectAll,
                t("cmd.select_all"),
                t("tip.select_all"),
                on,
                p,
            ) {
                doc(actions, Request::SelectAll);
            }
        });
    });
}

fn format_size(size: f32) -> String {
    if (size - size.round()).abs() < 0.01 {
        format!("{size:.0}")
    } else {
        format!("{size:.1}")
    }
}

fn triangle(painter: &egui::Painter, rect: Rect, color: Color32, up: bool) {
    let c = rect.right_top() + vec2(-5.0, 7.0);
    let points = if up {
        vec![c + vec2(-3.0, 2.0), c + vec2(3.0, 2.0), c + vec2(0.0, -2.0)]
    } else {
        vec![
            c + vec2(-3.0, -2.0),
            c + vec2(3.0, -2.0),
            c + vec2(0.0, 2.0),
        ]
    };
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

const COLORS: &[[u8; 3]] = &[
    [0, 0, 0],
    [68, 84, 106],
    [31, 78, 121],
    [43, 87, 154],
    [0, 112, 192],
    [0, 176, 240],
    [0, 176, 80],
    [146, 208, 80],
    [255, 192, 0],
    [255, 102, 0],
    [192, 0, 0],
    [255, 0, 0],
    [112, 48, 160],
    [128, 128, 128],
    [191, 191, 191],
    [255, 255, 255],
];

fn color_menu(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    ui.label(s.i18n.t("menu.font_color"));
    egui::Grid::new("font-colors")
        .spacing(vec2(3.0, 3.0))
        .show(ui, |ui| {
            for (index, rgb) in COLORS.iter().enumerate() {
                let [r, g, b] = *rgb;
                let (rect, response) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                ui.painter()
                    .rect_filled(rect, 2.0, Color32::from_rgb(r, g, b));
                ui.painter().rect_stroke(
                    rect,
                    2.0,
                    Stroke::new(1.0, Color32::from_gray(160)),
                    egui::StrokeKind::Inside,
                );
                if response.clicked() {
                    actions.push(Action::FontColor(*rgb));
                    doc(actions, Request::Format(FormatChange::Color(Some(*rgb))));
                    ui.close();
                }
                if index % 8 == 7 {
                    ui.end_row();
                }
            }
        });
    if ui.button(s.i18n.t("menu.automatic")).clicked() {
        doc(actions, Request::Format(FormatChange::Color(None)));
        ui.close();
    }
}

fn highlight_menu(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    use strict_ooxml::model::values::Highlight;
    ui.label(s.i18n.t("menu.highlight"));
    let choices = [
        (Highlight::Yellow, [255, 255, 0]),
        (Highlight::Green, [0, 255, 0]),
        (Highlight::Cyan, [0, 255, 255]),
        (Highlight::Magenta, [255, 0, 255]),
        (Highlight::Red, [255, 0, 0]),
        (Highlight::Blue, [0, 0, 255]),
        (Highlight::LightGray, [192, 192, 192]),
        (Highlight::DarkGray, [128, 128, 128]),
    ];
    ui.horizontal(|ui| {
        for (highlight, [r, g, b]) in choices {
            let (rect, response) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
            ui.painter()
                .rect_filled(rect, 2.0, Color32::from_rgb(r, g, b));
            if response.clicked() {
                doc(
                    actions,
                    Request::Format(FormatChange::Highlight(Some(highlight))),
                );
                ui.close();
            }
        }
    });
    if ui.button(s.i18n.t("menu.no_highlight")).clicked() {
        doc(actions, Request::Format(FormatChange::Highlight(None)));
        ui.close();
    }
}

/// Word's style gallery: a few quick styles as previews.
fn style_gallery(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    let editable = s.document && s.state.read_only.is_none();
    let p = s.palette;
    ui.horizontal_top(|ui| {
        for entry in s.styles.iter().take(6) {
            let name = localized_style(s.i18n, &entry.name);
            let lower = entry.name.to_lowercase();
            let (size, color, bold) = if lower == "title" {
                (17.0, ui.visuals().text_color(), false)
            } else if lower.starts_with("heading") {
                (
                    15.0 - lower
                        .chars()
                        .last()
                        .and_then(|c| c.to_digit(10))
                        .unwrap_or(1) as f32,
                    theme::ACCENT,
                    true,
                )
            } else {
                (12.0, ui.visuals().text_color(), false)
            };
            let selected = s.format.style.as_deref() == Some(entry.id.as_str())
                || (s.format.style.is_none() && lower == "normal");
            let (rect, response) =
                ui.allocate_exact_size(vec2(70.0, GROUP_HEIGHT - 6.0), Sense::click());
            let response = response.on_hover_text(&entry.name);
            paint_button_back(ui, rect, &response, selected && s.document, editable, p);
            ui.painter().rect_stroke(
                rect,
                3.0,
                Stroke::new(1.0, p.ribbon_line),
                egui::StrokeKind::Inside,
            );
            let family = if bold {
                FontFamily::Name(theme::BOLD.into())
            } else {
                FontFamily::Proportional
            };
            ui.painter().text(
                rect.center() - vec2(0.0, 8.0),
                egui::Align2::CENTER_CENTER,
                "АаBb",
                FontId::new(size.max(10.0), family),
                color,
            );
            let galley = ui.painter().layout(
                name,
                FontId::proportional(10.5),
                p.group_label,
                rect.width() - 4.0,
            );
            ui.painter().galley(
                egui::pos2(
                    rect.center().x - galley.size().x / 2.0,
                    rect.bottom() - galley.size().y - 3.0,
                ),
                galley,
                p.group_label,
            );
            if editable && response.clicked() {
                doc(actions, Request::Style(entry.id.clone()));
            }
        }
        if s.styles.len() > 6 {
            dropdown(ui, s.i18n.t("tip.more_styles"), editable, p, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for entry in s.styles {
                            let selected = s.format.style.as_deref() == Some(entry.id.as_str());
                            if ui
                                .selectable_label(selected, localized_style(s.i18n, &entry.name))
                                .clicked()
                            {
                                doc(actions, Request::Style(entry.id.clone()));
                                ui.close();
                            }
                        }
                    });
            });
        }
    });
}

/// Built-in style names shown in the interface language.
pub(crate) fn localized_style(i18n: &I18n, name: &str) -> String {
    let key = match name.to_lowercase().as_str() {
        "normal" => "style.normal",
        "title" => "style.title",
        "subtitle" => "style.subtitle",
        "quote" => "style.quote",
        "heading 1" => "style.heading1",
        "heading 2" => "style.heading2",
        "heading 3" => "style.heading3",
        "heading 4" => "style.heading4",
        _ => return name.to_owned(),
    };
    i18n.t(key).to_owned()
}

fn insert(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    let t = |key| s.i18n.t(key);
    let editable = s.document && s.state.read_only.is_none();
    let p = s.palette;
    group(ui, t("group.pages"), p, |ui| {
        if big(
            ui,
            Icon::PageBreak,
            t("cmd.page_break"),
            t("tip.page_break"),
            editable,
            p,
        ) {
            doc(actions, Request::Insert(Insert::PageBreak));
        }
        if big(
            ui,
            Icon::SectionBreak,
            t("cmd.section_break"),
            t("tip.section_break"),
            editable,
            p,
        ) {
            doc(actions, Request::Page(PageChange::SectionBreak));
        }
    });
    group(ui, t("group.tables"), p, |ui| {
        if big(ui, Icon::Table, t("cmd.table"), t("tip.table"), editable, p) {
            actions.push(Action::Open(Dialog::Table {
                rows: 3,
                columns: 3,
            }));
        }
    });
    group(ui, t("group.text"), p, |ui| {
        if big(
            ui,
            Icon::InsertFile,
            t("cmd.insert_file"),
            t("tip.insert_file"),
            editable,
            p,
        ) {
            actions.push(Action::InsertDocument);
        }
        if big(ui, Icon::Date, t("cmd.date"), t("tip.date"), editable, p) {
            actions.push(Action::InsertDate);
        }
    });
    group(ui, t("group.symbols"), p, |ui| {
        if big(
            ui,
            Icon::Symbol,
            t("cmd.symbol"),
            t("tip.symbol"),
            editable,
            p,
        ) {
            actions.push(Action::Open(Dialog::Symbols));
        }
    });
}

fn layout(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    let t = |key| s.i18n.t(key);
    let editable = s.document && s.state.read_only.is_none();
    let p = s.palette;
    group(ui, t("group.page_setup"), p, |ui| {
        ui.vertical(|ui| {
            if small(
                ui,
                Icon::Portrait,
                t("cmd.portrait"),
                t("tip.portrait"),
                editable,
                p,
            ) {
                doc(
                    actions,
                    Request::Page(PageChange::Orientation { landscape: false }),
                );
            }
            if small(
                ui,
                Icon::Landscape,
                t("cmd.landscape"),
                t("tip.landscape"),
                editable,
                p,
            ) {
                doc(
                    actions,
                    Request::Page(PageChange::Orientation { landscape: true }),
                );
            }
        });
        ui.add_enabled_ui(editable, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    icons::icon_widget(ui, Icon::PageSize, 18.0, accent(editable));
                    page_size_menu(ui, s, actions);
                });
                ui.horizontal(|ui| {
                    icons::icon_widget(ui, Icon::Margins, 18.0, accent(editable));
                    margins_menu(ui, s, actions);
                });
            });
        });
    });
    group(ui, t("group.breaks"), p, |ui| {
        if big(
            ui,
            Icon::PageBreak,
            t("cmd.page_break"),
            t("tip.page_break"),
            editable,
            p,
        ) {
            doc(actions, Request::Insert(Insert::PageBreak));
        }
        if big(
            ui,
            Icon::SectionBreak,
            t("cmd.section_break"),
            t("tip.section_break"),
            editable,
            p,
        ) {
            doc(actions, Request::Page(PageChange::SectionBreak));
        }
    });
    table_group(ui, s, actions);
}

fn table_group(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    let t = |key| s.i18n.t(key);
    let on = s.document;
    let editable = on && s.state.read_only.is_none();
    let p = s.palette;
    group(ui, t("group.table"), p, |ui| {
        ui.vertical(|ui| {
            for (key, tip, command) in [
                ("cmd.select_row", "tip.select_row", TableCommand::SelectRow),
                (
                    "cmd.select_column",
                    "tip.select_column",
                    TableCommand::SelectColumn,
                ),
                (
                    "cmd.select_table",
                    "tip.select_table",
                    TableCommand::SelectTable,
                ),
            ] {
                if small(ui, Icon::SelectAll, t(key), t(tip), on, p) {
                    doc(actions, Request::Table(command));
                }
            }
        });
        ui.vertical(|ui| {
            for (icon, key, tip, command) in [
                (
                    Icon::Table,
                    "cmd.row_above",
                    "tip.row_above",
                    TableCommand::RowAbove,
                ),
                (
                    Icon::Table,
                    "cmd.row_below",
                    "tip.row_below",
                    TableCommand::RowBelow,
                ),
                (
                    Icon::Close,
                    "cmd.delete_rows",
                    "tip.delete_rows",
                    TableCommand::DeleteRows,
                ),
            ] {
                if small(ui, icon, t(key), t(tip), editable, p) {
                    doc(actions, Request::Table(command));
                }
            }
        });
    });
}

fn page_size_menu(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    let t = |key| s.i18n.t(key);
    ui.menu_button(t("cmd.size"), |ui| {
        for (label, width, height) in [
            ("A4 (21 × 29,7 cm)", 11906, 16838),
            ("A5 (14,8 × 21 cm)", 8391, 11906),
            ("A3 (29,7 × 42 cm)", 16838, 23811),
            ("Letter (8,5 × 11 in)", 12240, 15840),
            ("Legal (8,5 × 14 in)", 12240, 20160),
        ] {
            if ui.button(label).clicked() {
                doc(actions, Request::Page(PageChange::Size { width, height }));
                ui.close();
            }
        }
    })
    .response
    .on_hover_text(t("tip.size_page"));
}

fn margins_menu(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    let t = |key| s.i18n.t(key);
    ui.menu_button(t("cmd.margins"), |ui| {
        for (key, margins) in [
            ("margins.normal", [1134, 850, 1134, 1701]),
            ("margins.narrow", [720, 720, 720, 720]),
            ("margins.moderate", [1440, 1080, 1440, 1080]),
            ("margins.wide", [1440, 2880, 1440, 2880]),
        ] {
            if ui.button(t(key)).clicked() {
                doc(actions, Request::Page(PageChange::Margins(margins)));
                ui.close();
            }
        }
    })
    .response
    .on_hover_text(t("tip.margins"));
}

fn review(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    let t = |key| s.i18n.t(key);
    let editable = s.document && s.state.read_only.is_none();
    let p = s.palette;
    group(ui, t("group.proofing"), p, |ui| {
        if big(
            ui,
            Icon::Words,
            t("cmd.word_count"),
            t("tip.word_count"),
            s.document,
            p,
        ) {
            actions.push(Action::Open(Dialog::WordCount));
        }
    });
    group(ui, t("group.changes"), p, |ui| {
        if big(
            ui,
            Icon::Accept,
            t("cmd.accept_all"),
            t("tip.accept_all"),
            editable,
            p,
        ) {
            doc(actions, Request::Revisions { accept: true });
        }
        if big(
            ui,
            Icon::Reject,
            t("cmd.reject_all"),
            t("tip.reject_all"),
            editable,
            p,
        ) {
            doc(actions, Request::Revisions { accept: false });
        }
    });
    group(ui, t("group.fidelity"), p, |ui| {
        if big(
            ui,
            Icon::Report,
            t("cmd.report"),
            t("tip.report"),
            s.document,
            p,
        ) {
            actions.push(Action::ToggleReport);
        }
    });
}

fn view(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    let t = |key| s.i18n.t(key);
    let p = s.palette;
    group(ui, t("group.show"), p, |ui| {
        ui.vertical(|ui| {
            let mut navigation = s.navigation;
            if ui.checkbox(&mut navigation, t("cmd.navigation")).changed() {
                actions.push(Action::ToggleNavigation);
            }
            let mut report = s.report;
            if ui.checkbox(&mut report, t("cmd.report_pane")).changed() {
                actions.push(Action::ToggleReport);
            }
            let mut system = s.system_fonts;
            if ui
                .checkbox(&mut system, t("cmd.system_fonts"))
                .on_hover_text(t("tip.system_fonts"))
                .changed()
            {
                actions.push(Action::ToggleSystemFonts);
            }
        });
    });
    group(ui, t("group.zoom"), p, |ui| {
        if big(
            ui,
            Icon::ZoomIn,
            t("cmd.zoom_in"),
            t("tip.zoom_in"),
            s.document,
            p,
        ) {
            actions.push(Action::Zoom(Zoom::In));
        }
        if big(
            ui,
            Icon::ZoomOut,
            t("cmd.zoom_out"),
            t("tip.zoom_out"),
            s.document,
            p,
        ) {
            actions.push(Action::Zoom(Zoom::Out));
        }
        ui.vertical(|ui| {
            if small(ui, Icon::OnePage, "100%", t("tip.zoom_100"), s.document, p) {
                actions.push(Action::Zoom(Zoom::Set(1.0)));
            }
            if small(
                ui,
                Icon::OnePage,
                t("cmd.one_page"),
                t("tip.one_page"),
                s.document,
                p,
            ) {
                actions.push(Action::Zoom(Zoom::OnePage));
            }
            if small(
                ui,
                Icon::PageWidth,
                t("cmd.page_width"),
                t("tip.page_width"),
                s.document,
                p,
            ) {
                actions.push(Action::Zoom(Zoom::PageWidth));
            }
        });
        let _ = s.zoom;
    });
    group(ui, t("group.interface"), p, |ui| {
        if big(
            ui,
            Icon::Theme,
            if s.dark {
                t("cmd.light")
            } else {
                t("cmd.dark")
            },
            t("tip.theme"),
            true,
            p,
        ) {
            actions.push(Action::ToggleDark);
        }
        ui.vertical(|ui| {
            icons::icon_widget(ui, Icon::Language, 18.0, theme::ACCENT);
            let current = s.i18n.language().to_owned();
            let name = s
                .i18n
                .languages()
                .find(|(code, _)| *code == current)
                .map_or(current.clone(), |(_, name)| name.to_owned());
            egui::ComboBox::from_id_salt("language")
                .selected_text(name)
                .show_ui(ui, |ui| {
                    for (code, name) in s.i18n.languages() {
                        if ui.selectable_label(code == current, name).clicked() {
                            actions.push(Action::Language(code.to_owned()));
                        }
                    }
                });
        });
    });
}

fn recognition(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    let t = |key| s.i18n.t(key);
    let p = s.palette;
    group(ui, t("group.import"), p, |ui| {
        if big(
            ui,
            Icon::Import,
            t("cmd.import_pdf"),
            t("tip.import_pdf"),
            true,
            p,
        ) {
            actions.push(Action::ImportDialog { ocr: false });
        }
        if big(
            ui,
            Icon::Scan,
            t("cmd.recognize"),
            t("tip.recognize"),
            cfg!(feature = "ocr"),
            p,
        ) {
            actions.push(Action::ImportDialog { ocr: true });
        }
    });
    group(ui, t("group.engine"), p, |ui| {
        if big(
            ui,
            Icon::Settings,
            t("cmd.ocr_settings"),
            t("tip.ocr_settings"),
            true,
            p,
        ) {
            actions.push(Action::Open(Dialog::Settings));
        }
    });
    group(ui, t("group.export"), p, |ui| {
        if big(
            ui,
            Icon::Pdf,
            t("cmd.export_pdf"),
            t("tip.export_pdf"),
            s.document,
            p,
        ) {
            actions.push(Action::Export(crate::worker::SaveFormat::Pdf));
        }
        if big(
            ui,
            Icon::Svg,
            t("cmd.export_svg"),
            t("tip.export_svg"),
            s.document,
            p,
        ) {
            actions.push(Action::Export(crate::worker::SaveFormat::Svg));
        }
        if big(
            ui,
            Icon::Txt,
            t("cmd.export_txt"),
            t("tip.export_txt"),
            s.document,
            p,
        ) {
            actions.push(Action::Export(crate::worker::SaveFormat::Text));
        }
    });
}
