//! Vector icons drawn with the painter, so they look the same on every
//! platform and in both themes and need no image files.

use eframe::egui::{self, pos2, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke};

/// Every icon the ribbon and the start page use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Icon {
    New,
    Open,
    Save,
    SaveAs,
    Pdf,
    Svg,
    Txt,
    Import,
    Scan,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    AlignLeft,
    AlignCenter,
    Bullets,
    Numbering,
    Picture,
    Print,
    Header,
    Footer,
    PageNumber,
    Crop,
    Rotate,
    Brightness,
    Contrast,
    Wrap,
    Formula,
    Delete,
    OriginalSize,
    AlignRight,
    Justify,
    PageBreak,
    Table,
    InsertFile,
    Date,
    Symbol,
    Find,
    Replace,
    ZoomIn,
    ZoomOut,
    OnePage,
    PageWidth,
    Accept,
    Reject,
    Report,
    Navigation,
    Theme,
    Language,
    Settings,
    About,
    Close,
    Portrait,
    Landscape,
    Margins,
    PageSize,
    SectionBreak,
    Clear,
    SelectAll,
    Words,
}

fn stroke(color: Color32, width: f32) -> Stroke {
    Stroke::new(width, color)
}

/// A page outline with a folded corner, inside `rect`.
fn page(painter: &Painter, rect: Rect, color: Color32, fill: Color32) {
    let w = rect.width();
    let fold = w * 0.28;
    let points = vec![
        rect.left_top(),
        pos2(rect.right() - fold, rect.top()),
        pos2(rect.right(), rect.top() + fold),
        rect.right_bottom(),
        rect.left_bottom(),
    ];
    painter.add(Shape::convex_polygon(points, fill, stroke(color, 1.2)));
    painter.line_segment(
        [
            pos2(rect.right() - fold, rect.top()),
            pos2(rect.right() - fold, rect.top() + fold),
        ],
        stroke(color, 1.0),
    );
    painter.line_segment(
        [
            pos2(rect.right() - fold, rect.top() + fold),
            pos2(rect.right(), rect.top() + fold),
        ],
        stroke(color, 1.0),
    );
}

/// Text lines inside `rect`, `widths` as fractions of its width.
fn lines(painter: &Painter, rect: Rect, widths: &[(f32, f32)], color: Color32, width: f32) {
    let count = widths.len().max(1);
    #[allow(clippy::cast_precision_loss)]
    let step = rect.height() / count as f32;
    for (index, (start, end)) in widths.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let y = rect.top() + step * (index as f32 + 0.5);
        painter.line_segment(
            [
                pos2(rect.left() + rect.width() * start, y),
                pos2(rect.left() + rect.width() * end, y),
            ],
            stroke(color, width),
        );
    }
}

fn badge(painter: &Painter, rect: Rect, text: &str, fill: Color32) {
    let band = Rect::from_min_size(
        pos2(rect.left() - rect.width() * 0.15, rect.center().y),
        vec2(rect.width() * 0.9, rect.height() * 0.38),
    );
    painter.rect_filled(band, 1.5, fill);
    painter.text(
        band.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(band.height() * 0.78),
        Color32::WHITE,
    );
}

fn arrow_head(painter: &Painter, tip: Pos2, direction: egui::Vec2, size: f32, color: Color32) {
    let d = direction.normalized();
    let n = vec2(-d.y, d.x);
    let back = tip - d * size;
    painter.add(Shape::convex_polygon(
        vec![tip, back + n * size * 0.6, back - n * size * 0.6],
        color,
        Stroke::NONE,
    ));
}

