//! The pixel gate for the PDF backend (`STAGE-8-TASK.md` SC-6, second half;
//! `CORE-QUEUE.md` §1, `O-2`).
//!
//! The SVG gate has measured our SVG against the WPS references since Stage 4.
//! Our PDF was never compared to them: it was checked for a matching page count,
//! a real `MediaBox` and embedded fonts, and nothing else. That left the whole
//! pipeline past the layout — writer, reader, rasterizer — unmeasured, and the
//! first run of this gate found two defects in it (a shape's translation off by
//! its own height, and every elliptical arc silently dropped) that no other test
//! in the workspace could see.
//!
//! **What this gate is.** The same comparison the SVG gate makes, against the same
//! committed references, through the whole pipeline: `docx -> place_pages -> PDF
//! -> PdfDocument -> Rasterizer -> PNG`. Every candidate is read back by *our*
//! reader, so a file that only we can parse fails here rather than in a user.
//!
//! **What it is measured with.** [`strict_ooxml_fidelity`] — the same SSIM, the
//! same ink profiles, the same structural bounds as the SVG gate. One definition
//! of "fidelity" for two renderers: a threshold quoted from one gate and applied
//! to the other would otherwise be a number about a different quantity.
//!
//! **What it is held to.** `coverage/render-gates.toml`: `[pdf]` for the score and
//! `[[pdf_classes]]` for the structural bounds. Both were written before this
//! gate ran, which is the only way a threshold is a threshold
//! (GATE-STRATEGY §3.1).
//!
//! ```text
//! cargo test -p strict-ooxml-pdf --features raster --test pdf_pixels
//! ```
//!
//! The `raster` feature is what turns the comparison on: without a rasterizer
//! there is no picture, and a gate that silently skipped itself would be worse
//! than no gate.

#![allow(
    clippy::expect_used,
    // Pixel and point arithmetic: a page is 816 × 1056 and a grid dimension comes
    // out of a PNG header, both far inside `f64`'s exact integer range.
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    // A ratchet pin is a number a measurement must equal exactly — that is the
    // whole of what it is.
    clippy::float_cmp
)]

use std::path::{Path, PathBuf};

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_fidelity::{
    crop, decode_png_gray, extent_delta, load_png_gray, measurement_line, ssim,
    structural_fidelity_with, Comparison, Edge, GatePolicy, Gray, StructuralFidelity,
};
use strict_ooxml_pdf::raster::{Rasterizer, Size};
use strict_ooxml_pdf::{PdfDocument, PdfLimits};
use strict_ooxml_render_pdf::render_with_source;
use strict_ooxml_render_svg::{place_pages, RenderOptions};
use strict_ooxml_wml::{parse_document, ParseOptions};

/// The graded fixtures.
fn strict_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict")
}

/// The reference PNGs of a document, sorted by page number.
fn reference_pages(dir: &Path) -> Vec<PathBuf> {
    let mut pages: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("a reference directory")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("png"))
        .collect();
    pages.sort();
    pages
}

/// What one document's PDF looks like once it has been read back.
struct Rendered {
    /// The PDF bytes the writer produced.
    bytes: Vec<u8>,
    /// How many pages the writer says there are.
    written_pages: usize,
    /// How many pages our reader finds.
    read_pages: usize,
    /// Characters the reader got out of the text layer.
    characters: usize,
    /// What reading it cost, as report lines.
    losses: Vec<String>,
}

/// Renders `docx` to PDF and reads it back with our own reader.
///
/// The read-back is not a formality: it is what turns this from "the writer made
/// a file" into "the pipeline made a page", and the losses it collects are the
/// first acceptance criterion below.
///
/// Returns `None` for a document this project cannot treat as Strict at all —
/// a package carrying both Strict and Transitional signals. That is not a gate
/// failure and not a silent skip: the corpus deliberately holds such files (they
/// are what the conformance work feeds on), and a Strict renderer has nothing to
/// say about them. The caller names every one it skipped, so an empty run cannot
/// look like a pass.
fn round_trip(docx: &Path) -> Option<Rendered> {
    let package = Package::open_path(docx, &OpenOptions::default()).ok()?;
    let document = parse_document(&package, &ParseOptions::default()).ok()?;
    let options = RenderOptions::default();
    let placed = place_pages(&document, &options, Some(&package)).expect("place");
    let output = render_with_source(&placed, &options, Some(&package)).expect("render");
    let mut read = PdfDocument::open(&output.bytes, PdfLimits::default())
        .expect("our own reader opens our own PDF");
    let read_pages = read.page_count();
    let mut characters = 0usize;
    for index in 1..=read_pages {
        let page = read.page(index).expect("a page the reader counted");
        characters += page.text().chars().count();
    }
    let losses = read
        .report()
        .losses()
        .iter()
        .map(ToString::to_string)
        .collect();
    Some(Rendered {
        bytes: output.bytes,
        written_pages: output.page_count,
        read_pages,
        characters,
        losses,
    })
}

