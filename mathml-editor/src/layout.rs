//! Measure the formula and paint it, recording a caret position for every gap.

#![allow(clippy::too_many_arguments)]

use std::sync::Arc;

use egui::{
    pos2, vec2, Color32, FontFamily, FontId, Galley, Painter, Pos2, Rect, Shape, Stroke, Vec2,
};

use crate::edit::Selection;
use crate::model::{Caret, Node, Row, Slot, Step, Token, TokenKind};

/// Font and colour the host wants the formula drawn with.
pub(crate) struct Look {
    pub(crate) italic: FontFamily,
    pub(crate) size: f32,
    pub(crate) ink: Color32,
    pub(crate) accent: Color32,
}

/// One place a click can put the caret.
pub(crate) struct Hit {
    pub(crate) trail: Vec<Step>,
    pub(crate) index: usize,
    /// Horizontal position of the caret.
    pub(crate) x: f32,
    /// Vertical span of the caret line.
    pub(crate) top: f32,
    pub(crate) bottom: f32,
    /// Clicks inside this rectangle belong to the caret's row.
    pub(crate) row: Rect,
}

struct Metrics {
    width: f32,
    height: f32,
    ascent: f32,
}

struct Laid {
    metrics: Metrics,
    kind: Kind,
}

enum Kind {
    Empty,
    Row {
        children: Vec<Laid>,
        gaps: Vec<f32>,
    },
    Glyph(Arc<Galley>),
    /// A horizontal stroke, used for an overline the font does not contain.
    Rule,
    /// A sign painted with strokes because the font has no glyph for it.
    Drawn(&'static str),
    /// An accent whose width is the base it sits on.
    Accent(crate::icons::Accent),
    Space,
    Fraction {
        numerator: Box<Laid>,
        denominator: Box<Laid>,
    },
    Radical {
        degree: Option<Box<Laid>>,
        body: Box<Laid>,
    },
    Script {
        base: Box<Laid>,
        sub: Option<Box<Laid>>,
        sup: Option<Box<Laid>>,
    },
    Limits {
        base: Box<Laid>,
        under: Option<Box<Laid>>,
        over: Option<Box<Laid>>,
        body: Option<Box<Laid>>,
        pad: f32,
    },
    Fenced {
        open: FenceLay,
        close: FenceLay,
        body: Box<Laid>,
    },
    Matrix(MatrixLay),
}

struct MatrixLay {
    cells: Vec<Vec<Laid>>,
    columns: Vec<f32>,
    row_heights: Vec<f32>,
    row_ascents: Vec<f32>,
    open: Option<FenceLay>,
    close: Option<FenceLay>,
}

enum FenceLay {
    Glyph(Arc<Galley>),
    Stroke {
        kind: crate::icons::Bracket,
        left: bool,
        height: f32,
    },
}

impl FenceLay {
    fn width(&self) -> f32 {
        match self {
            Self::Glyph(galley) => galley.size().x,
            Self::Stroke { height, .. } => height * 0.34,
        }
    }

    fn height(&self) -> f32 {
        match self {
            Self::Glyph(galley) => galley.size().y,
            Self::Stroke { height, .. } => *height,
        }
    }
}

struct Draw<'a> {
    painter: &'a Painter,
    caret: &'a Caret,
    selection: Option<&'a Selection>,
    hits: &'a mut Vec<Hit>,
    ink: Color32,
    accent: Color32,
    placeholder: Color32,
    caret_on: bool,
}

/// The caret a click should move to.
///
/// The smallest slot rectangle that contains the pointer wins, and inside it
/// the nearest gap wins. The paper around the formula is the root slot, so a
/// click beside an expression leaves that expression. A point on no slot falls
/// back to the nearest caret line.
pub(crate) fn pick(hits: &[Hit], pos: Pos2) -> Option<Caret> {
    let mut best_area = f32::INFINITY;
    let mut inside = false;
    for hit in hits {
        if hit.row.contains(pos) {
            inside = true;
            best_area = best_area.min(hit.row.width() * hit.row.height());
        }
    }
    let hit = if inside {
        hits.iter()
            .filter(|hit| {
                hit.row.contains(pos) && hit.row.width() * hit.row.height() <= best_area + 0.5
            })
            .min_by(|left, right| (left.x - pos.x).abs().total_cmp(&(right.x - pos.x).abs()))
    } else {
        hits.iter()
            .min_by(|left, right| gap_distance(left, pos).total_cmp(&gap_distance(right, pos)))
    }?;
    Some(Caret {
        trail: hit.trail.clone(),
        index: hit.index,
    })
}

fn gap_distance(hit: &Hit, pos: Pos2) -> f32 {
    let dy = if pos.y < hit.top {
        hit.top - pos.y
    } else if pos.y > hit.bottom {
        pos.y - hit.bottom
    } else {
        0.0
    };
    let dx = hit.x - pos.x;
    dx * dx + dy * dy
}

pub(crate) fn measure(ui: &egui::Ui, row: &Row, look: &Look) -> Vec2 {
    let laid = lay_row(ui, look, row, look.size);
    vec2(laid.metrics.width, laid.metrics.height)
}

pub(crate) fn paint(
    ui: &egui::Ui,
    painter: &Painter,
    origin: Pos2,
    canvas: Rect,
    row: &Row,
    look: &Look,
    caret: &Caret,
    selection: Option<&Selection>,
    hits: &mut Vec<Hit>,
    caret_on: bool,
) -> Vec2 {
    let laid = lay_row(ui, look, row, look.size);
    let mut draw = Draw {
        painter,
        caret,
        selection,
        hits,
        ink: look.ink,
        accent: look.accent,
        placeholder: look.ink.gamma_multiply(0.35),
        caret_on,
    };
    // The whole paper belongs to the root row. Nested slots keep tighter rectangles.
    paint_row(&mut draw, origin, row, &laid, &[], Some(canvas));
    vec2(laid.metrics.width, laid.metrics.height)
}

