//! P9 — font hints, theme font languages, and theme typeface charset.
//!
//! T-P9-1 keeps font-table hints (see `fonts.rs`) and the slots of a repeated
//! `w:rFonts`. T-P9-3: a charset that is in the model is written, and clearing
//! it is visible. `hint="cs"` is the one value Strict `ST_Hint` cannot carry;
//! the complex-script face stays, and the report cites `w:rFonts@hint`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use strict_ooxml_core::opc::{ConformancePolicy, OpenOptions, Package};
use strict_ooxml_core::part::PartId;
use strict_ooxml_testkit::{DocxBuilder, Family};
use strict_ooxml_wml::model::support::SupportStatus;
use strict_ooxml_wml::model::values::Fonts;
use strict_ooxml_wml::{parse_document, ParseOptions};
use strict_ooxml_write::{write_package, WriteOptions};

/// A document from the gitignored local corpus, or `None` (with a loud skip
/// line) when this checkout does not carry it.
fn local_corpus(relative: &str) -> Option<PathBuf> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    if path.is_file() {
        Some(path)
    } else {
        eprintln!(
            "SKIP: local corpus document not present: {}",
            path.display()
        );
        None
    }
}

fn open_transitional(path: &Path) -> Package {
    Package::open_reader(
        std::fs::read(path).expect("fixture").as_slice(),
        &OpenOptions::default()
            .conformance(ConformancePolicy::Normalize)
            .shared_normalization(Arc::new(
                strict_ooxml_core::normalize::transitional::TransitionalNormalizer::new(),
            )),
    )
    .expect("open")
}

fn part_text(bytes: &[u8], part: &str) -> String {
    let package = Package::open_reader(bytes, &OpenOptions::default()).expect("reopen");
    String::from_utf8_lossy(&package.read_part(&PartId::new(part)).expect(part)).into_owned()
}

#[test]
fn t_p9_overlay_keeps_slots_the_later_rfonts_does_not_set() {
    let mut fonts = Fonts {
        ascii: Some(Arc::from("Symbol")),
        h_ansi: Some(Arc::from("Symbol")),
        complex_script: Some(Arc::from("Symbol")),
        hint: Some(Arc::from("default")),
        ..Fonts::default()
    };
    fonts.overlay(Fonts {
        complex_script: Some(Arc::from("OpenSymbol")),
        ..Fonts::default()
    });
    assert_eq!(fonts.ascii.as_deref(), Some("Symbol"));
    assert_eq!(fonts.h_ansi.as_deref(), Some("Symbol"));
    assert_eq!(fonts.complex_script.as_deref(), Some("OpenSymbol"));
    assert_eq!(fonts.hint.as_deref(), Some("default"));
}

