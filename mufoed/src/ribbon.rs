//! The ribbon: tabs of grouped commands, as in Word and WPS.
//!
//! Every command is drawn by one small kit so the tabs line up: a group is a
//! fixed-height band of three rows (or one tall button) over its label; a big
//! button is an icon over a label of at most two lines, broken only between
//! words; small buttons in a column share one width; menus and split buttons
//! look like the buttons they sit beside; every button has an outline.

use eframe::egui::{self, pos2, vec2, Color32, FontFamily, FontId, Rect, RichText, Sense, Stroke};
use strict_ooxml::edit::ListPreset;

use crate::app::{Action, Dialog, Zoom};
use crate::i18n::I18n;
use crate::icons::{self, Icon};
use crate::theme::{self, Palette};
use crate::worker::CommentCommand;
use crate::worker::{
    Align, CaretFormat, EditState, ExportPurpose, FormatChange, HeaderFooterCommand, Insert,
    ObjectCommand, ObjectView, PageChange, PictureEdit, ReferenceCommand, Request, StyleEntry,
    TableCommand, WrapStyle, FONT_SIZES,
};

/// Ribbon tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RibbonTab {
    Home,
    Insert,
    Layout,
    References,
    Review,
    View,
    Recognition,
    /// A local language model's help with the text.
    Ai,
    /// The picture tools, shown while a drawing is selected.
    Picture,
    /// The header and footer tools, shown while one is edited.
    HeaderFooter,
}

impl RibbonTab {
    pub(crate) const ALL: [Self; 8] = [
        Self::Home,
        Self::Insert,
        Self::Layout,
        Self::References,
        Self::Review,
        Self::Ai,
        Self::View,
        Self::Recognition,
    ];

    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Home => "ribbon.home",
            Self::Insert => "ribbon.insert",
            Self::Layout => "ribbon.layout",
            Self::References => "ribbon.references",
            Self::Review => "ribbon.review",
            Self::View => "ribbon.view",
            Self::Recognition => "ribbon.recognition",
            Self::Ai => "ribbon.ai",
            Self::Picture => "ribbon.picture",
            Self::HeaderFooter => "ribbon.header_footer",
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
    pub(crate) ruler: bool,
    pub(crate) report: bool,
    pub(crate) dark: bool,
    pub(crate) zoom: f32,
    pub(crate) system_fonts: bool,
    /// Last colours picked, for the split buttons.
    pub(crate) font_color: [u8; 3],
    /// The selected drawing.
    pub(crate) object: Option<&'a ObjectView>,
    /// Its handles crop instead of resizing.
    pub(crate) crop: bool,
    /// The comments pane is shown.
    pub(crate) comments_open: bool,
    /// Misspelt words are marked.
    pub(crate) spelling: bool,
    /// The language the AI tab translates into.
    pub(crate) ai_language: &'a str,
    /// The AI pane is shown.
    pub(crate) ai_open: bool,
    /// The header or footer being edited.
    pub(crate) header_footer: Option<&'a crate::worker::HeaderFooterView>,
    /// Faces to draw font names in.
    pub(crate) fonts: &'a std::cell::RefCell<crate::docfonts::DocumentFonts>,
    /// The fonts the document names.
    pub(crate) document_fonts: &'a [String],
}

impl RibbonState<'_> {
    fn t<'k>(&'k self, key: &'k str) -> &'k str {
        self.i18n.t(key)
    }

    fn editable(&self) -> bool {
        self.document && self.state.read_only.is_none()
    }
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

/// The font names to choose from: the document's first, then every
/// installed family and the common Office ones (drawn with their bundled
/// stand-ins where not installed), each in its own face.
fn font_list(ui: &mut egui::Ui, s: &RibbonState<'_>, current: &str, actions: &mut Vec<Action>) {
    let mut all: Vec<String> = FONT_FAMILIES.iter().map(|f| (*f).to_owned()).collect();
    all.extend(crate::raster::installed_families().iter().cloned());
    all.sort_by_key(|name| name.to_lowercase());
    all.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    let sections = [
        (s.t("font.document_fonts"), s.document_fonts.to_vec()),
        (s.t("font.all_fonts"), all),
    ];
    for (title, names) in sections {
        if names.is_empty() {
            continue;
        }
        ui.label(RichText::new(title).size(11.5).weak());
        for name in names {
            let (rect, response) = ui.allocate_exact_size(vec2(260.0, 24.0), Sense::click());
            if !ui.is_rect_visible(rect) {
                continue;
            }
            let selected = name.eq_ignore_ascii_case(current);
            if selected {
                ui.painter().rect_filled(rect, 3.0, s.palette.pressed);
            } else if response.hovered() {
                ui.painter().rect_filled(rect, 3.0, s.palette.hover);
            }
            // The name in its own face, once that face is loaded.
            let family = s
                .fonts
                .borrow_mut()
                .family_named(ui.ctx(), &name)
                .unwrap_or(FontFamily::Proportional);
            ui.painter().text(
                rect.left_center() + vec2(8.0, 0.0),
                egui::Align2::LEFT_CENTER,
                &name,
                FontId::new(15.0, family),
                ui.visuals().text_color(),
            );
            if response.clicked() {
                doc(actions, Request::Format(FormatChange::Font(name.clone())));
                ui.close();
            }
        }
        ui.add_space(4.0);
    }
}

/// Underline styles, each drawn as it looks.
fn underline_menu(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    use strict_ooxml::model::values::Underline;
    let styles = [
        (Underline::Single, "underline.single"),
        (Underline::Double, "underline.double"),
        (Underline::Thick, "underline.thick"),
        (Underline::Dotted, "underline.dotted"),
        (Underline::Dash, "underline.dash"),
        (Underline::DotDash, "underline.dot_dash"),
        (Underline::Wave, "underline.wave"),
        (Underline::Words, "underline.words"),
    ];
    for (style, key) in styles {
        let (rect, response) = ui.allocate_exact_size(vec2(190.0, 26.0), Sense::click());
        if s.format.underline_style == Some(style) {
            ui.painter().rect_filled(rect, 3.0, s.palette.pressed);
        } else if response.hovered() {
            ui.painter().rect_filled(rect, 3.0, s.palette.hover);
        }
        let color = ui.visuals().text_color();
        ui.painter().text(
            rect.left_center() + vec2(10.0, -2.0),
            egui::Align2::LEFT_CENTER,
            s.t(key),
            FontId::proportional(14.0),
            color,
        );
        let y = rect.bottom() - 5.0;
        let (x0, x1) = (rect.left() + 120.0, rect.right() - 10.0);
        let thin = Stroke::new(1.0, color);
        match style {
            Underline::Double => {
                ui.painter()
                    .line_segment([pos2(x0, y - 2.0), pos2(x1, y - 2.0)], thin);
                ui.painter().line_segment([pos2(x0, y), pos2(x1, y)], thin);
            }
            Underline::Thick => {
                ui.painter()
                    .line_segment([pos2(x0, y), pos2(x1, y)], Stroke::new(2.5, color));
            }
            Underline::Dotted | Underline::Dash | Underline::DotDash => {
                let pattern: &[f32] = match style {
                    Underline::Dotted => &[1.5, 2.0],
                    Underline::Dash => &[5.0, 3.0],
                    _ => &[5.0, 2.0, 1.5, 2.0],
                };
                let mut x = x0;
                let mut index = 0;
                while x < x1 {
                    let length = pattern.get(index % pattern.len()).copied().unwrap_or(2.0);
                    if index % 2 == 0 {
                        ui.painter()
                            .line_segment([pos2(x, y), pos2((x + length).min(x1), y)], thin);
                    }
                    x += length;
                    index += 1;
                }
            }
            Underline::Wave => {
                let points: Vec<egui::Pos2> = (0..=40)
                    .map(|i| {
                        let t = f32::from(u8::try_from(i).unwrap_or(0)) / 40.0;
                        pos2(x0 + (x1 - x0) * t, y - 1.5 * (t * 24.0).sin())
                    })
                    .collect();
                ui.painter().add(egui::Shape::line(points, thin));
            }
            _ => {
                ui.painter().line_segment([pos2(x0, y), pos2(x1, y)], thin);
            }
        }
        if response.clicked() {
            doc(
                actions,
                Request::Format(FormatChange::UnderlineStyle(Some(style))),
            );
            ui.close();
        }
    }
    ui.separator();
    if ui.button(s.t("underline.none")).clicked() {
        doc(actions, Request::Format(FormatChange::UnderlineStyle(None)));
        ui.close();
    }
}

/// One row of a group.
const ROW: f32 = 22.0;
/// Space between rows and between buttons.
const GAP: f32 = 3.0;
/// A group's command area: three rows.
const GROUP_HEIGHT: f32 = ROW * 3.0 + GAP * 2.0;
/// The group label under it.
const LABEL_HEIGHT: f32 = 16.0;
/// A style gallery tile.
const TILE: f32 = 74.0;
/// Ribbon body height.
const BODY_HEIGHT: f32 = GROUP_HEIGHT + LABEL_HEIGHT + 8.0;
/// A big button's label wraps past this width.
const BIG_LABEL_WIDTH: f32 = 86.0;
/// Corner radius of buttons.
const RADIUS: u8 = 3;

fn label_font() -> FontId {
    FontId::proportional(12.0)
}

fn text_width(ui: &egui::Ui, text: &str, font: &FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
        .size()
        .x
}

// ----- tab strip ------------------------------------------------------------