fn lay_row(ui: &egui::Ui, look: &Look, row: &Row, size: f32) -> Laid {
    if row.nodes.is_empty() {
        return Laid {
            metrics: slot_metrics(size),
            kind: Kind::Empty,
        };
    }
    let children: Vec<Laid> = row
        .nodes
        .iter()
        .map(|node| lay_node(ui, look, node, size))
        .collect();
    let gaps: Vec<f32> = row
        .nodes
        .windows(2)
        .map(|pair| gap_between(&pair[0], &pair[1]))
        .collect();
    let ascent = children
        .iter()
        .map(|child| child.metrics.ascent)
        .fold(0.0_f32, f32::max);
    let descent = children
        .iter()
        .map(|child| child.metrics.height - child.metrics.ascent)
        .fold(0.0_f32, f32::max);
    let width = children
        .iter()
        .map(|child| child.metrics.width)
        .sum::<f32>()
        + gaps.iter().sum::<f32>();
    Laid {
        metrics: Metrics {
            width,
            height: (ascent + descent).max(size * 0.5),
            ascent,
        },
        kind: Kind::Row { children, gaps },
    }
}

fn lay_node(ui: &egui::Ui, look: &Look, node: &Node, size: f32) -> Laid {
    match node {
        Node::Token(token) => lay_token(ui, look, token, size),
        Node::Space => Laid {
            metrics: Metrics {
                width: size * 0.35,
                height: size,
                ascent: size * 0.8,
            },
            kind: Kind::Space,
        },
        Node::Fraction {
            numerator,
            denominator,
        } => lay_fraction(ui, look, numerator, denominator, size),
        Node::Radical { degree, radicand } => {
            lay_radical(ui, look, degree.as_ref(), radicand, size)
        }
        Node::Script { base, sub, sup } => {
            lay_script(ui, look, base, sub.as_ref(), sup.as_ref(), size)
        }
        Node::Limits {
            base,
            under,
            over,
            body,
        } => lay_limits(
            ui,
            look,
            base,
            under.as_ref(),
            over.as_ref(),
            body.as_ref(),
            size,
        ),
        Node::Fenced { open, close, body } => lay_fenced(ui, look, open, close, body, size),
        Node::Matrix { open, close, cells } => lay_matrix(ui, look, open, close, cells, size),
    }
}

fn lay_token(ui: &egui::Ui, look: &Look, token: &Token, size: f32) -> Laid {
    if overline(&token.text) {
        return Laid {
            metrics: Metrics {
                width: size * 0.85,
                height: (size * 0.22).max(4.0),
                ascent: size * 0.16,
            },
            kind: Kind::Rule,
        };
    }
    if let Some(sign) = missing_sign(ui, &token.text, size) {
        let box_side = size * 0.85;
        return Laid {
            metrics: Metrics {
                width: box_side,
                height: box_side,
                ascent: box_side,
            },
            kind: Kind::Drawn(sign),
        };
    }
    let italic =
        token.kind == TokenKind::Identifier && !token.upright && token.text.chars().count() == 1;
    let family = if italic {
        look.italic.clone()
    } else {
        FontFamily::Proportional
    };
    let galley = glyph(ui, &token.text, size, family, look.ink);
    Laid {
        metrics: glyph_metrics(&galley),
        kind: Kind::Glyph(galley),
    }
}

/// A palette sign the font cannot draw, so the formula paints it itself.
fn missing_sign(ui: &egui::Ui, text: &str, size: f32) -> Option<&'static str> {
    let sign = crate::icons::drawn(text)?;
    let font = FontId::new(size.max(8.0), FontFamily::Proportional);
    let present = ui.ctx().fonts_mut(|fonts| fonts.has_glyphs(&font, text));
    (!present).then_some(sign)
}

fn overline(text: &str) -> bool {
    let mut chars = text.chars();
    matches!(
        chars.next(),
        Some('\u{203E}' | '\u{00AF}' | '\u{02C9}' | '\u{0305}')
    ) && chars.next().is_none()
}

fn single_accent(row: &Row) -> Option<crate::icons::Accent> {
    match row.nodes.as_slice() {
        [Node::Token(token)] => crate::icons::accent_of(&token.text),
        _ => None,
    }
}

fn widen_accent(laid: &mut Laid, accent: crate::icons::Accent, width: f32, em: f32) {
    let height = if matches!(accent, crate::icons::Accent::Bar) {
        (em * 0.22).max(4.0)
    } else {
        em * 0.48
    };
    laid.metrics.width = width;
    laid.metrics.height = height;
    laid.metrics.ascent = height;
    let Kind::Row { children, .. } = &mut laid.kind else {
        return;
    };
    let Some(child) = children.first_mut() else {
        return;
    };
    child.metrics.width = width;
    child.metrics.height = height;
    child.metrics.ascent = height;
    child.kind = if matches!(accent, crate::icons::Accent::Bar) {
        Kind::Rule
    } else {
        Kind::Accent(accent)
    };
}

fn lay_fraction(ui: &egui::Ui, look: &Look, numerator: &Row, denominator: &Row, size: f32) -> Laid {
    let numerator = lay_row(ui, look, numerator, size);
    let denominator = lay_row(ui, look, denominator, size);
    let gap = size * 0.12;
    let width = numerator.metrics.width.max(denominator.metrics.width) + size * 0.35;
    let ascent = numerator.metrics.height + gap;
    let height = ascent + gap + denominator.metrics.height;
    Laid {
        metrics: Metrics {
            width,
            height,
            ascent,
        },
        kind: Kind::Fraction {
            numerator: Box::new(numerator),
            denominator: Box::new(denominator),
        },
    }
}

