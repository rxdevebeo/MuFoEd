//! Toolbar pictures drawn with strokes, so a missing font glyph never leaves an empty button.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind};

#[derive(Clone, Copy, Debug)]
pub(crate) enum Icon {
    Undo,
    Redo,
    Sup,
    Sub,
    Fraction,
    Sqrt,
    Root,
    Sum,
    Integral,
    Product,
    Parens,
    Matrix,
    Bar,
    Palette,
    Label(&'static str),
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Bracket {
    None,
    Round,
    Square,
    Curly,
    Single,
    Double,
    Angle,
    Ceil,
    Floor,
}

/// A mark that sits above a base.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Accent {
    Bar,
    Hat,
    Tilde,
    Dot,
    Ddot,
    Arrow,
    Check,
    Breve,
}

pub(crate) fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    if let Icon::Label(text) = icon {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(rect.height() * 0.7),
            color,
        );
        return;
    }
    let pen = Pen::new(painter, rect, color);
    match icon {
        Icon::Undo => undo(&pen),
        Icon::Redo => redo(&pen),
        Icon::Sup => sup(&pen),
        Icon::Sub => sub(&pen),
        Icon::Fraction => fraction(&pen),
        Icon::Sqrt => sqrt_sign(&pen, false),
        Icon::Root => sqrt_sign(&pen, true),
        Icon::Sum => sum(&pen),
        Icon::Integral => integral(&pen),
        Icon::Product => product(&pen),
        Icon::Parens => round(&pen),
        Icon::Matrix => matrix(&pen),
        Icon::Bar => bar(&pen),
        Icon::Palette => palette(&pen),
        Icon::Label(_) => {}
    }
}

pub(crate) fn brackets(painter: &Painter, rect: Rect, kind: Bracket, color: Color32) {
    if matches!(kind, Bracket::None) {
        corners(&Pen::new(painter, rect, color));
        return;
    }
    let mid = rect.center().x;
    let left = Rect::from_min_max(rect.left_top(), Pos2::new(mid, rect.bottom()));
    let right = Rect::from_min_max(Pos2::new(mid, rect.top()), rect.right_bottom());
    paint_side(painter, left, kind, true, color);
    paint_side(painter, right, kind, false, color);
}

/// One fence character, when this crate can draw it.
pub(crate) fn fence_side(text: &str) -> Option<(Bracket, bool)> {
    let side = match text {
        "(" => (Bracket::Round, true),
        ")" => (Bracket::Round, false),
        "[" => (Bracket::Square, true),
        "]" => (Bracket::Square, false),
        "{" => (Bracket::Curly, true),
        "}" => (Bracket::Curly, false),
        "|" => (Bracket::Single, true),
        "‖" => (Bracket::Double, true),
        "⟨" => (Bracket::Angle, true),
        "⟩" => (Bracket::Angle, false),
        "⌈" => (Bracket::Ceil, true),
        "⌉" => (Bracket::Ceil, false),
        "⌊" => (Bracket::Floor, true),
        "⌋" => (Bracket::Floor, false),
        _ => return None,
    };
    Some(side)
}

