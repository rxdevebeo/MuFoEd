//! Serialization of the auxiliary parts: `styles.xml`, `numbering.xml`,
//! `settings.xml`, `fontTable.xml`, `theme1.xml`, `footnotes.xml`,
//! `endnotes.xml` and the header/footer parts.
//!
//! A part is written only when the model carries content for it, so a document
//! that had no `numbering.xml` does not acquire one — that is what keeps a
//! round trip from growing parts it did not have (SC-3).

use strict_ooxml_wml::model::document::HeaderFooter;
use strict_ooxml_wml::model::ids::Ilvl;
use strict_ooxml_wml::model::inline::{Inline, RunContent};
use strict_ooxml_wml::model::notes::{Note, NoteKind, NoteTable};
use strict_ooxml_wml::model::numbering::{AbstractNum, Level, NumberingTable};
use strict_ooxml_wml::model::settings::{MathProperties, Settings};
use strict_ooxml_wml::model::styles::{DocDefaults, Style, StyleTable};
use strict_ooxml_wml::model::theme::Theme;
use strict_ooxml_wml::model::values::StyleType;

use crate::body::blocks;
use crate::ctx::{Ctx, NoteRole};
use crate::props::{note_properties, paragraph_properties, run_properties, table_properties};
use crate::xml::{WriteError, XmlWriter, NS_A, NS_M, NS_PIC, NS_R, NS_W, NS_WP};
use strict_ooxml_wml::model::fonts::{EmbedKind, FontTable};
use strict_ooxml_wml::parse::LOST_FONT_PART;

/// The namespace declarations a `w:` part carries.
///
/// `w14` used to be declared here because the paragraph writer emitted
/// `w14:paraId`/`w14:textId`, and it is not any more: `w14` is absent from
/// ECMA-376 entirely and Strict conformance is defined on the post-MCE part, so a
/// Strict part that declares the namespace is one line away from carrying an
/// attribute the standard does not have (ADR-0014).
/// `w:` and `r:`, plus `m:` for `m:mathPr`.
///
/// The maths vocabulary is listed here for the same reason `wp`/`a`/`pic` are
/// listed on a header that may hold a drawing: `start_root` filters declarations
/// down to the prefixes the part actually used, so naming `m` costs a document
/// that has no `m:mathPr` nothing, and a document that has one gets it declared.
/// Without the entry the writer emitted `<m:mathPr>` into a root that did not
/// declare the prefix, and the part did not parse at all.
const WML_NAMESPACES: [(&str, &str); 3] = [("w", NS_W), ("r", NS_R), ("m", NS_M)];

/// The namespaces a part that can hold **block or inline content** may use.
///
/// Wider than [`WML_NAMESPACES`] on purpose, and filtered at write time
/// ([`XmlWriter::start_root`](crate::xml::XmlWriter::start_root)): a header with a
/// picture in it needs `wp`, `a` and `pic`, and a header without one must not
/// declare them. Which of these parts a drawing can appear in is a property of the
/// *schema* (`EG_BlockLevelElts` reaches `w:drawing` in a header, a footer, a
/// footnote and an endnote) and not something a table per part would get right
/// twice — once for the part that has a picture and once for the part that does
/// not.
///
/// The vendor shapes are here for the same reason and were found the same way.
/// `strict-ooxml-write` reproduces `wps`/`wpg` shapes from the model (ADR-0014's
/// `XS-18`/`XS-19` debt, kept on purpose), and a **shape in a header** needs its
/// prefix declared on the header's root. It was not, and the part did not parse —
/// which is how this list learned that a header can hold more than a picture.
const CONTENT_NAMESPACES: [(&str, &str); 5] = [
    ("w", NS_W),
    ("r", NS_R),
    ("wp", NS_WP),
    ("a", NS_A),
    ("pic", NS_PIC),
];

/// [`CONTENT_NAMESPACES`] plus the vendor vocabularies a shape or a group can be
/// written in.
///
/// Separate from [`CONTENT_NAMESPACES`] so that the *document* part and a
/// decoration part do not have to grow their lists independently — and the
/// document part already declared all of these, which is why the defect was
/// invisible until a shape ended up outside `word/document.xml`.
fn content_and_vendor_namespaces() -> Vec<(&'static str, &'static str)> {
    let mut out: Vec<(&'static str, &'static str)> = CONTENT_NAMESPACES.to_vec();
    for (prefix, uri) in crate::drawing::namespaces() {
        if !out.iter().any(|(existing, _)| *existing == prefix) {
            out.push((prefix, uri));
        }
    }
    out
}

/// The namespace declarations the theme part carries.
const THEME_NAMESPACES: [(&str, &str); 1] = [("a", NS_A)];

