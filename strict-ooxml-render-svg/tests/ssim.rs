//! SSIM harness against WPS-produced reference PNGs
//! (`STAGE-4-RENDER-FIDELITY.md` S4F.5).
//!
//! Our SVG is rasterized with `resvg` using the bundled metric-compatible fonts,
//! converted to grayscale and compared page-by-page with the committed WPS
//! references (`strict-ooxml-core/tests/strict/refs/<doc>/page_N.png`). The
//! gate is the *worst* per-page SSIM plus the page-count invariant.
//!
//! References for chart/diagram documents (`strict-profile`) are page-count
//! checked only: Stage 4 cannot rasterize DrawingML charts, so per-pixel
//! comparison is not meaningful there (see `docs/stage-4-report.md`).
//!
//! # The arithmetic is not here
//!
//! SSIM, the ink profiles, the structural checks and the policy file live in
//! `strict-ooxml-fidelity`, because stage 8's PDF gate measures the *same
//! references* through a *second renderer* (`strict-ooxml-pdf/tests/pdf_pixels.rs`).
//! Two copies of SSIM would be two definitions of SSIM, and a threshold quoted
//! from one gate and applied to the other would be a number about a different
//! quantity. What stays here is what is specific to rasterizing an SVG:
//! `resvg`, the bundled faces, and the per-page ratchet.

#![allow(
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::float_cmp,
    clippy::similar_names,
    // Pixel arithmetic over buffers: a page is 816 × 1056, far below 2^53, and a
    // reference's dimensions come from a PNG header rather than from a count.
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::needless_borrows_for_generic_args
)]

use std::path::{Path, PathBuf};

use strict_ooxml_core::opc::{OpenOptions, Package};
use strict_ooxml_fidelity::{
    alignment_search_range, extent_delta, load_png_gray, measurement_line, structural_fidelity,
    structural_fidelity_with, worst_score, Comparison, Edge, GatePolicy, Gray, StructuralFidelity,
    StructuralLimits, Thresholds,
};
use strict_ooxml_render_svg::{render_with_media, RenderOptions};
use strict_ooxml_wml::{parse_document, ParseOptions};

/// The gate policy, read from `coverage/render-gates.toml`.
///
/// The SVG backend's half of it: the same file carries the PDF backend's
/// thresholds and its per-class structural bounds, and this gate reads only the
/// parts that are about rasterizing an SVG.
fn policy() -> &'static GatePolicy {
    GatePolicy::shared()
}

/// The structural limits for the mixed Stage-5 fixture
/// (`STAGE-5-REWORK-1` R5-1): the rasterizer-independent checks stay enforced
/// (ink coverage / blank page, alignment shift, ink centroid); only the
/// row/column profile-correlation thresholds are loosened, because the sparse
/// mixed table/footnote content makes them fall slightly below the text-only
/// thresholds.
fn stage5_limits() -> StructuralLimits {
    policy().limits_for("strict-stage5")
}

/// The Stage-5B floating-drawing limits (`STAGE-5B-REWORK-1` 5B-2).
fn stage5b_limits() -> StructuralLimits {
    policy().limits_for("strict-stage5b")
}

