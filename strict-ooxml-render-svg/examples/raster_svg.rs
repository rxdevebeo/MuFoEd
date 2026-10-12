//! Rasterize one saved SVG with the renderer's bundled fonts for visual audits.
#![allow(clippy::expect_used)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let input = args.get(1).expect("input SVG");
    let output = args.get(2).expect("output PNG");
    let mut options = resvg::usvg::Options::default();
    options
        .fontdb_mut()
        .load_fonts_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts"));
    let svg = std::fs::read_to_string(input).expect("read SVG");
    let tree = resvg::usvg::Tree::from_str(&svg, &options).expect("parse SVG");
    let size = tree.size().to_int_size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height()).expect("image");
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    pixmap.save_png(output).expect("save PNG");
}