pub(crate) fn paint_side(painter: &Painter, rect: Rect, kind: Bracket, left: bool, color: Color32) {
    let pen = Pen::new(painter, rect, color);
    match kind {
        Bracket::None => {}
        Bracket::Round => {
            if left {
                pen.quad([[0.78, 0.04], [0.12, 0.50], [0.78, 0.96]]);
            } else {
                pen.quad([[0.22, 0.04], [0.88, 0.50], [0.22, 0.96]]);
            }
        }
        Bracket::Square => {
            if left {
                pen.line(&[[0.72, 0.06], [0.28, 0.06], [0.28, 0.94], [0.72, 0.94]]);
            } else {
                pen.line(&[[0.28, 0.06], [0.72, 0.06], [0.72, 0.94], [0.28, 0.94]]);
            }
        }
        Bracket::Curly => curly_side(&pen, left),
        Bracket::Single => pen.line(&[[0.50, 0.04], [0.50, 0.96]]),
        Bracket::Double => {
            pen.line(&[[0.34, 0.04], [0.34, 0.96]]);
            pen.line(&[[0.66, 0.04], [0.66, 0.96]]);
        }
        Bracket::Angle => {
            if left {
                pen.line(&[[0.78, 0.06], [0.22, 0.50], [0.78, 0.94]]);
            } else {
                pen.line(&[[0.22, 0.06], [0.78, 0.50], [0.22, 0.94]]);
            }
        }
        Bracket::Ceil => {
            let x = if left { 0.30 } else { 0.70 };
            pen.line(&[[0.22, 0.08], [0.78, 0.08]]);
            pen.line(&[[x, 0.08], [x, 0.94]]);
        }
        Bracket::Floor => {
            let x = if left { 0.30 } else { 0.70 };
            pen.line(&[[x, 0.06], [x, 0.92]]);
            pen.line(&[[0.22, 0.92], [0.78, 0.92]]);
        }
    }
}

fn curly_side(pen: &Pen<'_>, left: bool) {
    if left {
        pen.cubic([[0.72, 0.04], [0.28, 0.08], [0.42, 0.40], [0.22, 0.50]]);
        pen.cubic([[0.22, 0.50], [0.42, 0.60], [0.28, 0.92], [0.72, 0.96]]);
    } else {
        pen.cubic([[0.28, 0.04], [0.72, 0.08], [0.58, 0.40], [0.78, 0.50]]);
        pen.cubic([[0.78, 0.50], [0.58, 0.60], [0.72, 0.92], [0.28, 0.96]]);
    }
}

/// The accent above a short base, for the toolbar menu.
pub(crate) fn accent(painter: &Painter, rect: Rect, kind: Accent, color: Color32) {
    let pen = Pen::new(painter, rect, color);
    pen.line(&[[0.14, 0.58], [0.36, 0.92]]);
    pen.line(&[[0.36, 0.58], [0.14, 0.92]]);
    let above = Rect::from_min_max(rect.left_top(), Pos2::new(rect.right(), rect.center().y));
    accent_mark(painter, above, kind, color);
}

/// Which accent `text` is, when it is a single accent character.
pub(crate) fn accent_of(text: &str) -> Option<Accent> {
    let mut chars = text.chars();
    let accent = match chars.next() {
        Some('\u{203E}' | '\u{00AF}' | '\u{02C9}' | '\u{0305}') => Accent::Bar,
        Some('ˆ') => Accent::Hat,
        Some('˜') => Accent::Tilde,
        Some('˙') => Accent::Dot,
        Some('¨') => Accent::Ddot,
        Some('→') => Accent::Arrow,
        Some('ˇ') => Accent::Check,
        Some('˘') => Accent::Breve,
        _ => return None,
    };
    chars.next().is_none().then_some(accent)
}

/// Line marks grow to the base. Dots stay a fixed size above its middle.
pub(crate) fn stretches(accent: Accent) -> bool {
    !matches!(accent, Accent::Dot | Accent::Ddot)
}

/// Paint `kind` into `rect`. A wide rect stretches the mark across the base.
pub(crate) fn paint_accent(painter: &Painter, rect: Rect, kind: Accent, color: Color32) {
    if kind == Accent::Arrow {
        stretched_arrow(painter, rect, color);
        return;
    }
    accent_mark(painter, rect, kind, color);
}

fn stretched_arrow(painter: &Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new((rect.height() * 0.12).clamp(1.2, 2.2), color);
    let head = (rect.height() * 0.9).min(rect.width() * 0.45).max(4.0);
    let y = rect.center().y;
    let neck = rect.right() - head;
    painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(neck, y)], stroke);
    painter.line_segment(
        [Pos2::new(neck, y - head * 0.42), Pos2::new(rect.right(), y)],
        stroke,
    );
    painter.line_segment(
        [Pos2::new(neck, y + head * 0.42), Pos2::new(rect.right(), y)],
        stroke,
    );
}