/// A page's size in points.
///
/// `Rasterizer::page_size` reports pixels at the scale it is given, so asking at
/// scale 1.0 asks for points — one point per pixel, with no conversion to undo.
fn page_points(rasterizer: &Rasterizer, number: usize) -> (f64, f64) {
    let size = rasterizer
        .page_size(
            number,
            &strict_ooxml_pdf::raster::RasterOptions {
                scale: 1.0,
                ..strict_ooxml_pdf::raster::RasterOptions::default()
            },
        )
        .expect("the rasterizer sees a page the reader does");
    (f64::from(size.width), f64::from(size.height))
}

/// The scale that puts a page onto a `width × height` grid.
///
/// `hayro` sizes its pixmap as `floor(points * scale)`, so the scale that lands
/// *on* a grid is `grid / points` up to a floating-point hair, and one pixel
/// short is as likely as one pixel long. Half a pixel of slack makes the result
/// at least the grid in both axes, and [`crop`] takes the window back to it.
///
/// The grid is not the same for every fixture and that is a fact about the
/// corpus, not a bug: every reference is 816 × 1056, which is 96 dpi for a page
/// that is Letter and 144 dpi for `strict-stage5b`, whose content sits on a
/// 408 × 528 pt page. Each page is asked for the scale that reproduces its own
/// reference, which is what keeps the comparison pixel-for-pixel.
fn scale_for(points: (f64, f64), width: usize, height: usize) -> f64 {
    let by_width = (width as f64 + 0.5) / points.0;
    let by_height = (height as f64 + 0.5) / points.1;
    by_width.min(by_height)
}

/// Rasterizes page `number` onto the reference's own pixel grid.
fn rasterize_onto(rasterizer: &Rasterizer, number: usize, grid: &Gray) -> Gray {
    let (width, height) = (grid.width(), grid.height());
    let png = rasterizer
        .page_png(
            number,
            &strict_ooxml_pdf::raster::RasterOptions {
                scale: scale_for(page_points(rasterizer, number), width, height),
                ..strict_ooxml_pdf::raster::RasterOptions::default()
            },
        )
        .expect("rasterize");
    let raw = decode_png_gray(&png).expect("the rasterizer's PNG decodes");
    crop(&raw, width, height).unwrap_or_else(|| {
        panic!(
            "page {number} rasterized {}x{}, which is smaller than the reference's {width}x{height}",
            raw.width(),
            raw.height()
        )
    })
}