/// The ribbon tab strip; returns whether the File button was clicked.
pub(crate) fn tab_strip(
    ui: &mut egui::Ui,
    tab: &mut RibbonTab,
    contextual: Option<RibbonTab>,
    i18n: &I18n,
    palette: Palette,
) -> bool {
    let mut file = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        let file_button = egui::Button::new(
            egui::RichText::new(i18n.t("ribbon.file"))
                .color(Color32::WHITE)
                .size(13.5),
        )
        .fill(theme::ACCENT)
        .corner_radius(RADIUS)
        .min_size(vec2(70.0, 26.0));
        if ui.add(file_button).clicked() {
            file = true;
        }
        ui.add_space(6.0);
        for candidate in RibbonTab::ALL.into_iter().chain(contextual) {
            let selected = *tab == candidate;
            let tool = Some(candidate) == contextual;
            let font = if selected {
                FontId::new(13.5, FontFamily::Name(theme::BOLD.into()))
            } else {
                FontId::proportional(13.5)
            };
            let text = i18n.t(candidate.key());
            // Width of the bold face, so a tab does not change size when chosen.
            let width = text_width(
                ui,
                text,
                &FontId::new(13.5, FontFamily::Name(theme::BOLD.into())),
            ) + 24.0;
            let (rect, response) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click());
            let radius = egui::CornerRadius {
                nw: 4,
                ne: 4,
                sw: 0,
                se: 0,
            };
            let fill = if selected {
                palette.ribbon
            } else if tool {
                // A contextual tab is tinted, as Word tints its tools.
                theme::ACCENT.gamma_multiply(0.18)
            } else if response.hovered() {
                palette.hover
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, radius, fill);
            let border = if selected {
                palette.ribbon_line
            } else {
                palette.tab_border
            };
            ui.painter().rect_stroke(
                rect,
                radius,
                Stroke::new(1.0, border),
                egui::StrokeKind::Inside,
            );
            if selected {
                ui.painter().line_segment(
                    [
                        rect.left_bottom() + vec2(6.0, -1.5),
                        rect.right_bottom() + vec2(-6.0, -1.5),
                    ],
                    Stroke::new(2.5, theme::ACCENT),
                );
            }
            let color = if selected {
                theme::ACCENT
            } else {
                palette.tab_text
            };
            ui.painter().text(
                rect.center() - vec2(0.0, 1.0),
                egui::Align2::CENTER_CENTER,
                text,
                font,
                color,
            );
            if response.clicked() {
                *tab = candidate;
            }
        }
    });
    file
}

// ----- body and groups ------------------------------------------------------

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
        style_widgets(ui, state.palette);
        egui::ScrollArea::horizontal()
            .id_salt("ribbon-scroll")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing = vec2(GAP, GAP);
                    ui.add_space(2.0);
                    match tab {
                        RibbonTab::Home => home(ui, state, actions),
                        RibbonTab::Insert => insert(ui, state, actions),
                        RibbonTab::Layout => layout(ui, state, actions),
                        RibbonTab::References => references(ui, state, actions),
                        RibbonTab::Review => review(ui, state, actions),
                        RibbonTab::Ai => ai_tab(ui, state, actions),
                        RibbonTab::View => view(ui, state, actions),
                        RibbonTab::Recognition => recognition(ui, state, actions),
                        RibbonTab::Picture => picture(ui, state, actions),
                        RibbonTab::HeaderFooter => header_footer(ui, state, actions),
                    }
                });
            });
    });
}

/// egui's own widgets in the ribbon (the font boxes) in the buttons' style.
fn style_widgets(ui: &mut egui::Ui, palette: Palette) {
    let style = ui.style_mut();
    style.spacing.interact_size.y = ROW;
    style.spacing.button_padding = vec2(6.0, 2.0);
    let widgets = &mut style.visuals.widgets;
    widgets.inactive.weak_bg_fill = palette.button;
    widgets.inactive.bg_fill = palette.button;
    widgets.inactive.bg_stroke = Stroke::new(1.0, palette.button_border);
    widgets.inactive.corner_radius = egui::CornerRadius::same(RADIUS);
    widgets.hovered.weak_bg_fill = palette.hover;
    widgets.hovered.bg_stroke = Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.7));
    widgets.hovered.corner_radius = egui::CornerRadius::same(RADIUS);
    widgets.active.corner_radius = egui::CornerRadius::same(RADIUS);
    widgets.open.corner_radius = egui::CornerRadius::same(RADIUS);
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.button_border);
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
            let (rect, _) = ui.allocate_exact_size(vec2(width, LABEL_HEIGHT), Sense::hover());
            ui.painter().galley(
                pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 2.0),
                galley,
                palette.group_label,
            );
        })
        .response;
    let rect = response.rect;
    ui.add_space(5.0);
    let x = rect.right() + 4.0;
    ui.painter().line_segment(
        [pos2(x, rect.top() + 2.0), pos2(x, rect.bottom() - 1.0)],
        Stroke::new(1.0, palette.ribbon_line),
    );
    ui.add_space(6.0);
}

/// A column of rows, top-aligned like every other group content.
fn column(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        vec2(0.0, GROUP_HEIGHT),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.spacing_mut().item_spacing = vec2(GAP, GAP);
            add(ui);
        },
    );
}

// ----- buttons --------------------------------------------------------------

/// How a button looks in its state.
#[derive(Clone, Copy)]
struct Look {
    on: bool,
    enabled: bool,
}

/// Face and outline of a button.
fn frame(ui: &egui::Ui, rect: Rect, response: &egui::Response, look: Look, palette: Palette) {
    frame_corners(
        ui,
        rect,
        response,
        look,
        palette,
        egui::CornerRadius::same(RADIUS),
    );
}

fn frame_corners(
    ui: &egui::Ui,
    rect: Rect,
    response: &egui::Response,
    look: Look,
    palette: Palette,
    corners: egui::CornerRadius,
) {
    let active = look.enabled && (response.hovered() || response.is_pointer_button_down_on());
    let fill = if look.enabled && (look.on || response.is_pointer_button_down_on()) {
        palette.pressed
    } else if active {
        palette.hover
    } else {
        palette.button
    };
    let border = if look.on {
        theme::ACCENT.gamma_multiply(0.75)
    } else if active {
        theme::ACCENT.gamma_multiply(0.55)
    } else {
        palette.button_border
    };
    ui.painter().rect_filled(rect, corners, fill);
    ui.painter().rect_stroke(
        rect,
        corners,
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
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

/// A big button's label as one or two lines, broken only between words: the
/// break that leaves the longer line shortest. One word too long for two
/// lines stays one line, and the button grows.
fn big_label_lines(ui: &egui::Ui, label: &str) -> Vec<String> {
    let font = label_font();
    if text_width(ui, label, &font) <= BIG_LABEL_WIDTH {
        return vec![label.to_owned()];
    }
    let words: Vec<&str> = label.split(' ').filter(|w| !w.is_empty()).collect();
    if words.len() < 2 {
        return vec![label.to_owned()];
    }
    let mut best: Option<(f32, Vec<String>)> = None;
    for split in 1..words.len() {
        let first = words[..split].join(" ");
        let second = words[split..].join(" ");
        let widest = text_width(ui, &first, &font).max(text_width(ui, &second, &font));
        if best.as_ref().is_none_or(|(width, _)| widest < *width) {
            best = Some((widest, vec![first, second]));
        }
    }
    best.map_or_else(|| vec![label.to_owned()], |(_, lines)| lines)
}

/// A large icon over a label of one or two lines.
fn big(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    tip: &str,
    enabled: bool,
    palette: Palette,
) -> egui::Response {
    let lines = big_label_lines(ui, label);
    let font = label_font();
    let widest = lines
        .iter()
        .map(|line| text_width(ui, line, &font))
        .fold(0.0, f32::max);
    let width = (widest + 14.0).max(56.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, GROUP_HEIGHT), Sense::click());
    let response = response.on_hover_text(tip);
    frame(ui, rect, &response, Look { on: false, enabled }, palette);
    let icon_rect =
        Rect::from_center_size(pos2(rect.center().x, rect.top() + 21.0), vec2(30.0, 30.0));
    let color = text_color(ui, enabled);
    icons::paint(ui.painter(), icon_rect, icon, color, accent(enabled));
    // The first line sits on the same baseline in every big button.
    let first_line = 42.0;
    for (index, line) in lines.iter().enumerate() {
        ui.painter().text(
            pos2(
                rect.center().x,
                rect.top() + first_line + index as f32 * 14.0,
            ),
            egui::Align2::CENTER_TOP,
            line,
            font.clone(),
            color,
        );
    }
    response
}

fn big_clicked(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    tip: &str,
    enabled: bool,
    palette: Palette,
) -> bool {
    big(ui, icon, label, tip, enabled, palette).clicked() && enabled
}

/// One small button of a column.
#[derive(Clone)]
struct Small<'a> {
    icon: Icon,
    label: &'a str,
    tip: &'a str,
    enabled: bool,
    /// A menu opens instead (drawn with ▾).
    menu: bool,
    on: bool,
}

impl<'a> Small<'a> {
    fn new(icon: Icon, label: &'a str, tip: &'a str, enabled: bool) -> Self {
        Self {
            icon,
            label,
            tip,
            enabled,
            menu: false,
            on: false,
        }
    }

    fn menu(mut self) -> Self {
        self.menu = true;
        self
    }

    /// Lit, for a mode that is on.
    fn with_on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }
}