fn lay_radical(
    ui: &egui::Ui,
    look: &Look,
    degree: Option<&Row>,
    radicand: &Row,
    size: f32,
) -> Laid {
    let body = lay_row(ui, look, radicand, size);
    let degree = degree.map(|row| lay_row(ui, look, row, size * 0.55));
    let degree_width = degree.as_ref().map_or(0.0, |item| item.metrics.width);
    let sign = size * 0.55;
    let pad = size * 0.18;
    let width = degree_width + sign + body.metrics.width + pad;
    let ascent = body.metrics.ascent + pad;
    let height = body.metrics.height + pad;
    Laid {
        metrics: Metrics {
            width,
            height,
            ascent,
        },
        kind: Kind::Radical {
            degree: degree.map(Box::new),
            body: Box::new(body),
        },
    }
}

fn lay_script(
    ui: &egui::Ui,
    look: &Look,
    base: &Row,
    sub: Option<&Row>,
    sup: Option<&Row>,
    size: f32,
) -> Laid {
    let script = size * 0.62;
    let base = lay_row(ui, look, base, size);
    let sub = sub.map(|row| lay_row(ui, look, row, script));
    let sup = sup.map(|row| lay_row(ui, look, row, script));
    let up = sup.as_ref().map_or(0.0, |item| item.metrics.height * 0.55);
    let down = sub.as_ref().map_or(0.0, |item| item.metrics.height * 0.35);
    let scripts = sub
        .as_ref()
        .map_or(0.0, |item| item.metrics.width)
        .max(sup.as_ref().map_or(0.0, |item| item.metrics.width));
    Laid {
        metrics: Metrics {
            width: base.metrics.width + scripts + size * 0.06,
            height: base.metrics.height + up + down,
            ascent: base.metrics.ascent + up,
        },
        kind: Kind::Script {
            base: Box::new(base),
            sub: sub.map(Box::new),
            sup: sup.map(Box::new),
        },
    }
}

fn lay_limits(
    ui: &egui::Ui,
    look: &Look,
    base: &Row,
    under: Option<&Row>,
    over: Option<&Row>,
    body: Option<&Row>,
    size: f32,
) -> Laid {
    let base_size = if single_operator(base) {
        size * 1.7
    } else {
        size
    };
    let base = lay_row(ui, look, base, base_size);
    let under = under.map(|row| lay_row(ui, look, row, size * 0.62));
    let accent = over.and_then(single_accent);
    let over_size = if accent.is_some_and(crate::icons::stretches) {
        size
    } else {
        size * 0.62
    };
    let mut over = over.map(|row| lay_row(ui, look, row, over_size));
    if let Some(accent) = accent.filter(|item| crate::icons::stretches(*item)) {
        let width = base
            .metrics
            .width
            .max(over.as_ref().map_or(0.0, |item| item.metrics.width));
        if let Some(laid) = over.as_mut() {
            widen_accent(laid, accent, width, size);
        }
    }
    let gap = size * 0.06;
    let body = body.map(|row| lay_row(ui, look, row, size));
    let over_h = over.as_ref().map_or(0.0, |item| item.metrics.height + gap);
    let under_h = under.as_ref().map_or(0.0, |item| item.metrics.height + gap);
    let column = base
        .metrics
        .width
        .max(over.as_ref().map_or(0.0, |item| item.metrics.width))
        .max(under.as_ref().map_or(0.0, |item| item.metrics.width));
    let column_height = over_h + base.metrics.height + under_h;
    let column_ascent = over_h + base.metrics.ascent;
    let body_ascent = body
        .as_ref()
        .map_or(column_ascent, |item| item.metrics.ascent);
    let body_descent = body.as_ref().map_or(column_height - column_ascent, |item| {
        item.metrics.height - item.metrics.ascent
    });
    let rise = column_ascent.max(body_ascent);
    let descent = (column_height - column_ascent).max(body_descent);
    let body_gap = body.as_ref().map_or(0.0, |_| size * 0.22);
    let body_width = body.as_ref().map_or(0.0, |item| item.metrics.width);
    Laid {
        metrics: Metrics {
            width: column + body_gap + body_width,
            height: rise + descent,
            ascent: rise,
        },
        kind: Kind::Limits {
            base: Box::new(base),
            under: under.map(Box::new),
            over: over.map(Box::new),
            body: body.map(Box::new),
            pad: gap,
        },
    }
}

fn lay_fenced(ui: &egui::Ui, look: &Look, open: &str, close: &str, body: &Row, size: f32) -> Laid {
    let body = lay_row(ui, look, body, size);
    let fence = body.metrics.height.max(size);
    let open = lay_fence(ui, open, fence, look.ink);
    let close = lay_fence(ui, close, fence, look.ink);
    let extra = (open.height() - body.metrics.height).max(0.0);
    Laid {
        metrics: Metrics {
            width: open.width() + body.metrics.width + close.width() + size * 0.12,
            height: body.metrics.height + extra,
            ascent: body.metrics.ascent + extra * 0.5,
        },
        kind: Kind::Fenced {
            open,
            close,
            body: Box::new(body),
        },
    }
}

