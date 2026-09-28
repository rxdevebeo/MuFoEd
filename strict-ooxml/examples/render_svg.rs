//! Renders a Strict `.docx` to one SVG file per page.
//!
//! ```text
//! cargo run -p strict-ooxml --example render_svg -- document.docx out/
//! ```

use strict_ooxml::{OpenOptions, RenderOptions, StrictDocument};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: render_svg <file.docx> [out_dir]")?;
    let out_dir = args.next().unwrap_or_else(|| ".".to_owned());

    let document = StrictDocument::open_path(&path, &OpenOptions::default())?;
    let pages = document.render_svg(&RenderOptions::default())?;
    for page in &pages {
        let name = format!("{out_dir}/page-{}.svg", page.index + 1);
        std::fs::write(&name, &page.svg)?;
        println!("{name}");
    }
    Ok(())
}