/// Paints `icon` in `rect` with `color`; `accent` marks the parts Word draws
/// in colour (the blue of a save disk, the red of a PDF).
#[allow(clippy::too_many_lines)]
pub(crate) fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32, accent: Color32) {
    let r = rect.shrink(rect.width() * 0.08);
    let paper = if color.r() > 128 {
        Color32::from_gray(40)
    } else {
        Color32::WHITE
    };
    let thin = (r.width() / 16.0).max(1.0);
    let page_rect = Rect::from_center_size(r.center(), vec2(r.width() * 0.68, r.height() * 0.9));
    match icon {
        Icon::New => {
            page(painter, page_rect, color, paper);
            let c = pos2(page_rect.right(), page_rect.bottom());
            let s = r.width() * 0.2;
            painter.circle_filled(c, s, accent);
            painter.line_segment(
                [c - vec2(s * 0.55, 0.0), c + vec2(s * 0.55, 0.0)],
                stroke(Color32::WHITE, thin * 1.3),
            );
            painter.line_segment(
                [c - vec2(0.0, s * 0.55), c + vec2(0.0, s * 0.55)],
                stroke(Color32::WHITE, thin * 1.3),
            );
        }
        Icon::Open => {
            let back = Rect::from_min_max(
                pos2(r.left(), r.top() + r.height() * 0.2),
                pos2(r.right() - r.width() * 0.1, r.bottom() - r.height() * 0.1),
            );
            painter.rect_filled(back, 2.0, Color32::from_rgb(232, 180, 60));
            let front = vec![
                pos2(r.left() + r.width() * 0.12, r.top() + r.height() * 0.42),
                pos2(r.right(), r.top() + r.height() * 0.42),
                pos2(r.right() - r.width() * 0.1, r.bottom() - r.height() * 0.1),
                pos2(r.left(), r.bottom() - r.height() * 0.1),
            ];
            painter.add(Shape::convex_polygon(
                front,
                Color32::from_rgb(250, 205, 90),
                stroke(Color32::from_rgb(190, 140, 30), thin),
            ));
        }
        Icon::Save | Icon::SaveAs => {
            let body = r.shrink(r.width() * 0.08);
            painter.rect_filled(body, 2.0, accent);
            let label = Rect::from_min_max(
                pos2(body.left() + body.width() * 0.2, body.top()),
                pos2(
                    body.right() - body.width() * 0.2,
                    body.top() + body.height() * 0.36,
                ),
            );
            painter.rect_filled(label, 0.0, Color32::from_gray(235));
            let slot = Rect::from_min_max(
                pos2(
                    body.left() + body.width() * 0.25,
                    body.bottom() - body.height() * 0.38,
                ),
                pos2(body.right() - body.width() * 0.25, body.bottom()),
            );
            painter.rect_filled(slot, 1.0, Color32::from_gray(250));
            if icon == Icon::SaveAs {
                let a = pos2(r.right(), r.top() + r.height() * 0.45);
                let b = pos2(r.left() + r.width() * 0.55, r.bottom());
                painter.line_segment([a, b], stroke(Color32::from_rgb(230, 120, 30), thin * 2.2));
            }
        }
        Icon::Pdf | Icon::Svg | Icon::Txt => {
            page(painter, page_rect, color, paper);
            let (text, fill) = match icon {
                Icon::Pdf => ("PDF", Color32::from_rgb(200, 40, 40)),
                Icon::Svg => ("SVG", Color32::from_rgb(230, 130, 20)),
                _ => ("TXT", Color32::from_gray(110)),
            };
            badge(painter, page_rect, text, fill);
        }
        Icon::Import => {
            page(
                painter,
                page_rect.translate(vec2(r.width() * 0.12, 0.0)),
                color,
                paper,
            );
            let y = r.center().y;
            let a = pos2(r.left(), y);
            let b = pos2(r.center().x + r.width() * 0.1, y);
            painter.line_segment([a, b], stroke(accent, thin * 2.0));
            arrow_head(
                painter,
                b + vec2(r.width() * 0.12, 0.0),
                vec2(1.0, 0.0),
                r.width() * 0.2,
                accent,
            );
        }
        Icon::Scan => {
            let c = r.shrink(r.width() * 0.1);
            let l = c.width() * 0.3;
            for (corner, dx, dy) in [
                (c.left_top(), 1.0, 1.0),
                (c.right_top(), -1.0, 1.0),
                (c.left_bottom(), 1.0, -1.0),
                (c.right_bottom(), -1.0, -1.0),
            ] {
                painter.line_segment(
                    [corner, corner + vec2(l * dx, 0.0)],
                    stroke(color, thin * 1.6),
                );
                painter.line_segment(
                    [corner, corner + vec2(0.0, l * dy)],
                    stroke(color, thin * 1.6),
                );
            }
            painter.text(
                c.center(),
                egui::Align2::CENTER_CENTER,
                "Aa",
                egui::FontId::proportional(c.height() * 0.5),
                accent,
            );
            painter.line_segment(
                [
                    pos2(c.left() - 1.0, c.center().y + c.height() * 0.32),
                    pos2(c.right() + 1.0, c.center().y + c.height() * 0.32),
                ],
                stroke(Color32::from_rgb(220, 50, 50), thin),
            );
        }
        Icon::Undo | Icon::Redo => {
            // A hook: a line back across the top, curling down, the arrow
            // head at the free end pointing the way it goes.
            let flip = if icon == Icon::Undo { 1.0 } else { -1.0 };
            let c = r.center() + vec2(flip * r.width() * 0.08, r.height() * 0.05);
            let radius = r.height() * 0.3;
            let mut points = Vec::new();
            // From the bottom of the curl, round the right side, to the top.
            for step in 0..=12 {
                #[allow(clippy::cast_precision_loss)]
                let t = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * step as f32 / 12.0;
                points.push(c + vec2(flip * t.cos() * radius, -t.sin() * radius));
            }
            let top = c + vec2(0.0, -radius);
            let tip = top + vec2(-flip * r.width() * 0.34, 0.0);
            points.reverse();
            points.insert(0, tip);
            painter.add(Shape::line(points, stroke(accent, thin * 2.2)));
            arrow_head(
                painter,
                tip - vec2(flip * r.width() * 0.08, 0.0),
                vec2(-flip, 0.0),
                r.width() * 0.24,
                accent,
            );
        }
        Icon::Cut => {
            let a = pos2(r.left() + r.width() * 0.3, r.bottom() - r.height() * 0.2);
            let b = pos2(r.right() - r.width() * 0.3, r.bottom() - r.height() * 0.2);
            painter.circle_stroke(a, r.width() * 0.14, stroke(accent, thin * 1.4));
            painter.circle_stroke(b, r.width() * 0.14, stroke(accent, thin * 1.4));
            painter.line_segment(
                [
                    a + vec2(r.width() * 0.08, -r.height() * 0.1),
                    pos2(r.right() - r.width() * 0.2, r.top()),
                ],
                stroke(color, thin * 1.4),
            );
            painter.line_segment(
                [
                    b + vec2(-r.width() * 0.08, -r.height() * 0.1),
                    pos2(r.left() + r.width() * 0.2, r.top()),
                ],
                stroke(color, thin * 1.4),
            );
        }
        Icon::Copy => {
            let a = Rect::from_min_size(
                r.left_top() + vec2(r.width() * 0.08, 0.0),
                vec2(r.width() * 0.58, r.height() * 0.72),
            );
            let b = a.translate(vec2(r.width() * 0.26, r.height() * 0.26));
            painter.rect(a, 1.5, paper, stroke(color, thin), egui::StrokeKind::Inside);
            painter.rect(
                b,
                1.5,
                paper,
                stroke(accent, thin),
                egui::StrokeKind::Inside,
            );
            lines(
                painter,
                b.shrink(b.width() * 0.2),
                &[(0.0, 1.0), (0.0, 1.0), (0.0, 0.6)],
                accent,
                thin,
            );
        }
        Icon::Paste => {
            let board = r.shrink2(vec2(r.width() * 0.14, r.height() * 0.04));
            painter.rect_filled(board, 2.0, Color32::from_rgb(190, 140, 80));
            let clip = Rect::from_center_size(
                pos2(board.center().x, board.top() + 1.0),
                vec2(board.width() * 0.5, board.height() * 0.16),
            );
            painter.rect_filled(clip, 1.5, Color32::from_gray(120));
            let sheet = Rect::from_min_max(
                board.left_top() + vec2(board.width() * 0.3, board.height() * 0.25),
                board.right_bottom() + vec2(board.width() * 0.1, 0.0),
            );
            painter.rect(
                sheet,
                1.0,
                paper,
                stroke(color, thin),
                egui::StrokeKind::Inside,
            );
            lines(
                painter,
                sheet.shrink(sheet.width() * 0.2),
                &[(0.0, 1.0), (0.0, 1.0), (0.0, 0.6)],
                color,
                thin,
            );
        }
        Icon::AlignLeft => lines(
            painter,
            r,
            &[(0.0, 1.0), (0.0, 0.6), (0.0, 1.0), (0.0, 0.6)],
            color,
            thin * 1.4,
        ),
        Icon::Picture => {
            // A framed landscape: a sun and a hill.
            let frame = r.shrink(r.width() * 0.04);
            painter.rect_stroke(
                frame,
                1.5,
                stroke(color, thin * 1.3),
                egui::StrokeKind::Inside,
            );
            painter.circle_filled(
                frame.left_top() + vec2(frame.width() * 0.3, frame.height() * 0.32),
                frame.width() * 0.1,
                accent,
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    frame.left_bottom() + vec2(thin * 1.5, -thin * 1.5),
                    frame.left_bottom() + vec2(frame.width() * 0.42, -frame.height() * 0.5),
                    frame.left_bottom() + vec2(frame.width() * 0.62, -frame.height() * 0.3),
                    frame.left_bottom() + vec2(frame.width() * 0.78, -frame.height() * 0.45),
                    frame.right_bottom() + vec2(-thin * 1.5, -thin * 1.5),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Print => {
            // A printer: body, paper coming out, a tray.
            let body = Rect::from_min_max(
                pos2(r.left(), r.top() + r.height() * 0.35),
                pos2(r.right(), r.top() + r.height() * 0.75),
            );
            painter.rect_stroke(
                body,
                2.0,
                stroke(color, thin * 1.3),
                egui::StrokeKind::Inside,
            );
            let paper_top = Rect::from_min_max(
                pos2(r.left() + r.width() * 0.22, r.top()),
                pos2(r.right() - r.width() * 0.22, body.top()),
            );
            painter.rect_stroke(
                paper_top,
                0.0,
                stroke(color, thin),
                egui::StrokeKind::Inside,
            );
            let paper_out = Rect::from_min_max(
                pos2(
                    r.left() + r.width() * 0.22,
                    body.bottom() - r.height() * 0.1,
                ),
                pos2(r.right() - r.width() * 0.22, r.bottom()),
            );
            painter.rect_filled(paper_out, 0.0, paper);
            painter.rect_stroke(
                paper_out,
                0.0,
                stroke(color, thin),
                egui::StrokeKind::Inside,
            );
            painter.circle_filled(
                pos2(body.right() - thin * 3.0, body.top() + thin * 3.0),
                thin,
                accent,
            );
        }
        Icon::Header | Icon::Footer | Icon::PageNumber => {
            page(painter, page_rect, color, paper);
            let band_h = page_rect.height() * 0.2;
            let band = if icon == Icon::Footer {
                Rect::from_min_max(
                    pos2(page_rect.left(), page_rect.bottom() - band_h),
                    page_rect.right_bottom(),
                )
            } else {
                Rect::from_min_max(
                    page_rect.left_top(),
                    pos2(page_rect.right(), page_rect.top() + band_h),
                )
            };
            if icon == Icon::PageNumber {
                painter.text(
                    pos2(page_rect.center().x, page_rect.bottom() - band_h * 0.6),
                    egui::Align2::CENTER_CENTER,
                    "1",
                    egui::FontId::proportional(band_h * 1.6),
                    accent,
                );
            } else {
                painter.rect_filled(band.shrink(thin), 0.0, accent);
            }
        }
        Icon::Crop => {
            // Two corner brackets crossing, as crop marks.
            let s = stroke(color, thin * 1.6);
            let (l, t, rr, b) = (
                r.left() + r.width() * 0.22,
                r.top() + r.height() * 0.22,
                r.right() - r.width() * 0.22,
                r.bottom() - r.height() * 0.22,
            );
            painter.line_segment([pos2(l, r.top()), pos2(l, b)], s);
            painter.line_segment([pos2(l, b), pos2(r.right(), b)], s);
            let s = stroke(accent, thin * 1.6);
            painter.line_segment([pos2(r.left(), t), pos2(rr, t)], s);
            painter.line_segment([pos2(rr, t), pos2(rr, r.bottom())], s);
        }
        Icon::Rotate => {
            let c = r.center();
            let radius = r.width() * 0.34;
            let points: Vec<egui::Pos2> = (0..=20u8)
                .map(|i| {
                    let a = std::f32::consts::PI * (0.15 + 1.45 * f32::from(i) / 20.0);
                    c + vec2(a.cos(), -a.sin()) * radius
                })
                .collect();
            let tip = points.last().copied().unwrap_or(c);
            painter.add(egui::Shape::line(points, stroke(color, thin * 1.5)));
            painter.add(egui::Shape::convex_polygon(
                vec![
                    tip + vec2(-radius * 0.45, -radius * 0.05),
                    tip + vec2(radius * 0.3, -radius * 0.4),
                    tip + vec2(radius * 0.15, radius * 0.4),
                ],
                accent,
                Stroke::NONE,
            ));
        }
        Icon::Brightness => {
            let c = r.center();
            painter.circle_filled(c, r.width() * 0.2, accent);
            for i in 0..8u8 {
                let a = std::f32::consts::FRAC_PI_4 * f32::from(i);
                let d = vec2(a.cos(), a.sin());
                painter.line_segment(
                    [c + d * r.width() * 0.3, c + d * r.width() * 0.45],
                    stroke(color, thin * 1.3),
                );
            }
        }
        Icon::Contrast => {
            let c = r.center();
            let radius = r.width() * 0.4;
            painter.circle_stroke(c, radius, stroke(color, thin * 1.3));
            let half: Vec<egui::Pos2> = (0..=16u8)
                .map(|i| {
                    let a = std::f32::consts::PI * (-0.5 + f32::from(i) / 16.0);
                    c + vec2(a.cos(), a.sin()) * radius
                })
                .collect();
            painter.add(egui::Shape::convex_polygon(half, color, Stroke::NONE));
        }
        Icon::Wrap => {
            // A picture with lines of text beside and around it.
            let pic = Rect::from_min_size(
                r.left_top() + vec2(0.0, r.height() * 0.3),
                vec2(r.width() * 0.45, r.height() * 0.4),
            );
            painter.rect_filled(pic, 1.0, accent);
            let s = stroke(color, thin * 1.3);
            for (i, full) in [(0.12, true), (0.4, false), (0.6, false), (0.88, true)] {
                let y = r.top() + r.height() * i;
                let x0 = if full {
                    r.left()
                } else {
                    r.left() + r.width() * 0.55
                };
                painter.line_segment([pos2(x0, y), pos2(r.right(), y)], s);
            }
        }
        Icon::Formula => {
            painter.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                "\u{2211}",
                egui::FontId::proportional(r.height() * 0.95),
                accent,
            );
        }
        Icon::Delete => {
            let c = r.center();
            let s = r.width() * 0.32;
            for (a, b) in [(vec2(-s, -s), vec2(s, s)), (vec2(-s, s), vec2(s, -s))] {
                painter.line_segment(
                    [c + a, c + b],
                    stroke(Color32::from_rgb(200, 50, 50), thin * 2.2),
                );
            }
        }
        Icon::OriginalSize => {
            let outer = r.shrink(r.width() * 0.04);
            painter.rect_stroke(outer, 1.0, stroke(color, thin), egui::StrokeKind::Inside);
            let inner = Rect::from_center_size(outer.center(), outer.size() * 0.5);
            painter.rect_filled(inner, 1.0, accent);
        }
        Icon::Bullets | Icon::Numbering => {
            // Three items: a marker in the accent colour, then a line.
            let step = r.height() / 3.0;
            for index in 0..3u8 {
                let y = r.top() + step * (f32::from(index) + 0.5);
                if icon == Icon::Bullets {
                    painter.circle_filled(pos2(r.left() + r.width() * 0.1, y), thin * 1.6, accent);
                } else {
                    painter.text(
                        pos2(r.left() + r.width() * 0.12, y),
                        egui::Align2::CENTER_CENTER,
                        (index + 1).to_string(),
                        egui::FontId::proportional(step * 1.05),
                        accent,
                    );
                }
                painter.line_segment(
                    [pos2(r.left() + r.width() * 0.32, y), pos2(r.right(), y)],
                    stroke(color, thin * 1.4),
                );
            }
        }
        Icon::AlignCenter => lines(
            painter,
            r,
            &[(0.0, 1.0), (0.2, 0.8), (0.0, 1.0), (0.2, 0.8)],
            color,
            thin * 1.4,
        ),
        Icon::AlignRight => lines(
            painter,
            r,
            &[(0.0, 1.0), (0.4, 1.0), (0.0, 1.0), (0.4, 1.0)],
            color,
            thin * 1.4,
        ),
        Icon::Justify => lines(
            painter,
            r,
            &[(0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0)],
            color,
            thin * 1.4,
        ),
        Icon::PageBreak | Icon::SectionBreak => {
            let top = Rect::from_min_max(
                pos2(page_rect.left(), r.top()),
                pos2(page_rect.right(), r.center().y - r.height() * 0.08),
            );
            let bottom = Rect::from_min_max(
                pos2(page_rect.left(), r.center().y + r.height() * 0.08),
                pos2(page_rect.right(), r.bottom()),
            );
            painter.rect(
                top,
                0.0,
                paper,
                stroke(color, thin),
                egui::StrokeKind::Inside,
            );
            painter.rect(
                bottom,
                0.0,
                paper,
                stroke(color, thin),
                egui::StrokeKind::Inside,
            );
            let y = r.center().y;
            let mut x = r.left();
            while x < r.right() {
                painter.line_segment(
                    [pos2(x, y), pos2((x + r.width() * 0.1).min(r.right()), y)],
                    stroke(accent, thin * 1.3),
                );
                x += r.width() * 0.18;
            }
        }
        Icon::Table => {
            let t = r.shrink(r.width() * 0.04);
            painter.rect(t, 1.0, paper, stroke(color, thin), egui::StrokeKind::Inside);
            let head =
                Rect::from_min_max(t.left_top(), pos2(t.right(), t.top() + t.height() / 3.0));
            painter.rect_filled(head.shrink(thin * 0.5), 0.0, accent);
            for i in 1..3 {
                #[allow(clippy::cast_precision_loss)]
                let f = i as f32 / 3.0;
                painter.line_segment(
                    [
                        pos2(t.left(), t.top() + t.height() * f),
                        pos2(t.right(), t.top() + t.height() * f),
                    ],
                    stroke(color, thin),
                );
                painter.line_segment(
                    [
                        pos2(t.left() + t.width() * f, t.top()),
                        pos2(t.left() + t.width() * f, t.bottom()),
                    ],
                    stroke(color, thin),
                );
            }
        }
        Icon::InsertFile => {
            page(painter, page_rect, color, paper);
            lines(
                painter,
                page_rect.shrink(page_rect.width() * 0.22),
                &[(0.0, 1.0), (0.0, 1.0), (0.0, 0.7)],
                color,
                thin,
            );
            let c = pos2(page_rect.right(), page_rect.bottom() - r.height() * 0.1);
            painter.line_segment(
                [c - vec2(r.width() * 0.35, 0.0), c],
                stroke(accent, thin * 2.0),
            );
            arrow_head(
                painter,
                c + vec2(r.width() * 0.08, 0.0),
                vec2(1.0, 0.0),
                r.width() * 0.18,
                accent,
            );
        }
        Icon::Date => {
            let body = r.shrink(r.width() * 0.06);
            painter.rect(
                body,
                2.0,
                paper,
                stroke(color, thin),
                egui::StrokeKind::Inside,
            );
            painter.rect_filled(
                Rect::from_min_max(
                    body.left_top(),
                    pos2(body.right(), body.top() + body.height() * 0.28),
                ),
                2.0,
                accent,
            );
            painter.text(
                body.center() + vec2(0.0, body.height() * 0.14),
                egui::Align2::CENTER_CENTER,
                "31",
                egui::FontId::proportional(body.height() * 0.45),
                color,
            );
        }
        Icon::Symbol => {
            painter.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                "Ω",
                egui::FontId::proportional(r.height() * 0.95),
                accent,
            );
        }
        Icon::Find | Icon::ZoomIn | Icon::ZoomOut | Icon::Replace => {
            let c = r.left_top() + vec2(r.width() * 0.42, r.height() * 0.42);
            let radius = r.width() * 0.3;
            painter.circle(c, radius, paper, stroke(color, thin * 1.6));
            let d = vec2(1.0, 1.0).normalized();
            painter.line_segment(
                [
                    c + d * radius,
                    r.right_bottom() - vec2(r.width() * 0.04, r.height() * 0.04),
                ],
                stroke(color, thin * 2.6),
            );
            match icon {
                Icon::ZoomIn => {
                    painter.line_segment(
                        [c - vec2(radius * 0.55, 0.0), c + vec2(radius * 0.55, 0.0)],
                        stroke(accent, thin * 1.6),
                    );
                    painter.line_segment(
                        [c - vec2(0.0, radius * 0.55), c + vec2(0.0, radius * 0.55)],
                        stroke(accent, thin * 1.6),
                    );
                }
                Icon::ZoomOut => {
                    painter.line_segment(
                        [c - vec2(radius * 0.55, 0.0), c + vec2(radius * 0.55, 0.0)],
                        stroke(accent, thin * 1.6),
                    );
                }
                Icon::Replace => {
                    painter.text(
                        c,
                        egui::Align2::CENTER_CENTER,
                        "ab",
                        egui::FontId::proportional(radius * 0.95),
                        accent,
                    );
                }
                _ => {}
            }
        }
        Icon::OnePage => {
            painter.rect(
                page_rect,
                1.0,
                paper,
                stroke(color, thin),
                egui::StrokeKind::Inside,
            );
            lines(
                painter,
                page_rect.shrink(page_rect.width() * 0.2),
                &[(0.0, 1.0), (0.0, 1.0), (0.0, 1.0), (0.0, 0.6)],
                accent,
                thin,
            );
        }
        Icon::PageWidth => {
            painter.rect(
                page_rect,
                1.0,
                paper,
                stroke(color, thin),
                egui::StrokeKind::Inside,
            );
            let y = r.center().y;
            painter.line_segment(
                [pos2(r.left(), y), pos2(r.right(), y)],
                stroke(accent, thin * 1.4),
            );
            arrow_head(
                painter,
                pos2(r.left(), y),
                vec2(-1.0, 0.0),
                r.width() * 0.16,
                accent,
            );
            arrow_head(
                painter,
                pos2(r.right(), y),
                vec2(1.0, 0.0),
                r.width() * 0.16,
                accent,
            );
        }
        Icon::Accept | Icon::Reject => {
            page(
                painter,
                page_rect.translate(vec2(-r.width() * 0.1, 0.0)),
                color,
                paper,
            );
            let c = pos2(r.right() - r.width() * 0.2, r.bottom() - r.height() * 0.22);
            let s = r.width() * 0.22;
            if icon == Icon::Accept {
                painter.circle_filled(c, s, Color32::from_rgb(40, 150, 60));
                painter.add(Shape::line(
                    vec![
                        c + vec2(-s * 0.5, 0.0),
                        c + vec2(-s * 0.1, s * 0.4),
                        c + vec2(s * 0.5, -s * 0.4),
                    ],
                    stroke(Color32::WHITE, thin * 1.4),
                ));
            } else {
                painter.circle_filled(c, s, Color32::from_rgb(200, 50, 50));
                painter.line_segment(
                    [c + vec2(-s * 0.45, -s * 0.45), c + vec2(s * 0.45, s * 0.45)],
                    stroke(Color32::WHITE, thin * 1.4),
                );
                painter.line_segment(
                    [c + vec2(s * 0.45, -s * 0.45), c + vec2(-s * 0.45, s * 0.45)],
                    stroke(Color32::WHITE, thin * 1.4),
                );
            }
        }
        Icon::Report => {
            page(painter, page_rect, color, paper);
            lines(
                painter,
                page_rect.shrink(page_rect.width() * 0.2),
                &[(0.0, 1.0), (0.0, 0.8), (0.0, 1.0), (0.0, 0.5)],
                accent,
                thin,
            );
        }
        Icon::Words => {
            page(painter, page_rect, color, paper);
            let upper = Rect::from_min_max(
                page_rect.left_top() + vec2(page_rect.width() * 0.2, page_rect.height() * 0.18),
                pos2(
                    page_rect.right() - page_rect.width() * 0.2,
                    page_rect.center().y,
                ),
            );
            lines(painter, upper, &[(0.0, 1.0), (0.0, 0.7)], color, thin);
            badge(painter, page_rect, "123", accent);
        }
        Icon::Navigation => {
            let body = r.shrink(r.width() * 0.04);
            painter.rect(
                body,
                1.5,
                paper,
                stroke(color, thin),
                egui::StrokeKind::Inside,
            );
            let side = Rect::from_min_max(
                body.left_top(),
                pos2(body.left() + body.width() * 0.36, body.bottom()),
            );
            painter.rect_filled(side.shrink(thin), 0.0, accent);
            lines(
                painter,
                Rect::from_min_max(
                    pos2(side.right() + thin * 2.0, body.top() + 2.0),
                    body.right_bottom() - vec2(2.0, 2.0),
                ),
                &[(0.0, 1.0), (0.0, 0.8), (0.0, 1.0)],
                color,
                thin,
            );
        }
        Icon::Theme => {
            let c = r.center();
            painter.circle_filled(c, r.width() * 0.38, color);
            painter.circle_filled(
                c + vec2(r.width() * 0.18, -r.height() * 0.14),
                r.width() * 0.32,
                paper,
            );
        }
        Icon::Language => {
            let c = r.center();
            let radius = r.width() * 0.42;
            painter.circle_stroke(c, radius, stroke(accent, thin * 1.3));
            painter.line_segment(
                [c - vec2(radius, 0.0), c + vec2(radius, 0.0)],
                stroke(accent, thin),
            );
            painter.line_segment(
                [c - vec2(0.0, radius), c + vec2(0.0, radius)],
                stroke(accent, thin),
            );
            let mut points = Vec::new();
            for step in 0..=16 {
                #[allow(clippy::cast_precision_loss)]
                let t = std::f32::consts::PI * (step as f32 / 16.0);
                points.push(c + vec2(t.sin() * radius * 0.45, -t.cos() * radius));
            }
            painter.add(Shape::line(points.clone(), stroke(accent, thin)));
            painter.add(Shape::line(
                points.iter().map(|p| pos2(2.0 * c.x - p.x, p.y)).collect(),
                stroke(accent, thin),
            ));
        }
        Icon::Settings => {
            let c = r.center();
            let outer = r.width() * 0.42;
            for step in 0..8 {
                #[allow(clippy::cast_precision_loss)]
                let t = std::f32::consts::TAU * step as f32 / 8.0;
                let d = vec2(t.cos(), t.sin());
                painter.line_segment(
                    [c + d * outer * 0.6, c + d * outer],
                    stroke(color, thin * 2.8),
                );
            }
            painter.circle(c, outer * 0.68, paper, stroke(color, thin * 1.8));
            painter.circle_filled(c, outer * 0.25, accent);
        }
        Icon::About => {
            let c = r.center();
            painter.circle_filled(c, r.width() * 0.42, accent);
            painter.text(
                c,
                egui::Align2::CENTER_CENTER,
                "i",
                egui::FontId::proportional(r.height() * 0.6),
                Color32::WHITE,
            );
        }
        Icon::Close => {
            let s = r.shrink(r.width() * 0.25);
            painter.line_segment([s.left_top(), s.right_bottom()], stroke(color, thin * 1.4));
            painter.line_segment([s.right_top(), s.left_bottom()], stroke(color, thin * 1.4));
        }
        Icon::Portrait | Icon::Landscape => {
            let size = if icon == Icon::Portrait {
                vec2(r.width() * 0.62, r.height() * 0.9)
            } else {
                vec2(r.width() * 0.9, r.height() * 0.62)
            };
            let p = Rect::from_center_size(r.center(), size);
            page(painter, p, color, paper);
            lines(
                painter,
                p.shrink(p.width().min(p.height()) * 0.22),
                &[(0.0, 1.0), (0.0, 1.0), (0.0, 0.6)],
                accent,
                thin,
            );
        }
        Icon::Margins => {
            painter.rect(
                page_rect,
                0.0,
                paper,
                stroke(color, thin),
                egui::StrokeKind::Inside,
            );
            let inner = page_rect.shrink(page_rect.width() * 0.2);
            painter.rect_stroke(inner, 0.0, stroke(accent, thin), egui::StrokeKind::Inside);
        }
        Icon::PageSize => {
            let big = Rect::from_min_size(r.left_top(), vec2(r.width() * 0.7, r.height() * 0.9));
            let small = Rect::from_min_size(
                r.left_top() + vec2(r.width() * 0.3, r.height() * 0.35),
                vec2(r.width() * 0.6, r.height() * 0.6),
            );
            painter.rect(
                big,
                0.0,
                paper,
                stroke(color, thin),
                egui::StrokeKind::Inside,
            );
            painter.rect(
                small,
                0.0,
                paper,
                stroke(accent, thin * 1.2),
                egui::StrokeKind::Inside,
            );
        }
        Icon::Clear => {
            painter.text(
                r.left_center() + vec2(r.width() * 0.35, -r.height() * 0.05),
                egui::Align2::CENTER_CENTER,
                "A",
                egui::FontId::proportional(r.height() * 0.85),
                color,
            );
            let c = r.right_bottom() - vec2(r.width() * 0.22, r.height() * 0.22);
            let s = r.width() * 0.17;
            painter.line_segment(
                [c - vec2(s, s), c + vec2(s, s)],
                stroke(Color32::from_rgb(210, 60, 60), thin * 1.8),
            );
            painter.line_segment(
                [c + vec2(s, -s), c + vec2(-s, s)],
                stroke(Color32::from_rgb(210, 60, 60), thin * 1.8),
            );
        }
        Icon::SelectAll => {
            let body = r.shrink(r.width() * 0.06);
            let mut x = body.left();
            while x < body.right() {
                let end = (x + body.width() * 0.12).min(body.right());
                painter.line_segment(
                    [pos2(x, body.top()), pos2(end, body.top())],
                    stroke(color, thin),
                );
                painter.line_segment(
                    [pos2(x, body.bottom()), pos2(end, body.bottom())],
                    stroke(color, thin),
                );
                x += body.width() * 0.22;
            }
            let mut y = body.top();
            while y < body.bottom() {
                let end = (y + body.height() * 0.12).min(body.bottom());
                painter.line_segment(
                    [pos2(body.left(), y), pos2(body.left(), end)],
                    stroke(color, thin),
                );
                painter.line_segment(
                    [pos2(body.right(), y), pos2(body.right(), end)],
                    stroke(color, thin),
                );
                y += body.height() * 0.22;
            }
            lines(
                painter,
                body.shrink(body.width() * 0.22),
                &[(0.0, 1.0), (0.0, 1.0), (0.0, 0.6)],
                accent,
                thin * 1.4,
            );
        }
    }
}

/// An icon-sized widget with no interaction, for labels.
pub(crate) fn icon_widget(
    ui: &mut egui::Ui,
    icon: Icon,
    size: f32,
    accent: Color32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), egui::Sense::hover());
    let color = ui.visuals().text_color();
    paint(ui.painter(), rect, icon, color, accent);
    response
}
