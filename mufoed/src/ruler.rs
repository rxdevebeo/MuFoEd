//! Rulers above and beside the desk, as Word draws them: the page's
//! margins shaded, centimetre ticks, and the caret paragraph's indents as
//! markers. Dragging a margin's edge or a marker changes it.

use eframe::egui::{self, pos2, vec2, Color32, Rect, Sense, Shape, Stroke};

use crate::canvas;
use crate::i18n::I18n;
use crate::tab::DocTab;
use crate::theme::Palette;
use crate::worker::{PageChange, Request, RulerInfo};

/// Thickness of a ruler, in points.
const THICK: f32 = 20.0;
/// Twips per centimetre.
const TWIPS_PER_CM: f32 = 566.929;

/// What a ruler drag moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Grip {
    LeftMargin,
    RightMargin,
    TopMargin,
    BottomMargin,
    /// The first line's indent (the top triangle).
    FirstLine,
    /// The other lines' indent, the first line staying (bottom triangle).
    Hanging,
    /// Both, together (the box under the bottom triangle).
    LeftIndent,
    RightIndent,
}

impl Grip {
    fn key(self) -> &'static str {
        match self {
            Self::LeftMargin => "ruler.left_margin",
            Self::RightMargin => "ruler.right_margin",
            Self::TopMargin => "ruler.top_margin",
            Self::BottomMargin => "ruler.bottom_margin",
            Self::FirstLine => "ruler.first_line",
            Self::Hanging => "ruler.hanging",
            Self::LeftIndent => "ruler.left_indent",
            Self::RightIndent => "ruler.right_indent",
        }
    }
}

/// A drag in progress: what, and where it is now, in twips from the page's
/// edge (left or top).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Dragging {
    pub(crate) grip: Grip,
    pub(crate) at: i32,
}

/// The desk with its rulers around it.
pub(crate) fn desk(
    ui: &mut egui::Ui,
    tab: &mut DocTab,
    palette: Palette,
    keyboard: bool,
    show: bool,
    i18n: &I18n,
) {
    let info = tab.cursor.as_ref().and_then(|cursor| cursor.ruler.clone());
    let Some(info) = info.filter(|_| show) else {
        canvas::show(ui, tab, palette, keyboard);
        return;
    };
    let full = ui.available_rect_before_wrap();
    let top = Rect::from_min_max(full.min, pos2(full.right(), full.top() + THICK));
    let left = Rect::from_min_max(
        pos2(full.left(), top.bottom()),
        pos2(full.left() + THICK, full.bottom()),
    );
    let rest = Rect::from_min_max(pos2(left.right(), top.bottom()), full.max);
    ui.scope_builder(egui::UiBuilder::new().max_rect(rest), |ui| {
        canvas::show(ui, tab, palette, keyboard);
    });
    ui.allocate_rect(full, Sense::hover());
    let page = tab.view.page_screens.get(info.page).copied().flatten();
    let painter = ui.painter_at(full);
    painter.rect_filled(top, 0.0, palette.ribbon);
    painter.rect_filled(left, 0.0, palette.ribbon);
    painter.rect_filled(
        Rect::from_min_size(full.min, vec2(THICK, THICK)),
        0.0,
        palette.ribbon,
    );
    let Some(page) = page else {
        return;
    };
    horizontal(ui, tab, &info, page, top, palette, i18n);
    vertical(ui, tab, &info, page, left, palette, i18n);
}

/// Screen x of a position `twips` from the page's left edge.
fn x_of(info: &RulerInfo, page: Rect, twips: i32) -> f32 {
    page.left() + page.width() * twips as f32 / info.width.max(1) as f32
}

fn y_of(info: &RulerInfo, page: Rect, twips: i32) -> f32 {
    page.top() + page.height() * twips as f32 / info.height.max(1) as f32
}

fn twips_at_x(info: &RulerInfo, page: Rect, x: f32) -> i32 {
    #[allow(clippy::cast_possible_truncation)]
    let twips = ((x - page.left()) / page.width().max(1.0) * info.width as f32).round() as i32;
    twips
}

fn twips_at_y(info: &RulerInfo, page: Rect, y: f32) -> i32 {
    #[allow(clippy::cast_possible_truncation)]
    let twips = ((y - page.top()) / page.height().max(1.0) * info.height as f32).round() as i32;
    twips
}

/// Centimetres, for a tooltip.
fn cm(twips: i32, i18n: &I18n) -> String {
    format!("{:.2} {}", twips as f32 / TWIPS_PER_CM, i18n.t("unit.cm"))
}