/// SC-6 criterion 1: our own reader opens what our own writer wrote, with the
/// page count intact and nothing lost on the way.
///
/// The loss ids that matter here are the ones that would make a page *look*
/// right while being wrong: an image the reader could not carry is a blank space
/// where the reference has a picture, and an uncarried filter is an image decoded
/// to nothing. Both are listed by name so that a document legitimately losing
/// something has to say which thing it was.
#[test]
fn our_own_pdf_reads_back_without_loss() {
    let policy = GatePolicy::shared();
    let mut checked = 0usize;
    let mut skipped: Vec<String> = Vec::new();
    let mut compared = 0usize;
    let mut blank: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(strict_dir()).expect("the corpus") {
        let path = entry.expect("a corpus entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("docx") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default();
        let Some(rendered) = round_trip(&path) else {
            skipped.push(name.to_owned());
            continue;
        };
        assert_eq!(
            rendered.written_pages, rendered.read_pages,
            "{name}: the writer made {} pages and our reader finds {}",
            rendered.written_pages, rendered.read_pages
        );
        if rendered.losses.is_empty() && rendered.characters == 0 {
            // A page with neither text nor a recorded loss is the shape a silent
            // drop takes, so it is never ignored — but it is not automatically a
            // failure either: a document whose content is a field instruction, or
            // a genuinely empty page, reads that way and is correct. Named, so a
            // new one is a thing a person looks at.
            blank.push(name.to_owned());
        }
        let forbidden: Vec<&str> = rendered
            .losses
            .iter()
            .map(String::as_str)
            .filter(|loss| {
                loss.starts_with("pdf.image.unsupported")
                    || loss.starts_with("pdf.image.missing")
                    || loss.contains("is not carried")
            })
            .collect();
        assert!(
            forbidden.is_empty(),
            "{name}: reading our own PDF lost something the gate depends on:\n  {}",
            forbidden.join("\n  ")
        );
        // `place_pages` is the layout's page count and `strict-profile` has two
        // pages against one reference set per document; the invariant that
        // matters here is the one above, plus the reference count below. A
        // document with no `refs/` directory at all is simply not a gated
        // fixture — the corpus holds plenty of those, and the gate says so
        // rather than looking them up.
        let reference_dir = strict_dir().join(format!("refs/{name}"));
        if reference_dir.is_dir() && !policy.is_page_count_only(name) {
            let references = reference_pages(&reference_dir);
            if !references.is_empty() {
                compared += 1;
                assert_eq!(
                    rendered.written_pages,
                    references.len(),
                    "{name}: the PDF has {} pages and the references have {}",
                    rendered.written_pages,
                    references.len()
                );
            }
        }
        checked += 1;
    }
    if !skipped.is_empty() {
        eprintln!(
            "skipped {} document(s) that are not Strict and therefore have no Strict PDF to \
             read back: {}",
            skipped.len(),
            skipped.join(", ")
        );
    }
    assert!(
        checked > 10,
        "only {checked} fixture(s) were checked and {} were skipped — a corpus this size has \
         Strict documents in it, so a run this short is a gate that has stopped looking",
        skipped.len()
    );
    assert!(
        compared >= 9,
        "only {compared} document(s) were compared against references, so the page-count \
         invariant is barely being checked"
    );
}

/// The per-page ink-extent drift each gated page of the **PDF** path is allowed
/// to keep, in px, as `(document, page, top, bottom)`.
///
/// This is the fine grain the class bounds cannot give. `strict-stage5c`'s
/// `max_extent_px` is 34 and its two pages measure 21 and 30, so four pixels of
/// drift on either one would pass the class and fail here — which is the whole
/// point: a bound fitted to the first failure names nothing, and a bound that
/// contains the worst page by four pixels rejects nothing.
///
/// Every value is the current measurement. Lowering one is how progress is
/// claimed; raising one is a decision somebody has to make on purpose, in a
/// reviewable diff next to the number it replaces. The SVG gate's equivalent
/// (`EXTENT_RATCHET` in `render-svg/tests/ssim.rs`) is the same idea for the other
/// backend, and the two tables are allowed to differ because the two renderers
/// antialias a long thin stroke differently.
const EXTENT_RATCHET: &[(&str, usize, f64, f64)] = &[
    ("strict-text", 0, 0.0, 1.0),
    ("strict-text-grid", 0, 0.0, 1.0),
    ("strict-stage5", 0, 0.0, -1.0),
    ("strict-stage5", 1, 1.0, 0.0),
    // -1 -> +2: this page's ink is mostly a page border, and a border's last
    // substantial row is the bottom edge, where one renderer puts the stroke's
    // antialiasing on one side and the other puts it on the other.
    ("strict-stage5b", 0, -1.0, 2.0),
    ("05-strict-math-simple", 0, 0.0, 3.0),
    ("06-strict-math-display", 0, 0.0, 6.0),
    ("07-strict-drawingml-shapes", 0, 0.0, 1.0),
    ("10-strict-math-eqarr", 0, 0.0, 7.0),
    ("strict-stage5c", 0, 1.0, 21.0),
    // 30 -> 28 -> 29 on this page, and every pixel of the move is accounted for.
    //
    // The stage-5C fixture is ours, and the XSD gate found markup in it that the
    // official schema rejects. Fixing the fixture changed the page, twice:
    //
    //   - `CT_BorderBox` holds exactly ONE `m:e`; the fixture asked for two, so
    //     the writer emitted two and the schema called the second unexpected. The
    //     second argument's ink was never in the WPS reference either, because WPS
    //     had already dropped it - so writing one moved us TOWARDS the reference,
    //     by two pixels. The reference page needs no regeneration.
    //   - `m:e` is element-only, so the ` + ` and ` = ` of the equation array were
    //     character data between runs and the schema rejected them. They now live
    //     in `m:t`, where the ink belongs, and the line is a pixel taller: back
    //     out by one.
    //
    // The pin is not a target and the page's SSIM (0.9513) is the number that
    // judges the rendering; what moved here is a fixture that was carrying markup
    // its own schema rejects.
    ("strict-stage5c", 1, 7.0, 29.0),
];

/// SC-6 criteria 2–4: every gated page, compared the way the SVG gate compares.
///
/// One document's worst page decides it, exactly as in the SVG gate: a document
/// whose second page lost its text has not rendered correctly on average.
#[test]
fn our_pdf_matches_the_wps_references() {
    let policy = GatePolicy::shared();
    let refs = strict_dir().join("refs");
    let mut graded = 0usize;
    let mut amber: Vec<String> = Vec::new();
    let mut structural_failures: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&refs).expect("read the reference directory") {
        let dir = entry.expect("a reference entry").path();
        if !dir.is_dir() {
            continue;
        }
        let name = dir
            .file_name()
            .and_then(|value| value.to_str())
            .expect("a document name")
            .to_owned();
        if !policy.ssim_documents().any(|entry| entry == name) {
            continue;
        }
        let docx = strict_dir().join(format!("{name}.docx"));
        assert!(docx.is_file(), "missing fixture {}", docx.display());
        let Some(rendered) = round_trip(&docx) else {
            continue;
        };
        let rasterizer = PdfDocument::open(&rendered.bytes, PdfLimits::default())
            .expect("open for rasterizing")
            .rasterizer()
            .expect("a rasterizer for our own PDF");

        let limits = policy.pdf_limits_for(&name);
        let mut comparisons: Vec<Comparison> = Vec::new();
        let mut structures: Vec<(usize, Result<StructuralFidelity, String>)> = Vec::new();
        for (index, reference_path) in reference_pages(&dir).iter().enumerate() {
            let reference = load_png_gray(reference_path).expect("the committed reference");
            let candidate = rasterize_onto(&rasterizer, index + 1, &reference);
            structures.push((
                index,
                structural_fidelity_with(&reference, &candidate, &limits),
            ));
            comparisons.push(Comparison::new(reference, candidate));
        }
        assert!(!comparisons.is_empty(), "{name}: no page to compare");

        let worst = strict_ooxml_fidelity::worst_score(&comparisons).expect("a score");
        eprintln!(
            "{name} [class {}]: worst SSIM = {worst:.4} \
             (threshold {}, margin {margin:+.4}, needs {needed:+.4})",
            policy.class_of(&name).unwrap_or("unclassified"),
            policy.pdf.ssim,
            margin = worst - policy.pdf.ssim,
            needed = policy.pdf.margin
        );
        structural_failures.extend(report_structure(&name, &structures));
        if grade(&name, worst, &policy.pdf) {
            amber.push(name.clone());
        }
        graded += 1;
    }
    assert!(graded > 0, "no SSIM-gated document was present");
    assert_amber_register_is_exact(&mut amber, &policy.pdf.amber, "pdf");
    // Every structural failure is named at once. A gate that stops at the first
    // bad page reports one problem and hides the rest.
    assert!(
        structural_failures.is_empty(),
        "{} page(s) outside the {} class bounds:\n  {}",
        structural_failures.len(),
        policy.class_of("strict-text").unwrap_or("?"),
        structural_failures.join("\n  ")
    );
}

