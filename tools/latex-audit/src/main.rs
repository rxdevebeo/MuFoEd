//! Reproducible audit client; never modifies the corpus or library sources.
use serde_json::{json, Value};
use std::{fs, path::Path, sync::Arc, time::Instant};
use strict_ooxml::{
    LatexOptions, MediaBag, OpenOptions, PageSelection, RenderOptions, StrictDocument, WriteOptions,
};

fn save(out: &Path, result: &Value) {
    fs::write(
        out.join("result.json"),
        serde_json::to_vec_pretty(result).unwrap(),
    )
    .unwrap();
}

fn raster(svg: &str, path: &Path) -> Result<(), String> {
    use resvg::usvg;
    use strict_ooxml_render_svg::font::builtin::{bundled_families, face_source};
    let mut db = usvg::fontdb::Database::new();
    let mut seen = vec![];
    for family in bundled_families() {
        for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
            if let Some(face) = face_source(family, bold, italic) {
                if !seen.contains(&face.data.as_ptr()) {
                    seen.push(face.data.as_ptr());
                    db.load_font_source(usvg::fontdb::Source::Binary(Arc::new(face.data)));
                }
            }
        }
    }
    db.set_serif_family("Tinos");
    db.set_sans_serif_family("Arimo");
    db.set_monospace_family("Cousine");
    let options = usvg::Options {
        fontdb: Arc::new(db),
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(svg, &options).map_err(|e| e.to_string())?;
    let size = tree.size().to_int_size();
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(size.width(), size.height()).ok_or("bitmap allocation")?;
    pixmap.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    pixmap.save_png(path).map_err(|e| e.to_string())
}

fn compose(a: [f64; 6], b: [f64; 6]) -> [f64; 6] {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}

fn point(m: [f64; 6], x: f64, y: f64) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

fn world_text<'a>(
    items: &'a [strict_ooxml_render_svg::Item],
    matrix: [f64; 6],
    out: &mut Vec<(&'a strict_ooxml_render_svg::TextItem, [f64; 4])>,
) {
    use strict_ooxml_render_svg::Item;
    for item in items {
        match item {
            Item::Group(g) => world_text(&g.items, compose(matrix, g.matrix), out),
            Item::Text(t) => {
                let (x, y) = point(matrix, t.x, t.baseline);
                let (right, end_y) = point(matrix, t.x + t.width, t.baseline);
                out.push((t, [x, y, right, end_y]));
            }
            _ => {}
        }
    }
}

fn audit(input: &Path, out: &Path, end: usize) {
    fs::create_dir_all(out).unwrap();
    let start = Instant::now();
    let mut result =
        json!({"input":input.to_string_lossy(),"render_page_limit":end,"stage":"import"});
    save(out, &result);
    let imported = match strict_ooxml_latex::import_file(input, &LatexOptions::default()) {
        Ok(v) => v,
        Err(e) => {
            result["error"] = json!(e.to_string());
            save(out, &result);
            return;
        }
    };
    let features: serde_json::Map<String, Value> = imported
        .report
        .feature_counts
        .iter()
        .map(|(k, v)| (k.clone(), json!({"loss":v.loss_count,"info":v.info_count})))
        .collect();
    result["import"] = json!({"loss_count":imported.report.loss_count,"info_count":imported.report.info_count,"tokens":imported.report.tokens,"text_scalars":imported.report.text_scalars,"nodes":imported.report.nodes,"included_files":imported.report.included_files,"details_truncated":imported.report.details_truncated,"feature_counts_truncated":imported.report.feature_counts_truncated,"features":features,
        "issues":imported.report.issues.iter().map(|i|json!({"file":i.file,"start":i.span.start,"end":i.span.end,"severity":format!("{:?}",i.severity),"feature":i.feature})).collect::<Vec<_>>()});
    result["stage"] = json!("write");
    result["import_ms"] = json!(start.elapsed().as_millis());
    save(out, &result);
    let mut media = MediaBag::new();
    for (part, bytes) in imported.media {
        media.insert(part, bytes);
    }
    let written = match strict_ooxml::write_package(
        &imported.document,
        Some(&media),
        &WriteOptions::default(),
    ) {
        Ok(v) => v,
        Err(e) => {
            result["error"] = json!(e.to_string());
            save(out, &result);
            return;
        }
    };
    result["write_report"] = json!(format!("{:?}", written.report));
    result["writer_no_silent_loss"] = json!(written.report.verify_no_silent_loss().is_ok());
    fs::write(out.join("document.docx"), &written.bytes).unwrap();
    result["stage"] = json!("reopen");
    save(out, &result);
    let document = match StrictDocument::open_reader(&written.bytes[..], &OpenOptions::default()) {
        Ok(v) => v,
        Err(e) => {
            result["error"] = json!(e.to_string());
            save(out, &result);
            return;
        }
    };
    result["stage"] = json!("render");
    save(out, &result);
    let options = if end == 0 {
        RenderOptions::default()
    } else {
        RenderOptions::default().pages(PageSelection::Range { start: 1, end })
    };
    let placement = match document.place(&options) {
        Ok(v) => v,
        Err(e) => {
            result["error"] = json!(e.to_string());
            save(out, &result);
            return;
        }
    };
    let mut overflow = vec![];
    let mut overflow_count = 0;
    let mut text_items = 0;
    for (page, p) in placement.pages.iter().enumerate() {
        let mut fragments = vec![];
        world_text(&p.items, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], &mut fragments);
        for (t, [x, y, right, end_y]) in fragments {
            text_items += 1;
            if ![x, y, right, end_y].iter().all(|v| v.is_finite())
                || x.min(right) < -1.0
                || x.max(right) > p.width_px + 1.0
                || y.min(end_y) < -1.0
                || y.max(end_y) > p.height_px + 1.0
            {
                overflow_count += 1;
                if overflow.len() < 200 {
                    overflow.push(json!({"page":page+1,"text":t.text,"x":x,"baseline":y,"end_x":right,"end_y":end_y,"page_width":p.width_px,"page_height":p.height_px}));
                }
            }
        }
        let svg = strict_ooxml_render_svg::paint_page(p, &options);
        fs::write(out.join(format!("page-{}.svg", page + 1)), &svg).unwrap();
        if page < 3 {
            if let Err(e) = raster(&svg, &out.join(format!("page-{}.png", page + 1))) {
                result["raster_error"] = json!(e);
            }
        }
    }
    result["render"] = json!({"pages":placement.pages.len(),"warnings":placement.warnings,"overflow_text":overflow,"overflow_count":overflow_count,"geometry":"page_coordinates_after_all_group_transforms; baseline_endpoints; clips_not_applied","text_items":text_items,"scope":if end==0 {"all_pages"} else {"prefix_only"}});
    result["stage"] = json!("complete");
    result["total_ms"] = json!(start.elapsed().as_millis());
    save(out, &result);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_affine_coordinates_are_composed_before_page_bounds_checks() {
        let translation = [1., 0., 0., 1., 100., 150.];
        let scaling = [2., 0., 0., 2., 10., 20.];
        assert_eq!(point(compose(translation, scaling), 3., -5.), (116., 160.));
        assert_eq!(
            point(compose(translation, scaling), 3., -5.),
            point(translation, 16., 10.)
        );
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert!(
        args.len() >= 3,
        "latex-audit INPUT.tex OUTPUT_DIR [PAGE_LIMIT; 0=all]"
    );
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            audit(
                Path::new(&args[1]),
                Path::new(&args[2]),
                args.get(3).and_then(|s| s.parse().ok()).unwrap_or(3),
            )
        })
        .unwrap()
        .join()
        .unwrap();
}