/// Writes `w:styles`.
pub fn styles_part(
    ctx: &mut Ctx<'_>,
    table: &StyleTable,
) -> std::result::Result<String, WriteError> {
    let mut xml = XmlWriter::new();
    xml.start_root("w:styles", &WML_NAMESPACES);
    if let Some(defaults) = table.defaults() {
        doc_defaults(ctx, &mut xml, defaults);
    }
    for style in table.iter() {
        style_element(ctx, &mut xml, style);
    }
    xml.end();
    xml.finish()
}

fn doc_defaults(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, defaults: &DocDefaults) {
    xml.start("w:docDefaults");
    xml.start("w:rPrDefault");
    run_properties(xml, &defaults.run);
    xml.end();
    xml.start("w:pPrDefault");
    paragraph_properties(ctx, xml, &defaults.paragraph);
    xml.end();
    xml.end();
    let _ = ctx;
}

/// Returns the Strict lexical value of a style type.
fn style_type_name(kind: StyleType) -> &'static str {
    match kind {
        StyleType::Paragraph => "paragraph",
        StyleType::Character => "character",
        StyleType::Table => "table",
        StyleType::Numbering => "numbering",
    }
}

fn style_element(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, style: &Style) {
    xml.start("w:style");
    xml.attr_w("type", style_type_name(style.style_type));
    xml.attr_w("styleId", style.id.as_str());
    // `w:default` is an ATTRIBUTE of `w:style`, not one of its children, so it
    // is written here rather than in the loop below.
    if style.is_default {
        xml.attr_w("default", "true");
    }
    // `CT_Style` is an xsd:sequence too, and it is one of the two places where
    // the writer's order was simply not the schema's: `w:rPr` comes BEFORE
    // `w:tblPr`, and the paragraph properties were emitted between them, so a
    // style with both a run default and a table default produced `w:pPr` after
    // `w:tblPr` and the schema answered "This element is not expected" (`XS-07`).
    // The order is [`strict_ooxml_write::order::STYLE`] and this loop is it.
    for name in crate::order::STYLE {
        match *name {
            "name" => {
                if let Some(name) = &style.name {
                    xml.empty_attr_w("w:name", "val", name.as_ref());
                }
            }
            "basedOn" => {
                if let Some(based_on) = &style.based_on {
                    xml.empty_attr_w("w:basedOn", "val", based_on.as_str());
                }
            }
            "next" => {
                if let Some(next) = &style.next {
                    xml.empty_attr_w("w:next", "val", next.as_str());
                }
            }
            "link" => {
                if let Some(link) = &style.link {
                    xml.empty_attr_w("w:link", "val", link.as_str());
                }
            }
            "uiPriority" => {
                if let Some(priority) = style.ui_priority {
                    xml.empty_attr_w("w:uiPriority", "val", priority);
                }
            }
            // Keyed on `semiHidden`, not on `hidden`: `CT_Style` declares BOTH,
            // and the model's `hidden` flag has always been written as
            // `w:semiHidden`. Arming this on `hidden` put the element two slots
            // before `w:uiPriority` - which is how the new order test found it
            // on its first run.
            "semiHidden" if style.hidden => xml.empty("w:semiHidden"),
            "pPr" => paragraph_properties(ctx, xml, &style.paragraph),
            "rPr" => run_properties(xml, &style.run),
            "tblPr" if style.style_type == StyleType::Table => {
                table_properties(xml, &style.table);
            }
            _ => {}
        }
    }
    let _ = ctx;
    xml.end();
}

/// Writes `w:numbering`.
pub fn numbering_part(
    ctx: &mut Ctx<'_>,
    table: &NumberingTable,
) -> std::result::Result<String, WriteError> {
    let mut xml = XmlWriter::new();
    xml.start_root("w:numbering", &WML_NAMESPACES);
    for abstract_num in table.abstracts() {
        abstract_num_element(ctx, &mut xml, abstract_num);
    }
    for num in table.nums() {
        xml.start("w:num");
        xml.attr_w("numId", num.num_id.0);
        xml.empty_attr_w("w:abstractNumId", "val", num.abstract_num_id.0);
        for over in &num.overrides {
            xml.start("w:lvlOverride");
            ilvl_attr(ctx, &mut xml, &over.ilvl, "w:lvlOverride");
            if let Some(start) = over.start_override {
                xml.empty_attr_w("w:startOverride", "val", start);
            }
            if let Some(level) = &over.level {
                level_element(ctx, &mut xml, level);
            }
            xml.end();
        }
        xml.end();
    }
    xml.end();
    xml.finish()
}

