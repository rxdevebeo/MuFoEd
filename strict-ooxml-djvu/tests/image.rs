//! `DjVu` regression tests using deterministic native fixtures.
#![allow(clippy::float_cmp)]
#![cfg(feature = "djvu-image")]
mod common;
use common::*;
use strict_ooxml_djvu::{DjvuDocument, DjvuLimits, Rect};
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

#[test]
fn fgbz_assigns_color_per_blit_and_validates_indices() {
    let mask = scan();
    let d = DjvuDocument::open(&mask, DjvuLimits::default()).expect("mask");
    let stream = d.pages()[0]
        .chunks
        .iter()
        .find(|c| c.id == *b"Sjbz")
        .expect("stream")
        .data();
    let (_, _, count) =
        djvu_jb2::decode_indexed_with_limits(stream, None, 10000, 100).expect("indexed");
    let mut palette = vec![0x80, 0, 1, 40, 25, 230];
    palette.extend(u24(count));
    palette.extend(djvu_bzz::bzz_encode(&vec![0; count * 2]));
    let chunks = |palette: &[u8]| {
        single(&[
            chunk(b"INFO", &info(100, 100, 72)),
            chunk(b"FGbz", palette),
            chunk(b"Sjbz", stream),
        ])
    };
    let (_, _, rgb) = decoded(&chunks(&palette));
    assert_eq!(
        &rgb[(10 * 100 + 10) * 3..(10 * 100 + 10) * 3 + 3],
        &[230, 25, 40]
    );
    let mut bad = vec![0x80, 0, 1, 40, 25, 230];
    bad.extend(u24(count));
    bad.extend(djvu_bzz::bzz_encode(&vec![255; count * 2]));
    let d = DjvuDocument::open(&chunks(&bad), DjvuLimits::default()).expect("container");
    assert!(d.pages()[0].image(d.limits).is_err());
    let d = DjvuDocument::open(&chunks(&palette), DjvuLimits::default()).expect("container");
    assert!(d.pages()[0]
        .image(DjvuLimits {
            max_ink_pixels: 9999,
            ..d.limits
        })
        .is_err());
    assert!(djvu_jb2::decode_indexed_with_limits(stream, None, 9999, 100).is_err());
}
#[test]
fn jpeg_layers_decode_rgb_and_refuse_conflicts_and_bad_headers() {
    let jpeg = include_bytes!("fixtures/red.jpg");
    let bytes = single(&[chunk(b"INFO", &info(16, 16, 72)), chunk(b"BGjp", jpeg)]);
    let (w, h, rgb) = decoded(&bytes);
    assert_eq!((w, h), (16, 16));
    assert!(rgb
        .chunks_exact(3)
        .all(|p| p[0] > 220 && p[1] < 40 && p[2] < 55));
    for chunks in [
        vec![chunk(b"BGjp", b"broken")],
        vec![chunk(b"BGjp", jpeg), chunk(b"BGjp", jpeg)],
        vec![chunk(b"FGjp", jpeg)],
    ] {
        let mut all = vec![chunk(b"INFO", &info(16, 16, 72))];
        all.extend(chunks);
        let d = DjvuDocument::open(&single(&all), DjvuLimits::default()).expect("framing");
        assert!(d.pages()[0].image(d.limits).is_err());
    }
    let mut bad = jpeg.to_vec();
    let sof = bad.windows(2).position(|w| w == [255, 192]).expect("SOF");
    bad[sof + 5..sof + 9].copy_from_slice(&[255, 255, 255, 255]);
    let d = DjvuDocument::open(
        &single(&[chunk(b"INFO", &info(16, 16, 72)), chunk(b"BGjp", &bad)]),
        DjvuLimits::default(),
    )
    .expect("framing");
    assert!(d.pages()[0].image(d.limits).is_err());
}
#[test]
fn mmr_white_and_inverted_masks_decode_and_truncated_rows_are_refused() {
    // Eight V(0) bits: eight white rows, each referenced to an all-white row.
    for flags in [0u8, 1] {
        let mmr = [b'M', b'M', b'R', flags, 0, 8, 0, 8, 255, 0, 0];
        let (_, _, rgb) = decoded(&single(&[
            chunk(b"INFO", &info(8, 8, 72)),
            chunk(b"Smmr", &mmr),
        ]));
        assert!(rgb.iter().all(|&p| p == if flags == 0 { 255 } else { 0 }));
    }
    for data in [
        vec![b'M', b'M', b'R', 0, 0, 8, 0, 8],
        vec![b'M', b'M', b'R', 2, 0, 8, 0, 8, 255],
    ] {
        let d = DjvuDocument::open(
            &single(&[chunk(b"INFO", &info(8, 8, 72)), chunk(b"Smmr", &data)]),
            DjvuLimits::default(),
        )
        .expect("framing");
        assert!(d.pages()[0].image(d.limits).is_err());
    }
}
#[test]
fn legacy_bm44_without_info_uses_native_dimensions_and_progressive_chunks() {
    let image = djvu_pixmap::GrayPixmap {
        width: 32,
        height: 32,
        data: vec![180; 1024],
    };
    let parts = djvu_iw44::encode::encode_iw44_gray(
        &image,
        &djvu_iw44::encode::Iw44EncodeOptions::default(),
    );
    let chunks = parts.iter().map(|p| chunk(b"BM44", p)).collect::<Vec<_>>();
    let bytes = file(&form(b"BM44", &chunks));
    let (w, h, rgb) = decoded(&bytes);
    assert_eq!((w, h), (32, 32));
    assert!(rgb.iter().all(|&v| (160..=200).contains(&v)));
}