/// The width that fits the widest label of a column.
fn small_width(ui: &egui::Ui, items: &[Small<'_>]) -> f32 {
    let font = label_font();
    items
        .iter()
        .map(|item| text_width(ui, item.label, &font) + if item.menu { 14.0 } else { 0.0 })
        .fold(0.0, f32::max)
        + 34.0
}

/// One small button, `width` wide.
fn small_one(ui: &mut egui::Ui, item: &Small<'_>, width: f32, palette: Palette) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
    let response = response.on_hover_text(item.tip);
    frame(
        ui,
        rect,
        &response,
        Look {
            on: item.on,
            enabled: item.enabled,
        },
        palette,
    );
    let color = text_color(ui, item.enabled);
    icons::paint(
        ui.painter(),
        Rect::from_min_size(rect.left_top() + vec2(5.0, 3.0), vec2(16.0, 16.0)),
        item.icon,
        color,
        accent(item.enabled),
    );
    ui.painter().text(
        pos2(rect.left() + 27.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        item.label,
        label_font(),
        color,
    );
    if item.menu {
        triangle_down(
            ui.painter(),
            pos2(rect.right() - 9.0, rect.center().y),
            color,
        );
    }
    response
}

/// A column of small buttons of one width; the index of the one clicked.
fn small_column(ui: &mut egui::Ui, items: &[Small<'_>], palette: Palette) -> Option<usize> {
    let width = small_width(ui, items);
    let mut clicked = None;
    column(ui, |ui| {
        for (index, item) in items.iter().enumerate() {
            if small_one(ui, item, width, palette).clicked() && item.enabled {
                clicked = Some(index);
            }
        }
    });
    clicked
}

fn triangle_down(painter: &egui::Painter, center: egui::Pos2, color: Color32) {
    painter.add(egui::Shape::convex_polygon(
        vec![
            center + vec2(-3.5, -1.5),
            center + vec2(3.5, -1.5),
            center + vec2(0.0, 2.5),
        ],
        color,
        Stroke::NONE,
    ));
}

/// A square button of a row: a glyph (B, I, U…) or an icon, and an optional
/// ▾ half joined to it that opens a menu.
struct Square<'a> {
    glyph: &'a str,
    family: FontFamily,
    icon: Option<Icon>,
    on: bool,
    tip: &'a str,
    enabled: bool,
}

impl<'a> Square<'a> {
    fn glyph(glyph: &'a str, tip: &'a str, enabled: bool) -> Self {
        Self {
            glyph,
            family: FontFamily::Proportional,
            icon: None,
            on: false,
            tip,
            enabled,
        }
    }

    fn icon(icon: Icon, tip: &'a str, enabled: bool) -> Self {
        Self {
            glyph: "",
            family: FontFamily::Proportional,
            icon: Some(icon),
            on: false,
            tip,
            enabled,
        }
    }

    fn family(mut self, family: FontFamily) -> Self {
        self.family = family;
        self
    }

    fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }
}

/// Draws a square button; `split` leaves its right corners square for a ▾.
fn square(
    ui: &mut egui::Ui,
    item: &Square<'_>,
    split: bool,
    palette: Palette,
    decorate: impl FnOnce(&egui::Painter, Rect, Color32),
) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ROW + 2.0, ROW), Sense::click());
    let response = response.on_hover_text(item.tip);
    let corners = if split {
        egui::CornerRadius {
            nw: RADIUS,
            sw: RADIUS,
            ne: 0,
            se: 0,
        }
    } else {
        egui::CornerRadius::same(RADIUS)
    };
    frame_corners(
        ui,
        rect,
        &response,
        Look {
            on: item.on,
            enabled: item.enabled,
        },
        palette,
        corners,
    );
    let color = text_color(ui, item.enabled);
    if let Some(icon) = item.icon {
        icons::paint(
            ui.painter(),
            rect.shrink(4.0),
            icon,
            color,
            accent(item.enabled),
        );
    } else {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            item.glyph,
            FontId::new(14.0, item.family.clone()),
            color,
        );
    }
    decorate(ui.painter(), rect, color);
    item.enabled && response.clicked()
}

/// The ▾ half of a split button, joined to the square before it.
fn split_arrow(
    ui: &mut egui::Ui,
    tip: &str,
    enabled: bool,
    palette: Palette,
    add: impl FnOnce(&mut egui::Ui),
) {
    // Overlap the square's right edge so the two share one outline.
    ui.add_space(-GAP - 1.0);
    let (rect, response) = ui.allocate_exact_size(vec2(13.0, ROW), Sense::click());
    let response = response.on_hover_text(tip);
    frame_corners(
        ui,
        rect,
        &response,
        Look { on: false, enabled },
        palette,
        egui::CornerRadius {
            nw: 0,
            sw: 0,
            ne: RADIUS,
            se: RADIUS,
        },
    );
    triangle_down(ui.painter(), rect.center(), text_color(ui, enabled));
    if enabled {
        egui::Popup::menu(&response).show(add);
    }
}

/// A small button that opens a menu.
fn small_menu(
    ui: &mut egui::Ui,
    item: &Small<'_>,
    width: f32,
    palette: Palette,
    add: impl FnOnce(&mut egui::Ui),
) {
    let response = small_one(ui, item, width, palette);
    if item.enabled {
        egui::Popup::menu(&response).show(add);
    }
}

/// A big button that opens a menu.
fn big_menu(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    tip: &str,
    enabled: bool,
    palette: Palette,
    add: impl FnOnce(&mut egui::Ui),
) {
    let response = big(ui, icon, label, tip, enabled, palette);
    triangle_down(
        ui.painter(),
        pos2(response.rect.right() - 8.0, response.rect.top() + 8.0),
        text_color(ui, enabled),
    );
    if enabled {
        egui::Popup::menu(&response).show(add);
    }
}