fn abstract_num_element(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, abstract_num: &AbstractNum) {
    xml.start("w:abstractNum");
    xml.attr_w("abstractNumId", abstract_num.id.0);
    if let Some(kind) = &abstract_num.multi_level_type {
        xml.empty_attr_w("w:multiLevelType", "val", kind.as_ref());
    }
    if let Some(link) = &abstract_num.num_style_link {
        xml.empty_attr_w("w:numStyleLink", "val", link.as_str());
    }
    if let Some(link) = &abstract_num.style_link {
        xml.empty_attr_w("w:styleLink", "val", link.as_str());
    }
    for level in &abstract_num.levels {
        level_element(ctx, xml, level);
    }
    xml.end();
}

/// Writes a `w:ilvl` attribute, refusing a level the schema does not have.
///
/// `ST_DecimalNumber` is unbounded, so the range is a convention both backends
/// rely on and neither the schema nor the reader enforces on the way in - the
/// reader clamps, the renderer clamps, and until this the writer wrote whatever
/// it was handed (`STAGE-10-TASK.md` E33). Writing it verbatim would leave the
/// two backends disagreeing about one document with neither saying so.
fn ilvl_attr(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, ilvl: &Ilvl, at: &str) {
    if ilvl.is_valid() {
        xml.attr_w("ilvl", ilvl.0);
    } else {
        ctx.report_unsupported(
            "w:ilvl",
            &format!(
                "list level {} is outside the schema's 0..={} range and was not written to {at}",
                ilvl.0,
                Ilvl::MAX
            ),
            &strict_ooxml_core::error::SourceLocation::unknown(),
        );
    }
}

fn level_element(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, level: &Level) {
    xml.start("w:lvl");
    ilvl_attr(ctx, xml, &level.ilvl, "w:lvl");
    if let Some(start) = level.start {
        xml.empty_attr_w("w:start", "val", start);
    }
    if let Some(format) = &level.format {
        xml.empty_attr_w("w:numFmt", "val", format.as_ref());
    }
    if let Some(style) = &level.paragraph_style {
        xml.empty_attr_w("w:pStyle", "val", style.as_str());
    }
    if let Some(text) = &level.text {
        xml.empty_attr_w("w:lvlText", "val", text.as_ref());
    }
    if let Some(justification) = &level.justification {
        xml.empty_attr_w("w:lvlJc", "val", justification.as_str());
    }
    if let Some(suffix) = &level.suffix {
        xml.empty_attr_w("w:suff", "val", suffix.as_ref());
    }
    if let Some(restart) = level.restart {
        xml.empty_attr_w("w:lvlRestart", "val", restart);
    }
    if level.is_legal {
        xml.empty("w:isLgl");
    }
    if level.tentative {
        xml.empty("w:tentative");
    }
    paragraph_properties(ctx, xml, &level.paragraph);
    run_properties(xml, &level.run);
    let _ = ctx;
    xml.end();
}

/// Writes `w:settings`.
///
/// The children go out in the order `CT_Settings` declares, which is
/// [`crate::order::SETTINGS`], and the loop below IS that order. It matters for
/// one child in particular: `w:compat` is declared after `w:footnotePr` and
/// `w:endnotePr` and before `w:themeFontLang`, and the writer used to put it
/// last of all - so `w:compat` was the element libxml2 named in every one of the
/// sixteen "This element is not expected" messages `w:settings` produced across
/// the corpus (`XS-06`). `w:compat` is not a Transitional leftover: it is declared
/// in Strict at `wml.xsd:2815`.
pub fn settings_part(
    ctx: &mut Ctx<'_>,
    settings: &Settings,
) -> std::result::Result<String, WriteError> {
    let mut xml = XmlWriter::new();
    xml.start_root("w:settings", &WML_NAMESPACES);
    for name in crate::order::SETTINGS {
        settings_child(ctx, &mut xml, settings, name);
    }
    xml.end();
    xml.finish()
}

