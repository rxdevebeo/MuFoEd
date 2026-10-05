//! `DjVu` regression tests using deterministic native fixtures.
#![allow(clippy::float_cmp)]
#![cfg(feature = "djvu-image")]
mod common;
use common::*;
use strict_ooxml_djvu::{DjvuDocument, DjvuLimits};
fn decoded(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let doc = DjvuDocument::open(bytes, DjvuLimits::default()).expect("fixture");
    let png = doc.pages()[0].image_png(doc.limits).expect("PNG");
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png))
        .read_info()
        .expect("PNG decoder");
    let mut data = vec![0; decoder.output_buffer_size().expect("size")];
    let info = decoder.next_frame(&mut data).expect("frame");
    data.truncate(info.buffer_size());
    (info.width, info.height, data)
}
#[test]
fn jb2_mask_is_lossless_in_top_left_coordinates() {
    let (w, h, rgb) = decoded(&scan());
    assert_eq!((w, h), (100, 100));
    assert_eq!(
        &rgb[(10 * 100 + 10) * 3..(10 * 100 + 10) * 3 + 3],
        &[0, 0, 0]
    );
    assert_eq!(
        &rgb[(80 * 100 + 10) * 3..(80 * 100 + 10) * 3 + 3],
        &[255, 255, 255]
    );
}
#[test]
fn info_and_stream_pixel_budgets_are_checked_independently() {
    let d = DjvuDocument::open(&scan(), DjvuLimits::default()).expect("fixture");
    assert!(d.pages()[0]
        .image_png(DjvuLimits {
            max_ink_pixels: 9999,
            ..d.limits
        })
        .is_err());
    let mut mask = djvu_bitmap::Bitmap::new(200, 200);
    mask.set_black(1, 1);
    let stream = djvu_jb2::encode::encode_jb2_lossless(&mask);
    let bytes = single(&[chunk(b"INFO", &info(100, 100, 72)), chunk(b"Sjbz", &stream)]);
    let d = DjvuDocument::open(&bytes, DjvuLimits::default()).expect("framing");
    assert!(d.pages()[0]
        .image_png(DjvuLimits {
            max_ink_pixels: 10000,
            ..d.limits
        })
        .is_err());
}
#[test]
fn unsupported_layers_are_not_sent_as_a_partial_composite() {
    let bytes = single(&[
        chunk(b"INFO", &info(100, 100, 72)),
        chunk(b"Smmr", &[0, 0, 0]),
    ]);
    let doc = DjvuDocument::open(&bytes, DjvuLimits::default()).expect("container");
    assert!(doc.pages()[0].image_png(doc.limits).is_err());
}
#[test]
fn iw44_progressive_chunks_decode_one_image() {
    let image = djvu_pixmap::GrayPixmap {
        width: 64,
        height: 64,
        data: vec![180; 4096],
    };
    let slices = djvu_iw44::encode::encode_iw44_gray(
        &image,
        &djvu_iw44::encode::Iw44EncodeOptions::default(),
    );
    let mut chunks = vec![chunk(b"INFO", &info(64, 64, 72))];
    for data in slices {
        chunks.push(chunk(b"BG44", &data));
    }
    let (w, h, bytes) = decoded(&single(&chunks));
    assert_eq!((w, h), (64, 64));
    assert!(bytes.iter().all(|&v| (160..=200).contains(&v)));
}