fn lay_matrix(
    ui: &egui::Ui,
    look: &Look,
    open: &str,
    close: &str,
    cells: &[Vec<Row>],
    size: f32,
) -> Laid {
    let cell_size = size * 0.92;
    let cells: Vec<Vec<Laid>> = cells
        .iter()
        .map(|row| {
            row.iter()
                .map(|cell| lay_row(ui, look, cell, cell_size))
                .collect()
        })
        .collect();
    let columns_n = cells.iter().map(Vec::len).max().unwrap_or(0);
    let pad = size * 0.28;
    let columns = (0..columns_n)
        .map(|column| {
            cells
                .iter()
                .filter_map(|row| row.get(column))
                .map(|cell| cell.metrics.width)
                .fold(0.0_f32, f32::max)
                + pad
        })
        .collect::<Vec<_>>();
    let row_heights: Vec<f32> = cells
        .iter()
        .map(|row| {
            row.iter()
                .map(|cell| cell.metrics.height)
                .fold(0.0_f32, f32::max)
                + pad * 0.45
        })
        .collect();
    let row_ascents: Vec<f32> = cells
        .iter()
        .map(|row| {
            row.iter()
                .map(|cell| cell.metrics.ascent)
                .fold(0.0_f32, f32::max)
        })
        .collect();
    let grid = Metrics {
        width: columns.iter().sum(),
        height: row_heights.iter().sum(),
        ascent: 0.0,
    };
    let (open, close) = if open.is_empty() && close.is_empty() {
        (None, None)
    } else {
        let fence = grid.height.max(size);
        (
            Some(lay_fence(ui, open, fence, look.ink)),
            Some(lay_fence(ui, close, fence, look.ink)),
        )
    };
    let fence_w =
        open.as_ref().map_or(0.0, FenceLay::width) + close.as_ref().map_or(0.0, FenceLay::width);
    let extra = open
        .as_ref()
        .map_or(0.0, |item| (item.height() - grid.height).max(0.0));
    let height = grid.height + extra;
    Laid {
        metrics: Metrics {
            width: grid.width + fence_w + size * 0.08,
            height,
            ascent: height * 0.55,
        },
        kind: Kind::Matrix(MatrixLay {
            cells,
            columns,
            row_heights,
            row_ascents,
            open,
            close,
        }),
    }
}

fn paint_row(
    draw: &mut Draw<'_>,
    origin: Pos2,
    row: &Row,
    laid: &Laid,
    trail: &[Step],
    slot: Option<Rect>,
) {
    let ink = Rect::from_min_size(origin, vec2(laid.metrics.width, laid.metrics.height));
    let row_rect = slot.unwrap_or(ink);
    let Kind::Row { children, gaps } = &laid.kind else {
        let active = draw.caret.trail == trail;
        let color = if active {
            draw.accent
        } else {
            draw.placeholder
        };
        if active {
            draw.painter
                .rect_filled(ink, 2.0, draw.accent.gamma_multiply(0.16));
        }
        draw.painter
            .rect_stroke(ink, 2.0, Stroke::new(1.0, color), egui::StrokeKind::Inside);
        draw.mark(
            trail,
            0,
            origin.x + laid.metrics.width * 0.5,
            ink.top(),
            ink.bottom(),
            row_rect,
        );
        return;
    };
    let mut places = Vec::with_capacity(children.len());
    let mut x = origin.x;
    for (index, child) in children.iter().enumerate() {
        places.push(x);
        x += child.metrics.width;
        if let Some(gap) = gaps.get(index) {
            x += gap;
        }
    }
    places.push(x);
    if let Some(selection) = draw.selection {
        if selection.trail == trail
            && selection.start < selection.end
            && selection.end < places.len()
        {
            let rect = Rect::from_min_max(
                pos2(places[selection.start], origin.y),
                pos2(places[selection.end], origin.y + laid.metrics.height),
            );
            draw.painter
                .rect_filled(rect, 2.0, draw.accent.gamma_multiply(0.28));
        }
    }
    for (index, child) in children.iter().enumerate() {
        let x = places[index];
        draw.mark(
            trail,
            index,
            x,
            origin.y,
            origin.y + laid.metrics.height,
            row_rect,
        );
        let y = origin.y + laid.metrics.ascent - child.metrics.ascent;
        paint_node(draw, pos2(x, y), &row.nodes[index], child, trail, index);
    }
    draw.mark(
        trail,
        children.len(),
        places[children.len()],
        origin.y,
        origin.y + laid.metrics.height,
        row_rect,
    );
}

fn paint_node(
    draw: &mut Draw<'_>,
    origin: Pos2,
    node: &Node,
    laid: &Laid,
    trail: &[Step],
    index: usize,
) {
    match (node, &laid.kind) {
        (Node::Token(_), _) => paint_token(draw, origin, laid),
        (
            Node::Fenced { body, .. },
            Kind::Fenced {
                open,
                close,
                body: laid_body,
            },
        ) => {
            paint_fenced(
                draw, origin, laid, body, open, close, laid_body, trail, index,
            );
        }
        (
            Node::Fraction {
                numerator,
                denominator,
            },
            Kind::Fraction {
                numerator: num,
                denominator: den,
            },
        ) => {
            paint_fraction(
                draw,
                origin,
                laid,
                numerator,
                denominator,
                num,
                den,
                trail,
                index,
            );
        }
        (
            Node::Radical { degree, radicand },
            Kind::Radical {
                degree: laid_degree,
                body,
            },
        ) => {
            paint_radical(
                draw,
                origin,
                laid,
                degree.as_ref(),
                radicand,
                laid_degree.as_deref(),
                body,
                trail,
                index,
            );
        }
        (
            Node::Script { base, sub, sup },
            Kind::Script {
                base: laid_base,
                sub: lower,
                sup: upper,
            },
        ) => paint_script(
            draw,
            origin,
            laid,
            base,
            sub.as_ref(),
            sup.as_ref(),
            laid_base,
            lower.as_deref(),
            upper.as_deref(),
            trail,
            index,
        ),
        (Node::Limits { .. }, Kind::Limits { .. }) => {
            paint_limit_node(draw, origin, node, laid, trail, index);
        }
        (Node::Matrix { cells, .. }, Kind::Matrix(matrix)) => {
            paint_matrix(draw, origin, laid, cells, matrix, trail, index);
        }
        _ => {}
    }
}