/// The Stage-5C formula limits (`STAGE-5C-REWORK-1` C1/C4).
fn stage5c_limits() -> StructuralLimits {
    policy().limits_for("strict-stage5c")
}
/// The per-fixture ink-extent drift each gated page is allowed to keep, in px.
///
/// This is the ratchet: it records the measured drift of every gated page
/// against its pinned WPS reference, so a layout change that makes a page
/// *worse* fails even while the class bound is still loose. Each value must be
/// the current measurement — lowering one is the way to claim progress, and it
/// is the number the Stage-5C rework has to move down.
const EXTENT_RATCHET: &[(&str, usize, f64, f64)] = &[
    ("strict-text", 0, 0.0, 1.0),
    ("strict-text-grid", 0, 0.0, 1.0),
    ("strict-stage5", 0, 0.0, -1.0),
    ("strict-stage5", 1, 1.0, 0.0),
    ("strict-stage5b", 0, -1.0, 1.0),
    // 17 -> 21: a stacked fraction's parts are now placed **relative to its rule**,
    // each carrying its own extent into that placement, so a fraction is taller
    // than the symmetric version it replaces — and on this page, which is three
    // display formulas of nothing but fractions and radicals, that is 4 px more
    // page. It is not claimed as progress, and the pin is not a target: what the
    // change buys is on the other two measurements. The rule used to be drawn
    // through the numerator's descenders, and the page's SSIM went from 0.9496 —
    // below the threshold, i.e. failing — to 0.9513, legible and measured. The
    // layout's own invariant is now a unit test rather than a number of pixels.
    ("strict-stage5c", 0, 1.0, 21.0),
    // AUD-50: several `m:oMath` children of one `m:oMathPara` are laid out one
    // display line each (was: nodes merged into one horizontal expression).
    // That adds roughly one formula height per multi-equation paragraph, so
    // page 1's bottom ink edge moves by ≈240 px and its top edge settles at
    // +3. Pins track the new stacking; `coverage/render-gates.toml` carries
    // the matching override. Waiver `STAGE5C-P1-LAYOUT` tracks bringing the
    // WPS reference (or a follow-up layout pass) back inside the old band.
    ("strict-stage5c", 1, 3.0, 240.0),
    // -2 -> 4: the same trade on the simplest formula page, where one fraction gets
    // 6 px taller. The page's SSIM went from 0.9632 to 0.9747.
    ("05-strict-math-simple", 0, 0.0, 4.0),
    // -10 -> 5: the same trade on the display-math page, and the largest single
    // change here — 15 px, because three stacked fractions each get taller. The
    // page's SSIM went from 0.9496 (below the threshold) to 0.9762, and its
    // structural failures are gone.
    ("06-strict-math-display", 0, 0.0, 5.0),
    ("07-strict-drawingml-shapes", 0, 0.0, 1.0),
    // -3 -> 7: the same trade on the equation-array page. SSIM 0.9573 -> 0.9695.
    ("10-strict-math-eqarr", 0, 0.0, 7.0),
];

/// Repository path to the real Strict corpus (`tests/strict/`).
fn strict_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../strict-ooxml-core/tests/strict")
}

/// Repository path to the bundled fonts.
fn fonts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts")
}

/// Rasterizes `svg` onto a `width × height` page via `resvg` with bundled fonts.
///
/// The page is built through [`strict_ooxml_fidelity::from_rgba8`], which is the
/// same compositing and the same luma the PDF gate's candidate goes through: two
/// gates that disagree about what a pixel is are two gates.
fn rasterize_gray(svg: &str, width: u32, height: u32, fonts: &Path) -> Gray {
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_fonts_dir(fonts);
    let tree = resvg::usvg::Tree::from_str(svg, &options).expect("parse our SVG");
    let size = tree.size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).expect("allocate raster target");
    let transform = resvg::tiny_skia::Transform::from_scale(
        f32::from(u16::try_from(width).expect("width fits u16")) / size.width(),
        f32::from(u16::try_from(height).expect("height fits u16")) / size.height(),
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    strict_ooxml_fidelity::from_rgba8(pixmap.data(), width as usize, height as usize)
}

/// Returns the reference PNGs of a document directory, sorted by page number.
fn reference_pages(dir: &Path) -> Vec<PathBuf> {
    let mut pages: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("read reference dir")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("png"))
        .collect();
    pages.sort();
    pages
}