/// Prints the structural numbers of every page and collects the violations.
///
/// The numbers are printed whether or not the page passes: a gate that reports
/// only the first bad page hides the rest, which is how a class ends up quietly
/// out of tolerance on three documents while the log names one.
fn report_structure(
    name: &str,
    structures: &[(usize, Result<StructuralFidelity, String>)],
) -> Vec<String> {
    let mut failures = Vec::new();
    for (page, structure) in structures {
        match structure {
            Ok(value) => eprintln!("{}", measurement_line(*page, value)),
            Err(reason) => {
                eprintln!("  page {page}: STRUCTURAL FAIL: {reason}");
                failures.push(format!("{name} page {page}: {reason}"));
            }
        }
    }
    failures
}

/// Grades one document's worst-page SSIM; returns `true` when it is amber.
///
/// Below the threshold is a regression and fails. Inside the margin band the
/// document passes only because it is written down in the policy's amber list, so
/// a new fixture cannot join the gate just below the line without being
/// registered on purpose.
///
/// # Panics
///
/// If `worst` is below the threshold, or is inside the margin band without being
/// registered.
fn grade(name: &str, worst: f64, thresholds: &strict_ooxml_fidelity::Thresholds) -> bool {
    assert!(
        worst >= thresholds.ssim,
        "{name}: worst SSIM {worst:.4} < {}",
        thresholds.ssim
    );
    if worst - thresholds.ssim >= thresholds.margin {
        return false;
    }
    assert!(
        thresholds.amber.iter().any(|entry| entry == name),
        "{name}: worst SSIM {worst:.4} clears {} by only {:.4}, below the {:.2} margin, \
         and is not registered in the policy's amber list",
        thresholds.ssim,
        worst - thresholds.ssim,
        thresholds.margin
    );
    true
}