/// Ticks every quarter centimetre, numbers every centimetre, counted from
/// `origin` (the margin) both ways.
#[allow(clippy::too_many_arguments)]
fn ticks(
    painter: &egui::Painter,
    from: f32,
    to: f32,
    origin: f32,
    per_cm: f32,
    across: (f32, f32),
    horizontal: bool,
    color: Color32,
) {
    if per_cm < 4.0 {
        return;
    }
    let step = per_cm / 4.0;
    let start = ((from - origin) / step).floor() as i32;
    let end = ((to - origin) / step).ceil() as i32;
    for index in start..=end {
        let at = origin + index as f32 * step;
        if at < from || at > to {
            continue;
        }
        let (near, far) = across;
        let middle = f32::midpoint(near, far);
        let length = match index.rem_euclid(4) {
            0 => 0.0,
            2 => 4.0,
            _ => 2.0,
        };
        if index.rem_euclid(4) == 0 {
            if index != 0 {
                let label = (index / 4).abs().to_string();
                let pos = if horizontal {
                    pos2(at, middle)
                } else {
                    pos2(middle, at)
                };
                painter.text(
                    pos,
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(9.0),
                    color,
                );
            }
            continue;
        }
        let (a, b) = if horizontal {
            (
                pos2(at, middle - length / 2.0),
                pos2(at, middle + length / 2.0),
            )
        } else {
            (
                pos2(middle - length / 2.0, at),
                pos2(middle + length / 2.0, at),
            )
        };
        painter.line_segment([a, b], Stroke::new(1.0, color));
    }
}

/// A triangle marker pointing down (`down`) or up, its tip at `tip`.
fn triangle(painter: &egui::Painter, tip: egui::Pos2, down: bool, fill: Color32, line: Color32) {
    let h = 5.0;
    let points = if down {
        vec![tip, tip + vec2(-4.5, -h), tip + vec2(4.5, -h)]
    } else {
        vec![tip, tip + vec2(4.5, h), tip + vec2(-4.5, h)]
    };
    painter.add(Shape::convex_polygon(points, fill, Stroke::new(1.0, line)));
}

