//! Native PDF axial shadings and coloured tiling patterns.
use crate::units::rgb;
use pdf_writer::{Content, Name, Pdf, Rect, Ref};
use strict_ooxml_render_svg::layout::PathPaint;

pub(crate) fn write(pdf: &mut Pdf, id: Ref, paint: &PathPaint) {
    match paint {
        PathPaint::LinearGradient { stops, start, end } => {
            let mut shading = pdf.indirect(id).dict();
            shading.pair(Name(b"ShadingType"), 2);
            shading.pair(Name(b"ColorSpace"), Name(b"DeviceRGB"));
            shading.insert(Name(b"Coords")).array().items([
                start.0 as f32,
                start.1 as f32,
                end.0 as f32,
                end.1 as f32,
            ]);
            shading.insert(Name(b"Extend")).array().items([true, true]);
            let mut function = shading.insert(Name(b"Function")).dict();
            function.pair(Name(b"FunctionType"), 3);
            function
                .insert(Name(b"Domain"))
                .array()
                .items([0.0_f32, 1.0]);
            {
                let mut functions = function.insert(Name(b"Functions")).array();
                for pair in stops.windows(2) {
                    let [first, last] = pair else {
                        continue;
                    };
                    let mut interpolation = functions.push().dict();
                    interpolation.pair(Name(b"FunctionType"), 2);
                    interpolation
                        .insert(Name(b"Domain"))
                        .array()
                        .items([0.0_f32, 1.0]);
                    let (r, g, b) = rgb(&first.1);
                    interpolation.insert(Name(b"C0")).array().items([r, g, b]);
                    let (r, g, b) = rgb(&last.1);
                    interpolation.insert(Name(b"C1")).array().items([r, g, b]);
                    interpolation.pair(Name(b"N"), 1);
                }
            }
            function.insert(Name(b"Bounds")).array().items(
                stops
                    .iter()
                    .skip(1)
                    .take(stops.len().saturating_sub(2))
                    .map(|stop| stop.0 as f32),
            );
            function
                .insert(Name(b"Encode"))
                .array()
                .items(stops.windows(2).flat_map(|_| [0.0_f32, 1.0]));
        }
        PathPaint::Pattern {
            foreground,
            background,
            size,
            lines,
            rectangles,
        } => {
            let edge = *size as f32;
            let mut content = Content::new();
            let (r, g, b) = rgb(background);
            content
                .set_fill_rgb(r, g, b)
                .rect(0.0, 0.0, edge, edge)
                .fill_nonzero();
            let (r, g, b) = rgb(foreground);
            content
                .set_fill_rgb(r, g, b)
                .set_stroke_rgb(r, g, b)
                .set_line_width(edge * 0.125);
            for (x1, y1, x2, y2) in lines {
                content
                    .move_to(*x1 as f32 * edge, *y1 as f32 * edge)
                    .line_to(*x2 as f32 * edge, *y2 as f32 * edge)
                    .stroke();
            }
            for (x, y, w, h) in rectangles {
                content
                    .rect(
                        *x as f32 * edge,
                        *y as f32 * edge,
                        *w as f32 * edge,
                        *h as f32 * edge,
                    )
                    .fill_nonzero();
            }
            let data = content.finish().into_vec();
            let mut pattern = pdf.stream(id, &data);
            pattern.pair(Name(b"Type"), Name(b"Pattern"));
            pattern.pair(Name(b"PatternType"), 1);
            pattern.pair(Name(b"PaintType"), 1);
            pattern.pair(Name(b"TilingType"), 1);
            pattern.pair(Name(b"BBox"), Rect::new(0.0, 0.0, edge, edge));
            pattern.pair(Name(b"XStep"), edge);
            pattern.pair(Name(b"YStep"), edge);
            pattern.insert(Name(b"Resources")).dict();
        }
    }
}