/// Writes the `w:settings` child called `name`, when the model has it.
/// Writes `m:mathPr` and its children.
///
/// The order below IS `CT_MathPr`'s `xsd:sequence` - `mathFont`, `brkBin`,
/// `brkBinSub`, `smallFrac`, `dispDef`, `lMargin`, `rMargin`, `defJc`, `preSp`,
/// `postSp`, `interSp`, `intraSp`, then the `xsd:choice` of `wrapIndent` /
/// `wrapRight`, then `intLim`, `naryLim` - and it is neither alphabetical nor
/// the order the fields are read in by anything else. `brkBinSub` sorts before
/// `brkBin`, `intLim` before `intraSp`, and the choice is nested one level below
/// the sequence so a reader of the sequence's direct children never sees it.
///
/// Every child is written only when the model holds it, because all of them are
/// optional: writing an empty `m:preSp` would be a value the producer never
/// chose. `wrapIndent` and `wrapRight` are arms of one choice, and the model
/// keeps both — the schema, not the parser, decides that a document with both is
/// invalid, and a malformed value the XSD gate names beats a value silently
/// dropped.
fn math_properties(xml: &mut XmlWriter, properties: &MathProperties) {
    xml.start("m:mathPr");
    let children: [(&str, &Option<std::sync::Arc<str>>); 16] = [
        ("m:mathFont", &properties.math_font),
        ("m:brkBin", &properties.break_binary_operator),
        ("m:brkBinSub", &properties.break_binary_sub),
        ("m:smallFrac", &properties.small_fraction),
        ("m:dispDef", &properties.display_default),
        ("m:lMargin", &properties.left_margin),
        ("m:rMargin", &properties.right_margin),
        ("m:defJc", &properties.default_justification),
        ("m:preSp", &properties.pre_space),
        ("m:postSp", &properties.post_space),
        ("m:interSp", &properties.inter_space),
        ("m:intraSp", &properties.intra_space),
        ("m:wrapIndent", &properties.wrap_indent),
        ("m:wrapRight", &properties.wrap_right),
        ("m:intLim", &properties.integral_limit),
        ("m:naryLim", &properties.nary_limit),
    ];
    for (name, value) in children {
        if let Some(value) = value {
            xml.empty_attr(name, "m:val", value.as_ref());
        }
    }
    xml.end();
}

fn settings_child(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, settings: &Settings, name: &str) {
    match name {
        "zoom" => {
            if let Some(zoom) = settings.zoom {
                xml.start("w:zoom");
                // `ST_DecimalNumberOrPercent` is `union(s:ST_Percentage)` and
                // `s:ST_Percentage`'s pattern requires the sign - `100` is not a
                // value this attribute can hold and `100%` is (`XS-05`). Every
                // producer in the corpus that writes valid Strict writes the sign:
                // LibreOffice, docx4j and the Open XML SDK fixtures all write
                // `w:percent="100%"`, and the ones that write `100` fail their own
                // schema.
                xml.attr("w:percent", format!("{}%", zoom.percent.unwrap_or(100)));
                if let Some(kind) = zoom.kind {
                    let value = match kind {
                        strict_ooxml_wml::model::settings::DocumentZoom::None => "none",
                        strict_ooxml_wml::model::settings::DocumentZoom::FullPage => "fullPage",
                        strict_ooxml_wml::model::settings::DocumentZoom::BestFit => "bestFit",
                        strict_ooxml_wml::model::settings::DocumentZoom::TextFit => "textFit",
                    };
                    xml.attr_w("val", value);
                }
                xml.end();
            }
        }
        "displayBackgroundShape" if settings.display_background_shape => {
            xml.empty("w:displayBackgroundShape");
        }
        "hideSpellingErrors" if settings.hide_spelling_errors => {
            xml.empty("w:hideSpellingErrors");
        }
        "hideGrammaticalErrors" if settings.hide_grammatical_errors => {
            xml.empty("w:hideGrammaticalErrors");
        }
        "proofState" if settings.proofing => xml.empty("w:proofState"),
        "trackRevisions" if settings.track_revisions => xml.empty("w:trackRevisions"),
        "documentProtection" => {
            if let Some(edit) = &settings.document_protection {
                xml.empty_attr_w("w:documentProtection", "edit", edit.as_ref());
            }
        }
        "defaultTabStop" => {
            if let Some(stop) = settings.default_tab_stop {
                xml.empty_attr_w("w:defaultTabStop", "val", stop.0);
            }
        }
        "autoHyphenation" if settings.auto_hyphenation => xml.empty("w:autoHyphenation"),
        "hyphenationZone" => {
            if let Some(zone) = settings.hyphenation_zone {
                xml.empty_attr_w("w:hyphenationZone", "val", zone.0);
            }
        }
        "doNotHyphenateCaps" if settings.do_not_hyphenate_caps => {
            xml.empty("w:doNotHyphenateCaps");
        }
        "evenAndOddHeaders" if settings.even_and_odd_headers => {
            xml.empty("w:evenAndOddHeaders");
        }
        "mirrorMargins" if settings.mirror_margins => xml.empty("w:mirrorMargins"),
        // The Strict home of the flag Transitional writes inside `w:sectPr`,
        // where `EG_SectPrContents` has no slot for it. See
        // `Settings::gutter_at_top`.
        "gutterAtTop" if settings.gutter_at_top => xml.empty("w:gutterAtTop"),
        "footnotePr" => {
            if !settings.footnote_properties.is_empty() {
                note_properties(xml, "w:footnotePr", &settings.footnote_properties);
            }
        }
        "endnotePr" => {
            if !settings.endnote_properties.is_empty() {
                note_properties(xml, "w:endnotePr", &settings.endnote_properties);
            }
        }
        "compat" => {
            if !settings.compatibility.is_empty() {
                xml.start("w:compat");
                // w:compat holds w:compatSetting elements keyed by name, which is
                // what the reader records; writing the key as an element name would
                // produce markup no reader recognises.
                for (name, value) in &settings.compatibility {
                    xml.start("w:compatSetting");
                    xml.attr_w("name", name.as_ref());
                    xml.attr_w("val", value.as_ref());
                    xml.end();
                }
                xml.end();
            }
        }
        "mathPr" => {
            if let Some(mathematics) = &settings.math_properties {
                math_properties(xml, mathematics);
            }
        }
        "themeFontLang" => {
            if let Some(language) = &settings.theme_font_lang {
                xml.empty_attr_w("w:themeFontLang", "val", language.as_ref());
            }
        }
        "decimalSymbol" => {
            if let Some(symbol) = &settings.decimal_symbol {
                xml.empty_attr_w("w:decimalSymbol", "val", symbol.as_ref());
            }
        }
        "listSeparator" => {
            if let Some(separator) = &settings.list_separator {
                xml.empty_attr_w("w:listSeparator", "val", separator.as_ref());
            }
        }
        _ => {}
    }
    let _ = ctx;
}