/// The amber register must be exact in both directions, or it rots.
///
/// A stale entry keeps a fixed document looking like outstanding debt, and a
/// missing one lets a new near-miss through without anybody deciding to allow it.
/// The per-document check in the gate rejects the unregistered case; this rejects
/// the stale one.
fn assert_amber_register_is_exact(observed: &mut [String], registered: &[String], which: &str) {
    if !observed.is_empty() {
        eprintln!(
            "AMBER [{which}] (clears the threshold, not the margin): {}",
            observed.join(", ")
        );
    }
    observed.sort();
    let mut expected: Vec<String> = registered.to_vec();
    expected.sort_unstable();
    assert_eq!(
        observed, expected,
        "the {which} amber list is out of date: the documents actually inside the margin \
         band have changed (amber is the observed set)"
    );
}

/// The extent ratchet, and its coverage: every gated page must be pinned.
///
/// Two directions, because both have bitten. A pinned value that moves is a
/// regression the class bounds may be too loose to see; a gated page with no
/// pinned value escapes the fine-grained check entirely and the class bound is
/// all that holds it.
#[test]
fn the_extent_ratchet_pins_every_gated_page_of_the_pdf_path() {
    let policy = GatePolicy::shared();
    for (document, page, top, bottom) in EXTENT_RATCHET {
        let docx = strict_dir().join(format!("{document}.docx"));
        assert!(docx.is_file(), "{document} is ratcheted but has no fixture");
        let Some(rendered) = round_trip(&docx) else {
            continue;
        };
        let rasterizer = PdfDocument::open(&rendered.bytes, PdfLimits::default())
            .expect("open for rasterizing")
            .rasterizer()
            .expect("a rasterizer for our own PDF");
        let reference =
            load_png_gray(&reference_pages(&strict_dir().join(format!("refs/{document}")))[*page])
                .expect("the committed reference");
        let candidate = rasterize_onto(&rasterizer, page + 1, &reference);
        let top_delta = extent_delta(&reference, &candidate, Edge::Top);
        let bottom_delta = extent_delta(&reference, &candidate, Edge::Bottom);
        assert_eq!(
            top_delta, *top,
            "{document} page {page}: top ink edge moved (was {top}, now {top_delta})"
        );
        assert_eq!(
            bottom_delta, *bottom,
            "{document} page {page}: bottom ink edge moved (was {bottom}, now {bottom_delta})"
        );
    }

    let mut gated: Vec<(String, usize)> = Vec::new();
    for name in policy.ssim_documents() {
        if policy.is_page_count_only(name) {
            continue;
        }
        let dir = strict_dir().join(format!("refs/{name}"));
        for (index, _) in reference_pages(&dir).iter().enumerate() {
            gated.push((name.to_owned(), index));
        }
    }
    let pinned: std::collections::BTreeSet<(String, usize)> = EXTENT_RATCHET
        .iter()
        .map(|(name, page, _, _)| ((*name).to_owned(), *page))
        .collect();
    for entry in &gated {
        assert!(
            pinned.contains(entry),
            "{entry:?} is gated but not pinned in the PDF gate's EXTENT_RATCHET"
        );
    }
    let mut stale: Vec<&(String, usize)> = pinned
        .iter()
        .filter(|entry| !gated.contains(entry))
        .collect();
    stale.sort();
    assert!(
        stale.is_empty(),
        "the PDF gate's EXTENT_RATCHET pins pages that are no longer gated: {stale:?} \
         — a pin for a page nobody compares is a number that protects nothing"
    );
}