fn accent_mark(painter: &Painter, rect: Rect, kind: Accent, color: Color32) {
    let pen = Pen::new(painter, rect, color);
    match kind {
        Accent::Bar => pen.line(&[[0.08, 0.70], [0.92, 0.70]]),
        Accent::Hat => pen.line(&[[0.10, 0.78], [0.50, 0.16], [0.90, 0.78]]),
        Accent::Tilde => pen.cubic([[0.10, 0.62], [0.30, 0.16], [0.70, 0.84], [0.90, 0.38]]),
        Accent::Dot => pen.dot(0.62, 0.42, 0.14),
        Accent::Ddot => {
            pen.dot(0.40, 0.42, 0.11);
            pen.dot(0.74, 0.42, 0.11);
        }
        Accent::Arrow => {
            pen.line(&[[0.08, 0.50], [0.78, 0.50]]);
            pen.line(&[[0.58, 0.22], [0.92, 0.50], [0.58, 0.78]]);
        }
        Accent::Check => pen.line(&[[0.10, 0.22], [0.50, 0.78], [0.90, 0.22]]),
        Accent::Breve => pen.quad([[0.12, 0.28], [0.50, 0.86], [0.88, 0.28]]),
    }
}

/// A sign this crate can paint without a font glyph.
pub(crate) fn drawn(symbol: &str) -> Option<&'static str> {
    match symbol {
        "∂" => Some("∂"),
        "→" => Some("→"),
        "∈" => Some("∈"),
        "∉" => Some("∉"),
        "⊂" => Some("⊂"),
        "∞" => Some("∞"),
        "ˆ" => Some("ˆ"),
        "˜" => Some("˜"),
        "˙" => Some("˙"),
        "¨" => Some("¨"),
        "ˇ" => Some("ˇ"),
        "˘" => Some("˘"),
        _ => None,
    }
}

/// A glyph the interface font does not have.
pub(crate) fn symbol(painter: &Painter, rect: Rect, symbol: &str, color: Color32) {
    let pen = Pen::new(painter, rect, color);
    match symbol {
        "∂" => partial(&pen),
        "→" => arrow(&pen),
        "∈" => element(&pen, false),
        "∉" => element(&pen, true),
        "⊂" => subset(&pen),
        "∞" => infinity(&pen),
        "ˆ" => accent_mark(painter, rect, Accent::Hat, color),
        "˜" => accent_mark(painter, rect, Accent::Tilde, color),
        "˙" => accent_mark(painter, rect, Accent::Dot, color),
        "¨" => accent_mark(painter, rect, Accent::Ddot, color),
        "ˇ" => accent_mark(painter, rect, Accent::Check, color),
        "˘" => accent_mark(painter, rect, Accent::Breve, color),
        _ => {}
    }
}

struct Pen<'a> {
    painter: &'a Painter,
    rect: Rect,
    color: Color32,
    width: f32,
}