/// Writes `w:fontTable`.
///
/// **The faces a document embeds, not only the faces it names.** Before
/// 2026-10-02 this part was derived from the families the styles and runs
/// mention and nothing else was read, so every `w:embed*` element and every
/// `word/fonts/*.ttf` behind one went missing — sixteen binaries in two corpus
/// documents, named by nothing (`W7-DROPPED`).
///
/// Two things the earlier version got wrong are fixed here rather than repeated:
///
/// - it wrote `w:family` and `w:pitch` as EMPTY elements. `CT_String` requires
///   `w:val`, so `<w:family/>` was worse than no element at all, and nothing was
///   expressed by those empties (`XS-01`). This version writes only what the model
///   knows: `@w:name`, which is the one attribute `CT_Font` requires;
/// - it could not express an embedded face at all. `ctx.font_rel` gives the id
///   **this part's own** `.rels` will carry, which is the whole difficulty: the
///   source's ids belong to `word/_rels/fontTable.xml.rels` and mean nothing
///   here.
pub fn font_table_part(
    ctx: &mut Ctx<'_>,
    table: &FontTable,
    families: &[String],
) -> std::result::Result<String, WriteError> {
    let mut xml = XmlWriter::new();
    xml.start_root("w:fonts", &WML_NAMESPACES);
    // The model's own entries first, in source order, and then the families the
    // document merely names. A face this project embeds has a table entry with
    // facts about it; a face it does not has a name and nothing else, which is
    // what `CT_Font` asks for and all that can honestly be written.
    let mut seen: Vec<&str> = Vec::new();
    for entry in &table.fonts {
        seen.push(&entry.name);
        xml.start("w:font");
        xml.attr_w("name", entry.name.as_ref());
        for kind in EmbedKind::all() {
            let Some(font) = entry.embeds.get(&kind) else {
                continue;
            };
            if font.part.as_str() == LOST_FONT_PART {
                ctx.report_unsupported(
                    kind.element(),
                    &format!(
                        "the embedded {} of {} could not be resolved when the document was read, \
                         so the face is named without its bytes",
                        face_name(kind),
                        entry.name
                    ),
                    &strict_ooxml_core::error::SourceLocation::unknown(),
                );
                continue;
            }
            let Some(rel) = ctx.font_rel(&font.part) else {
                continue;
            };
            xml.start(kind.element());
            xml.attr_r_opt("id", Some(rel));
            xml.attr_w_opt("fontKey", font.font_key.as_deref());
            if font.subsetted {
                xml.attr_w("subsetted", "true");
            }
            xml.end();
        }
        xml.end();
    }
    for family in families {
        if seen.contains(&family.as_str()) {
            continue;
        }
        xml.empty_attr_w("w:font", "name", family);
    }
    xml.end();
    let _ = ctx;
    xml.finish()
}

/// The English name of a face, for a report line a person reads.
fn face_name(kind: EmbedKind) -> &'static str {
    match kind {
        EmbedKind::Regular => "regular face",
        EmbedKind::Bold => "bold face",
        EmbedKind::Italic => "italic face",
        EmbedKind::BoldItalic => "bold-italic face",
    }
}