/// A row with a check box, drawn like a small button.
fn check(
    ui: &mut egui::Ui,
    label: &str,
    tip: &str,
    value: bool,
    width: f32,
    palette: Palette,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
    let response = response.on_hover_text(tip);
    frame(
        ui,
        rect,
        &response,
        Look {
            on: false,
            enabled: true,
        },
        palette,
    );
    let color = text_color(ui, true);
    let mark = Rect::from_center_size(pos2(rect.left() + 13.0, rect.center().y), vec2(13.0, 13.0));
    let box_border = if ui.visuals().dark_mode {
        Color32::from_gray(150)
    } else {
        Color32::from_gray(120)
    };
    ui.painter().rect(
        mark,
        2.0,
        if value { theme::ACCENT } else { palette.button },
        Stroke::new(1.0, if value { theme::ACCENT } else { box_border }),
        egui::StrokeKind::Inside,
    );
    if value {
        ui.painter().add(egui::Shape::line(
            vec![
                mark.center() + vec2(-3.5, 0.0),
                mark.center() + vec2(-1.0, 2.5),
                mark.center() + vec2(3.5, -2.5),
            ],
            Stroke::new(1.6, Color32::WHITE),
        ));
    }
    ui.painter().text(
        pos2(rect.left() + 27.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        label_font(),
        color,
    );
    response.clicked()
}

/// The bullets (`bullet`) or numbering split button: the square switches
/// the caret's list of that kind on or off, the arrow opens the formats. The
/// square is lit, and its tip names the format, while the caret is in such a
/// list.
fn list_button(ui: &mut egui::Ui, s: &RibbonState<'_>, bullet: bool, actions: &mut Vec<Action>) {
    let editable = s.editable();
    let p = s.palette;
    let current = s
        .format
        .list
        .as_ref()
        .filter(|list| s.document && list.bullet == bullet);
    let (icon, name) = if bullet {
        (Icon::Bullets, s.t("tip.bullets"))
    } else {
        (Icon::Numbering, s.t("tip.numbering"))
    };
    let tip = current.map_or_else(
        || name.to_owned(),
        |list| {
            s.i18n.tf(
                "tip.list_current",
                &[
                    ("list", &name),
                    ("sample", &list.sample),
                    ("level", &(list.level + 1)),
                ],
            )
        },
    );
    if square(
        ui,
        &Square::icon(icon, &tip, editable).on(current.is_some()),
        true,
        p,
        |_, _, _| {},
    ) {
        let preset = if current.is_some() {
            None
        } else if bullet {
            Some(ListPreset::Bullet)
        } else {
            Some(ListPreset::Decimal)
        };
        doc(actions, Request::List(preset));
    }
    split_arrow(ui, s.t("tip.list_formats"), editable, p, |ui| {
        ui.set_min_width(150.0);
        let selected = current.and_then(|list| list.preset);
        ui.horizontal_wrapped(|ui| {
            ui.set_max_width(150.0);
            for preset in ListPreset::ALL
                .into_iter()
                .filter(|preset| preset.is_bullet() == bullet)
            {
                let label = RichText::new(preset.sample()).size(16.0);
                let button = egui::Button::selectable(selected == Some(preset), label)
                    .min_size(vec2(44.0, 36.0));
                if ui.add(button).clicked() {
                    doc(actions, Request::List(Some(preset)));
                    ui.close();
                }
            }
        });
        ui.separator();
        if ui
            .add_enabled(current.is_some(), egui::Button::new(s.t("cmd.list_none")))
            .clicked()
        {
            doc(actions, Request::List(None));
            ui.close();
        }
    });
}

fn doc(actions: &mut Vec<Action>, request: Request) {
    actions.push(Action::Doc(request));
}

// ----- Home -----------------------------------------------------------------

fn home(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    let on = s.document;
    let editable = s.editable();
    let p = s.palette;
    group(ui, s.t("group.clipboard"), p, |ui| {
        if big_clicked(
            ui,
            Icon::Paste,
            s.t("cmd.paste"),
            s.t("tip.paste"),
            editable,
            p,
        ) {
            actions.push(Action::Paste);
        }
        match small_column(
            ui,
            &[
                Small::new(Icon::Cut, s.t("cmd.cut"), s.t("tip.cut"), editable),
                Small::new(Icon::Copy, s.t("cmd.copy"), s.t("tip.copy"), on),
                Small::new(
                    Icon::SelectAll,
                    s.t("cmd.select_all"),
                    s.t("tip.select_all"),
                    on,
                ),
            ],
            p,
        ) {
            Some(0) => doc(actions, Request::Copy { cut: true }),
            Some(1) => doc(actions, Request::Copy { cut: false }),
            Some(_) => doc(actions, Request::SelectAll),
            None => {}
        }
    });
    group(ui, s.t("group.font"), p, |ui| font_group(ui, s, actions));
    group(ui, s.t("group.paragraph"), p, |ui| {
        column(ui, |ui| {
            ui.horizontal(|ui| {
                list_button(ui, s, true, actions);
                list_button(ui, s, false, actions);
                ui.add_space(4.0);
                for (icon, more, tip) in [
                    (Icon::IndentLess, false, "tip.indent_less"),
                    (Icon::IndentMore, true, "tip.indent_more"),
                ] {
                    if square(
                        ui,
                        &Square::icon(icon, s.t(tip), editable),
                        false,
                        p,
                        |_, _, _| {},
                    ) {
                        doc(actions, Request::IndentStep(more));
                    }
                }
            });
            ui.horizontal(|ui| {
                for (icon, align, tip) in [
                    (Icon::AlignLeft, Align::Left, "tip.align_left"),
                    (Icon::AlignCenter, Align::Center, "tip.align_center"),
                    (Icon::AlignRight, Align::Right, "tip.align_right"),
                    (Icon::Justify, Align::Justify, "tip.justify"),
                ] {
                    let current = on && s.format.align.unwrap_or(Align::Left) == align;
                    if square(
                        ui,
                        &Square::icon(icon, s.t(tip), editable).on(current),
                        false,
                        p,
                        |_, _, _| {},
                    ) {
                        doc(actions, Request::Align(align));
                    }
                }
            });
            let item = Small::new(
                Icon::PageBreak,
                s.t("cmd.page_break"),
                s.t("tip.page_break"),
                editable,
            );
            let width = 4.0 * (ROW + 2.0) + 3.0 * GAP;
            let width = width.max(small_width(ui, std::slice::from_ref(&item)));
            if small_one(ui, &item, width, p).clicked() && editable {
                doc(actions, Request::Insert(Insert::PageBreak));
            }
        });
    });
    if !s.styles.is_empty() {
        group(ui, s.t("group.styles"), p, |ui| {
            style_gallery(ui, s, actions);
        });
    }
    group(ui, s.t("group.editing"), p, |ui| {
        match small_column(
            ui,
            &[
                Small::new(Icon::Find, s.t("cmd.find"), s.t("tip.find"), on),
                Small::new(
                    Icon::Replace,
                    s.t("cmd.replace"),
                    s.t("tip.replace"),
                    editable,
                ),
                Small::new(
                    Icon::SelectAll,
                    s.t("cmd.select_all"),
                    s.t("tip.select_all"),
                    on,
                ),
            ],
            p,
        ) {
            Some(0) => actions.push(Action::Open(Dialog::Find { replace: false })),
            Some(1) => actions.push(Action::Open(Dialog::Find { replace: true })),
            Some(_) => doc(actions, Request::SelectAll),
            None => {}
        }
    });
}

fn font_group(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    let editable = s.editable();
    let p = s.palette;
    column(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add_enabled_ui(editable, |ui| {
                let current = s.format.font.clone().unwrap_or_default();
                egui::ComboBox::from_id_salt("font-family")
                    .width(140.0)
                    .height(320.0)
                    .selected_text(if current.is_empty() {
                        s.t("font.default").to_owned()
                    } else {
                        current.clone()
                    })
                    .show_ui(ui, |ui| {
                        font_list(ui, s, &current, actions);
                    })
                    .response
                    .on_hover_text(s.t("tip.font"));
                let size = s.format.size;
                egui::ComboBox::from_id_salt("font-size")
                    .width(46.0)
                    .height(320.0)
                    .selected_text(size.map_or_else(String::new, format_size))
                    .show_ui(ui, |ui| {
                        for candidate in FONT_SIZES {
                            let selected = size.is_some_and(|size| (size - candidate).abs() < 0.01);
                            if ui
                                .selectable_label(selected, format_size(*candidate))
                                .clicked()
                            {
                                doc(actions, Request::Format(FormatChange::Size(*candidate)));
                            }
                        }
                    })
                    .response
                    .on_hover_text(s.t("tip.size"));
            });
            if square(
                ui,
                &Square::glyph("A", s.t("tip.grow"), editable),
                false,
                p,
                |painter, rect, color| {
                    triangle(painter, rect, color, true);
                },
            ) {
                doc(actions, Request::Format(FormatChange::Grow));
            }
            if square(
                ui,
                &Square::glyph("A", s.t("tip.shrink"), editable),
                false,
                p,
                |painter, rect, color| {
                    triangle(painter, rect, color, false);
                },
            ) {
                doc(actions, Request::Format(FormatChange::Shrink));
            }
            if square(
                ui,
                &Square::icon(Icon::Clear, s.t("tip.clear_format"), editable),
                false,
                p,
                |_, _, _| {},
            ) {
                doc(actions, Request::Format(FormatChange::Clear));
            }
        });
        ui.horizontal(|ui| {
            let bold = FontFamily::Name(theme::BOLD.into());
            let italic = FontFamily::Name(theme::ITALIC.into());
            if square(
                ui,
                &Square::glyph("B", s.t("tip.bold"), editable)
                    .family(bold)
                    .on(s.format.bold),
                false,
                p,
                |_, _, _| {},
            ) {
                doc(actions, Request::Format(FormatChange::Bold));
            }
            if square(
                ui,
                &Square::glyph("I", s.t("tip.italic"), editable)
                    .family(italic)
                    .on(s.format.italic),
                false,
                p,
                |_, _, _| {},
            ) {
                doc(actions, Request::Format(FormatChange::Italic));
            }
            if square(
                ui,
                &Square::glyph("U", s.t("tip.underline"), editable).on(s.format.underline),
                true,
                p,
                |painter, rect, color| {
                    painter.line_segment(
                        [
                            rect.left_bottom() + vec2(8.0, -5.0),
                            rect.right_bottom() + vec2(-8.0, -5.0),
                        ],
                        Stroke::new(1.2, color),
                    );
                },
            ) {
                doc(actions, Request::Format(FormatChange::Underline));
            }
            split_arrow(ui, s.t("tip.underline_style"), editable, p, |ui| {
                underline_menu(ui, s, actions);
            });
            if square(
                ui,
                &Square::glyph("S", s.t("tip.strike"), editable)
                    .on(s.format.strike || s.format.double_strike),
                true,
                p,
                |painter, rect, color| {
                    painter.line_segment(
                        [
                            rect.left_center() + vec2(7.0, 0.5),
                            rect.right_center() + vec2(-7.0, 0.5),
                        ],
                        Stroke::new(1.2, color),
                    );
                },
            ) {
                doc(actions, Request::Format(FormatChange::Strike));
            }
            split_arrow(ui, s.t("tip.strike_style"), editable, p, |ui| {
                for (lines, key) in [
                    (1u8, "strike.single"),
                    (2, "strike.double"),
                    (0, "strike.none"),
                ] {
                    let current = match lines {
                        1 => s.format.strike,
                        2 => s.format.double_strike,
                        _ => !s.format.strike && !s.format.double_strike,
                    };
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(170.0, 26.0), Sense::click());
                    if current {
                        ui.painter().rect_filled(rect, 3.0, p.pressed);
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, 3.0, p.hover);
                    }
                    let color = ui.visuals().text_color();
                    let text = ui.painter().text(
                        rect.left_center() + vec2(10.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        s.t(key),
                        FontId::proportional(14.0),
                        color,
                    );
                    for line in 0..lines {
                        let y =
                            text.center().y + f32::from(line) * 3.0 - f32::from(lines - 1) * 1.5;
                        ui.painter().line_segment(
                            [pos2(text.left() - 2.0, y), pos2(text.right() + 2.0, y)],
                            Stroke::new(1.0, color),
                        );
                    }
                    if response.clicked() {
                        doc(actions, Request::Format(FormatChange::StrikeStyle(lines)));
                        ui.close();
                    }
                }
            });
            if square(
                ui,
                &Square::glyph("x²", s.t("tip.superscript"), editable).on(s.format.superscript),
                false,
                p,
                |_, _, _| {},
            ) {
                doc(actions, Request::Format(FormatChange::Superscript));
            }
            if square(
                ui,
                &Square::glyph("x₂", s.t("tip.subscript"), editable).on(s.format.subscript),
                false,
                p,
                |_, _, _| {},
            ) {
                doc(actions, Request::Format(FormatChange::Subscript));
            }
            ui.add_space(6.0);
            let [r, g, b] = s.font_color;
            let chosen = Color32::from_rgb(r, g, b);
            if square(
                ui,
                &Square::glyph("A", s.t("tip.font_color"), editable),
                true,
                p,
                |painter, rect, _| {
                    painter.rect_filled(
                        Rect::from_min_size(
                            rect.left_bottom() + vec2(6.0, -6.0),
                            vec2(rect.width() - 12.0, 3.5),
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
            split_arrow(ui, s.t("tip.font_color_more"), editable, p, |ui| {
                color_menu(ui, s, actions);
            });
            ui.add_space(GAP);
            if square(
                ui,
                &Square::glyph("ab", s.t("tip.highlight"), editable),
                true,
                p,
                |painter, rect, _| {
                    painter.rect_filled(
                        Rect::from_min_size(
                            rect.left_bottom() + vec2(5.0, -6.0),
                            vec2(rect.width() - 10.0, 3.5),
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
            split_arrow(ui, s.t("tip.highlight"), editable, p, |ui| {
                highlight_menu(ui, s, actions);
            });
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

fn swatch(ui: &mut egui::Ui, rgb: [u8; 3]) -> bool {
    let [r, g, b] = rgb;
    let (rect, response) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
    ui.painter()
        .rect_filled(rect, 2.0, Color32::from_rgb(r, g, b));
    let border = if response.hovered() {
        theme::ACCENT
    } else {
        Color32::from_gray(160)
    };
    ui.painter().rect_stroke(
        rect,
        2.0,
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    response.clicked()
}

fn color_menu(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    ui.label(s.t("menu.font_color"));
    egui::Grid::new("font-colors")
        .spacing(vec2(3.0, 3.0))
        .show(ui, |ui| {
            for (index, rgb) in COLORS.iter().enumerate() {
                if swatch(ui, *rgb) {
                    actions.push(Action::FontColor(*rgb));
                    doc(actions, Request::Format(FormatChange::Color(Some(*rgb))));
                    ui.close();
                }
                if index % 8 == 7 {
                    ui.end_row();
                }
            }
        });
    if ui.button(s.t("menu.automatic")).clicked() {
        doc(actions, Request::Format(FormatChange::Color(None)));
        ui.close();
    }
}

fn highlight_menu(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    use strict_ooxml::model::values::Highlight;
    ui.label(s.t("menu.highlight"));
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
        for (highlight, rgb) in choices {
            if swatch(ui, rgb) {
                doc(
                    actions,
                    Request::Format(FormatChange::Highlight(Some(highlight))),
                );
                ui.close();
            }
        }
    });
    if ui.button(s.t("menu.no_highlight")).clicked() {
        doc(actions, Request::Format(FormatChange::Highlight(None)));
        ui.close();
    }
}

/// `text` cut with an ellipsis to fit `width`.
fn elide(ui: &egui::Ui, text: &str, font: &FontId, width: f32) -> String {
    if text_width(ui, text, font) <= width {
        return text.to_owned();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>() + "…";
        if text_width(ui, &candidate, font) <= width {
            return candidate;
        }
    }
    "…".to_owned()
}

/// Word's style gallery: a few quick styles as previews.
fn style_gallery(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    let editable = s.editable();
    let p = s.palette;
    for entry in s.styles.iter().take(6) {
        let name = localized_style(s.i18n, &entry.name);
        let lower = entry.name.to_lowercase();
        let level = lower
            .chars()
            .last()
            .and_then(|c| c.to_digit(10))
            .unwrap_or(1) as f32;
        let (size, color, bold) = if lower == "title" {
            (17.0, ui.visuals().text_color(), false)
        } else if lower.starts_with("heading") {
            (15.0 - level, theme::ACCENT, true)
        } else {
            (12.0, ui.visuals().text_color(), false)
        };
        let selected = s.document
            && (s.format.style.as_deref() == Some(entry.id.as_str())
                || (s.format.style.is_none() && lower == "normal"));
        let (rect, response) = ui.allocate_exact_size(vec2(TILE, GROUP_HEIGHT), Sense::click());
        let response = response.on_hover_text(&name);
        frame(
            ui,
            rect,
            &response,
            Look {
                on: selected,
                enabled: editable,
            },
            p,
        );
        let family = if bold {
            FontFamily::Name(theme::BOLD.into())
        } else {
            FontFamily::Proportional
        };
        ui.painter().text(
            rect.center() - vec2(0.0, 9.0),
            egui::Align2::CENTER_CENTER,
            "АаBb",
            FontId::new(size.max(10.0), family),
            color,
        );
        let font = FontId::proportional(11.0);
        let label = elide(ui, &name, &font, TILE - 8.0);
        ui.painter().text(
            pos2(rect.center().x, rect.bottom() - 8.0),
            egui::Align2::CENTER_CENTER,
            label,
            font,
            p.group_label,
        );
        if editable && response.clicked() {
            doc(actions, Request::Style(entry.id.clone()));
        }
    }
    if s.styles.len() > 6 {
        let (rect, response) = ui.allocate_exact_size(vec2(15.0, GROUP_HEIGHT), Sense::click());
        let response = response.on_hover_text(s.t("tip.more_styles"));
        frame(
            ui,
            rect,
            &response,
            Look {
                on: false,
                enabled: editable,
            },
            p,
        );
        triangle_down(ui.painter(), rect.center(), text_color(ui, editable));
        if editable {
            egui::Popup::menu(&response).show(|ui| {
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
    }
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

// ----- Insert ---------------------------------------------------------------

fn insert(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    let editable = s.editable();
    let p = s.palette;
    group(ui, s.t("group.pages"), p, |ui| {
        if big_clicked(
            ui,
            Icon::PageBreak,
            s.t("cmd.page_break"),
            s.t("tip.page_break"),
            editable,
            p,
        ) {
            doc(actions, Request::Insert(Insert::PageBreak));
        }
        if big_clicked(
            ui,
            Icon::Columns,
            s.t("cmd.column_break"),
            s.t("tip.column_break"),
            editable,
            p,
        ) {
            doc(actions, Request::Insert(Insert::ColumnBreak));
        }
        if big_clicked(
            ui,
            Icon::SectionBreak,
            s.t("cmd.section_break"),
            s.t("tip.section_break"),
            editable,
            p,
        ) {
            doc(actions, Request::Page(PageChange::SectionBreak));
        }
    });
    group(ui, s.t("group.illustrations"), p, |ui| {
        if big_clicked(
            ui,
            Icon::Picture,
            s.t("cmd.picture"),
            s.t("tip.picture"),
            editable,
            p,
        ) {
            actions.push(Action::InsertPicture);
        }
    });
    group(ui, s.t("group.header_footer"), p, |ui| {
        header_footer_buttons(ui, s, actions);
    });
    group(ui, s.t("group.tables"), p, |ui| {
        if big_clicked(
            ui,
            Icon::Table,
            s.t("cmd.table"),
            s.t("tip.table"),
            editable,
            p,
        ) {
            actions.push(Action::Open(Dialog::Table {
                rows: 3,
                columns: 3,
            }));
        }
    });
    group(ui, s.t("group.text"), p, |ui| {
        if big_clicked(
            ui,
            Icon::InsertFile,
            s.t("cmd.insert_file"),
            s.t("tip.insert_file"),
            editable,
            p,
        ) {
            actions.push(Action::InsertDocument);
        }
        if big_clicked(
            ui,
            Icon::Date,
            s.t("cmd.date"),
            s.t("tip.date"),
            editable,
            p,
        ) {
            actions.push(Action::InsertDate);
        }
    });
    group(ui, s.t("group.symbols"), p, |ui| {
        if big_clicked(
            ui,
            Icon::Symbol,
            s.t("cmd.symbol"),
            s.t("tip.symbol"),
            editable,
            p,
        ) {
            actions.push(Action::Open(Dialog::Symbols));
        }
        if big_clicked(
            ui,
            Icon::Formula,
            s.t("cmd.formula"),
            s.t("tip.formula"),
            editable,
            p,
        ) {
            actions.push(Action::FormulaDialog);
        }
    });
}

// ----- References ------------------------------------------------------------

fn references(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    let editable = s.editable();
    let p = s.palette;
    group(ui, s.t("group.toc"), p, |ui| {
        big_menu(
            ui,
            Icon::Toc,
            s.t("cmd.toc"),
            s.t("tip.toc"),
            editable,
            p,
            |ui| {
                let label = if s.state.table_of_contents {
                    s.t("toc.update")
                } else {
                    s.t("toc.insert")
                };
                if ui.button(label).clicked() {
                    actions.push(Action::TableOfContents);
                    ui.close();
                }
                if ui
                    .add_enabled(
                        s.state.table_of_contents,
                        egui::Button::new(s.t("toc.remove")),
                    )
                    .clicked()
                {
                    doc(
                        actions,
                        Request::References(ReferenceCommand::RemoveTableOfContents),
                    );
                    ui.close();
                }
            },
        );
    });
    group(ui, s.t("group.notes"), p, |ui| {
        if big_clicked(
            ui,
            Icon::Footnote,
            s.t("cmd.footnote"),
            s.t("tip.footnote"),
            editable,
            p,
        ) {
            doc(
                actions,
                Request::References(ReferenceCommand::InsertNote { endnote: false }),
            );
        }
        column(ui, |ui| {
            let items = [
                Small::new(
                    Icon::Footnote,
                    s.t("cmd.endnote"),
                    s.t("tip.endnote"),
                    editable,
                ),
                Small::new(
                    Icon::Redo,
                    s.t("cmd.next_note"),
                    s.t("tip.next_note"),
                    s.document,
                ),
                Small::new(
                    Icon::Undo,
                    s.t("cmd.previous_note"),
                    s.t("tip.previous_note"),
                    s.document,
                ),
            ];
            match small_column(ui, &items, p) {
                Some(0) => doc(
                    actions,
                    Request::References(ReferenceCommand::InsertNote { endnote: true }),
                ),
                Some(1) => doc(
                    actions,
                    Request::References(ReferenceCommand::NextNote { back: false }),
                ),
                Some(_) => doc(
                    actions,
                    Request::References(ReferenceCommand::NextNote { back: true }),
                ),
                None => {}
            }
        });
    });
    group(ui, s.t("group.links"), p, |ui| {
        if big_clicked(
            ui,
            Icon::Bookmark,
            s.t("cmd.bookmark"),
            s.t("tip.bookmark"),
            s.document,
            p,
        ) {
            actions.push(Action::Open(Dialog::Bookmarks {
                name: String::new(),
                selected: None,
            }));
        }
    });
}

// ----- Headers and footers -------------------------------------------------

/// The header, footer and page number menus, on Insert and on the header
/// and footer tools.
fn header_footer_buttons(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    let editable = s.editable();
    let p = s.palette;
    for (header, icon, label, tip) in [
        (true, Icon::Header, "cmd.header", "tip.header"),
        (false, Icon::Footer, "cmd.footer", "tip.footer"),
    ] {
        if big_clicked(ui, icon, s.t(label), s.t(tip), editable, p) {
            actions.push(Action::EnterHeaderFooter { header });
        }
    }
    big_menu(
        ui,
        Icon::PageNumber,
        s.t("cmd.page_number"),
        s.t("tip.page_number"),
        editable,
        p,
        |ui| {
            for (top, title) in [(true, "page_number.top"), (false, "page_number.bottom")] {
                ui.menu_button(s.t(title), |ui| {
                    for (align, key) in [
                        (Align::Left, "page_number.left"),
                        (Align::Center, "page_number.center"),
                        (Align::Right, "page_number.right"),
                    ] {
                        if ui.button(s.t(key)).clicked() {
                            actions.push(Action::PageNumbers {
                                top,
                                align,
                                of_total: false,
                            });
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button(s.t("page_number.of_total")).clicked() {
                        actions.push(Action::PageNumbers {
                            top,
                            align: Align::Center,
                            of_total: true,
                        });
                        ui.close();
                    }
                });
            }
            ui.separator();
            if ui.button(s.t("page_number.format")).clicked() {
                actions.push(Action::Open(Dialog::PageStart {
                    start: 1,
                    restart: false,
                }));
                ui.close();
            }
            if ui.button(s.t("page_number.remove")).clicked() {
                doc(actions, Request::HeaderFooter(HeaderFooterCommand::Remove));
                ui.close();
            }
        },
    );
}

fn header_footer(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    let p = s.palette;
    group(ui, s.t("group.header_footer"), p, |ui| {
        header_footer_buttons(ui, s, actions);
    });
    if let Some(view) = s.header_footer {
        group(ui, s.t("group.options"), p, |ui| {
            column(ui, |ui| {
                let font = label_font();
                let width = [s.t("hf.first_page"), s.t("hf.even_odd")]
                    .iter()
                    .map(|label| text_width(ui, label, &font))
                    .fold(0.0, f32::max)
                    + 36.0;
                if check(
                    ui,
                    s.t("hf.first_page"),
                    s.t("tip.hf_first_page"),
                    view.first_page,
                    width,
                    p,
                ) {
                    doc(
                        actions,
                        Request::HeaderFooter(HeaderFooterCommand::FirstPage(!view.first_page)),
                    );
                }
                if check(
                    ui,
                    s.t("hf.even_odd"),
                    s.t("tip.hf_even_odd"),
                    view.even_odd,
                    width,
                    p,
                ) {
                    doc(
                        actions,
                        Request::HeaderFooter(HeaderFooterCommand::EvenOdd(!view.even_odd)),
                    );
                }
            });
        });
    }
    group(ui, s.t("group.close"), p, |ui| {
        if big_clicked(
            ui,
            Icon::Close,
            s.t("cmd.close_header_footer"),
            s.t("tip.close_header_footer"),
            true,
            p,
        ) {
            doc(actions, Request::HeaderFooter(HeaderFooterCommand::Exit));
        }
    });
}

// ----- Picture --------------------------------------------------------------

/// EMU per centimetre.
const EMU_PER_CM: f64 = 360_000.0;

/// A size in centimetres, edited in place: the value is the drawing's until
/// the field is touched, then the field's until it is let go, when the new
/// size is returned.
fn size_field(ui: &mut egui::Ui, id: &str, emu: i64, unit: &str) -> Option<i64> {
    let id = ui.id().with(id);
    #[allow(clippy::cast_precision_loss)]
    let current = emu as f64 / EMU_PER_CM;
    let mut value = ui.data(|data| data.get_temp::<f64>(id)).unwrap_or(current);
    let response = ui.add_sized(
        vec2(86.0, ROW),
        egui::DragValue::new(&mut value)
            .range(0.05..=200.0)
            .speed(0.02)
            .fixed_decimals(2)
            .suffix(format!(" {unit}")),
    );
    let editing = response.dragged() || response.has_focus();
    if editing {
        ui.data_mut(|data| data.insert_temp(id, value));
    } else {
        ui.data_mut(|data| data.remove::<f64>(id));
    }
    let done = response.drag_stopped() || response.lost_focus() || (response.changed() && !editing);
    #[allow(clippy::cast_possible_truncation)]
    let emu_now = (value * EMU_PER_CM).round() as i64;
    (done && (emu_now - emu).abs() > 100).then_some(emu_now)
}

fn picture(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    let editable = s.editable();
    let p = s.palette;
    let Some(object) = s.object else {
        return;
    };
    group(ui, s.t("group.size"), p, |ui| {
        column(ui, |ui| {
            let unit = s.t("unit.cm");
            // Either side keeps the proportions.
            #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
            let other = |side: i64, from: i64, to: i64| {
                (side as f64 * to as f64 / from.max(1) as f64).round() as i64
            };
            ui.horizontal(|ui| {
                ui.add_sized(vec2(64.0, ROW), egui::Label::new(s.t("label.height")));
                if let Some(height) = size_field(ui, "picture-height", object.height, unit) {
                    if editable {
                        let width = other(object.width, object.height, height);
                        doc(
                            actions,
                            Request::Object(ObjectCommand::Resize { width, height }),
                        );
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.add_sized(vec2(64.0, ROW), egui::Label::new(s.t("label.width")));
                if let Some(width) = size_field(ui, "picture-width", object.width, unit) {
                    if editable {
                        let height = other(object.height, object.width, width);
                        doc(
                            actions,
                            Request::Object(ObjectCommand::Resize { width, height }),
                        );
                    }
                }
            });
            let item = Small::new(
                Icon::OriginalSize,
                s.t("cmd.original_size"),
                s.t("tip.original_size"),
                editable && object.picture,
            );
            let width = 64.0 + 86.0 + GAP;
            if small_one(ui, &item, width, p).clicked() && editable && object.picture {
                doc(actions, Request::Object(ObjectCommand::ResetSize));
            }
        });
    });
    let pixels = editable && object.picture;
    let edit = |actions: &mut Vec<Action>, edit: PictureEdit| {
        doc(actions, Request::Object(ObjectCommand::Edit(edit)));
    };
    group(ui, s.t("group.adjust_picture"), p, |ui| {
        column(ui, |ui| {
            let crop =
                Small::new(Icon::Crop, s.t("cmd.crop"), s.t("tip.crop"), pixels).with_on(s.crop);
            let rotate =
                Small::new(Icon::Rotate, s.t("cmd.rotate"), s.t("tip.rotate"), pixels).menu();
            let width = small_width(ui, &[crop.clone(), rotate.clone()]);
            if small_one(ui, &crop, width, p).clicked() && pixels {
                actions.push(Action::ToggleCrop);
            }
            small_menu(ui, &rotate, width, p, |ui| {
                for (key, value) in [
                    ("cmd.rotate_right", PictureEdit::Rotate { clockwise: true }),
                    ("cmd.rotate_left", PictureEdit::Rotate { clockwise: false }),
                    ("cmd.flip_horizontal", PictureEdit::Flip { vertical: false }),
                    ("cmd.flip_vertical", PictureEdit::Flip { vertical: true }),
                ] {
                    if ui.button(s.t(key)).clicked() {
                        edit(actions, value);
                        ui.close();
                    }
                }
            });
        });
        column(ui, |ui| {
            let brightness = Small::new(
                Icon::Brightness,
                s.t("cmd.brightness"),
                s.t("tip.brightness"),
                pixels,
            )
            .menu();
            let contrast = Small::new(
                Icon::Contrast,
                s.t("cmd.contrast"),
                s.t("tip.contrast"),
                pixels,
            )
            .menu();
            let width = small_width(ui, &[brightness.clone(), contrast.clone()]);
            for (item, is_brightness) in [(brightness, true), (contrast, false)] {
                small_menu(ui, &item, width, p, |ui| {
                    if ui.button(s.t("cmd.adjust_more")).clicked() {
                        actions.push(Action::AdjustDialog);
                        ui.close();
                    }
                    ui.separator();
                    for step in [-40i8, -20, 20, 40] {
                        let label = format!("{step:+}%");
                        if ui.button(label).clicked() {
                            let amount = f32::from(step) / 100.0;
                            edit(
                                actions,
                                if is_brightness {
                                    PictureEdit::Adjust {
                                        brightness: amount,
                                        contrast: 0.0,
                                    }
                                } else {
                                    PictureEdit::Adjust {
                                        brightness: 0.0,
                                        contrast: amount,
                                    }
                                },
                            );
                            ui.close();
                        }
                    }
                });
            }
        });
    });
    group(ui, s.t("group.arrange"), p, |ui| {
        big_menu(
            ui,
            Icon::Wrap,
            s.t("cmd.wrap"),
            s.t("tip.wrap"),
            editable,
            p,
            |ui| {
                for style in WrapStyle::ALL {
                    if ui
                        .selectable_label(object.wrap == style, s.t(style.key()))
                        .clicked()
                    {
                        doc(actions, Request::Object(ObjectCommand::Wrap(style)));
                        ui.close();
                    }
                }
            },
        );
    });
    group(ui, s.t("group.edit_picture"), p, |ui| {
        column(ui, |ui| {
            let items = [
                Small::new(
                    Icon::SaveAs,
                    s.t("cmd.save_picture"),
                    s.t("tip.save_picture"),
                    object.picture,
                ),
                Small::new(
                    Icon::Open,
                    s.t("cmd.external_edit"),
                    s.t("tip.external_edit"),
                    pixels,
                ),
                Small::new(
                    Icon::Delete,
                    s.t("cmd.delete_picture"),
                    s.t("tip.delete_picture"),
                    editable,
                ),
            ];
            match small_column(ui, &items, p) {
                Some(0) => doc(
                    actions,
                    Request::Object(ObjectCommand::Export {
                        purpose: ExportPurpose::Save,
                    }),
                ),
                Some(1) => doc(
                    actions,
                    Request::Object(ObjectCommand::Export {
                        purpose: ExportPurpose::External,
                    }),
                ),
                Some(_) => doc(actions, Request::Object(ObjectCommand::Delete)),
                None => {}
            }
        });
        group(ui, s.t("group.picture_text"), p, |ui| {
            if big_clicked(
                ui,
                Icon::Translate,
                s.t("cmd.translate_picture"),
                s.t("tip.translate_picture"),
                s.editable() && s.object.is_some_and(|object| object.picture),
                p,
            ) {
                actions.push(Action::TranslatePicture);
            }
        });
    });
    group(ui, s.t("group.recognize_picture"), p, |ui| {
        let can = editable && object.picture && cfg!(feature = "ocr");
        if big_clicked(
            ui,
            Icon::Scan,
            s.t("cmd.picture_text"),
            s.t("tip.picture_text"),
            can,
            p,
        ) {
            actions.push(Action::RecognizePicture { formula: false });
        }
        if big_clicked(
            ui,
            Icon::Formula,
            s.t("cmd.picture_formula"),
            s.t("tip.picture_formula"),
            can,
            p,
        ) {
            actions.push(Action::RecognizePicture { formula: true });
        }
    });
}

// ----- Layout ---------------------------------------------------------------

fn layout(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    let editable = s.editable();
    let p = s.palette;
    group(ui, s.t("group.page_setup"), p, |ui| {
        match small_column(
            ui,
            &[
                Small::new(
                    Icon::Portrait,
                    s.t("cmd.portrait"),
                    s.t("tip.portrait"),
                    editable,
                ),
                Small::new(
                    Icon::Landscape,
                    s.t("cmd.landscape"),
                    s.t("tip.landscape"),
                    editable,
                ),
            ],
            p,
        ) {
            Some(0) => doc(
                actions,
                Request::Page(PageChange::Orientation { landscape: false }),
            ),
            Some(_) => doc(
                actions,
                Request::Page(PageChange::Orientation { landscape: true }),
            ),
            None => {}
        }
        let size = Small::new(
            Icon::PageSize,
            s.t("cmd.size"),
            s.t("tip.size_page"),
            editable,
        )
        .menu();
        let margins = Small::new(
            Icon::Margins,
            s.t("cmd.margins"),
            s.t("tip.margins"),
            editable,
        )
        .menu();
        let width = small_width(ui, &[size, margins]);
        column(ui, |ui| {
            let size = Small::new(
                Icon::PageSize,
                s.t("cmd.size"),
                s.t("tip.size_page"),
                editable,
            )
            .menu();
            small_menu(ui, &size, width, p, |ui| {
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
            });
            let margins = Small::new(
                Icon::Margins,
                s.t("cmd.margins"),
                s.t("tip.margins"),
                editable,
            )
            .menu();
            small_menu(ui, &margins, width, p, |ui| {
                for (key, margins) in [
                    ("margins.normal", [1134, 850, 1134, 1701]),
                    ("margins.narrow", [720, 720, 720, 720]),
                    ("margins.moderate", [1440, 1080, 1440, 1080]),
                    ("margins.wide", [1440, 2880, 1440, 2880]),
                ] {
                    if ui.button(s.t(key)).clicked() {
                        doc(actions, Request::Page(PageChange::Margins(margins)));
                        ui.close();
                    }
                }
            });
        });
    });
    group(ui, s.t("group.columns"), p, |ui| {
        big_menu(
            ui,
            Icon::Columns,
            s.t("cmd.columns"),
            s.t("tip.columns"),
            editable,
            p,
            |ui| {
                // Twips of the A4 text width a preset splits.
                let presets: [(&str, u16, Option<Vec<i32>>); 5] = [
                    ("columns.one", 1, None),
                    ("columns.two", 2, None),
                    ("columns.three", 3, None),
                    ("columns.left", 2, Some(vec![2835, 5953])),
                    ("columns.right", 2, Some(vec![5953, 2835])),
                ];
                for (key, count, widths) in presets {
                    if ui.button(s.t(key)).clicked() {
                        doc(
                            actions,
                            Request::Page(PageChange::Columns {
                                count,
                                space: 708,
                                separator: false,
                                widths,
                            }),
                        );
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button(s.t("columns.more")).clicked() {
                    actions.push(Action::Open(Dialog::Columns {
                        count: 2,
                        space_cm: 1.25,
                        separator: false,
                    }));
                    ui.close();
                }
            },
        );
    });
    group(ui, s.t("group.breaks"), p, |ui| {
        if big_clicked(
            ui,
            Icon::PageBreak,
            s.t("cmd.page_break"),
            s.t("tip.page_break"),
            editable,
            p,
        ) {
            doc(actions, Request::Insert(Insert::PageBreak));
        }
        if big_clicked(
            ui,
            Icon::Columns,
            s.t("cmd.column_break"),
            s.t("tip.column_break"),
            editable,
            p,
        ) {
            doc(actions, Request::Insert(Insert::ColumnBreak));
        }
        if big_clicked(
            ui,
            Icon::SectionBreak,
            s.t("cmd.section_break"),
            s.t("tip.section_break"),
            editable,
            p,
        ) {
            doc(actions, Request::Page(PageChange::SectionBreak));
        }
    });
    group(ui, s.t("group.table"), p, |ui| {
        let on = s.document;
        if let Some(index) = small_column(
            ui,
            &[
                Small::new(
                    Icon::SelectAll,
                    s.t("cmd.select_row"),
                    s.t("tip.select_row"),
                    on,
                ),
                Small::new(
                    Icon::SelectAll,
                    s.t("cmd.select_column"),
                    s.t("tip.select_column"),
                    on,
                ),
                Small::new(
                    Icon::SelectAll,
                    s.t("cmd.select_table"),
                    s.t("tip.select_table"),
                    on,
                ),
            ],
            p,
        ) {
            let command = [
                TableCommand::SelectRow,
                TableCommand::SelectColumn,
                TableCommand::SelectTable,
            ];
            if let Some(command) = command.get(index) {
                doc(actions, Request::Table(*command));
            }
        }
        if let Some(index) = small_column(
            ui,
            &[
                Small::new(
                    Icon::Table,
                    s.t("cmd.row_above"),
                    s.t("tip.row_above"),
                    editable,
                ),
                Small::new(
                    Icon::Table,
                    s.t("cmd.row_below"),
                    s.t("tip.row_below"),
                    editable,
                ),
                Small::new(
                    Icon::Close,
                    s.t("cmd.delete_rows"),
                    s.t("tip.delete_rows"),
                    editable,
                ),
            ],
            p,
        ) {
            let command = [
                TableCommand::RowAbove,
                TableCommand::RowBelow,
                TableCommand::DeleteRows,
            ];
            if let Some(command) = command.get(index) {
                doc(actions, Request::Table(*command));
            }
        }
    });
}

// ----- Review ---------------------------------------------------------------

fn ai_tab(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    use crate::app::AiAction;
    let editable = s.editable();
    let on = s.document;
    let p = s.palette;
    group(ui, s.t("group.ai_text"), p, |ui| {
        big_menu(
            ui,
            Icon::Summary,
            s.t("ai.summary"),
            s.t("tip.ai_summary"),
            on,
            p,
            |ui| {
                if ui.button(s.t("ai.of_selection")).clicked() {
                    actions.push(Action::Ai(AiAction::Summary { whole: false }));
                    ui.close();
                }
                if ui.button(s.t("ai.of_document")).clicked() {
                    actions.push(Action::Ai(AiAction::Summary { whole: true }));
                    ui.close();
                }
            },
        );
        big_menu(
            ui,
            Icon::Rewrite,
            s.t("ai.rewrite"),
            s.t("tip.ai_rewrite"),
            on,
            p,
            |ui| {
                for key in [
                    "ai.improve",
                    "ai.formal",
                    "ai.simpler",
                    "ai.shorter",
                    "ai.fix",
                ] {
                    if ui.button(s.t(key)).clicked() {
                        actions.push(Action::Ai(AiAction::Rewrite(
                            rewrite_instruction(key).to_owned(),
                        )));
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button(s.t("ai.own")).clicked() {
                    actions.push(Action::Open(Dialog::AiPrompt {
                        text: String::new(),
                    }));
                    ui.close();
                }
            },
        );
    });
    group(ui, s.t("group.ai_translate"), p, |ui| {
        let language = crate::ai::LANGUAGES
            .iter()
            .find(|(name, _)| *name == s.ai_language)
            .map_or(s.ai_language, |(_, key)| s.t(key));
        column(ui, |ui| {
            let item = Small::new(Icon::Translate, language, s.t("tip.ai_language"), true).menu();
            let width = small_width(ui, std::slice::from_ref(&item));
            small_menu(ui, &item, width, p, |ui| {
                for (name, key) in crate::ai::LANGUAGES {
                    if ui
                        .selectable_label(*name == s.ai_language, s.t(key))
                        .clicked()
                    {
                        actions.push(Action::AiLanguage((*name).to_owned()));
                        ui.close();
                    }
                }
            });
        });
        big_menu(
            ui,
            Icon::Translate,
            s.t("ai.translate"),
            s.t("tip.ai_translate"),
            on,
            p,
            |ui| {
                if ui.button(s.t("ai.translate_selection")).clicked() {
                    actions.push(Action::Ai(AiAction::TranslateSelection));
                    ui.close();
                }
                if ui
                    .add_enabled(editable, egui::Button::new(s.t("ai.translate_document")))
                    .clicked()
                {
                    actions.push(Action::Ai(AiAction::TranslateDocument));
                    ui.close();
                }
                if ui.button(s.t("ai.parallel")).clicked() {
                    actions.push(Action::Ai(AiAction::Parallel));
                    ui.close();
                }
            },
        );
    });
    group(ui, s.t("group.ai_style"), p, |ui| {
        if big_clicked(
            ui,
            Icon::Styles,
            s.t("ai.styles"),
            s.t("tip.ai_styles"),
            editable,
            p,
        ) {
            actions.push(Action::Ai(AiAction::Styles));
        }
        if big_clicked(
            ui,
            Icon::Assess,
            s.t("ai.assess"),
            s.t("tip.ai_assess"),
            on,
            p,
        ) {
            actions.push(Action::Ai(AiAction::Assess));
        }
        if big_clicked(
            ui,
            Icon::Rewrite,
            s.t("ai.unify"),
            s.t("tip.ai_unify"),
            editable,
            p,
        ) {
            actions.push(Action::Ai(AiAction::Unify));
        }
    });
    group(ui, s.t("group.ai_assistant"), p, |ui| {
        column(ui, |ui| {
            let items = [
                Small::new(Icon::Sparkle, s.t("ai.pane"), s.t("tip.ai_pane"), true)
                    .with_on(s.ai_open),
                Small::new(
                    Icon::Sparkle,
                    s.t("ai.settings"),
                    s.t("tip.ai_settings"),
                    true,
                ),
            ];
            match small_column(ui, &items, p) {
                Some(0) => actions.push(Action::ToggleAiPane),
                Some(_) => actions.push(Action::AiSetup),
                None => {}
            }
        });
    });
}

/// What the model is told for each rewriting choice.
fn rewrite_instruction(key: &str) -> &'static str {
    match key {
        "ai.formal" => "make it more formal and businesslike",
        "ai.simpler" => "make it simpler and easier to read",
        "ai.shorter" => "make it shorter, keeping every point",
        "ai.fix" => "correct spelling, grammar and punctuation only",
        _ => "improve its clarity and flow",
    }
}

fn review(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    let editable = s.editable();
    let p = s.palette;
    group(ui, s.t("group.proofing"), p, |ui| {
        column(ui, |ui| {
            let check = Small::new(
                Icon::Spelling,
                s.t("cmd.spelling"),
                s.t("tip.spelling"),
                true,
            )
            .with_on(s.spelling);
            let languages = Small::new(
                Icon::Words,
                s.t("cmd.dictionaries"),
                s.t("tip.dictionaries"),
                true,
            )
            .menu();
            let width = small_width(ui, &[check.clone(), languages.clone()]);
            if small_one(ui, &check, width, p).clicked() {
                actions.push(Action::ToggleSpelling);
            }
            small_menu(ui, &languages, width, p, |ui| {
                for language in crate::spell::LANGUAGES {
                    let name = s.t(language.name_key);
                    if crate::spell::installed(language.code) {
                        ui.add_enabled(false, egui::Button::new(format!("✓ {name}")));
                    } else if ui
                        .button(format!(
                            "{name} — {} ({:.1} MB)",
                            s.t("spell.download"),
                            language.size_mb
                        ))
                        .clicked()
                    {
                        actions.push(Action::DownloadDictionary(language.code));
                        ui.close();
                    }
                }
            });
        });
        if big_clicked(
            ui,
            Icon::Words,
            s.t("cmd.word_count"),
            s.t("tip.word_count"),
            s.document,
            p,
        ) {
            actions.push(Action::Open(Dialog::WordCount));
        }
    });
    group(ui, s.t("group.comments"), p, |ui| {
        if big_clicked(
            ui,
            Icon::Comment,
            s.t("cmd.new_comment"),
            s.t("tip.new_comment"),
            editable,
            p,
        ) {
            actions.push(Action::NewComment);
        }
        column(ui, |ui| {
            let items = [
                Small::new(
                    Icon::Undo,
                    s.t("cmd.previous_comment"),
                    s.t("tip.previous_comment"),
                    s.document,
                ),
                Small::new(
                    Icon::Redo,
                    s.t("cmd.next_comment"),
                    s.t("tip.next_comment"),
                    s.document,
                ),
                Small::new(
                    Icon::Comment,
                    s.t("cmd.show_comments"),
                    s.t("tip.show_comments"),
                    s.document,
                )
                .with_on(s.comments_open),
            ];
            match small_column(ui, &items, p) {
                Some(0) => doc(
                    actions,
                    Request::Comments(CommentCommand::Step { back: true }),
                ),
                Some(1) => doc(
                    actions,
                    Request::Comments(CommentCommand::Step { back: false }),
                ),
                Some(_) => actions.push(Action::ToggleComments),
                None => {}
            }
        });
    });
    group(ui, s.t("group.changes"), p, |ui| {
        if big_clicked(
            ui,
            Icon::Accept,
            s.t("cmd.accept_all"),
            s.t("tip.accept_all"),
            editable,
            p,
        ) {
            doc(actions, Request::Revisions { accept: true });
        }
        if big_clicked(
            ui,
            Icon::Reject,
            s.t("cmd.reject_all"),
            s.t("tip.reject_all"),
            editable,
            p,
        ) {
            doc(actions, Request::Revisions { accept: false });
        }
    });
    group(ui, s.t("group.fidelity"), p, |ui| {
        if big_clicked(
            ui,
            Icon::Report,
            s.t("cmd.report"),
            s.t("tip.report"),
            s.document,
            p,
        ) {
            actions.push(Action::ToggleReport);
        }
    });
}

// ----- View -----------------------------------------------------------------

fn view(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    let p = s.palette;
    group(ui, s.t("group.show"), p, |ui| {
        let font = label_font();
        let width = [
            s.t("cmd.navigation"),
            s.t("cmd.report_pane"),
            s.t("cmd.system_fonts"),
        ]
        .iter()
        .map(|label| text_width(ui, label, &font))
        .fold(0.0, f32::max)
            + 36.0;
        column(ui, |ui| {
            if check(
                ui,
                s.t("cmd.navigation"),
                s.t("cmd.navigation"),
                s.navigation,
                width,
                p,
            ) {
                actions.push(Action::ToggleNavigation);
            }
            if check(
                ui,
                s.t("cmd.report_pane"),
                s.t("tip.report"),
                s.report,
                width,
                p,
            ) {
                actions.push(Action::ToggleReport);
            }
            if check(
                ui,
                s.t("cmd.system_fonts"),
                s.t("tip.system_fonts"),
                s.system_fonts,
                width,
                p,
            ) {
                actions.push(Action::ToggleSystemFonts);
            }
        });
        column(ui, |ui| {
            let width = text_width(ui, s.t("cmd.ruler"), &label_font()) + 36.0;
            if check(ui, s.t("cmd.ruler"), s.t("tip.ruler"), s.ruler, width, p) {
                actions.push(Action::ToggleRuler);
            }
        });
    });
    group(ui, s.t("group.zoom"), p, |ui| {
        if big_clicked(
            ui,
            Icon::ZoomIn,
            s.t("cmd.zoom_in"),
            s.t("tip.zoom_in"),
            s.document,
            p,
        ) {
            actions.push(Action::Zoom(Zoom::In));
        }
        if big_clicked(
            ui,
            Icon::ZoomOut,
            s.t("cmd.zoom_out"),
            s.t("tip.zoom_out"),
            s.document,
            p,
        ) {
            actions.push(Action::Zoom(Zoom::Out));
        }
        match small_column(
            ui,
            &[
                Small::new(Icon::OnePage, "100%", s.t("tip.zoom_100"), s.document),
                Small::new(
                    Icon::OnePage,
                    s.t("cmd.one_page"),
                    s.t("tip.one_page"),
                    s.document,
                ),
                Small::new(
                    Icon::PageWidth,
                    s.t("cmd.page_width"),
                    s.t("tip.page_width"),
                    s.document,
                ),
            ],
            p,
        ) {
            Some(0) => actions.push(Action::Zoom(Zoom::Set(1.0))),
            Some(1) => actions.push(Action::Zoom(Zoom::OnePage)),
            Some(_) => actions.push(Action::Zoom(Zoom::PageWidth)),
            None => {}
        }
        let _ = s.zoom;
    });
    group(ui, s.t("group.interface"), p, |ui| {
        let theme_label = if s.dark {
            s.t("cmd.light")
        } else {
            s.t("cmd.dark")
        };
        if big_clicked(ui, Icon::Theme, theme_label, s.t("tip.theme"), true, p) {
            actions.push(Action::ToggleDark);
        }
        let current = s.i18n.language().to_owned();
        let name = s
            .i18n
            .languages()
            .find(|(code, _)| *code == current)
            .map_or(current.clone(), |(_, name)| name.to_owned());
        big_menu(
            ui,
            Icon::Language,
            &name,
            s.t("settings.language"),
            true,
            p,
            |ui| {
                for (code, name) in s.i18n.languages() {
                    if ui.selectable_label(code == current, name).clicked() {
                        actions.push(Action::Language(code.to_owned()));
                        ui.close();
                    }
                }
            },
        );
    });
}

// ----- Recognition ----------------------------------------------------------

fn recognition(ui: &mut egui::Ui, s: &RibbonState<'_>, actions: &mut Vec<Action>) {
    let p = s.palette;
    group(ui, s.t("group.import"), p, |ui| {
        if big_clicked(
            ui,
            Icon::Import,
            s.t("cmd.import_pdf"),
            s.t("tip.import_pdf"),
            true,
            p,
        ) {
            actions.push(Action::ImportDialog { ocr: false });
        }
        if big_clicked(
            ui,
            Icon::Scan,
            s.t("cmd.recognize"),
            s.t("tip.recognize"),
            cfg!(feature = "ocr"),
            p,
        ) {
            actions.push(Action::ImportDialog { ocr: true });
        }
    });
    group(ui, s.t("group.engine"), p, |ui| {
        if big_clicked(
            ui,
            Icon::Settings,
            s.t("cmd.ocr_settings"),
            s.t("tip.ocr_settings"),
            true,
            p,
        ) {
            actions.push(Action::Open(Dialog::Settings));
        }
    });
    group(ui, s.t("group.export"), p, |ui| {
        if big_clicked(
            ui,
            Icon::Pdf,
            s.t("cmd.export_pdf"),
            s.t("tip.export_pdf"),
            s.document,
            p,
        ) {
            actions.push(Action::Export(crate::worker::SaveFormat::Pdf));
        }
        if big_clicked(
            ui,
            Icon::Svg,
            s.t("cmd.export_svg"),
            s.t("tip.export_svg"),
            s.document,
            p,
        ) {
            actions.push(Action::Export(crate::worker::SaveFormat::Svg));
        }
        if big_clicked(
            ui,
            Icon::Txt,
            s.t("cmd.export_txt"),
            s.t("tip.export_txt"),
            s.document,
            p,
        ) {
            actions.push(Action::Export(crate::worker::SaveFormat::Text));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Labels break between words only, the longer line as short as it gets.
    #[test]
    fn big_labels_break_between_words() {
        let ctx = egui::Context::default();
        let mut lines = Vec::new();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            for label in [
                "Распознать скан",
                "Импорт PDF / DjVu",
                "Таблица",
                "Отчёт о поддержке",
                "Подзаголовокдлинныйбезпробелов",
            ] {
                lines.push(big_label_lines(ui, label));
            }
        });
        for (label, lines) in [
            "Распознать скан",
            "Импорт PDF / DjVu",
            "Таблица",
            "Отчёт о поддержке",
            "Подзаголовокдлинныйбезпробелов",
        ]
        .iter()
        .zip(&lines)
        {
            assert!(lines.len() <= 2, "{label}: {lines:?}");
            assert_eq!(lines.join(" "), *label, "no letters lost or moved");
            for line in lines {
                assert!(!line.starts_with(' ') && !line.ends_with(' '), "{line:?}");
            }
        }
        assert_eq!(lines[2], vec!["Таблица".to_owned()]);
        assert_eq!(lines[4].len(), 1, "one long word stays whole");
    }
}