#[test]
fn mmr_horizontal_and_vertical_modes_preserve_black_runs() {
    // First row uses horizontal W(2), B(4), then V(0); subsequent rows use three V(0).
    let bits = format!("00101110111{}", "111".repeat(7));
    let mut mmr = b"MMR\0\0\x08\0\x08".to_vec();
    for part in bits.as_bytes().chunks(8) {
        let mut byte = 0u8;
        for (bit, &c) in part.iter().enumerate() {
            if c == b'1' {
                byte |= 1 << (7 - bit);
            }
        }
        mmr.push(byte);
    }
    mmr.extend([0, 0]);
    let (_, _, rgb) = decoded(&single(&[
        chunk(b"INFO", &info(8, 8, 72)),
        chunk(b"Smmr", &mmr),
    ]));
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(
                rgb[(y * 8 + x) * 3],
                if (2..6).contains(&x) { 0 } else { 255 }
            );
        }
    }
}

/// The black rectangle of `scan()` is x 10..40, y 10..15 at 72 dpi.
#[test]
fn ink_boxes_are_the_component_bounding_boxes() {
    let doc = DjvuDocument::open(&scan(), DjvuLimits::default()).expect("fixture");
    let boxes = doc.pages()[0].ink_boxes(doc.limits).expect("boxes");
    assert_eq!(
        boxes,
        [Rect {
            x: 10.0,
            y: 10.0,
            width: 30.0,
            height: 5.0
        }]
    );
}

#[test]
fn disjoint_components_are_separate_boxes() {
    let mut mask = djvu_bitmap::Bitmap::new(100, 100);
    for y in 5..10 {
        for x in 5..10 {
            mask.set_black(x, y);
        }
    }
    for y in 50..55 {
        for x in 50..60 {
            mask.set_black(x, y);
        }
    }
    let bytes = single(&[
        chunk(b"INFO", &info(100, 100, 72)),
        chunk(b"Sjbz", &djvu_jb2::encode::encode_jb2_lossless(&mask)),
    ]);
    let doc = DjvuDocument::open(&bytes, DjvuLimits::default()).expect("fixture");
    let boxes = doc.pages()[0].ink_boxes(doc.limits).expect("boxes");
    assert_eq!(boxes.len(), 2, "{boxes:?}");
    assert_eq!(boxes[0].width, 5.0);
    assert_eq!(boxes[1].width, 10.0);
}

#[test]
fn a_page_without_a_mask_has_no_ink_boxes() {
    let bytes = single(&[
        chunk(b"INFO", &info(100, 100, 72)),
        chunk(b"TXTa", &text("only text")),
    ]);
    let doc = DjvuDocument::open(&bytes, DjvuLimits::default()).expect("fixture");
    assert!(doc.pages()[0].ink_boxes(doc.limits).unwrap().is_empty());
}

#[test]
fn region_png_crops_the_composite() {
    let doc = DjvuDocument::open(&scan(), DjvuLimits::default()).expect("fixture");
    let png = doc.pages()[0]
        .region_png(
            doc.limits,
            Rect {
                x: 10.0,
                y: 10.0,
                width: 30.0,
                height: 5.0,
            },
        )
        .expect("crop");
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png))
        .read_info()
        .expect("PNG decoder");
    let mut data = vec![0; decoder.output_buffer_size().expect("size")];
    let info = decoder.next_frame(&mut data).expect("frame");
    assert_eq!((info.width, info.height), (30, 5));
    assert!(
        data.iter().all(|&sample| sample == 0),
        "the crop is the black rectangle"
    );
    // An empty region is refused, not a zero-size image.
    let doc = DjvuDocument::open(&scan(), DjvuLimits::default()).expect("fixture");
    assert!(doc.pages()[0]
        .region_png(
            doc.limits,
            Rect {
                x: 500.0,
                y: 500.0,
                width: 1.0,
                height: 1.0
            }
        )
        .is_err());
}