impl Pen<'_> {
    fn new(painter: &Painter, rect: Rect, color: Color32) -> Pen<'_> {
        Pen {
            painter,
            rect,
            color,
            width: (rect.height() * 0.075).clamp(1.2, 2.05),
        }
    }

    fn at(&self, x: f32, y: f32) -> Pos2 {
        Pos2::new(
            self.rect.left() + self.rect.width() * x,
            self.rect.top() + self.rect.height() * y,
        )
    }

    fn line(&self, points: &[[f32; 2]]) {
        let points = points
            .iter()
            .map(|point| self.at(point[0], point[1]))
            .collect();
        self.painter
            .add(Shape::line(points, Stroke::new(self.width, self.color)));
    }

    fn cubic(&self, points: [[f32; 2]; 4]) {
        self.painter
            .add(egui::epaint::CubicBezierShape::from_points_stroke(
                points.map(|point| self.at(point[0], point[1])),
                false,
                Color32::TRANSPARENT,
                Stroke::new(self.width, self.color),
            ));
    }

    fn quad(&self, points: [[f32; 2]; 3]) {
        self.painter
            .add(egui::epaint::QuadraticBezierShape::from_points_stroke(
                points.map(|point| self.at(point[0], point[1])),
                false,
                Color32::TRANSPARENT,
                Stroke::new(self.width, self.color),
            ));
    }

    fn dot(&self, x: f32, y: f32, radius: f32) {
        self.painter
            .circle_filled(self.at(x, y), self.rect.height() * radius, self.color);
    }

    fn frame(&self, x: f32, y: f32, w: f32, h: f32) {
        let rect = Rect::from_min_max(self.at(x, y), self.at(x + w, y + h));
        self.painter.rect_stroke(
            rect,
            1.0,
            Stroke::new(self.width, self.color),
            StrokeKind::Inside,
        );
    }
}

fn undo(pen: &Pen<'_>) {
    pen.cubic([[0.78, 0.58], [0.78, 0.16], [0.20, 0.16], [0.30, 0.58]]);
    pen.line(&[[0.16, 0.44], [0.30, 0.66]]);
    pen.line(&[[0.46, 0.44], [0.30, 0.66]]);
}

fn redo(pen: &Pen<'_>) {
    pen.cubic([[0.22, 0.58], [0.22, 0.16], [0.80, 0.16], [0.70, 0.58]]);
    pen.line(&[[0.54, 0.44], [0.70, 0.66]]);
    pen.line(&[[0.84, 0.44], [0.70, 0.66]]);
}

fn sup(pen: &Pen<'_>) {
    pen.line(&[[0.10, 0.46], [0.40, 0.90]]);
    pen.line(&[[0.40, 0.46], [0.10, 0.90]]);
    pen.line(&[[0.52, 0.14], [0.88, 0.14], [0.52, 0.36], [0.88, 0.36]]);
}

fn sub(pen: &Pen<'_>) {
    pen.line(&[[0.10, 0.10], [0.40, 0.50]]);
    pen.line(&[[0.40, 0.10], [0.10, 0.50]]);
    pen.line(&[[0.52, 0.58], [0.88, 0.58], [0.52, 0.82], [0.88, 0.82]]);
}

fn fraction(pen: &Pen<'_>) {
    pen.line(&[[0.08, 0.50], [0.92, 0.50]]);
    pen.line(&[[0.28, 0.22], [0.72, 0.22]]);
    pen.line(&[[0.28, 0.78], [0.72, 0.78]]);
}

fn sqrt_sign(pen: &Pen<'_>, nth: bool) {
    if nth {
        pen.line(&[[0.06, 0.30], [0.20, 0.10], [0.20, 0.36]]);
        pen.line(&[[0.26, 0.62], [0.40, 0.86], [0.56, 0.30], [0.96, 0.30]]);
    } else {
        pen.line(&[[0.06, 0.55], [0.22, 0.82], [0.40, 0.18], [0.94, 0.18]]);
        pen.line(&[[0.56, 0.40], [0.80, 0.74]]);
        pen.line(&[[0.80, 0.40], [0.56, 0.74]]);
    }
}

fn sum(pen: &Pen<'_>) {
    pen.line(&[
        [0.78, 0.14],
        [0.20, 0.14],
        [0.56, 0.50],
        [0.20, 0.86],
        [0.78, 0.86],
    ]);
}

fn integral(pen: &Pen<'_>) {
    // A tall stem with short serifs. A wide curve reads as the letter S.
    pen.line(&[[0.56, 0.07], [0.72, 0.01]]);
    pen.cubic([[0.58, 0.08], [0.50, 0.34], [0.52, 0.66], [0.44, 0.92]]);
    pen.line(&[[0.44, 0.92], [0.28, 0.99]]);
}

