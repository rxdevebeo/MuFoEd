//! Print the ink boxes and photographic background regions of each page.
//!
//! A figure is either ink the text zones do not cover (from the bitonal mask) or
//! a photographic background plane. This example makes both visible per page, so
//! the figure policy can be checked against a real file without a model.
#![allow(clippy::doc_markdown)]
use strict_ooxml_djvu::{DjvuDocument, DjvuLimits};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).ok_or("requires a DjVu path")?;
    let bytes = std::fs::read(&path)?;
    let document = DjvuDocument::open(&bytes, DjvuLimits::default())?;
    for page in document.pages() {
        let ink = page.ink_boxes(document.limits).unwrap_or_default();
        let background = page.background_regions(document.limits).unwrap_or_default();
        println!(
            "page {}: {} ink box(es), {} background region(s)",
            page.number,
            ink.len(),
            background.len()
        );
        for region in &background {
            println!(
                "  bg {:.1},{:.1} {:.1}x{:.1}",
                region.x, region.y, region.width, region.height
            );
        }
    }
    Ok(())
}
