//! The picture cache (`STAGE-8-OPEN.md` Q-26).
//!
//! A `Do` decodes the picture it draws. A document that draws the same logo on
//! every page — the same scanned figure on every spread — decoded it once per
//! draw, and the page was holding the result anyway, because a placed image
//! carries its bytes into the page content.
//!
//! The cache is on the **document**, not the page: the second page of a spread
//! draws what the first drew, and a per-page cache would decode it twice and
//! then throw the map away. These tests are about the contract, not the
//! milliseconds — the timing is on the corpus, in `Q-26`:
//!
//! | file | picture draws | distinct pictures | read |
//! |---|---|---|---|
//! | `cpio.5.pdf` | 2 291 | 22 | 0.09 s |
//! | `RM0090 STM32F4xx` | 3 981 | 1 067 | 0.6 s |
//! | `Балашова` (139 MiB) | 102 902 | 72 661 | 10.3 s → 8.9 s |
//!
//! The last row is the honest one: a photo album draws nearly every picture
//! once, so no cache can help a decode that was going to happen anyway. The
//! ceiling is there for that file, and the repeats are what the cache is for.
#![allow(clippy::doc_markdown)]

use strict_ooxml_pdf::content::Item;
use strict_ooxml_pdf::{PdfDocument, PdfLimits};

/// Assembles a PDF from object bodies, all of them text except where a body says
/// otherwise.
fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n");
    let mut offsets: Vec<usize> = Vec::new();
    let mut body: Vec<u8> = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len() + body.len());
        body.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        body.extend_from_slice(object);
        body.extend_from_slice(b"\nendobj\n");
    }
    out.extend_from_slice(&body);
    let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for offset in &offsets {
        use std::fmt::Write as _;
        let _ = write!(table, "{offset:010} 00000 n ");
    }
    out.extend_from_slice(table.as_bytes());
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n0\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn text(body: &str) -> Vec<u8> {
    body.as_bytes().to_vec()
}

