//! The facade owns image bytes after the importer and resolver return.
#![cfg(feature = "html")]
#[test]
fn html_roundtrips_through_the_shared_strict_package() {
    let opened = strict_ooxml::StrictDocument::import_html(
        b"<p><b>before</b></p><table><tr><td>A</td><td>B</td></tr></table><ol><li>one<li>two</ol>",
        &strict_ooxml::HtmlOptions::default(),
        None,
    )
    .unwrap();
    assert_eq!(opened.import_report.loss_count, 0);
    opened.write_report.verify_no_silent_loss().unwrap();
    let reopened = strict_ooxml::StrictDocument::open_reader(
        &opened.strict_bytes[..],
        &strict_ooxml::OpenOptions::default(),
    )
    .unwrap();
    assert!(reopened.document().body.blocks.len() >= 4);
    assert_eq!(reopened.document().numbering.nums().count(), 1);
}

#[test]
fn html_media_outlives_the_resolver_and_source() {
    // Valid 1x1 PNG, provided through an external resource resolver.
    let png = vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 8, 215, 99, 248, 207, 192, 240, 31,
        0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];
    let document = {
        let payload = png.clone();
        let resolver = move |_: &str, _: usize| Ok(payload.clone());
        let source = String::from("<p>A<img src='https://example.test/p.png'>B</p>");
        strict_ooxml::StrictDocument::import_html(
            source.as_bytes(),
            &strict_ooxml::HtmlOptions::default(),
            Some(&resolver),
        )
        .unwrap()
        .document
    };
    let image = document.document().media.iter().next().unwrap();
    assert_eq!(document.package().read_part(&image.part).unwrap(), png);
}