/// Writes `a:theme`.
///
/// The model keeps only the font sets and the colour scheme, which is what the
/// cascade needs, so the theme part is written as a minimal but schema-shaped
/// document: a missing element format would make Word report the file as
/// unreadable content.
///
/// The format scheme is a placeholder - three `phClr` fills, three lines, three
/// effects - because the model does not carry the source's real ones. That is a
/// loss, and it is **recorded here**, not left to the parse phase: a document
/// built rather than parsed (a PDF conversion, a hand-built model) never went
/// through `parse/theme.rs`, so for those the loss would otherwise be silent
/// (`STAGE-10-TASK.md` E35, SC-10). `ctx` is passed for exactly this reason.
pub fn theme_part(ctx: &mut Ctx<'_>, theme: &Theme) -> std::result::Result<String, WriteError> {
    ctx.report_partial(
        "a:fmtScheme",
        "theme fill, line and effect styles are not carried by the model; a \
         placeholder scheme was written",
        &strict_ooxml_core::error::SourceLocation::unknown(),
    );
    let mut xml = XmlWriter::new();
    xml.start_root("a:theme", &THEME_NAMESPACES);
    xml.attr("name", "strict-ooxml");
    xml.start("a:themeElements");
    xml.start("a:clrScheme");
    xml.attr("name", "strict-ooxml");
    for slot in [
        "dk1", "lt1", "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5",
        "accent6", "hlink", "folHlink",
    ] {
        // Every slot is written as `a:srgbClr`, including `dk1`/`lt1`. A real
        // producer writes those two as `a:sysClr val="windowText"`, which the
        // reader resolves through the *last resolved* colour; writing the
        // resolved value directly is the same colour, is valid inside any slot,
        // and is what makes the part a fixed point of the reader.
        //
        // The model stores colours as `#rrggbb`; `a:srgbClr/@val` is six hex
        // digits with no prefix, so the marker is stripped on the way out.
        let value = theme
            .colors
            .slot(slot)
            .unwrap_or("000000")
            .trim_start_matches('#');
        xml.start(&format!("a:{slot}"));
        xml.empty_attr("a:srgbClr", "val", value);
        xml.end();
    }
    xml.end();
    xml.start("a:fontScheme");
    xml.attr("name", "strict-ooxml");
    for (name, set) in [
        ("a:majorFont", &theme.fonts.major),
        ("a:minorFont", &theme.fonts.minor),
    ] {
        xml.start(name);
        // All three are written, always. `CT_FontCollection` declares `latin`,
        // `ea` and `cs` with `minOccurs="1"` and no default, so an `a:majorFont`
        // that carries only the typeface the model happened to hold is invalid
        // (`XS-03`), and an empty typeface is a legal `xsd:string` that means
        // "no face for this script" - which is what the model actually knows.
        font_collection(&mut xml, "a:latin", set.latin.as_deref());
        font_collection(&mut xml, "a:ea", set.east_asia.as_deref());
        font_collection(&mut xml, "a:cs", set.cs.as_deref());
        xml.end();
    }
    xml.end();
    format_scheme(&mut xml);
    xml.end();
    xml.end();
    xml.finish()
}

/// Writes the smallest `a:fmtScheme` the schema accepts.
///
/// `CT_StyleMatrix` requires all four lists, and each list requires THREE
/// entries - `EG_FillProperties` with `minOccurs="3"`, `a:ln` with
/// `minOccurs="3"`, `a:effectStyle` with `minOccurs="3"`. The previous version
/// wrote the four lists empty, which is 88 violations across the corpus, four of
/// them on every single package, and it is a known gap from stage 8 that was
/// never written into any report.
///
/// The entries carry no information, and that is stated rather than dressed up:
/// the reader records `a:fmtScheme` as `Partial` with the reason "theme
/// effects/fills/line styles are not resolved" (`parse/theme.rs`), so the loss is
/// already declared where it happens. What is written here is the placeholder
/// shape a theme with no format scheme has - `phClr` is DrawingML's own word for
/// "the colour the shape supplies", which is exactly the honest answer when the
/// part knows no fill.
///
/// The alternative - not writing `a:fmtScheme` at all - is not available:
/// `CT_BaseStyles` requires `clrScheme`, `fontScheme` and `fmtScheme`, all three
/// `minOccurs="1"`, so dropping it would trade 88 violations for 22 and leave a
/// theme part the schema rejects at its root.
/// The four fill entries a fill list needs, in schema order.
///
/// `EG_FillProperties` carries `minOccurs="3"`, so one is not enough; three
/// `a:solidFill` over `phClr` is the smallest set that validates.
fn fill_list(xml: &mut XmlWriter, list: &str) {
    xml.start(list);
    for _ in 0..3 {
        xml.start("a:solidFill");
        xml.empty_attr("a:schemeClr", "val", "phClr");
        xml.end();
    }
    xml.end();
}

