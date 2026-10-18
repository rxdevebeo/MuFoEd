//! Standalone SVG export of a chemistry document.
//!
//! The output is self-contained: no scripts, no external resources and no
//! embedded HTML. Text stays as SVG text, so its look depends on the fonts the
//! viewer has.

use std::fmt::Write as _;

use crate::model::{ChemDocument, Point};
use crate::render::{object_prims, prims_bounds, Align, Prim, Theme};

impl ChemDocument {
    /// Exports every object as a standalone SVG drawing.
    ///
    /// # Errors
    ///
    /// Returns a validation error before generating output.
    pub fn to_svg(&self) -> Result<String, String> {
        self.validate()?;
        let theme = Theme::default();
        let mut prims = vec![];
        for object in &self.objects {
            prims.extend(object_prims(object, &theme, false));
        }
        let (min, max) =
            prims_bounds(&prims).unwrap_or((Point::new(0.0, 0.0), Point::new(320.0, 200.0)));
        let pad = 16.0;
        let min = Point::new(min.x - pad, min.y - pad);
        let width = (max.x - min.x + pad * 2.0).max(1.0);
        let height = (max.y - min.y + pad * 2.0).max(1.0);
        let mut svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"{} {} {width} {height}\">\n",
            min.x, min.y
        );
        svg.push_str("<rect x=\"0\" y=\"0\" width=\"100%\" height=\"100%\" fill=\"#ffffff\"/>\n");
        for prim in &prims {
            write_prim(&mut svg, prim);
        }
        svg.push_str("</svg>\n");
        Ok(svg)
    }
}

fn write_prim(svg: &mut String, prim: &Prim) {
    match prim {
        Prim::Line {
            from,
            to,
            width,
            color,
        } => {
            let _ = writeln!(
                svg,
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"{width}\"/>",
                from.x,
                from.y,
                to.x,
                to.y,
                hex(*color)
            );
        }
        Prim::Polygon { points, fill } => {
            let mut list = String::new();
            for point in points {
                let _ = write!(list, "{},{} ", point.x, point.y);
            }
            let _ = writeln!(svg, "<polygon points=\"{list}\" fill=\"{}\"/>", hex(*fill));
        }
        Prim::Text {
            pos,
            text,
            size,
            color,
            align,
            italic,
        } => {
            let anchor = match align {
                Align::Left => "start",
                Align::Center => "middle",
                Align::Right => "end",
            };
            let style = if *italic {
                " font-style=\"italic\""
            } else {
                ""
            };
            let _ = writeln!(
                svg,
                "<text x=\"{}\" y=\"{}\" font-family=\"sans-serif\" font-size=\"{size}\" text-anchor=\"{anchor}\" dominant-baseline=\"central\" fill=\"{}\"{style}>{}</text>",
                pos.x,
                pos.y,
                hex(*color),
                escape(text)
            );
        }
    }
}

fn hex(value: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", value[0], value[1], value[2])
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ChemObject;

    #[test]
    fn a_structure_exports_well_formed_svg() {
        let mut reaction = crate::mhchem::parse("2H2 + O2 -> 2H2O").unwrap();
        reaction.id = 1;
        let document = ChemDocument {
            version: crate::model::VERSION,
            objects: vec![ChemObject::Reaction(reaction)],
        };
        let svg = document.to_svg().unwrap();
        assert!(svg.starts_with("<svg"));
        assert!(svg.trim_end().ends_with("</svg>"));
        assert!(svg.contains("<text"));
        assert!(!svg.contains("script"));
    }
}