/// Prints the structural numbers of every page of a document and returns the
/// pages that violated a bound.
///
/// The numbers are printed whether or not the page passes, and the caller
/// collects the failures instead of panicking here: a gate that stops at the
/// first bad page reports one problem and hides the rest, which is how a class
/// ends up quietly failing three documents while the log names one.
fn report_structure(
    name: &str,
    structures: &[(usize, Result<StructuralFidelity, String>)],
) -> Vec<String> {
    let mut failures = Vec::new();
    for (page, structure) in structures {
        match structure {
            Ok(structure) => eprintln!("{}", measurement_line(*page, structure)),
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
/// Below `thresholds.ssim` is a regression and panics. In
/// `[ssim, ssim + margin)` the document passes only because it is written down in
/// that table's `amber` list, so a new fixture cannot join the gate just below
/// the line without being registered on purpose.
///
/// # Panics
///
/// If `worst` is below the threshold, or is inside the margin band without
/// being registered.
fn grade_ssim(name: &str, worst: f64, thresholds: &Thresholds) -> bool {
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
        "{name}: worst SSIM {worst:.4} clears {} by only {margin_b:.4}, \
         below the {margin:.2} margin, and is not registered in that table's amber list",
        thresholds.ssim,
        margin_b = worst - thresholds.ssim,
        margin = thresholds.margin
    );
    true
}

#[test]
fn matches_wps_references() {
    let refs = strict_dir().join("refs");
    assert!(refs.is_dir(), "missing references at {}", refs.display());
    let fonts = fonts_dir();
    let mut checked = 0u32;
    let mut gated = 0u32;
    let mut amber: Vec<String> = Vec::new();
    let mut structural_failures: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&refs).expect("read refs dir") {
        let dir = entry.expect("refs entry").path();
        if !dir.is_dir() {
            continue;
        }
        let name = dir
            .file_name()
            .and_then(|value| value.to_str())
            .expect("document name")
            .to_owned();
        let docx = strict_dir().join(format!("{name}.docx"));
        assert!(docx.is_file(), "missing fixture {}", docx.display());
        let package = Package::open_path(&docx, &OpenOptions::default()).expect("open package");
        let document = parse_document(&package, &ParseOptions::default()).expect("parse document");
        let pages = render_with_media(&document, &RenderOptions::default(), Some(&package))
            .expect("render document");
        let ref_pages = reference_pages(&dir);
        assert_eq!(
            pages.len(),
            ref_pages.len(),
            "{name}: rendered page count differs from the reference"
        );

        let mut comparisons = Vec::new();
        let mut structures: Vec<(usize, Result<StructuralFidelity, String>)> = Vec::new();
        for (page, reference_path) in pages.iter().zip(&ref_pages) {
            let reference = load_png_gray(reference_path).expect("the committed reference");
            let candidate = rasterize_gray(
                &page.svg,
                reference.width() as u32,
                reference.height() as u32,
                &fonts,
            );
            if policy().is_page_count_only(&name) {
                // The page-count invariant is all that is meaningful here: the
                // content is raster data this renderer does not produce, so the
                // reference and our render legitimately differ completely.
                eprintln!(
                    "  page {}: page count only (raster content out of scope)",
                    page.index
                );
                continue;
            }
            let limits = policy().limits_for(&name);
            structures.push((
                page.index,
                structural_fidelity_with(&reference, &candidate, &limits),
            ));
            comparisons.push(Comparison::new(reference, candidate));
        }
        checked += 1;

        if policy().ssim_documents().any(|entry| entry == name) && !comparisons.is_empty() {
            let worst = worst_score(&comparisons).expect("at least one comparison");
            eprintln!(
                "{name} [class {}]: worst SSIM = {worst:.4} (margin {margin:+.4}, needs {needed:+.4})",
                policy().class_of(&name).unwrap_or("unclassified"),
                margin = worst - policy().svg.ssim,
                needed = policy().svg.margin
            );
            structural_failures.extend(report_structure(&name, &structures));
            if grade_ssim(&name, worst, &policy().svg) {
                amber.push(name.clone());
            }
            gated += 1;
        }
    }
    assert!(checked > 0, "no reference documents were checked");
    assert!(gated > 0, "no SSIM-gated documents were present");
    assert_amber_register_is_exact(&mut amber, &policy().svg.amber);
    // Every structural failure is named at once. A gate that panics on the
    // first bad page reports one document and hides the rest, and the way the
    // `formulas` class ended up quietly out of tolerance on three documents is
    // that the earlier bounds were fitted to whichever one failed first.
    assert!(
        structural_failures.is_empty(),
        "{} page(s) outside the {} class bounds:\n  {}",
        structural_failures.len(),
        policy()
            .class_of(structural_failures[0].split(" page").next().unwrap_or(""))
            .unwrap_or("?"),
        structural_failures.join("\n  ")
    );
}

/// The amber register must be exact in both directions, or it rots.
///
/// A stale entry keeps a fixed document looking like outstanding debt, and a
/// missing one lets a new near-miss through without anybody deciding to allow it.
/// The per-document check in the gate above rejects the unregistered case; this
/// rejects the stale one, and it is why closing `07-AMBER` had to happen in the same
/// commit that took `07-strict-drawingml-shapes` out of the list.
fn assert_amber_register_is_exact(observed: &mut [String], registered: &[String]) {
    if !observed.is_empty() {
        eprintln!(
            "AMBER [svg] (clears the threshold, not the {:.2} margin): {}",
            policy().svg.margin,
            observed.join(", ")
        );
    }
    observed.sort();
    let mut expected: Vec<String> = registered.to_vec();
    expected.sort_unstable();
    assert_eq!(
        observed, expected,
        "the amber list is out of date: the documents actually inside the margin band \
         have changed (amber is the observed set)"
    );
}

#[test]
fn rasterization_is_deterministic() {
    let refs = strict_dir().join("refs/strict-text");
    let Some(first_page) = reference_pages(&refs).into_iter().next() else {
        return;
    };
    let reference = load_png_gray(&first_page).expect("the reference");
    let pages = render("strict-text");
    let a = rasterize_gray(
        &pages[0].svg,
        reference.width() as u32,
        reference.height() as u32,
        &fonts_dir(),
    );
    let b = rasterize_gray(
        &pages[0].svg,
        reference.width() as u32,
        reference.height() as u32,
        &fonts_dir(),
    );
    assert_eq!(a, b, "rasterization must be byte-deterministic");
}

/// Builds a tiny text-like page: two dark rows of "ink", shifted by `dx` px.
fn text_like(width: usize, height: usize, rows: [usize; 2], dx: isize) -> Gray {
    let mut pixels = vec![1.0; width * height];
    for row in rows {
        for col in 0..width {
            let source = col as isize - dx;
            if source >= 10 && (source as usize) < width - 10 {
                pixels[row * width + col] = 0.0;
            }
        }
    }
    Gray::new(width, height, pixels)
}

#[test]
fn structural_check_rejects_blank_and_shifted_pages() {
    let (width, height) = (64, 96);
    let reference = text_like(width, height, [20, 60], 0);
    // Identical candidate passes.
    assert!(structural_fidelity(&reference, &reference).is_ok());
    // A blank page is rejected (ink ratio).
    let blank = Gray::filled(width, height, 1.0);
    assert!(structural_fidelity(&reference, &blank).is_err());
    // A 3px vertical shift is rejected, a 2px one is not.
    assert!(structural_fidelity(&reference, &text_like(width, height, [23, 63], 0)).is_err());
    assert!(structural_fidelity(&reference, &text_like(width, height, [22, 62], 0)).is_ok());
    // The same horizontally (R-1).
    assert!(structural_fidelity(&reference, &text_like(width, height, [20, 60], 3)).is_err());
    assert!(structural_fidelity(&reference, &text_like(width, height, [20, 60], 2)).is_ok());
}

#[test]
fn structural_check_rejects_blank_stage5_page() {
    // The relaxed Stage-5 limits must still reject a blank render (the hole
    // R5-1 flagged): the rasterizer-independent ink-coverage check applies.
    let (reference, _candidate) = rasterize_reference_pair("strict-stage5", 0);
    let blank = Gray::filled(reference.width(), reference.height(), 1.0);
    assert!(
        structural_fidelity_with(&reference, &blank, &stage5_limits()).is_err(),
        "a blank candidate must be rejected even with the relaxed Stage-5 limits"
    );
}

#[test]
fn structural_check_rejects_blank_stage5b_page() {
    // STAGE-5B-REWORK-1 5B-2: even with the relaxed 5B limits, a blank render
    // (or one with no shape ink) must be rejected by the mandatory ink check.
    let (reference, _candidate) = rasterize_reference_pair("strict-stage5b", 0);
    let (width, height) = (reference.width(), reference.height());
    assert!(
        width > 68 && height > 68,
        "the fixture is smaller than its border"
    );
    let blank = Gray::filled(width, height, 1.0);
    assert!(
        structural_fidelity_with(&reference, &blank, &stage5b_limits()).is_err(),
        "a blank candidate must be rejected even with the relaxed Stage-5B limits"
    );
    // A page-border-only render (shapes/group/text/picture missing) is also
    // under-inked relative to the reference and must be rejected.
    let mut pixels = vec![1.0; width * height];
    for y in 0..height {
        for x in 0..width {
            let on_border = (32..34).contains(&y)
                || (height.saturating_sub(34)..height.saturating_sub(32)).contains(&y)
                || (32..34).contains(&x)
                || (width.saturating_sub(34)..width.saturating_sub(32)).contains(&x);
            if on_border {
                pixels[y * width + x] = 0.0;
            }
        }
    }
    let border_only = Gray::new(width, height, pixels);
    assert!(
        structural_fidelity_with(&reference, &border_only, &stage5b_limits()).is_err(),
        "a page-border-only candidate must be rejected (shape ink missing)"
    );
}

/// STAGE-5C-REWORK-1 C4: the 5C bounds must reject a blank page and a
/// materially drifted one. The gate only means something if it can fail, and
/// it can only be shown to fail on a page that *does* pass otherwise — so the
/// pass/fail pair runs on `05-strict-math-simple`, the one fixture in the class
/// that meets the bounds.
///
/// The other class fixtures no longer pass under the **class** bounds, which is
/// the point of the tightened centroid: `06`, `07` and `strict-stage5c` are outside
/// it, and the gate says so by name. A rejection asserted against a page that
/// already fails proves nothing, so those documents are only checked for *not*
/// silently passing.
///
/// The bounds used here are **this page's own** — its class's, or its override if
/// it has one — and not the loosest set in the file. That distinction is the whole
/// reason the override exists: `strict-stage5c` carries a 26 px shift bound of its
/// own, and borrowing it here would make this control assert that a 20 px drift is
/// acceptable, which is the opposite of what it is for.
#[test]
fn structural_check_rejects_blank_and_shifted_stage5c_pages() {
    let (document, page) = ("05-strict-math-simple", 0usize);
    let limits = policy().limits_for(document);
    let (reference, candidate) = rasterize_reference_pair(document, page);
    // The unshifted render passes, so the rejections below are attributable.
    assert!(
        structural_fidelity_with(&reference, &candidate, &limits).is_ok(),
        "{document} page {page} must pass its own gate for the rest of this test to mean \
         anything"
    );
    // A blank render is rejected by the ink ratio.
    let blank = Gray::filled(reference.width(), reference.height(), 1.0);
    assert!(
        structural_fidelity_with(&reference, &blank, &limits).is_err(),
        "{document}: a blank render must be rejected"
    );
    // A vertical drift beyond the alignment bound is rejected.
    let shifted = candidate.shifted_down(20);
    assert!(
        structural_fidelity_with(&reference, &shifted, &limits).is_err(),
        "{document}: a 20px vertical drift must be rejected under its own bounds \
         ({} px shift allowed)",
        limits.max_shift_px
    );
}

/// The class must not be entirely out of tolerance.
///
/// If no fixture met the bounds, the rejection test above would pass for the
/// wrong reason, and the class would be describing nothing.
#[test]
fn at_least_one_formula_fixture_meets_the_class_bounds() {
    let mut meeting = Vec::new();
    let mut outside = Vec::new();
    for document in policy()
        .ssim_documents()
        .filter(|name| policy().class_of(name) == Some("formulas"))
    {
        let (reference, candidate) = rasterize_reference_pair(document, 0);
        let result = structural_fidelity_with(&reference, &candidate, &stage5c_limits());
        if result.is_ok() {
            meeting.push(document);
        } else {
            outside.push(format!("{document}: {result:?}"));
        }
    }
    assert!(
        !meeting.is_empty(),
        "no formula fixture meets the class bounds, so the gate is not discriminating: {outside:?}"
    );
}

/// Renders `name` and returns its page `page` as `(reference, candidate)` at the
/// reference's own resolution.
fn rasterize_reference_pair(name: &str, page: usize) -> (Gray, Gray) {
    let dir = strict_dir().join(format!("refs/{name}"));
    let reference = load_png_gray(&reference_pages(&dir)[page]).expect("the reference");
    let pages = render(name);
    let candidate = rasterize_gray(
        &pages[page].svg,
        reference.width() as u32,
        reference.height() as u32,
        &fonts_dir(),
    );
    (reference, candidate)
}

/// Lays a fixture out, with its media resolved.
fn render(name: &str) -> Vec<strict_ooxml_render_svg::Page> {
    let package = Package::open_path(
        &strict_dir().join(format!("{name}.docx")),
        &OpenOptions::default(),
    )
    .expect("open package");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    render_with_media(&document, &RenderOptions::default(), Some(&package)).expect("render")
}

#[test]
fn structural_check_rejects_real_horizontal_shift() {
    let (reference, candidate) = rasterize_reference_pair("strict-text", 0);
    // The unshifted render passes.
    assert!(structural_fidelity(&reference, &candidate).is_ok());
    // A horizontal drift of >= 3 px is rejected (R-1 criterion 1).
    for dx in [3isize, 5, 8, 10] {
        let shifted = candidate.shifted_right(dx);
        assert!(
            structural_fidelity(&reference, &shifted).is_err(),
            "dx={dx} must be rejected"
        );
    }
}

#[test]
fn structural_check_rejects_real_vertical_shift() {
    let (reference, candidate) = rasterize_reference_pair("strict-text-grid", 0);
    let shifted = candidate.shifted_down(3);
    assert!(
        structural_fidelity(&reference, &shifted).is_err(),
        "a 3px vertical shift must be rejected"
    );
}

/// The alignment search range must be strictly wider than the bound it is
/// checked against, or the shift check can never fail: the reported shift is
/// clamped to the search window, so a page that drifted further than the bound
/// would report a shift inside it. This pins the invariant for every limit set
/// rather than trusting the arithmetic at each call site.
#[test]
fn alignment_search_range_is_wider_than_every_bound() {
    for (name, limits) in [
        ("default", StructuralLimits::default()),
        ("mixed", stage5_limits()),
        ("graphics", stage5b_limits()),
        ("formulas", stage5c_limits()),
        (
            "formulas, own override",
            policy().limits_for("strict-stage5c"),
        ),
        ("text, PDF backend", policy().pdf_limits_for("strict-text")),
        (
            "graphics, PDF backend",
            policy().pdf_limits_for("strict-stage5b"),
        ),
    ] {
        assert!(
            alignment_search_range(&limits) > limits.max_shift_px,
            "{name}: search range {} must exceed the bound {}",
            alignment_search_range(&limits),
            limits.max_shift_px
        );
    }
}

/// The shift bound must reject a drift *beyond* the bound, not merely a drift
/// inside the search window. The 5C limits are the widest, so they are the
/// case that previously could not fail: a 20 px vertical drift is well outside
/// the ±12 px window the gate used to search, and the old check reported it as
/// an in-bounds shift.
#[test]
fn a_drift_beyond_the_shift_bound_is_rejected() {
    let (reference, candidate) = rasterize_reference_pair("strict-stage5c", 1);
    for drift in [20isize, 40, 80] {
        let shifted = candidate.shifted_down(drift);
        let result = structural_fidelity_with(&reference, &shifted, &stage5c_limits());
        assert!(
            result.is_err(),
            "a {drift}px vertical drift must be rejected, got {result:?}"
        );
    }
}

/// The ink extent is what catches a block that is a whole line too tall: the
/// centroid averages over the page and barely moves, and the correlation is
/// computed after the best alignment, so neither sees it. This is the real
/// defect behind the relaxed 5C limits.
#[test]
fn the_extent_check_sees_drift_the_centroid_does_not() {
    let (reference, candidate) = rasterize_reference_pair("strict-stage5c", 1);
    // Every bound except the extent one is disabled, so the extent check is
    // the only thing that can reject this page.
    let without_extent = StructuralLimits::unbounded();
    assert!(
        structural_fidelity_with(&reference, &candidate, &without_extent).is_ok(),
        "with the extent bound disabled the fixture must pass, \
         otherwise it is rejected by an unrelated bound and the test proves nothing"
    );
    // Stretched by 5 px about the middle: the two halves cancel in the
    // centroid, but both ink edges move.
    let stretched = candidate.stretched_vertically(5);
    let just_extent = StructuralLimits {
        max_extent_px: 2.0,
        ..without_extent
    };
    let error = structural_fidelity_with(&reference, &stretched, &just_extent)
        .expect_err("a 10px vertical stretch must be caught by the extent bound");
    assert!(
        error.contains("ink edge"),
        "the extent bound must be what rejects the stretch, got: {error}"
    );
}

/// The extent ratchet: every gated page's ink-extent drift is pinned to its
/// current measured value against the pinned WPS reference.
///
/// The class bounds in [`StructuralLimits`] are coarse — the 5C set is
/// carried by a single loose `max_extent_px` so that `05` (0 px) and
/// `strict-stage5c` page 2 (18 px) share one number. This test is the fine
/// grain: it fails the moment any page drifts *further*, so a layout change
/// cannot hide inside the loose bound, and it fails when a page gets better
/// and the pinned value was not lowered, which is how progress is claimed.
#[test]
fn structural_extent_ratchet_pins_the_known_drift() {
    for (document, page, top, bottom) in EXTENT_RATCHET {
        let (reference, candidate) = rasterize_reference_pair(document, *page);
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
}

/// The ratchet must cover every gated page: a fixture added to the gate
/// without a pinned extent would escape the fine-grained check.
#[test]
fn every_gated_page_is_pinned_by_the_extent_ratchet() {
    let mut gated: Vec<(String, usize)> = Vec::new();
    for name in policy().ssim_documents() {
        if policy().is_page_count_only(name) {
            continue;
        }
        let dir = strict_dir().join(format!("refs/{name}"));
        for (index, _) in reference_pages(&dir).into_iter().enumerate() {
            gated.push((name.to_owned(), index));
        }
    }
    let pinned: std::collections::BTreeSet<(String, usize)> = EXTENT_RATCHET
        .iter()
        .map(|(name, page, _, _)| ((*name).to_owned(), *page))
        .collect();
    for entry in gated {
        assert!(
            pinned.contains(&entry),
            "{entry:?} is gated but not pinned in EXTENT_RATCHET"
        );
    }
}

/// Every reference directory must declare its class.
///
/// An unclassified document silently inherits the strict defaults, which
/// sounds safe but is not: it means a fixture can be added to the corpus
/// without anyone deciding what it is held to, and the class it *should* have
/// had (the loosened `formulas` bounds, say) is never applied. The decision
/// has to be made on purpose, in the policy file.
#[test]
fn every_reference_document_declares_a_class() {
    let refs = strict_dir().join("refs");
    let mut unclassified = Vec::new();
    for entry in std::fs::read_dir(&refs).expect("read refs dir").flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let name = dir
            .file_name()
            .and_then(|value| value.to_str())
            .expect("document name")
            .to_owned();
        if policy().is_page_count_only(&name) || policy().class_of(&name).is_some() {
            continue;
        }
        unclassified.push(name);
    }
    assert!(
        unclassified.is_empty(),
        "these reference documents are in neither a class nor page_count_only \
         in coverage/render-gates.toml: {unclassified:?}"
    );
}

/// The gate must actually exercise every class it declares, or a class can
/// rot into a table of numbers nothing is checked against.
#[test]
fn every_declared_class_is_exercised() {
    for class in &policy().classes {
        assert!(
            !class.documents.is_empty(),
            "class {} declares no documents",
            class.name
        );
        for document in &class.documents {
            let dir = strict_dir().join(format!("refs/{document}"));
            assert!(
                dir.is_dir(),
                "class {} lists {document}, which has no references",
                class.name
            );
        }
    }
}