fn format_scheme(xml: &mut XmlWriter) {
    xml.start("a:fmtScheme");
    xml.attr("name", "strict-ooxml");

    // The four lists in `CT_StyleMatrix`'s order: fillStyleLst, lnStyleLst,
    // effectStyleLst, bgFillStyleLst. It is a sequence, so the two fill lists
    // cannot be written next to each other however natural that looks.
    fill_list(xml, "a:fillStyleLst");

    xml.start("a:lnStyleLst");
    for width in ["9525", "25400", "38100"] {
        xml.start("a:ln");
        xml.attr("w", width);
        xml.attr("cap", "flat");
        xml.attr("cmpd", "sng");
        xml.attr("algn", "ctr");
        xml.start("a:solidFill");
        xml.empty_attr("a:schemeClr", "val", "phClr");
        xml.end();
        xml.empty("a:prstDash");
        xml.end();
    }
    xml.end();

    xml.start("a:effectStyleLst");
    for _ in 0..3 {
        xml.start("a:effectStyle");
        xml.empty("a:effectLst");
        xml.end();
    }
    xml.end();

    fill_list(xml, "a:bgFillStyleLst");

    xml.end();
}

fn font_collection(xml: &mut XmlWriter, name: &str, typeface: Option<&str>) {
    xml.start(name);
    xml.attr("typeface", typeface.unwrap_or(""));
    xml.end();
}

/// Writes `w:footnotes` or `w:endnotes`.
pub fn notes_part(
    ctx: &mut Ctx<'_>,
    table: &NoteTable,
    is_footnote: bool,
) -> std::result::Result<String, WriteError> {
    let mut xml = XmlWriter::new();
    xml.start_root(
        if is_footnote {
            "w:footnotes"
        } else {
            "w:endnotes"
        },
        &content_and_vendor_namespaces(),
    );
    let role = if is_footnote {
        NoteRole::Footnote
    } else {
        NoteRole::Endnote
    };
    let ctx = &mut *ctx;
    ctx.set_note_role(role);
    for note in table.iter() {
        note_element(ctx, &mut xml, note, is_footnote);
    }
    xml.end();
    xml.finish()
}

/// Whether a note's leading paragraph already carries the reference marker.
///
/// Word's separator notes put a run holding `w:footnoteRef`/`w:endnoteRef` at
/// the start of their first paragraph, and the parser records it as
/// [`RunContent::NoteRef`]. The writer used to emit that run itself *as well*,
/// which is invisible on the first write and grows the note by one run on every
/// later one — a round trip that never settles. Emitting it only when the model
/// does not already carry it keeps a note Word wrote byte-identical and still
/// gives a hand-built note the marker it needs.
fn leading_paragraph_has_reference(note: &Note) -> bool {
    let Some(block) = note.blocks.first() else {
        return false;
    };
    let strict_ooxml_wml::model::block::Block::Paragraph(paragraph) = block else {
        return false;
    };
    paragraph.inlines.iter().any(
        |inline| matches!(inline, Inline::Run(run) if run.content.contains(&RunContent::NoteRef)),
    )
}

fn note_element(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, note: &Note, is_footnote: bool) {
    let element = if is_footnote {
        "w:footnote"
    } else {
        "w:endnote"
    };
    let reference = if is_footnote {
        "w:footnoteRef"
    } else {
        "w:endnoteRef"
    };
    xml.start(element);
    xml.attr_w("type", note.kind.as_str());
    xml.attr_w("id", note.id);
    xml.start("w:p");
    // Word's own separator notes carry a run holding the reference element and
    // nothing else; reproducing that keeps the note area identical. The model
    // usually already has that run, so this is a fallback, not an addition.
    if !leading_paragraph_has_reference(note) {
        xml.start("w:r");
        xml.empty(reference);
        xml.end();
    }
    for block in &note.blocks {
        // The first paragraph already exists, so a leading paragraph is merged
        // into it by writing only its inlines.
        if let strict_ooxml_wml::model::block::Block::Paragraph(paragraph) = block {
            for inline in &paragraph.inlines {
                crate::body::inline_item(ctx, xml, inline);
            }
            continue;
        }
        crate::body::block_item(ctx, xml, block);
    }
    xml.end();
    xml.end();
}

/// The separator/continuation notes a fresh document must declare.
///
/// Word treats a missing `w:separator` note as a note area with no separator
/// rule, which changes the page, so a written package always carries them when
/// it carries notes at all.
#[must_use]
pub fn default_separator_notes(is_footnote: bool) -> Vec<Note> {
    // The separator note holds the role's own reference element (`w:footnoteRef`
    // or `w:endnoteRef`); the model records both as [`RunContent::NoteRef`], and
    // the writer picks the element name from the part it is writing. So the
    // content is the same for either role and only the caller differs.
    let _ = is_footnote;
    let reference = strict_ooxml_wml::model::inline::RunContent::NoteRef;
    let paragraph = |content: Vec<strict_ooxml_wml::model::inline::RunContent>| {
        strict_ooxml_wml::model::block::Block::Paragraph(
            strict_ooxml_wml::model::block::Paragraph {
                props: Default::default(),
                inlines: vec![strict_ooxml_wml::model::inline::Inline::Run(
                    strict_ooxml_wml::model::inline::Run {
                        props: Default::default(),
                        content,
                        location: strict_ooxml_core::error::SourceLocation::unknown(),
                    },
                )],
                rsids: Default::default(),
                para_id: None,
                text_id: None,
                location: strict_ooxml_core::error::SourceLocation::unknown(),
            },
        )
    };
    vec![
        Note {
            id: -1,
            kind: NoteKind::Separator,
            blocks: vec![paragraph(vec![reference])],
            location: strict_ooxml_core::error::SourceLocation::unknown(),
        },
        Note {
            id: 0,
            kind: NoteKind::ContinuationSeparator,
            blocks: vec![paragraph(Vec::new())],
            location: strict_ooxml_core::error::SourceLocation::unknown(),
        },
    ]
}

