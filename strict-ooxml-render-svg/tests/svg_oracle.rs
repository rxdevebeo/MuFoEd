//! Independent SVG oracle: an external XML parser (`roxmltree`) plus structural
//! invariants (`STAGE-4-TASK.md` §8.2).
//!
//! The oracle is exercised against deliberately corrupted SVG so its failure
//! modes are proven (self-check).

#![allow(
    clippy::expect_used,
    clippy::cast_possible_truncation,
    clippy::default_trait_access,
    clippy::format_collect,
    clippy::doc_markdown
)]

mod common;

use common::render_body;

/// Parses one coordinate or a shaped cluster list. Every token is checked.
/// A missing number is an error; nothing is replaced with zero.
fn parse_coord_list(attribute: &str, value: &str) -> Result<Vec<f64>, String> {
    let mut numbers = Vec::new();
    for token in value.split_whitespace() {
        let number: f64 = token
            .parse()
            .map_err(|_| format!("bad coordinate {attribute}={value}"))?;
        if !number.is_finite() {
            return Err(format!("non-finite {attribute}={value}"));
        }
        numbers.push(number);
    }
    if numbers.is_empty() {
        return Err(format!("empty coordinate {attribute}"));
    }
    Ok(numbers)
}

/// Checks one SVG document against the page geometry.
fn check_svg(svg: &str, width: f64, height: f64) -> Result<(), String> {
    let document =
        roxmltree::Document::parse(svg).map_err(|error| format!("XML parse: {error}"))?;
    let root = document.root_element();
    if root.tag_name().name() != "svg" {
        return Err("root element is not <svg>".to_owned());
    }
    if root.attribute("viewBox").is_none() {
        return Err("missing viewBox".to_owned());
    }
    for attribute in ["width", "height"] {
        let value = root
            .attribute(attribute)
            .ok_or_else(|| format!("missing {attribute}"))?;
        let number: f64 = value
            .parse()
            .map_err(|_| format!("non-numeric {attribute}: {value}"))?;
        if !number.is_finite() || number <= 0.0 {
            return Err(format!("bad {attribute}: {value}"));
        }
    }

    let tolerance = 2.0;
    for node in document.descendants() {
        if !node.is_element() {
            continue;
        }
        for attribute in ["x", "y", "x1", "y1", "x2", "y2"] {
            if let Some(value) = node.attribute(attribute) {
                let numbers = parse_coord_list(attribute, value)?;
                let limit = width.max(height) * 4.0 + tolerance;
                for number in numbers {
                    if number < -tolerance || number > limit {
                        return Err(format!("coordinate out of bounds: {attribute}={value}"));
                    }
                }
            }
        }
        for attribute in ["width", "height"] {
            if let Some(value) = node.attribute(attribute) {
                if let Ok(number) = value.parse::<f64>() {
                    if !number.is_finite() || number < 0.0 {
                        return Err(format!("bad {attribute}={value}"));
                    }
                }
            }
        }
    }
    Ok(())
}

#[test]
fn every_rendered_page_is_valid_and_within_bounds() {
    let pages = render_body(
        "<w:p><w:r><w:t>Oracle check</w:t></w:r></w:p><w:tbl><w:tblGrid><w:gridCol w:w=\"3000\"/><w:gridCol w:w=\"3000\"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>cell</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>two</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
    );
    for page in &pages {
        if let Err(error) = check_svg(&page.svg, page.width_px, page.height_px) {
            panic!("page {} invalid: {error}\n{}", page.index, page.svg);
        }
    }
}

#[test]
fn oracle_rejects_corrupted_svg() {
    let pages = render_body("<w:p><w:r><w:t>x</w:t></w:r></w:p>");
    let good = &pages[0].svg;
    assert!(check_svg(good, pages[0].width_px, pages[0].height_px).is_ok());

    let svg = |body: &str| {
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"100\" height=\"100\" viewBox=\"0 0 100 100\">{body}</svg>"
        )
    };

    // Non-finite coordinate.
    assert!(check_svg(&svg("<text x=\"NaN\" y=\"1\">x</text>"), 100.0, 100.0).is_err());
    assert!(check_svg(&svg("<text x=\"1 NaN\" y=\"1\">x</text>"), 100.0, 100.0).is_err());
    assert!(check_svg(&svg("<text x=\"1 999999\" y=\"1\">x</text>"), 100.0, 100.0).is_err());
    // Out-of-bounds coordinate.
    assert!(check_svg(&svg("<text x=\"999999\" y=\"1\">x</text>"), 100.0, 100.0).is_err());
    // Missing viewBox.
    assert!(check_svg("<svg width=\"100\" height=\"100\"/>", 100.0, 100.0).is_err());
    // Malformed XML.
    assert!(check_svg("<svg>", 100.0, 100.0).is_err());
}
