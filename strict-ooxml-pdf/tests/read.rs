//! Integration for the reader, against PDFs written by hand.
//!
//! The fixtures are built as byte strings rather than produced by this
//! workspace's own writer. That is deliberate: a round trip through our own
//! writer would prove the two halves agree with each other and nothing else,
//! while a hand-written PDF states what the specification says and is checked
//! against an independent reader (`lopdf`) as well as against our placement.
//! The round trip itself needs `strict-ooxml-render-pdf` as a dev-dependency,
//! which is blocked while another crate in the workspace does not build
//! (`STAGE-8-OPEN.md` O-9).

use strict_ooxml_pdf::{
    content::{Item, Matrix, Rgb},
    PdfDocument, PdfLimits,
};

mod common;

/// A one-page PDF with a font, a media box and a content stream.
///
/// The object numbers are fixed: 1 catalog, 2 pages, 3 page, 4 content,
/// The smallest font: one glyph, one width, one `ToUnicode` entry.
const FONT: &str = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 65 /LastChar 66 /Widths [600 600] >>";

/// A page whose content draws `Hello` at a known position.
fn hello_pdf() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        "<< /Length 0 >>\nstream\nBT /F1 12 Tf 1 0 0 1 20 50 Tm (AB) Tj ET\nendstream",
        FONT,
    ];
    common::pdf_with(&objects)
}

fn open(bytes: &[u8]) -> PdfDocument {
    PdfDocument::open(bytes, PdfLimits::default()).expect("the PDF opens")
}

#[test]
fn a_page_reports_its_size_and_its_glyphs() {
    let mut pdf = open(&hello_pdf());
    assert_eq!(pdf.page_count(), 1);
    let pages = pdf.pages().expect("pages");
    assert_eq!(pages.len(), 1);
    let page = &pages[0];
    assert!(
        (page.geometry.width - 200.0).abs() < 1e-9,
        "{}",
        page.geometry.width
    );
    assert!(
        (page.geometry.height - 100.0).abs() < 1e-9,
        "{}",
        page.geometry.height
    );
    assert_eq!(page.geometry.rotation, 0);

    let glyphs: Vec<&strict_ooxml_pdf::Glyph> = page
        .items()
        .iter()
        .filter_map(|item| match item {
            Item::Glyph(glyph) => Some(glyph),
            _ => None,
        })
        .collect();
    assert_eq!(glyphs.len(), 2, "{:?}", page.items());
    assert_eq!(glyphs[0].text, "A");
    assert_eq!(glyphs[1].text, "B");
    // The baseline is 50 pt from the bottom of a 100 pt page, so 50 from the top
    // too; getting this wrong is a one-line bug that moves every line.
    assert!((glyphs[0].y - 50.0).abs() < 0.01, "{}", glyphs[0].y);
    assert!((glyphs[0].x - 20.0).abs() < 0.01, "{}", glyphs[0].x);
    // 12 pt text with a 600/1000 advance: the second glyph starts 7.2 pt later.
    assert!((glyphs[1].x - 27.2).abs() < 0.01, "{}", glyphs[1].x);
    assert!((glyphs[0].size - 12.0).abs() < 0.01, "{}", glyphs[0].size);
    assert_eq!(glyphs[0].font, "Helvetica");
    assert!(pdf.report().is_clean(), "{}", pdf.report());
}

#[test]
fn a_transformed_page_places_its_glyphs_in_device_space() {
    // `2 0 0 2 0 0` doubles everything, and `cm` composes with the text matrix.
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        "<< /Length 0 >>\nstream\nq 2 0 0 2 0 0 cm BT /F1 10 Tf 1 0 0 1 10 10 Tm (A) Tj ET Q\nendstream",
        FONT,
    ];
    let mut pdf = open(&common::pdf_with(&objects));
    let page = pdf.page(1).expect("page 1");
    let Some(Item::Glyph(glyph)) = page.items().first() else {
        panic!("no glyph: {:?}", page.items());
    };
    assert!((glyph.x - 20.0).abs() < 0.01, "{}", glyph.x);
    // The baseline is 10 pt up in user space, doubled, on a 200 pt page: the
    // y from the top is 200 - 20 = 180.
    assert!((glyph.y - 180.0).abs() < 0.01, "{}", glyph.y);
    assert!((glyph.size - 20.0).abs() < 0.01, "{}", glyph.size);
}