#[allow(clippy::too_many_lines)]
fn horizontal(
    ui: &mut egui::Ui,
    tab: &mut DocTab,
    info: &RulerInfo,
    page: Rect,
    strip: Rect,
    palette: Palette,
    i18n: &I18n,
) {
    let painter = ui.painter_at(strip);
    let [_, right_margin, _, left_margin] = info.margins;
    let dragging = tab.view.ruler_drag.filter(|d| {
        matches!(
            d.grip,
            Grip::LeftMargin
                | Grip::RightMargin
                | Grip::FirstLine
                | Grip::Hanging
                | Grip::LeftIndent
                | Grip::RightIndent
        )
    });
    // Margins as dragged so far.
    let mut left_edge = left_margin;
    let mut right_edge = info.width - right_margin;
    if let Some(drag) = dragging {
        match drag.grip {
            Grip::LeftMargin => left_edge = drag.at,
            Grip::RightMargin => right_edge = drag.at,
            _ => {}
        }
    }
    let band = strip.shrink2(vec2(0.0, 3.0));
    let paper = Rect::from_x_y_ranges(
        x_of(info, page, 0)..=x_of(info, page, info.width),
        band.y_range(),
    );
    let text = Rect::from_x_y_ranges(
        x_of(info, page, left_edge)..=x_of(info, page, right_edge),
        band.y_range(),
    );
    painter.rect_filled(paper, 0.0, palette.desk);
    painter.rect_filled(text, 0.0, Color32::WHITE);
    painter.rect_stroke(
        paper,
        0.0,
        Stroke::new(1.0, palette.ribbon_line),
        egui::StrokeKind::Inside,
    );
    let per_cm = page.width() / (info.width as f32 / TWIPS_PER_CM);
    ticks(
        &painter,
        paper.left(),
        paper.right(),
        text.left(),
        per_cm,
        (band.top(), band.bottom()),
        true,
        Color32::from_gray(90),
    );
    // Indent markers, from the text's left edge.
    let mut markers: Vec<(Grip, Rect)> = Vec::new();
    if let Some(indent) = info.indent {
        let [mut left, mut right, mut first] = indent;
        if let Some(drag) = dragging {
            let from_margin = drag.at - left_edge;
            match drag.grip {
                Grip::FirstLine => first = from_margin - left,
                Grip::Hanging => {
                    // The first line stays where it was.
                    let first_at = left + first;
                    left = from_margin;
                    first = first_at - left;
                }
                Grip::LeftIndent => left = from_margin,
                Grip::RightIndent => right = (info.width - right_margin) - drag.at,
                _ => {}
            }
        }
        let line = Color32::from_gray(70);
        let fill = Color32::from_gray(235);
        let first_x = x_of(info, page, left_edge + left + first);
        let left_x = x_of(info, page, left_edge + left);
        let right_x = x_of(info, page, right_edge - right);
        triangle(&painter, pos2(first_x, band.top() + 6.0), true, fill, line);
        triangle(
            &painter,
            pos2(left_x, band.bottom() - 7.0),
            false,
            fill,
            line,
        );
        let square = Rect::from_min_max(
            pos2(left_x - 4.5, band.bottom() - 6.0),
            pos2(left_x + 4.5, band.bottom() - 1.0),
        );
        painter.rect(
            square,
            0.0,
            fill,
            Stroke::new(1.0, line),
            egui::StrokeKind::Inside,
        );
        triangle(
            &painter,
            pos2(right_x, band.bottom() - 7.0),
            false,
            fill,
            line,
        );
        markers.push((
            Grip::FirstLine,
            Rect::from_center_size(pos2(first_x, band.top() + 3.0), vec2(10.0, 7.0)),
        ));
        markers.push((Grip::LeftIndent, square.expand(1.0)));
        markers.push((
            Grip::Hanging,
            Rect::from_center_size(pos2(left_x, band.bottom() - 9.0), vec2(10.0, 6.0)),
        ));
        markers.push((
            Grip::RightIndent,
            Rect::from_center_size(pos2(right_x, band.bottom() - 4.5), vec2(10.0, 9.0)),
        ));
    }
    let response = ui.interact(
        strip,
        ui.id().with(("ruler-h", tab.id)),
        Sense::click_and_drag(),
    );
    let pointer = response
        .interact_pointer_pos()
        .or_else(|| response.hover_pos());
    let grip_at = |pos: egui::Pos2| -> Option<Grip> {
        if let Some((grip, _)) = markers.iter().find(|(_, rect)| rect.contains(pos)) {
            return Some(*grip);
        }
        if (pos.x - text.left()).abs() <= 3.0 {
            Some(Grip::LeftMargin)
        } else if (pos.x - text.right()).abs() <= 3.0 {
            Some(Grip::RightMargin)
        } else {
            None
        }
    };
    if response.hovered() {
        if let Some(grip) = pointer.and_then(grip_at) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            response.clone().on_hover_text(i18n.t(grip.key()));
        }
    }
    if response.drag_started() {
        let origin = ui.input(|input| input.pointer.press_origin());
        if let Some(grip) = origin.and_then(grip_at) {
            tab.view.ruler_drag = Some(Dragging {
                grip,
                at: twips_at_x(info, page, origin.map_or(0.0, |p| p.x)),
            });
        }
    }
    if let (Some(drag), Some(pos)) = (tab.view.ruler_drag.as_mut(), pointer) {
        if matches!(
            drag.grip,
            Grip::LeftMargin
                | Grip::RightMargin
                | Grip::FirstLine
                | Grip::Hanging
                | Grip::LeftIndent
                | Grip::RightIndent
        ) {
            // Snap to an eighth of a centimetre, as Word does.
            #[allow(clippy::cast_possible_truncation)]
            let snap = (TWIPS_PER_CM / 8.0) as i32;
            let raw = twips_at_x(info, page, pos.x).clamp(0, info.width);
            drag.at = (raw + snap / 2) / snap * snap;
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            // A guide down the page.
            let x = x_of(info, page, drag.at);
            ui.ctx()
                .layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    ui.id().with("guide"),
                ))
                .line_segment(
                    [pos2(x, strip.bottom()), pos2(x, page.bottom())],
                    Stroke::new(1.0, Color32::from_gray(120)),
                );
            response.clone().on_hover_text(cm(drag.at, i18n));
        }
    }
    if response.drag_stopped() {
        if let Some(drag) = tab.view.ruler_drag.take() {
            finish(tab, info, drag);
        }
    }
}