fn paint_token(draw: &Draw<'_>, origin: Pos2, laid: &Laid) {
    match &laid.kind {
        Kind::Glyph(galley) => blit(draw, origin, galley),
        Kind::Rule => {
            let y = origin.y + laid.metrics.height - 1.2;
            draw.painter.hline(
                origin.x..=(origin.x + laid.metrics.width.max(1.0)),
                y,
                Stroke::new(1.6, draw.ink),
            );
        }
        Kind::Drawn(sign) => {
            let rect = Rect::from_min_size(origin, vec2(laid.metrics.width, laid.metrics.height));
            crate::icons::symbol(draw.painter, rect, sign, draw.ink);
        }
        Kind::Accent(kind) => {
            let rect = Rect::from_min_size(origin, vec2(laid.metrics.width, laid.metrics.height));
            crate::icons::paint_accent(draw.painter, rect, *kind, draw.ink);
        }
        _ => {}
    }
}

fn blit(draw: &Draw<'_>, origin: Pos2, galley: &Arc<Galley>) {
    draw.painter
        .galley(origin, Arc::clone(galley), Color32::TRANSPARENT);
}

fn lay_fence(ui: &egui::Ui, text: &str, height: f32, ink: Color32) -> FenceLay {
    if let Some((kind, left)) = crate::icons::fence_side(text) {
        let font = FontId::new(height.max(8.0), FontFamily::Proportional);
        let present = ui.ctx().fonts_mut(|fonts| fonts.has_glyphs(&font, text));
        if !present {
            return FenceLay::Stroke { kind, left, height };
        }
    }
    FenceLay::Glyph(glyph(ui, text, height, FontFamily::Proportional, ink))
}

fn paint_fence(draw: &Draw<'_>, origin: Pos2, fence: &FenceLay) {
    match fence {
        FenceLay::Glyph(galley) => blit(draw, origin, galley),
        FenceLay::Stroke { kind, left, height } => {
            let rect = Rect::from_min_size(origin, vec2(*height * 0.34, *height));
            crate::icons::paint_side(draw.painter, rect, *kind, *left, draw.ink);
        }
    }
}

fn paint_fenced(
    draw: &mut Draw<'_>,
    origin: Pos2,
    laid: &Laid,
    body: &Row,
    open: &FenceLay,
    close: &FenceLay,
    laid_body: &Laid,
    trail: &[Step],
    index: usize,
) {
    let body_y = origin.y + laid.metrics.ascent - laid_body.metrics.ascent;
    let body_x = origin.x + open.width() + 2.0;
    let open_y = body_y + laid_body.metrics.height * 0.5 - open.height() * 0.5;
    paint_fence(draw, pos2(origin.x, open_y), open);
    let close_x = body_x + laid_body.metrics.width + 2.0;
    let body_slot = Rect::from_min_max(
        pos2(body_x, origin.y),
        pos2(close_x, origin.y + laid.metrics.height),
    );
    paint_row(
        draw,
        pos2(body_x, body_y),
        body,
        laid_body,
        &child(trail, index, Slot::Body),
        Some(body_slot),
    );
    let close_y = body_y + laid_body.metrics.height * 0.5 - close.height() * 0.5;
    paint_fence(draw, pos2(close_x, close_y), close);
}

fn paint_fraction(
    draw: &mut Draw<'_>,
    origin: Pos2,
    laid: &Laid,
    numerator: &Row,
    denominator: &Row,
    num: &Laid,
    den: &Laid,
    trail: &[Step],
    index: usize,
) {
    let width = laid.metrics.width;
    let right = origin.x + width;
    let bottom = origin.y + laid.metrics.height;
    let num_x = origin.x + (width - num.metrics.width) * 0.5;
    let den_x = origin.x + (width - den.metrics.width) * 0.5;
    let bar = origin.y + num.metrics.height + (laid.metrics.ascent - num.metrics.height) * 0.5;
    paint_row(
        draw,
        pos2(num_x, origin.y),
        numerator,
        num,
        &child(trail, index, Slot::Numerator),
        Some(Rect::from_min_max(
            pos2(origin.x, origin.y),
            pos2(right, bar),
        )),
    );
    draw.painter.hline(
        (origin.x + 1.0)..=(right - 1.0),
        bar,
        Stroke::new(1.4, draw.ink),
    );
    let den_y = bottom - den.metrics.height;
    paint_row(
        draw,
        pos2(den_x, den_y),
        denominator,
        den,
        &child(trail, index, Slot::Denominator),
        Some(Rect::from_min_max(pos2(origin.x, bar), pos2(right, bottom))),
    );
}