#[test]
fn a_graphics_state_stack_is_honoured() {
    // The `Q` restores the identity, so the second glyph is *not* scaled.
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        "<< /Length 0 >>\nstream\nq 2 0 0 2 0 0 cm BT /F1 10 Tf 1 0 0 1 0 0 Tm (A) Tj ET Q\nBT /F1 10 Tf 1 0 0 1 5 5 Tm (A) Tj ET\nendstream",
        FONT,
    ];
    let mut pdf = open(&common::pdf_with(&objects));
    let page = pdf.page(1).expect("page 1");
    let glyphs: Vec<&strict_ooxml_pdf::Glyph> = page
        .items()
        .iter()
        .filter_map(|item| match item {
            Item::Glyph(glyph) => Some(glyph),
            _ => None,
        })
        .collect();
    assert_eq!(glyphs.len(), 2);
    assert!((glyphs[0].size - 20.0).abs() < 0.01, "{}", glyphs[0].size);
    assert!((glyphs[1].size - 10.0).abs() < 0.01, "{}", glyphs[1].size);
    assert!((glyphs[1].x - 5.0).abs() < 0.01, "{}", glyphs[1].x);
}

#[test]
fn a_rectangle_becomes_a_path_with_its_fill() {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << >> /Contents 4 0 R >>",
        "<< /Length 0 >>\nstream\n1 0 0 rg 10 20 30 40 re f\nendstream",
    ];
    let mut pdf = open(&common::pdf_with(&objects));
    let page = pdf.page(1).expect("page 1");
    let Some(Item::Vector(vector)) = page.items().first() else {
        panic!("no path: {:?}", page.items());
    };
    assert_eq!(vector.fill.map(Rgb::to_rgb8), Some([255, 0, 0]));
    assert!(vector.stroke.is_none());
    // `re` is a closed subpath: five points, the last equal to the first.
    assert_eq!(vector.subpaths.len(), 1);
    assert_eq!(vector.subpaths[0].points.len(), 5);
    assert_eq!(vector.subpaths[0].points[0], vector.subpaths[0].points[4]);
    assert_eq!(vector.subpaths[0].points[1], (40.0, 20.0));
}

#[test]
fn a_curve_is_flattened_within_the_documented_tolerance() {
    // A quarter-circle-ish curve; the flattening must stay within a small
    // fraction of a point of the true curve, which is what SC-9 asks of the
    // writer and what makes the round trip stable.
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << >> /Contents 4 0 R >>",
        "<< /Length 0 >>\nstream\n0 0 m 0 55.23 44.77 55.23 100 0 c 0 0 0 RG S\nendstream",
    ];
    let mut pdf = open(&common::pdf_with(&objects));
    let page = pdf.page(1).expect("page 1");
    let Some(Item::Vector(vector)) = page.items().first() else {
        panic!("no path: {:?}", page.items());
    };
    let points = &vector.subpaths[0].points;
    assert!(
        points.len() > 4,
        "a curve must be subdivided: {} points",
        points.len()
    );
    assert_eq!(*points.first().expect("a point"), (0.0, 0.0));
    assert_eq!(*points.last().expect("a point"), (100.0, 0.0));

    // The distance is measured against the *exact cubic*, not against a circle:
    // the fixture is the kappa approximation of a quarter circle, which at
    // r=100 sits about 0.07 pt off the circle it approximates, and a test that
    // compared against the circle would be measuring the fixture.
    let from = (0.0f64, 0.0f64);
    let control1 = (0.0f64, 55.23f64);
    let control2 = (44.77f64, 55.23f64);
    let to = (100.0f64, 0.0f64);
    // The subpath starts with the `m` point, so the first point is t=0 and the
    // last is t=1; the flattened interior sits between them.
    let count = points.len();
    for (index, point) in points.iter().enumerate() {
        let count = f64::from(u32::try_from(count).expect("a small point count"));
        let t = f64::from(u32::try_from(index).expect("a small index")) / (count - 1.0);
        let u = 1.0 - t;
        let expected = (
            u * u * u * from.0
                + 3.0 * u * u * t * control1.0
                + 3.0 * u * t * t * control2.0
                + t * t * t * to.0,
            u * u * u * from.1
                + 3.0 * u * u * t * control1.1
                + 3.0 * u * t * t * control2.1
                + t * t * t * to.1,
        );
        let error = (point.0 - expected.0).hypot(point.1 - expected.1);
        assert!(
            error < 0.01,
            "point {index} {point:?} is {error} pt from the curve (expected {expected:?})"
        );
    }
    assert!(vector.stroke.is_some());
    assert!(vector.fill.is_none());
}

#[test]
fn a_tj_adjustment_moves_the_pen_without_drawing() {
    // A `TJ` number is a shift in thousandths of an em, and a *negative* number
    // moves the pen forward: `-1000` at 10 pt is 10 pt to the right. The pen must
    // move and no glyph may appear for the number itself.
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        "<< /Length 0 >>\nstream\nBT /F1 10 Tf 1 0 0 1 10 50 Tm [(A) -1000 (B)] TJ ET\nendstream",
        FONT,
    ];
    let mut pdf = open(&common::pdf_with(&objects));
    let page = pdf.page(1).expect("page 1");
    let glyphs: Vec<&strict_ooxml_pdf::Glyph> = page
        .items()
        .iter()
        .filter_map(|item| match item {
            Item::Glyph(glyph) => Some(glyph),
            _ => None,
        })
        .collect();
    assert_eq!(glyphs.len(), 2);
    // A is 6 pt wide at 10 pt, and the -1000 adjustment adds 10 pt.
    assert!((glyphs[0].x - 10.0).abs() < 0.01, "{}", glyphs[0].x);
    assert!((glyphs[1].x - 26.0).abs() < 0.01, "{}", glyphs[1].x);
}