/// Writes a `w:hdr` or `w:ftr` part.
pub fn header_footer_part(
    ctx: &mut Ctx<'_>,
    header_footer: &HeaderFooter,
) -> std::result::Result<String, WriteError> {
    let mut xml = XmlWriter::new();
    xml.start_root(
        if header_footer.is_header {
            "w:hdr"
        } else {
            "w:ftr"
        },
        &content_and_vendor_namespaces(),
    );
    blocks(ctx, &mut xml, &header_footer.blocks);
    if header_footer.blocks.is_empty() {
        xml.start("w:p");
        xml.end();
    }
    xml.end();
    xml.finish()
}

/// Collects the font families the document names, in first-seen order.
///
/// The order is the document's, not a hash map's, so `fontTable.xml` is
/// reproducible.
#[must_use]
pub fn font_families(styles: &StyleTable) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |family: Option<&std::sync::Arc<str>>| {
        if let Some(family) = family {
            let family = family.to_string();
            if !out.contains(&family) {
                out.push(family);
            }
        }
    };
    for style in styles.iter() {
        if let Some(fonts) = &style.run.fonts {
            push(fonts.ascii.as_ref());
            push(fonts.h_ansi.as_ref());
            push(fonts.east_asia.as_ref());
            push(fonts.complex_script.as_ref());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use strict_ooxml_core::normalize::report::NormalizationReport;
    use strict_ooxml_wml::model::ids::StyleId;
    use strict_ooxml_wml::model::props::RunProperties;
    use strict_ooxml_wml::model::styles::{Style, StyleTable};
    use strict_ooxml_wml::model::values::{StyleType, TriState};

    use super::{font_families, styles_part, Ctx};

    #[test]
    fn a_style_keeps_its_type_and_relationships() {
        let mut table = StyleTable::new();
        table.insert(Style {
            id: StyleId::new("Heading1"),
            style_type: StyleType::Paragraph,
            name: Some("heading 1".into()),
            based_on: Some(StyleId::new("Normal")),
            next: None,
            link: None,
            is_default: false,
            hidden: false,
            ui_priority: Some(9),
            table: Default::default(),
            paragraph: Default::default(),
            run: RunProperties {
                bold: TriState::On,
                size: Some(strict_ooxml_wml::model::values::HalfPoints(32)),
                ..RunProperties::default()
            },
            based_on_chain: Vec::new(),
            location: strict_ooxml_core::error::SourceLocation::unknown(),
        });
        let mut report = NormalizationReport::new();
        let mut ctx = Ctx::new(&mut report);
        let xml = styles_part(&mut ctx, &table).expect("styles_part balances");
        assert!(
            xml.contains("<w:style w:type=\"paragraph\" w:styleId=\"Heading1\">"),
            "{xml}"
        );
        assert!(xml.contains("<w:basedOn w:val=\"Normal\"/>"), "{xml}");
        assert!(xml.contains("<w:sz w:val=\"32\"/>"), "{xml}");
        assert!(report.losses().is_empty());
    }

    #[test]
    fn font_families_are_deduplicated_in_document_order() {
        let mut table = StyleTable::new();
        for (index, family) in ["Arial", "Times", "Arial"].iter().enumerate() {
            table.insert(Style {
                id: StyleId::new(format!("S{index}")),
                style_type: StyleType::Character,
                name: None,
                based_on: None,
                next: None,
                link: None,
                is_default: false,
                hidden: false,
                ui_priority: None,
                table: Default::default(),
                paragraph: Default::default(),
                run: RunProperties {
                    fonts: Some(strict_ooxml_wml::model::values::Fonts {
                        ascii: Some((*family).into()),
                        ..strict_ooxml_wml::model::values::Fonts::default()
                    }),
                    ..RunProperties::default()
                },
                based_on_chain: Vec::new(),
                location: strict_ooxml_core::error::SourceLocation::unknown(),
            });
        }
        assert_eq!(font_families(&table), vec!["Arial", "Times"]);
    }
}