fn stream_of(body: &[u8]) -> Vec<u8> {
    let mut out = format!("<< /Length {} >>\nstream\n", body.len()).into_bytes();
    out.extend_from_slice(body);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A 1×1 red image, as bytes: three samples, and no route through a `String`.
fn image() -> Vec<u8> {
    let mut out = b"<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB \
          /BitsPerComponent 8 /Length 3 >>\nstream\n"
        .to_vec();
    out.extend_from_slice(&[0xff, 0x00, 0x00]);
    out.extend_from_slice(b"\nendstream");
    out
}

/// An image whose `/Length` promises more than the stream holds: a file that is
/// broken, and the one case where the reader refuses.
fn broken_image() -> Vec<u8> {
    let mut out = b"<< /Type /XObject /Subtype /Image /Width 4 /Height 4 /ColorSpace /DeviceRGB \
          /BitsPerComponent 8 /Filter /FlateDecode /Length 400 >>\nstream\n"
        .to_vec();
    out.extend_from_slice(&[0x00, 0x01, 0x02, 0x03]);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A content stream that draws the same picture `times` times.
fn draws(times: usize) -> Vec<u8> {
    let mut body = String::new();
    for index in 0..times {
        let x = 10 + index * 20;
        let _ =
            std::fmt::Write::write_fmt(&mut body, format_args!("q 20 0 0 20 {x} 50 cm /Im0 Do Q "));
    }
    stream_of(body.as_bytes())
}

/// One page that draws the picture `times` times.
fn one_page(times: usize) -> Vec<u8> {
    pdf(&[
        text("<< /Type /Catalog /Pages 2 0 R >>"),
        text("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        text(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /XObject << /Im0 \
             5 0 R >> >> /Contents 4 0 R >>",
        ),
        draws(times),
        image(),
    ])
}

/// Two pages drawing the same picture, the way a spread does.
fn two_pages() -> Vec<u8> {
    pdf(&[
        text("<< /Type /Catalog /Pages 2 0 R >>"),
        text("<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>"),
        text(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /XObject << /Im0 \
             7 0 R >> >> /Contents 5 0 R >>",
        ),
        text(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /XObject << /Im0 \
             7 0 R >> >> /Contents 6 0 R >>",
        ),
        draws(2),
        draws(2),
        image(),
    ])
}

/// One page drawing a picture that cannot be decoded, twice.
fn one_broken_page() -> Vec<u8> {
    pdf(&[
        text("<< /Type /Catalog /Pages 2 0 R >>"),
        text("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        text(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /XObject << /Im0 \
             5 0 R >> >> /Contents 4 0 R >>",
        ),
        draws(2),
        broken_image(),
    ])
}

fn pictures(page: &strict_ooxml_pdf::PdfPage) -> Vec<&strict_ooxml_pdf::PlacedImage> {
    page.items()
        .iter()
        .filter_map(|item| match item {
            Item::Image(placed) => Some(placed),
            _ => None,
        })
        .collect()
}

/// Three draws of one picture are three pictures on the page and **one** decode.
///
/// The three carries are equal and the page still has all three, because the
/// cache is about the work and not about what the page holds: a page that drew a
/// logo once has to be able to place it three times.
#[test]
fn the_same_picture_drawn_three_times_is_decoded_once() {
    let mut document = PdfDocument::open(&one_page(3), PdfLimits::default()).expect("open");
    let page = document.page(1).expect("page");
    let placed = pictures(&page);
    assert_eq!(placed.len(), 3, "the page draws three pictures");
    assert!(
        placed.iter().all(|item| item.image.is_some()),
        "a cached picture is still a picture: {:?}",
        placed
            .iter()
            .map(|item| item.missing.clone())
            .collect::<Vec<_>>()
    );
    let first = placed[0].image.as_ref().expect("decoded");
    assert!(
        placed[1..]
            .iter()
            .all(|item| item.image.as_ref() == Some(first)),
        "every draw carries the same bytes"
    );
    assert_eq!(
        first.raw_samples(),
        Some((&[0xff, 0x00, 0x00][..], 3)),
        "the red pixel, three components"
    );
    assert_eq!(document.distinct_images(), 1, "one XObject, one decode");
    assert_eq!(
        document.cached_image_bytes(),
        3,
        "one 1×1 RGB picture is three bytes, however many times it is drawn"
    );
}

/// The second page of a spread draws what the first drew, and that is the case a
/// per-page cache would miss.
#[test]
fn a_picture_drawn_on_two_pages_is_decoded_once() {
    let mut document = PdfDocument::open(&two_pages(), PdfLimits::default()).expect("open");
    for number in 1..=2 {
        let page = document.page(number).expect("page");
        assert_eq!(pictures(&page).len(), 2, "page {number} draws two pictures");
    }
    assert_eq!(
        document.distinct_images(),
        1,
        "the cache belongs to the document, not to the page"
    );
}

/// A refusal is a fact about the object, so it is cached too — and the caller
/// still records a loss for **each** draw, because the number of draws is what a
/// report is about.
#[test]
fn a_broken_picture_is_asked_about_once_and_reported_twice() {
    let mut document = PdfDocument::open(&one_broken_page(), PdfLimits::default()).expect("open");
    let page = document.page(1).expect("page");
    let placed = pictures(&page);
    assert_eq!(placed.len(), 2, "both draws are on the page");
    assert!(
        placed.iter().all(|item| item.missing.is_some()),
        "a refusal is a refusal on every draw: {:?}",
        placed
            .iter()
            .map(|item| item.missing.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        document.distinct_images(),
        1,
        "the broken stream is inflated once, not once per draw"
    );
    let losses = document.report().losses();
    let reported: Vec<_> = losses
        .iter()
        .filter(|loss| loss.id == "pdf.image.missing")
        .collect();
    assert_eq!(
        reported.len(),
        1,
        "the same reason twice is one entry with a count"
    );
    assert_eq!(
        reported[0].count, 2,
        "and the count is the number of draws, not the number of decodes: {}",
        reported[0].detail
    );
}

/// With no room in the cache, nothing is held and every draw decodes again —
/// which is the state the reader was in before there was a cache, and the reason
/// a full cache is a cost and not a bug.
#[test]
fn a_cache_with_no_room_changes_nothing_but_the_time() {
    let limits = PdfLimits {
        max_cached_image_bytes: 0,
        ..PdfLimits::default()
    };
    let mut document = PdfDocument::open(&one_page(3), limits).expect("open");
    let page = document.page(1).expect("page");
    let placed = pictures(&page);
    assert_eq!(placed.len(), 3);
    assert!(
        placed.iter().all(|item| item.image.is_some()),
        "an uncached draw still decodes"
    );
    assert_eq!(document.distinct_images(), 0, "and nothing is held");
    assert_eq!(document.cached_image_bytes(), 0);
}

/// A refusal costs nothing to hold, so it is kept even when there is no room for
/// pictures: a document whose every picture is broken should not re-inflate a
/// broken stream on every draw, and a ceiling of zero is the extreme of that.
#[test]
fn a_refusal_is_held_even_when_no_picture_fits() {
    let limits = PdfLimits {
        max_cached_image_bytes: 0,
        ..PdfLimits::default()
    };
    let mut document = PdfDocument::open(&one_broken_page(), limits).expect("open");
    let _ = document.page(1).expect("page");
    assert_eq!(document.distinct_images(), 1, "a refusal is worth no bytes");
    assert_eq!(document.cached_image_bytes(), 0, "and it holds no bytes");
}
