#![allow(clippy::too_many_lines)]
use crate::{Diagram, ShapeKind};
use std::fmt::Write as _;
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn color(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}
impl Diagram {
    /// Exports a standalone SVG without scripts, external resources or embedded HTML.
    /// Text remains SVG text; appearance depends on the viewer's available fonts.
    /// # Errors
    /// Returns a scene validation error before generating output.
    pub fn to_svg(&self) -> Result<String, String> {
        self.validate()?;
        let mut min_x = 0.0_f32;
        let mut min_y = 0.0_f32;
        let mut max_x = 640.0_f32;
        let mut max_y = 480.0_f32;
        for n in &self.nodes {
            let (a, b) = n.visual_bounds();
            min_x = min_x.min(a.x - 40.0);
            min_y = min_y.min(a.y - 40.0);
            max_x = max_x.max(b.x + 40.0);
            max_y = max_y.max(b.y + 40.0);
        }
        for c in &self.connections {
            for p in self.route(c) {
                min_x = min_x.min(p.x - 16.0);
                min_y = min_y.min(p.y - 16.0);
                max_x = max_x.max(p.x + 16.0);
                max_y = max_y.max(p.y + 16.0);
            }
        }
        let mut svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"{min_x} {min_y} {} {}\">\n",max_x-min_x,max_y-min_y,max_x-min_x,max_y-min_y);
        for c in &self.connections {
            let path = self.route(c);
            let mut points = String::new();
            for p in &path {
                let _ = write!(points, "{},{} ", p.x, p.y);
            }
            let stroke = color(c.color);
            let _=writeln!(svg,"<polyline points=\"{points}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"{}\"{}/>",c.width,dash(c.dash));
            let mut ends = vec![];
            if c.end_arrow {
                if let [.., a, b] = path.as_slice() {
                    ends.push((*a, *b));
                }
            }
            if c.start_arrow {
                if let [a, b, ..] = path.as_slice() {
                    ends.push((*b, *a));
                }
            }
            for (a, b) in ends {
                let dx = b.x - a.x;
                let dy = b.y - a.y;
                let length = dx.hypot(dy);
                if length > 0.0 {
                    let (dx, dy) = (dx / length, dy / length);
                    let _ = writeln!(
                        svg,
                        "<polygon points=\"{},{} {},{} {},{}\" fill=\"{stroke}\"/>",
                        b.x,
                        b.y,
                        b.x - 10.0 * dx - 4.0 * dy,
                        b.y - 10.0 * dy + 4.0 * dx,
                        b.x - 10.0 * dx + 4.0 * dy,
                        b.y - 10.0 * dy - 4.0 * dx
                    );
                }
            }
        }
        for n in &self.nodes {
            let (x, y, w, h) = (n.position.x, n.position.y, n.size.x, n.size.y);
            let _ = writeln!(
                svg,
                "<g transform=\"rotate({} {} {})\">",
                n.style.rotation,
                n.center().x,
                n.center().y
            );
            let style = format!(
                "fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\"{}",
                if n.style.no_fill {
                    "none".into()
                } else {
                    color(n.fill)
                },
                if n.style.no_stroke {
                    "none".into()
                } else {
                    color(n.stroke)
                },
                n.style.width,
                dash(n.style.dash)
            );
            match n.kind {
                ShapeKind::Rectangle | ShapeKind::Rounded => {
                    let _=writeln!(svg,"<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" rx=\"{}\" {style}/>",if n.kind==ShapeKind::Rounded {w.min(h)/6.0}else{0.0});
                }
                ShapeKind::Ellipse => {
                    let _ = writeln!(
                        svg,
                        "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\" {style}/>",
                        x + w / 2.0,
                        y + h / 2.0,
                        w / 2.0,
                        h / 2.0
                    );
                }
                ShapeKind::Diamond
                | ShapeKind::Triangle
                | ShapeKind::Trapezoid
                | ShapeKind::Hexagon
                | ShapeKind::Document
                | ShapeKind::RightArrow => {
                    let mut points = String::new();
                    let mut local = n.clone();
                    local.style.rotation = 0.0;
                    for p in local.outline() {
                        let _ = write!(points, "{},{} ", p.x, p.y);
                    }
                    let _ = writeln!(svg, "<polygon points=\"{points}\" {style}/>");
                }
                ShapeKind::Text => {}
            }
            let _=writeln!(svg,"<defs><clipPath id=\"text-{}\"><rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\"/></clipPath></defs>",n.id);
            let lines = n.label_lines();
            let start = y + h / 2.0
                - f32::from(u16::try_from(lines.len().saturating_sub(1)).unwrap_or(u16::MAX))
                    * n.font_size
                    * 0.6;
            let (anchor, tx) = match n.style.align {
                crate::TextAlign::Left => ("start", x + n.style.padding),
                crate::TextAlign::Center => ("middle", x + w / 2.0),
                crate::TextAlign::Right => ("end", x + w - n.style.padding),
            };
            let _=writeln!(svg,"<text clip-path=\"url(#text-{})\" text-anchor=\"{anchor}\" font-family=\"sans-serif\" font-size=\"{}\" font-weight=\"{}\" font-style=\"{}\" fill=\"#1e1e1e\">",n.id,n.font_size,if n.style.bold{"bold"}else{"normal"},if n.style.italic{"italic"}else{"normal"});
            for (i, line) in lines.iter().enumerate() {
                let _ = writeln!(
                    svg,
                    "<tspan x=\"{tx}\" y=\"{}\" dominant-baseline=\"central\">{}</tspan>",
                    start + f32::from(u16::try_from(i).unwrap_or(u16::MAX)) * n.font_size * 1.2,
                    escape(line)
                );
            }
            svg.push_str("</text></g>\n");
        }
        svg.push_str("</svg>\n");
        Ok(svg)
    }
}

fn dash(value: crate::Dash) -> &'static str {
    match value {
        crate::Dash::Solid => "",
        crate::Dash::Dashed => " stroke-dasharray=\"7 4\"",
        crate::Dash::Dotted => " stroke-dasharray=\"1.5 4\"",
    }
}