#[test]
fn invisible_text_is_measured_but_not_drawn() {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        "<< /Length 0 >>\nstream\nBT /F1 10 Tf 3 Tr 1 0 0 1 10 50 Tm (A) Tj ET\nendstream",
        FONT,
    ];
    let mut pdf = open(&common::pdf_with(&objects));
    let page = pdf.page(1).expect("page 1");
    // `3 Tr` means invisible: a reader that ignores it draws a layer of text
    // the producer deliberately hid.
    assert!(
        page.items().is_empty(),
        "invisible text must not be drawn: {:?}",
        page.items()
    );
}

#[test]
fn a_construct_the_reader_does_not_carry_is_reported() {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> /Contents 4 0 R >>",
        "<< /Length 0 >>\nstream\n/sh0 sh 0 0 100 100 re f Q\nendstream",
    ];
    let mut pdf = open(&common::pdf_with(&objects));
    let _page = pdf.page(1).expect("page 1");
    let ids: Vec<&str> = pdf
        .report()
        .losses()
        .iter()
        .map(|loss| loss.id.as_str())
        .collect();
    assert!(ids.contains(&"pdf.shading"), "{ids:?}");
    assert!(!pdf.report().is_clean());
}

#[test]
fn a_crop_box_smaller_than_the_media_box_is_the_page() {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 400 400] >>",
        "<< /Type /Page /Parent 2 0 R /CropBox [50 50 250 150] /Resources << >> /Contents 4 0 R >>",
        "<< /Length 0 >>\nstream\nendstream",
    ];
    let mut pdf = open(&common::pdf_with(&objects));
    let page = pdf.page(1).expect("page 1");
    // The crop box is what is shown, and what the reader measures against.
    assert!(
        (page.geometry.width - 200.0).abs() < 1e-9,
        "{}",
        page.geometry.width
    );
    assert!(
        (page.geometry.height - 100.0).abs() < 1e-9,
        "{}",
        page.geometry.height
    );
}

#[test]
fn a_page_that_never_closes_its_graphics_state_is_still_readable() {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        "<< /Length 0 >>\nstream\nq 2 0 0 2 0 0 cm BT /F1 10 Tf 1 0 0 1 0 0 Tm (A) Tj ET\nendstream",
        FONT,
    ];
    let mut pdf = open(&common::pdf_with(&objects));
    let page = pdf.page(1).expect("page 1");
    assert!(!page.items().is_empty());
}

/// A quarter turn, written out because the content stream is the only place a
/// rotation comes from.
fn rotation_90() -> Matrix {
    Matrix {
        a: 0.0,
        b: 1.0,
        c: -1.0,
        d: 0.0,
        e: 0.0,
        f: 0.0,
    }
}

#[test]
fn a_matrix_scale_factor_survives_a_rotation() {
    let upright = Matrix::scale(2.0, 2.0);
    let rotated = Matrix::chain(Matrix::scale(2.0, 2.0), rotation_90());
    assert!((upright.scale_factor() - 2.0).abs() < 1e-9);
    assert!(
        (rotated.scale_factor() - 2.0).abs() < 1e-6,
        "{}",
        rotated.scale_factor()
    );
}

/// `d` sets a dash pattern that the stroked path carries (in points), and is no
/// longer reported as a construct the reader drops: a dashed table border in a
/// PDF this project wrote must read back as dashed.
#[test]
fn a_dash_pattern_is_carried_on_the_stroked_path() {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> /Contents 4 0 R >>",
        "<< /Length 0 >>\nstream\n[3 1] 0 d 10 10 m 100 10 l S [] 0 d 10 20 m 100 20 l S\nendstream",
    ];
    let mut pdf = open(&common::pdf_with(&objects));
    let page = pdf.page(1).expect("page 1");
    let dashes: Vec<Vec<f64>> = page
        .items()
        .iter()
        .filter_map(|item| match item {
            strict_ooxml_pdf::Item::Vector(vector) => Some(vector.dash.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(dashes, [vec![3.0, 1.0], Vec::new()], "{dashes:?}");
    let ids: Vec<&str> = pdf
        .report()
        .losses()
        .iter()
        .map(|loss| loss.id.as_str())
        .collect();
    assert!(!ids.contains(&"pdf.dash"), "{ids:?}");
}
