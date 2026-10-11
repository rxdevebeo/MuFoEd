//! Generate the small DOCX gallery used to inspect non-solid fills and wrapping.
use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    let directory = std::env::args().nth(1).map_or_else(
        || PathBuf::from("render_out/graphics-gallery"),
        PathBuf::from,
    );
    std::fs::create_dir_all(&directory)?;
    for (name, bytes) in strict_ooxml_testkit::graphics::gallery() {
        std::fs::write(directory.join(name), bytes)?;
    }
    println!("{}", directory.display());
    Ok(())
}