fn paint_radical(
    draw: &mut Draw<'_>,
    origin: Pos2,
    laid: &Laid,
    degree: Option<&Row>,
    radicand: &Row,
    laid_degree: Option<&Laid>,
    body: &Laid,
    trail: &[Step],
    index: usize,
) {
    let degree_width = laid_degree.map_or(0.0, |item| item.metrics.width);
    if let (Some(degree), Some(laid_degree)) = (degree, laid_degree) {
        let degree_slot = Rect::from_min_size(
            pos2(origin.x, origin.y),
            vec2(laid_degree.metrics.width, laid_degree.metrics.height),
        );
        paint_row(
            draw,
            pos2(origin.x, origin.y),
            degree,
            laid_degree,
            &child(trail, index, Slot::Degree),
            Some(degree_slot),
        );
    }
    let top = origin.y + laid.metrics.ascent - body.metrics.ascent - 3.0;
    let body_x = origin.x + degree_width + laid.metrics.height * 0.28;
    let body_y = origin.y + laid.metrics.ascent - body.metrics.ascent;
    let stroke = Stroke::new(1.3, draw.ink);
    let hook = pos2(
        origin.x + degree_width + 1.0,
        body_y + body.metrics.height * 0.55,
    );
    let knee = pos2(
        origin.x + degree_width + laid.metrics.height * 0.12,
        body_y + body.metrics.height * 0.92,
    );
    let rise = pos2(body_x - 2.0, top);
    let end = pos2(body_x + body.metrics.width + 3.0, top);
    draw.painter
        .add(Shape::line(vec![hook, knee, rise, end], stroke));
    let body_slot = Rect::from_min_max(
        pos2(origin.x, body_y),
        pos2(origin.x + laid.metrics.width, body_y + body.metrics.height),
    );
    paint_row(
        draw,
        pos2(body_x, body_y),
        radicand,
        body,
        &child(trail, index, Slot::Radicand),
        Some(body_slot),
    );
}

fn paint_script(
    draw: &mut Draw<'_>,
    origin: Pos2,
    laid: &Laid,
    base: &Row,
    sub: Option<&Row>,
    sup: Option<&Row>,
    laid_base: &Laid,
    lower: Option<&Laid>,
    upper: Option<&Laid>,
    trail: &[Step],
    index: usize,
) {
    let base_pos = pos2(
        origin.x,
        origin.y + laid.metrics.ascent - laid_base.metrics.ascent,
    );
    let right = origin.x + laid.metrics.width;
    let side = origin.x + laid_base.metrics.width + 1.0;
    let base_slot = Rect::from_min_max(
        pos2(origin.x, base_pos.y),
        pos2(side, base_pos.y + laid_base.metrics.height),
    );
    paint_row(
        draw,
        base_pos,
        base,
        laid_base,
        &child(trail, index, Slot::Base),
        Some(base_slot),
    );
    if let (Some(sup), Some(upper)) = (sup, upper) {
        let sup_bottom = origin.y + upper.metrics.height;
        paint_row(
            draw,
            pos2(side, origin.y),
            sup,
            upper,
            &child(trail, index, Slot::Superscript),
            Some(Rect::from_min_max(
                pos2(side, origin.y),
                pos2(right, sup_bottom),
            )),
        );
    }
    if let (Some(sub), Some(lower)) = (sub, lower) {
        let y = origin.y + laid.metrics.height - lower.metrics.height;
        paint_row(
            draw,
            pos2(side, y),
            sub,
            lower,
            &child(trail, index, Slot::Subscript),
            Some(Rect::from_min_max(
                pos2(side, y),
                pos2(right, y + lower.metrics.height),
            )),
        );
    }
}

fn paint_limit_node(
    draw: &mut Draw<'_>,
    origin: Pos2,
    node: &Node,
    laid: &Laid,
    trail: &[Step],
    index: usize,
) {
    let (
        Node::Limits {
            base,
            under,
            over,
            body,
        },
        Kind::Limits {
            base: laid_base,
            under: laid_under,
            over: laid_over,
            body: laid_body,
            pad,
        },
    ) = (node, &laid.kind)
    else {
        return;
    };
    paint_limits(
        draw,
        origin,
        laid,
        base,
        under.as_ref(),
        over.as_ref(),
        body.as_ref(),
        laid_base,
        laid_under.as_deref(),
        laid_over.as_deref(),
        laid_body.as_deref(),
        *pad,
        trail,
        index,
    );
}

fn paint_limits(
    draw: &mut Draw<'_>,
    origin: Pos2,
    laid: &Laid,
    base: &Row,
    under: Option<&Row>,
    over: Option<&Row>,
    body: Option<&Row>,
    laid_base: &Laid,
    laid_under: Option<&Laid>,
    laid_over: Option<&Laid>,
    laid_body: Option<&Laid>,
    pad: f32,
    trail: &[Step],
    index: usize,
) {
    let column = laid_base
        .metrics
        .width
        .max(laid_over.map_or(0.0, |item| item.metrics.width))
        .max(laid_under.map_or(0.0, |item| item.metrics.width));
    let column_right = origin.x + column;
    let bottom = origin.y + laid.metrics.height;
    let base_x = origin.x + (column - laid_base.metrics.width) * 0.5;
    let base_y = origin.y + laid.metrics.ascent - laid_base.metrics.ascent;
    let base_bottom = base_y + laid_base.metrics.height;
    if let (Some(over), Some(laid_over)) = (over, laid_over) {
        let x = origin.x + (column - laid_over.metrics.width) * 0.5;
        let over_top = base_y - pad - laid_over.metrics.height;
        paint_row(
            draw,
            pos2(x, over_top),
            over,
            laid_over,
            &child(trail, index, Slot::Over),
            Some(Rect::from_min_max(
                pos2(origin.x, over_top.min(origin.y)),
                pos2(column_right, base_y),
            )),
        );
    }
    let base_slot_bottom = if under.is_some() { base_bottom } else { bottom };
    paint_row(
        draw,
        pos2(base_x, base_y),
        base,
        laid_base,
        &child(trail, index, Slot::Base),
        Some(Rect::from_min_max(
            pos2(origin.x, base_y),
            pos2(column_right, base_slot_bottom),
        )),
    );
    if let (Some(under), Some(laid_under)) = (under, laid_under) {
        let x = origin.x + (column - laid_under.metrics.width) * 0.5;
        let under_y = base_bottom + pad;
        paint_row(
            draw,
            pos2(x, under_y),
            under,
            laid_under,
            &child(trail, index, Slot::Under),
            Some(Rect::from_min_max(
                pos2(origin.x, base_bottom),
                pos2(column_right, bottom),
            )),
        );
    }
    if let (Some(body), Some(laid_body)) = (body, laid_body) {
        let body_x = column_right + (laid.metrics.width - column - laid_body.metrics.width);
        let body_y = origin.y + laid.metrics.ascent - laid_body.metrics.ascent;
        paint_row(
            draw,
            pos2(body_x, body_y),
            body,
            laid_body,
            &child(trail, index, Slot::Body),
            Some(Rect::from_min_max(
                pos2(column_right, origin.y),
                pos2(origin.x + laid.metrics.width, bottom),
            )),
        );
    }
}