fn vertical(
    ui: &mut egui::Ui,
    tab: &mut DocTab,
    info: &RulerInfo,
    page: Rect,
    strip: Rect,
    palette: Palette,
    i18n: &I18n,
) {
    let painter = ui.painter_at(strip);
    let [top_margin, _, bottom_margin, _] = info.margins;
    let mut top_edge = top_margin;
    let mut bottom_edge = info.height - bottom_margin;
    if let Some(drag) = tab.view.ruler_drag {
        match drag.grip {
            Grip::TopMargin => top_edge = drag.at,
            Grip::BottomMargin => bottom_edge = drag.at,
            _ => {}
        }
    }
    let band = strip.shrink2(vec2(3.0, 0.0));
    let paper = Rect::from_x_y_ranges(
        band.x_range(),
        y_of(info, page, 0)..=y_of(info, page, info.height),
    );
    let text = Rect::from_x_y_ranges(
        band.x_range(),
        y_of(info, page, top_edge)..=y_of(info, page, bottom_edge),
    );
    painter.rect_filled(paper, 0.0, palette.desk);
    painter.rect_filled(text, 0.0, Color32::WHITE);
    painter.rect_stroke(
        paper,
        0.0,
        Stroke::new(1.0, palette.ribbon_line),
        egui::StrokeKind::Inside,
    );
    let per_cm = page.height() / (info.height as f32 / TWIPS_PER_CM);
    ticks(
        &painter,
        paper.top(),
        paper.bottom(),
        text.top(),
        per_cm,
        (band.left(), band.right()),
        false,
        Color32::from_gray(90),
    );
    let response = ui.interact(
        strip,
        ui.id().with(("ruler-v", tab.id)),
        Sense::click_and_drag(),
    );
    let pointer = response
        .interact_pointer_pos()
        .or_else(|| response.hover_pos());
    let grip_at = |pos: egui::Pos2| {
        if (pos.y - text.top()).abs() <= 3.0 {
            Some(Grip::TopMargin)
        } else if (pos.y - text.bottom()).abs() <= 3.0 {
            Some(Grip::BottomMargin)
        } else {
            None
        }
    };
    if response.hovered() {
        if let Some(grip) = pointer.and_then(grip_at) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            response.clone().on_hover_text(i18n.t(grip.key()));
        }
    }
    if response.drag_started() {
        let origin = ui.input(|input| input.pointer.press_origin());
        if let Some(grip) = origin.and_then(grip_at) {
            tab.view.ruler_drag = Some(Dragging {
                grip,
                at: twips_at_y(info, page, origin.map_or(0.0, |p| p.y)),
            });
        }
    }
    if let (Some(drag), Some(pos)) = (tab.view.ruler_drag.as_mut(), pointer) {
        if matches!(drag.grip, Grip::TopMargin | Grip::BottomMargin) {
            #[allow(clippy::cast_possible_truncation)]
            let snap = (TWIPS_PER_CM / 8.0) as i32;
            let raw = twips_at_y(info, page, pos.y).clamp(0, info.height);
            drag.at = (raw + snap / 2) / snap * snap;
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
    }
    if response.drag_stopped() {
        if let Some(drag) = tab.view.ruler_drag.take() {
            finish(tab, info, drag);
        }
    }
}

/// Sends what a finished drag changed.
fn finish(tab: &DocTab, info: &RulerInfo, drag: Dragging) {
    let [top, right, bottom, left] = info.margins;
    let minimum = 0;
    match drag.grip {
        Grip::LeftMargin => {
            let left = drag.at.clamp(minimum, info.width - right - 567);
            tab.send(Request::Page(PageChange::Margins([
                top, right, bottom, left,
            ])));
        }
        Grip::RightMargin => {
            let right = (info.width - drag.at).clamp(minimum, info.width - left - 567);
            tab.send(Request::Page(PageChange::Margins([
                top, right, bottom, left,
            ])));
        }
        Grip::TopMargin => {
            let top = drag.at.clamp(minimum, info.height - bottom - 567);
            tab.send(Request::Page(PageChange::Margins([
                top, right, bottom, left,
            ])));
        }
        Grip::BottomMargin => {
            let bottom = (info.height - drag.at).clamp(minimum, info.height - top - 567);
            tab.send(Request::Page(PageChange::Margins([
                top, right, bottom, left,
            ])));
        }
        Grip::FirstLine | Grip::Hanging | Grip::LeftIndent | Grip::RightIndent => {
            let Some([mut indent_left, mut indent_right, mut first]) = info.indent else {
                return;
            };
            let from_margin = drag.at - left;
            match drag.grip {
                Grip::FirstLine => first = from_margin - indent_left,
                Grip::Hanging => {
                    let first_at = indent_left + first;
                    indent_left = from_margin;
                    first = first_at - indent_left;
                }
                Grip::LeftIndent => indent_left = from_margin,
                _ => indent_right = (info.width - right) - drag.at,
            }
            tab.send(Request::Indent([indent_left, indent_right, first]));
        }
    }
}