/// The gate must be able to fail on the very pages it passes.
///
/// A gate that has never rejected anything is a set of numbers nobody has seen
/// work. The pair is: this page passes its own bounds, and a blank render and a
/// drifted one do not — under the *PDF* gate's bounds for that class, because a
/// demonstration run against somebody else's bounds proves nothing about the
/// bounds in force.
#[test]
fn the_pdf_gate_can_fail_on_a_page_it_passes() {
    let policy = GatePolicy::shared();
    let reference = load_png_gray(&reference_pages(&strict_dir().join("refs/strict-text"))[0])
        .expect("the reference");
    let rasterizer = {
        let rendered =
            round_trip(&strict_dir().join("strict-text.docx")).expect("a Strict fixture");
        PdfDocument::open(&rendered.bytes, PdfLimits::default())
            .expect("open")
            .rasterizer()
            .expect("a rasterizer")
    };
    let candidate = rasterize_onto(&rasterizer, 1, &reference);
    let limits = policy.pdf_limits_for("strict-text");
    assert!(
        structural_fidelity_with(&reference, &candidate, &limits).is_ok(),
        "strict-text page 0 must pass its own PDF gate for the rest of this test to mean \
         anything"
    );

    // A blank render: the ink ratio rejects it.
    let blank = Gray::filled(reference.width(), reference.height(), 1.0);
    assert!(
        structural_fidelity_with(&reference, &blank, &limits).is_err(),
        "strict-text: a blank render must be rejected — a pipeline that rasterizes nothing \
         is not a pipeline that matched"
    );

    // A lost band: the top third of the page's ink, gone.
    let cut = reference.height() / 3;
    let stripped = blank_above(&candidate, cut);
    assert!(
        structural_fidelity_with(&reference, &stripped, &limits).is_err(),
        "strict-text: losing the top third of the page's ink must be rejected"
    );

    // A whole page moved down: the alignment bound rejects it.
    let moved = candidate.shifted_down(20);
    assert!(
        structural_fidelity_with(&reference, &moved, &limits).is_err(),
        "strict-text: a 20px vertical drift must be rejected"
    );
}

/// The page with paper in place of its top `rows` rows.
///
/// This is what a lost band looks like from the pipeline's side: everything above
/// a line is missing, and nothing is *wrong* with what is left — which is why
/// the ink ratio alone does not catch it and the extent and the row profile do.
fn blank_above(page: &Gray, rows: usize) -> Gray {
    let mut pixels = page.pixels().to_vec();
    for pixel in pixels.iter_mut().take(rows * page.width()) {
        *pixel = 1.0;
    }
    Gray::new(page.width(), page.height(), pixels)
}

/// The rasterizer must reproduce its own page, so the gate is not comparing two
/// different pictures.
///
/// `strict-ooxml-pdf/tests/raster.rs` already checks determinism on a
/// hand-written file; this says the same thing about the file *this* pipeline
/// produces, which is the only one the gate above compares.
#[test]
fn rasterizing_our_own_pdf_is_deterministic() {
    let rendered = round_trip(&strict_dir().join("strict-text.docx")).expect("a Strict fixture");
    let rasterizer = PdfDocument::open(&rendered.bytes, PdfLimits::default())
        .expect("open")
        .rasterizer()
        .expect("a rasterizer");
    let grid = load_png_gray(&reference_pages(&strict_dir().join("refs/strict-text"))[0])
        .expect("the reference");
    let first = rasterize_onto(&rasterizer, 1, &grid);
    let second = rasterize_onto(&rasterizer, 1, &grid);
    assert_eq!(first, second, "rasterization must be byte-deterministic");
}