fn paint_matrix(
    draw: &mut Draw<'_>,
    origin: Pos2,
    laid: &Laid,
    cells: &[Vec<Row>],
    matrix: &MatrixLay,
    trail: &[Step],
    index: usize,
) {
    let open_w = matrix.open.as_ref().map_or(0.0, FenceLay::width);
    if let Some(open) = &matrix.open {
        let y = origin.y + (laid.metrics.height - open.height()) * 0.5;
        paint_fence(draw, pos2(origin.x, y), open);
    }
    let mut y = origin.y + (laid.metrics.height - matrix.row_heights.iter().sum::<f32>()) * 0.5;
    for (row_index, row) in cells.iter().enumerate() {
        let mut x = origin.x + open_w;
        for (column, cell) in row.iter().enumerate() {
            let Some(laid_cell) = matrix
                .cells
                .get(row_index)
                .and_then(|cells| cells.get(column))
            else {
                continue;
            };
            let width = matrix
                .columns
                .get(column)
                .copied()
                .unwrap_or(laid_cell.metrics.width);
            let cell_x = x + (width - laid_cell.metrics.width) * 0.5;
            let ascent = matrix
                .row_ascents
                .get(row_index)
                .copied()
                .unwrap_or(laid_cell.metrics.ascent);
            let cell_y = y + (ascent - laid_cell.metrics.ascent).max(0.0);
            let cell_trail = child(trail, index, Slot::Cell(row_index, column));
            let row_height = matrix.row_heights.get(row_index).copied().unwrap_or(0.0);
            // The cell owns its whole grid rectangle, so a neighbour's ink cannot steal the click.
            let cell_rect = Rect::from_min_size(pos2(x, y), vec2(width, row_height));
            paint_row(
                draw,
                pos2(cell_x, cell_y),
                cell,
                laid_cell,
                &cell_trail,
                Some(cell_rect),
            );
            x += width;
        }
        y += matrix.row_heights.get(row_index).copied().unwrap_or(0.0);
    }
    if let Some(close) = &matrix.close {
        let x = origin.x + laid.metrics.width - close.width();
        let y = origin.y + (laid.metrics.height - close.height()) * 0.5;
        paint_fence(draw, pos2(x, y), close);
    }
}

impl Draw<'_> {
    fn mark(&mut self, trail: &[Step], index: usize, x: f32, top: f32, bottom: f32, row: Rect) {
        if self.caret_on && self.caret.trail == trail && self.caret.index == index {
            self.painter.line_segment(
                [pos2(x, top + 1.0), pos2(x, bottom - 1.0)],
                Stroke::new(1.6, self.accent),
            );
        }
        self.hits.push(Hit {
            trail: trail.to_vec(),
            index,
            x,
            top,
            bottom,
            row,
        });
    }
}

fn child(trail: &[Step], index: usize, slot: Slot) -> Vec<Step> {
    let mut trail = trail.to_vec();
    trail.push(Step { index, slot });
    trail
}

fn glyph(ui: &egui::Ui, text: &str, size: f32, family: FontFamily, color: Color32) -> Arc<Galley> {
    ui.painter()
        .layout_no_wrap(text.to_owned(), FontId::new(size.max(8.0), family), color)
}

fn glyph_metrics(galley: &Galley) -> Metrics {
    let size = galley.size();
    Metrics {
        width: size.x.max(4.0),
        height: size.y,
        ascent: size.y * 0.8,
    }
}

fn slot_metrics(size: f32) -> Metrics {
    Metrics {
        width: size * 0.55,
        height: size,
        ascent: size * 0.8,
    }
}

fn gap_between(left: &Node, right: &Node) -> f32 {
    if matches!(left, Node::Space) || matches!(right, Node::Space) {
        return 0.0;
    }
    let wide =
        |node: &Node| matches!(node, Node::Token(token) if token.kind == TokenKind::Operator);
    if wide(left) || wide(right) {
        4.0
    } else {
        1.5
    }
}

fn single_operator(row: &Row) -> bool {
    matches!(
        row.nodes.as_slice(),
        [Node::Token(token)] if token.kind == TokenKind::Operator && token.text.chars().count() == 1
    )
}

#[cfg(test)]
mod tests {
    use egui::{Color32, FontFamily, Pos2, Rect};

    use super::{paint, pick, Hit, Look};
    use crate::model::{Caret, Node, Row, Slot, TokenKind};