fn product(pen: &Pen<'_>) {
    pen.line(&[[0.16, 0.16], [0.84, 0.16]]);
    pen.line(&[[0.30, 0.16], [0.30, 0.86]]);
    pen.line(&[[0.70, 0.16], [0.70, 0.86]]);
}

fn bar(pen: &Pen<'_>) {
    pen.line(&[[0.16, 0.16], [0.84, 0.16]]);
    pen.line(&[[0.26, 0.40], [0.74, 0.88]]);
    pen.line(&[[0.74, 0.40], [0.26, 0.88]]);
}

fn matrix(pen: &Pen<'_>) {
    round(pen);
    pen.frame(0.30, 0.20, 0.16, 0.22);
    pen.frame(0.54, 0.20, 0.16, 0.22);
    pen.frame(0.30, 0.56, 0.16, 0.22);
    pen.frame(0.54, 0.56, 0.16, 0.22);
}

fn palette(pen: &Pen<'_>) {
    pen.cubic([[0.16, 0.78], [0.10, 0.30], [0.48, 0.22], [0.40, 0.52]]);
    pen.line(&[[0.40, 0.52], [0.18, 0.92]]);
    pen.line(&[[0.62, 0.28], [0.90, 0.28]]);
    pen.line(&[[0.76, 0.16], [0.90, 0.28], [0.76, 0.40]]);
    pen.cubic([[0.86, 0.62], [0.62, 0.55], [0.62, 0.92], [0.86, 0.84]]);
}

fn round(pen: &Pen<'_>) {
    pen.quad([[0.34, 0.06], [0.08, 0.50], [0.34, 0.94]]);
    pen.quad([[0.66, 0.06], [0.92, 0.50], [0.66, 0.94]]);
}

fn corners(pen: &Pen<'_>) {
    pen.line(&[[0.22, 0.36], [0.22, 0.18], [0.40, 0.18]]);
    pen.line(&[[0.60, 0.18], [0.78, 0.18], [0.78, 0.36]]);
    pen.line(&[[0.22, 0.64], [0.22, 0.82], [0.40, 0.82]]);
    pen.line(&[[0.60, 0.82], [0.78, 0.82], [0.78, 0.64]]);
}

fn partial(pen: &Pen<'_>) {
    pen.cubic([[0.46, 0.06], [0.82, 0.02], [0.80, 0.30], [0.52, 0.32]]);
    pen.cubic([[0.52, 0.32], [0.86, 0.36], [0.82, 0.82], [0.48, 0.80]]);
    pen.cubic([[0.48, 0.80], [0.16, 0.78], [0.16, 0.40], [0.48, 0.36]]);
}

fn arrow(pen: &Pen<'_>) {
    pen.line(&[[0.08, 0.50], [0.78, 0.50]]);
    pen.line(&[[0.56, 0.26], [0.90, 0.50], [0.56, 0.74]]);
}

fn element(pen: &Pen<'_>, slash: bool) {
    pen.cubic([[0.26, 0.12], [0.74, 0.06], [0.74, 0.94], [0.26, 0.88]]);
    pen.line(&[[0.30, 0.50], [0.70, 0.50]]);
    if slash {
        pen.line(&[[0.18, 0.82], [0.82, 0.18]]);
    }
}

fn subset(pen: &Pen<'_>) {
    pen.cubic([[0.74, 0.12], [0.26, 0.06], [0.26, 0.94], [0.74, 0.88]]);
}

fn infinity(pen: &Pen<'_>) {
    pen.cubic([[0.50, 0.50], [0.50, 0.16], [0.08, 0.16], [0.08, 0.50]]);
    pen.cubic([[0.08, 0.50], [0.08, 0.84], [0.50, 0.84], [0.50, 0.50]]);
    pen.cubic([[0.50, 0.50], [0.50, 0.16], [0.92, 0.16], [0.92, 0.50]]);
    pen.cubic([[0.92, 0.50], [0.92, 0.84], [0.50, 0.84], [0.50, 0.50]]);
}