#[test]
#[allow(clippy::too_many_lines)]
fn t_p9_round_trip_preserves_fonts_and_names_hint_cs() {
    let theme = "\
<a:themeElements>\
<a:fontScheme name=\"Office\">\
<a:majorFont>\
<a:latin typeface=\"Calibri Light\" panose=\"020F0302020204030204\" charset=\"86\" pitchFamily=\"49\"/>\
<a:ea typeface=\"MS Mincho\" charset=\"80\" pitchFamily=\"49\"/>\
<a:cs typeface=\"Arial\" charset=\"0\" pitchFamily=\"34\"/>\
</a:majorFont>\
<a:minorFont>\
<a:latin typeface=\"Calibri\" charset=\"1\" pitchFamily=\"0\"/>\
<a:ea typeface=\"\"/>\
<a:cs typeface=\"\"/>\
</a:minorFont>\
</a:fontScheme>\
</a:themeElements>\
<a:objectDefaults>\
<a:txDef><a:spPr/><a:bodyPr/><a:lstStyle><a:defPPr><a:defRPr>\
<a:latin typeface=\"+mn-lt\"/><a:ea typeface=\"+mn-ea\"/><a:cs typeface=\"+mn-cs\"/>\
</a:defRPr></a:defPPr></a:lstStyle></a:txDef>\
</a:objectDefaults>";
    let bytes = DocxBuilder::new(Family::Strict)
        .body(
            "<w:p><w:r><w:rPr>\
<w:rFonts w:ascii=\"Symbol\" w:hAnsi=\"Symbol\" w:cs=\"Symbol\" w:hint=\"default\"/>\
<w:rFonts w:cs=\"OpenSymbol\"/>\
</w:rPr><w:t>bullet</w:t></w:r>\
<w:r><w:rPr><w:rFonts w:cs=\"Arial\" w:hint=\"cs\"/></w:rPr><w:t>cs</w:t></w:r></w:p>",
        )
        .part_xml(
            "word/settings.xml",
            "w:settings",
            "<w:themeFontLang w:val=\"en-US\" w:eastAsia=\"ja-JP\" w:bidi=\"ar-SA\"/>",
        )
        .rel("rIdSettings", "settings", "settings.xml")
        .content_type(
            "/word/settings.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml",
        )
        .part_xml("word/theme/theme1.xml", "a:theme", theme)
        .rel("rIdTheme", "theme", "theme/theme1.xml")
        .content_type(
            "/word/theme/theme1.xml",
            "application/vnd.openxmlformats-officedocument.theme+xml",
        )
        .build();
    let package =
        Package::open_reader(bytes.as_slice(), &OpenOptions::default()).expect("open synthetic");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let hint = document
        .support
        .get("w:rFonts@hint")
        .expect("hint=cs is a named partial");
    assert_eq!(hint.status, SupportStatus::Partial);
    let language = document
        .settings
        .theme_font_lang
        .as_ref()
        .expect("themeFontLang");
    assert_eq!(language.val.as_deref(), Some("en-US"));
    assert_eq!(language.east_asia.as_deref(), Some("ja-JP"));
    assert_eq!(language.bidi.as_deref(), Some("ar-SA"));
    let theme_model = document.theme.as_ref().expect("theme");
    assert_eq!(theme_model.fonts.major.latin.charset.as_deref(), Some("86"));
    assert_eq!(
        theme_model.fonts.major.latin.pitch_family.as_deref(),
        Some("49")
    );
    assert_eq!(
        theme_model
            .text_defaults
            .as_ref()
            .and_then(|fonts| fonts.latin.name.as_deref()),
        Some("+mn-lt")
    );

    let written =
        write_package(&document, Some(&package), &WriteOptions::default()).expect("write");
    let document_xml = part_text(&written.bytes, "/word/document.xml");
    assert!(
        document_xml.contains(r#"w:ascii="Symbol""#)
            && document_xml.contains(r#"w:cs="OpenSymbol""#)
            && document_xml.contains(r#"w:hint="default""#),
        "overlay must keep Symbol and hint=default: {document_xml}"
    );
    assert!(
        !document_xml.contains(r#"w:hint="cs""#),
        "Strict ST_Hint cannot carry cs: {document_xml}"
    );
    assert!(
        document_xml.contains(r#"w:cs="Arial""#),
        "the complex-script face stays: {document_xml}"
    );
    let settings = part_text(&written.bytes, "/word/settings.xml");
    assert!(
        settings.contains(r#"w:eastAsia="ja-JP""#) && settings.contains(r#"w:bidi="ar-SA""#),
        "themeFontLang scripts must be written: {settings}"
    );
    let theme_xml = part_text(&written.bytes, "/word/theme/theme1.xml");
    assert!(
        theme_xml.contains(r#"charset="86""#)
            && theme_xml.contains(r#"pitchFamily="49""#)
            && theme_xml.contains(r#"panose="020F0302020204030204""#)
            && theme_xml.contains(r#"charset="1""#)
            && theme_xml.contains(r#"pitchFamily="0""#)
            && theme_xml.contains(r#"typeface="+mn-lt""#),
        "theme charset, pitch, panose and defRPr must be written: {theme_xml}"
    );

    let mut cleared = document;
    cleared
        .theme
        .as_mut()
        .expect("theme")
        .fonts
        .major
        .latin
        .charset = None;
    let dropped = write_package(&cleared, Some(&package), &WriteOptions::default()).expect("write");
    let theme_xml = part_text(&dropped.bytes, "/word/theme/theme1.xml");
    assert!(
        !theme_xml.contains(r#"charset="86""#),
        "T-P9-3: cleared charset must not be written: {theme_xml}"
    );
    assert!(
        theme_xml.contains(r#"pitchFamily="49""#),
        "clearing charset must not drop pitchFamily: {theme_xml}"
    );
}

#[test]
fn t_p9_numbering_symbol_face_survives_the_opensymbol_overlay() {
    let Some(path) = local_corpus("../strict-ooxml-core/tests/docx/docx-jinja2-demo.docx") else {
        return;
    };
    let package = open_transitional(&path);
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let written =
        write_package(&document, Some(&package), &WriteOptions::default()).expect("write");
    let numbering = part_text(&written.bytes, "/word/numbering.xml");
    assert!(
        numbering.contains(r#"w:ascii="Symbol""#)
            && numbering.contains(r#"w:hAnsi="Symbol""#)
            && numbering.contains(r#"w:hint="default""#)
            && numbering.contains(r#"w:cs="OpenSymbol""#),
        "numbering bullet faces must survive the second rFonts: {numbering}"
    );
}

#[test]
fn t_p9_contoso_theme_font_languages_round_trip() {
    // CC0/023 is built on the same style template as the local Contoso guide
    // (1277 themeColor, 784 themeFill in styles.xml, themeFontLang eastAsia
    // ja-JP; counted in its XML), and it runs in CI (ci-core).
    let path = strict_ooxml_testkit::corpus_doc!("cc0/023").path;
    let package = open_transitional(&path);
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let language = document
        .settings
        .theme_font_lang
        .as_ref()
        .expect("CC0/023 themeFontLang");
    assert_eq!(language.east_asia.as_deref(), Some("ja-JP"));
    let written =
        write_package(&document, Some(&package), &WriteOptions::default()).expect("write");
    let settings = part_text(&written.bytes, "/word/settings.xml");
    assert!(
        settings.contains(r#"w:eastAsia="ja-JP""#) && !settings.contains("w:bidi="),
        "CC0/023 keeps eastAsia and does not invent bidi: {settings}"
    );
}

#[test]
fn t_p9_object_defaults_keep_the_def_rpr_faces_and_the_rest_of_the_element() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../strict-ooxml-core/tests/samples/DOCX_Mathematical_Equations_2fd0caa825.docx");
    let package = open_transitional(&path);
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let theme = document.theme.as_ref().expect("theme");
    assert_eq!(
        theme
            .shape_defaults
            .as_ref()
            .and_then(|fonts| fonts.latin.name.as_deref()),
        Some("Helvetica Neue Medium")
    );
    assert_eq!(
        theme
            .text_defaults
            .as_ref()
            .and_then(|fonts| fonts.latin.name.as_deref()),
        Some("+mn-lt")
    );
    let written =
        write_package(&document, Some(&package), &WriteOptions::default()).expect("write");
    let theme_xml = part_text(&written.bytes, "/word/theme/theme1.xml");
    assert!(
        theme_xml.contains(r#"typeface="Helvetica Neue Medium""#)
            && theme_xml.contains(r#"typeface="+mn-lt""#)
            && theme_xml.contains(r#"typeface="+mn-ea""#)
            && theme_xml.contains(r#"typeface="+mn-cs""#)
            && theme_xml.contains("<a:lnDef>")
            && theme_xml.contains(r#"typeface="Helvetica Neue""#)
            && theme_xml.contains(r#"val="100%""#)
            && theme_xml.contains(r#"lim="400%""#)
            && !theme_xml.contains("100000")
            && !theme_xml.contains("400000"),
        "objectDefaults must be copied, with Strict percentages: {theme_xml}"
    );
}

/// The jinja2-demo construct, built here so it runs everywhere: a numbering
/// level whose `w:rPr` carries two `w:rFonts`, the second adding only
/// `w:cs="OpenSymbol"` (LibreOffice writes bullets this way). The bullet's
/// Symbol faces and the second element's complex-script face must all survive.
#[test]
fn t_p9_numbering_second_rfonts_keeps_both_faces_synthetic() {
    let numbering = "<w:abstractNum w:abstractNumId=\"0\">\
<w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/>\
<w:lvlText w:val=\"\u{f0b7}\"/><w:rPr>\
<w:rFonts w:ascii=\"Symbol\" w:hAnsi=\"Symbol\" w:hint=\"default\"/>\
<w:rFonts w:cs=\"OpenSymbol\"/></w:rPr></w:lvl></w:abstractNum>\
<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>";
    let bytes = DocxBuilder::new(Family::Transitional)
        .body(
            "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr>\
<w:r><w:t>item</w:t></w:r></w:p>",
        )
        .part_xml("word/numbering.xml", "w:numbering", numbering)
        .rel("rIdNumbering", "numbering", "numbering.xml")
        .content_type(
            "/word/numbering.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml",
        )
        .build();
    let package = Package::open_reader(
        bytes.as_slice(),
        &OpenOptions::default()
            .conformance(ConformancePolicy::Normalize)
            .shared_normalization(Arc::new(
                strict_ooxml_core::normalize::transitional::TransitionalNormalizer::new(),
            )),
    )
    .expect("open");
    let document = parse_document(&package, &ParseOptions::default()).expect("parse");
    let written =
        write_package(&document, Some(&package), &WriteOptions::default()).expect("write");
    let numbering = part_text(&written.bytes, "/word/numbering.xml");
    assert!(
        numbering.contains(r#"w:ascii="Symbol""#)
            && numbering.contains(r#"w:hAnsi="Symbol""#)
            && numbering.contains(r#"w:hint="default""#)
            && numbering.contains(r#"w:cs="OpenSymbol""#),
        "numbering bullet faces must survive the second rFonts: {numbering}"
    );
}