    fn hits_of(row: &Row) -> Vec<Hit> {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(500.0, 400.0))),
            ..Default::default()
        };
        let mut hits = Vec::new();
        let _ = ctx.run_ui(input, |ui| {
            let look = Look {
                italic: FontFamily::Proportional,
                size: 28.0,
                ink: Color32::BLACK,
                accent: Color32::BLUE,
            };
            paint(
                ui,
                ui.painter(),
                Pos2::new(20.0, 20.0),
                Rect::from_min_size(Pos2::ZERO, egui::vec2(500.0, 400.0)),
                row,
                &look,
                &Caret::default(),
                None,
                &mut hits,
                false,
            );
        });
        hits
    }

    fn slot_at(hits: &[Hit], pos: Pos2) -> Option<Slot> {
        pick(hits, pos).and_then(|caret| caret.trail.last().map(|step| step.slot))
    }

    #[test]
    fn a_click_in_the_lower_matrix_row_lands_there() {
        let hits = hits_of(&matrix());
        let bottom = hits
            .iter()
            .find(|hit| {
                matches!(
                    hit.trail.last().map(|step| step.slot),
                    Some(Slot::Cell(1, 0))
                )
            })
            .expect("the lower row has a hit");
        let caret = slot_at(&hits, bottom.row.center());
        assert!(matches!(caret, Some(Slot::Cell(1, _))), "{caret:?}");
    }

    #[test]
    fn a_click_in_the_second_column_lands_there_after_the_first_is_filled() {
        let mut row = matrix();
        let Node::Matrix { cells, .. } = &mut row.nodes[0] else {
            panic!("matrix");
        };
        cells[0][0] = Row::token(TokenKind::Identifier, "a", false);
        let hits = hits_of(&row);
        let second = hits
            .iter()
            .find(|hit| {
                matches!(
                    hit.trail.last().map(|step| step.slot),
                    Some(Slot::Cell(0, 1))
                )
            })
            .expect("the second column has a hit");
        assert_eq!(slot_at(&hits, second.row.center()), Some(Slot::Cell(0, 1)));
        let first = hits
            .iter()
            .find(|hit| {
                matches!(
                    hit.trail.last().map(|step| step.slot),
                    Some(Slot::Cell(0, 0))
                )
            })
            .expect("the first column has a hit");
        assert_eq!(slot_at(&hits, first.row.center()), Some(Slot::Cell(0, 0)));
    }

    #[test]
    fn a_click_beside_a_fraction_leaves_it() {
        let row = Row {
            nodes: vec![Node::Fraction {
                numerator: Row::token(TokenKind::Identifier, "a", false),
                denominator: Row::token(TokenKind::Identifier, "b", false),
            }],
        };
        let hits = hits_of(&row);
        let caret = pick(&hits, Pos2::new(420.0, 80.0)).expect("a caret");
        assert!(caret.trail.is_empty(), "{caret:?}");
        assert_eq!(caret.index, 1);
        let numerator = hits
            .iter()
            .find(|hit| {
                matches!(
                    hit.trail.last().map(|step| step.slot),
                    Some(Slot::Numerator)
                )
            })
            .expect("numerator");
        assert_eq!(
            slot_at(&hits, numerator.row.center()),
            Some(Slot::Numerator)
        );
    }

    #[test]
    fn a_sign_missing_from_the_font_is_drawn() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(240.0, 160.0))),
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            let look = Look {
                italic: FontFamily::Proportional,
                size: 28.0,
                ink: Color32::BLACK,
                accent: Color32::BLUE,
            };
            let row = Row {
                nodes: vec![Node::token(TokenKind::Operator, "∈", true)],
            };
            let font = egui::FontId::proportional(28.0);
            let missing = ui.ctx().fonts_mut(|fonts| !fonts.has_glyphs(&font, "∈"));
            let size = super::measure(ui, &row, &look);
            if missing {
                assert!(
                    (size.x - 28.0 * 0.85).abs() < 0.5,
                    "the stroke box is {size:?}"
                );
            }
            let mut hits = Vec::new();
            super::paint(
                ui,
                ui.painter(),
                Pos2::new(12.0, 12.0),
                Rect::from_min_size(Pos2::ZERO, egui::vec2(240.0, 160.0)),
                &row,
                &look,
                &Caret::default(),
                None,
                &mut hits,
                false,
            );
            assert!(hits.iter().any(|hit| hit.trail.is_empty()));
        });
    }

    #[test]
    fn a_hat_spans_its_base() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 200.0))),
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            let look = Look {
                italic: FontFamily::Proportional,
                size: 28.0,
                ink: Color32::BLACK,
                accent: Color32::BLUE,
            };
            let node = Node::Limits {
                base: Row {
                    nodes: vec![
                        Node::token(TokenKind::Identifier, "a", false),
                        Node::token(TokenKind::Operator, "+", true),
                        Node::token(TokenKind::Identifier, "b", false),
                    ],
                },
                under: None,
                over: Some(Row::token(TokenKind::Operator, "ˆ", true)),
                body: None,
            };
            let laid = super::lay_node(ui, &look, &node, 28.0);
            let super::Kind::Limits { base, over, .. } = &laid.kind else {
                panic!("expected limits");
            };
            let over = over.as_ref().expect("the hat");
            assert!(
                over.metrics.width + 0.5 >= base.metrics.width,
                "hat {} base {}",
                over.metrics.width,
                base.metrics.width
            );
        });
    }

    fn matrix() -> Row {
        Row {
            nodes: vec![Node::Matrix {
                open: "(".to_owned(),
                close: ")".to_owned(),
                cells: vec![
                    vec![Row::default(), Row::default()],
                    vec![Row::default(), Row::default()],
                ],
            }],
        }
    }
}