/// The scale that puts a page on a grid must be at least the grid.
///
/// This is the invariant the whole comparison rests on and it is easy to break by
/// changing how `page_points` or `scale_for` read the rasterizer: a scale a
/// hundredth too small floors the pixmap one pixel short, the crop is refused,
/// and every page in the gate fails at once for a reason that has nothing to do
/// with fidelity.
#[test]
fn a_page_lands_on_the_reference_grid() {
    for name in [
        "strict-text",
        "strict-stage5b",
        "07-strict-drawingml-shapes",
    ] {
        let references = reference_pages(&strict_dir().join(format!("refs/{name}")));
        let Some(first) = references.first() else {
            continue;
        };
        let grid = load_png_gray(first).expect("the reference");
        let rendered = round_trip(&strict_dir().join(format!("{name}.docx")))
            .expect("a gated fixture is Strict");
        let rasterizer = PdfDocument::open(&rendered.bytes, PdfLimits::default())
            .expect("open")
            .rasterizer()
            .expect("a rasterizer");
        let points = page_points(&rasterizer, 1);
        let scale = scale_for(points, grid.width(), grid.height());
        let size: Size = rasterizer
            .page_size(
                1,
                &strict_ooxml_pdf::raster::RasterOptions {
                    scale,
                    ..strict_ooxml_pdf::raster::RasterOptions::default()
                },
            )
            .expect("the page size");
        assert!(
            size.width >= grid.width() as u32 && size.height >= grid.height() as u32,
            "{name}: at scale {scale} the page is {}x{} px, the grid is {}x{}",
            size.width,
            size.height,
            grid.width(),
            grid.height()
        );
    }
}

/// Every graded document must be graded, on both sides.
///
/// A document in a class but with no references cannot be compared, and a
/// reference directory in no class is compared by nobody: both are ways for the
/// gate to look complete while covering less than the file says.
#[test]
fn every_graded_document_has_references_and_back() {
    let policy = GatePolicy::shared();
    for name in policy.ssim_documents() {
        assert!(
            strict_dir().join(format!("refs/{name}")).is_dir(),
            "class membership names {name}, which has no references"
        );
        assert!(
            strict_dir().join(format!("{name}.docx")).is_file(),
            "class membership names {name}, which has no fixture"
        );
    }
    for entry in std::fs::read_dir(strict_dir().join("refs")).expect("refs") {
        let dir = entry.expect("a reference entry").path();
        if !dir.is_dir() {
            continue;
        }
        let name = dir.file_name().and_then(|v| v.to_str()).unwrap_or_default();
        if policy.is_page_count_only(name) {
            continue;
        }
        assert!(
            policy.class_of(name).is_some(),
            "{name} has references but is in neither a class nor page_count_only"
        );
    }
}

/// The score the gate reads is the score the whole pipeline produces.
///
/// A single page, end to end, with the number printed: this is the one test a
/// person reads to find out what the PDF gate currently measures, and it fails if
/// the two backends' arithmetic is ever wired up differently.
#[test]
fn one_page_end_to_end_is_named() {
    let name = "strict-text";
    let reference = load_png_gray(&reference_pages(&strict_dir().join(format!("refs/{name}")))[0])
        .expect("the reference");
    let rendered =
        round_trip(&strict_dir().join(format!("{name}.docx"))).expect("a gated fixture is Strict");
    let rasterizer = PdfDocument::open(&rendered.bytes, PdfLimits::default())
        .expect("open")
        .rasterizer()
        .expect("a rasterizer");
    let candidate = rasterize_onto(&rasterizer, 1, &reference);
    let score = ssim(&reference, &candidate);
    eprintln!(
        "{name} page 0: ssim {score:.4}, {} characters read back, {} loss(es)",
        rendered.characters,
        rendered.losses.len()
    );
    assert!(
        score >= GatePolicy::shared().pdf.ssim,
        "{name}: {score:.4} is below the threshold"
    );
}
