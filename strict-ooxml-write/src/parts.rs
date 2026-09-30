//! Serialization of the auxiliary parts: `styles.xml`, `numbering.xml`,
//! `settings.xml`, `fontTable.xml`, `theme1.xml`, `footnotes.xml`,
//! `endnotes.xml` and the header/footer parts.
//!
//! A part is written only when the model carries content for it, so a document
//! that had no `numbering.xml` does not acquire one — that is what keeps a
//! round trip from growing parts it did not have (SC-3).

use std::sync::Arc;

use strict_ooxml_wml::model::document::HeaderFooter;
use strict_ooxml_wml::model::inline::{Inline, RunContent};
use strict_ooxml_wml::model::notes::{Note, NoteKind, NoteTable};
use strict_ooxml_wml::model::numbering::{AbstractNum, Level, NumberingTable};
use strict_ooxml_wml::model::settings::Settings;
use strict_ooxml_wml::model::styles::{DocDefaults, Style, StyleTable};
use strict_ooxml_wml::model::theme::Theme;
use strict_ooxml_wml::model::values::StyleType;

use crate::body::blocks;
use crate::ctx::{Ctx, NoteRole};
use crate::props::{note_properties, paragraph_properties, run_properties, table_properties};
use crate::xml::{XmlWriter, NS_A, NS_R, NS_W, NS_W14};

/// The namespace declarations a `w:` part carries.
///
/// `w14` is here because the paragraph writer emits `w14:paraId`/`w14:textId`
/// (§17.3.1.26) on every paragraph that has them, and a header or footer is
/// made of paragraphs like any other. Without the declaration those parts were
/// not well-formed — an unbound prefix — so four documents in the local corpus
/// produced a header Word could not open.
const WML_NAMESPACES: [(&str, &str); 3] = [("w", NS_W), ("r", NS_R), ("w14", NS_W14)];

/// The namespace declarations the theme part carries.
const THEME_NAMESPACES: [(&str, &str); 1] = [("a", NS_A)];

/// Writes `w:styles`.
pub fn styles_part(ctx: &mut Ctx<'_>, table: &StyleTable) -> String {
    let mut xml = XmlWriter::new();
    xml.start_root("w:styles", &WML_NAMESPACES);
    if let Some(defaults) = table.defaults() {
        doc_defaults(ctx, &mut xml, defaults);
    }
    for style in table.iter() {
        style_element(ctx, &mut xml, style);
    }
    xml.end();
    xml.finish().expect("balanced")
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
    if style.is_default {
        xml.attr_w("default", "true");
    }
    if let Some(name) = &style.name {
        xml.empty_attr_w("w:name", "val", name.as_ref());
    }
    if let Some(based_on) = &style.based_on {
        xml.empty_attr_w("w:basedOn", "val", based_on.as_str());
    }
    if let Some(next) = &style.next {
        xml.empty_attr_w("w:next", "val", next.as_str());
    }
    if let Some(link) = &style.link {
        xml.empty_attr_w("w:link", "val", link.as_str());
    }
    if let Some(priority) = style.ui_priority {
        xml.empty_attr_w("w:uiPriority", "val", priority);
    }
    if style.hidden {
        xml.empty("w:semiHidden");
    }
    if style.style_type == StyleType::Table {
        table_properties(xml, &style.table);
    }
    paragraph_properties(ctx, xml, &style.paragraph);
    run_properties(xml, &style.run);
    let _ = ctx;
    xml.end();
}

/// Writes `w:numbering`.
pub fn numbering_part(ctx: &mut Ctx<'_>, table: &NumberingTable) -> String {
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
            xml.attr_w("ilvl", over.ilvl.0);
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
    xml.finish().expect("balanced")
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

fn level_element(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, level: &Level) {
    xml.start("w:lvl");
    xml.attr_w("ilvl", level.ilvl.0);
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
pub fn settings_part(ctx: &mut Ctx<'_>, settings: &Settings) -> String {
    let mut xml = XmlWriter::new();
    xml.start_root("w:settings", &WML_NAMESPACES);
    if let Some(zoom) = settings.zoom {
        xml.start("w:zoom");
        xml.attr_w("percent", zoom.percent.unwrap_or(100));
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
    if settings.even_and_odd_headers {
        xml.empty("w:evenAndOddHeaders");
    }
    if settings.display_background_shape {
        xml.empty("w:displayBackgroundShape");
    }
    if let Some(stop) = settings.default_tab_stop {
        xml.empty_attr_w("w:defaultTabStop", "val", stop.0);
    }
    if settings.auto_hyphenation {
        xml.empty("w:autoHyphenation");
    }
    if settings.do_not_hyphenate_caps {
        xml.empty("w:doNotHyphenateCaps");
    }
    if let Some(zone) = settings.hyphenation_zone {
        xml.empty_attr_w("w:hyphenationZone", "val", zone.0);
    }
    if let Some(edit) = &settings.document_protection {
        xml.empty_attr_w("w:documentProtection", "edit", edit.as_ref());
    }
    if settings.hide_spelling_errors {
        xml.empty("w:hideSpellingErrors");
    }
    if settings.hide_grammatical_errors {
        xml.empty("w:hideGrammaticalErrors");
    }
    if settings.proofing {
        xml.empty("w:proofState");
    }
    if settings.track_revisions {
        xml.empty("w:trackRevisions");
    }
    if !settings.footnote_properties.is_empty() {
        note_properties(&mut xml, "w:footnotePr", &settings.footnote_properties);
    }
    if !settings.endnote_properties.is_empty() {
        note_properties(&mut xml, "w:endnotePr", &settings.endnote_properties);
    }
    if let Some(language) = &settings.theme_font_lang {
        xml.empty_attr_w("w:themeFontLang", "val", language.as_ref());
    }
    if let Some(symbol) = &settings.decimal_symbol {
        xml.empty_attr_w("w:decimalSymbol", "val", symbol.as_ref());
    }
    if let Some(separator) = &settings.list_separator {
        xml.empty_attr_w("w:listSeparator", "val", separator.as_ref());
    }
    if settings.mirror_margins {
        xml.empty("w:mirrorMargins");
    }
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
    let _ = ctx;
    xml.end();
    xml.finish().expect("balanced")
}

/// Writes `w:fontTable` listing the faces the document references.
///
/// The model does not carry a font table, so the part is derived from the faces
/// the styles and runs actually name. A missing face is not a loss: Word
/// substitutes it, and the renderer's own font mapping already does the same.
pub fn font_table_part(ctx: &mut Ctx<'_>, families: &[String]) -> String {
    let mut xml = XmlWriter::new();
    xml.start_root("w:fonts", &WML_NAMESPACES);
    for family in families {
        xml.start("w:font");
        xml.attr_w("name", family);
        xml.empty("w:charset");
        xml.empty("w:family");
        xml.empty("w:pitch");
        xml.end();
    }
    xml.end();
    let _ = ctx;
    xml.finish().expect("balanced")
}

/// Writes `a:theme`.
///
/// The model keeps only the font sets and the colour scheme, which is what the
/// cascade needs, so the theme part is written as a minimal but schema-shaped
/// document: a missing element format would make Word report the file as
/// unreadable content.
pub fn theme_part(theme: &Theme) -> String {
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
        font_collection(&mut xml, "a:latin", set.latin.as_ref());
        font_collection(&mut xml, "a:ea", set.east_asia.as_ref());
        font_collection(&mut xml, "a:cs", set.cs.as_ref());
        xml.end();
    }
    xml.end();
    xml.start("a:fmtScheme");
    xml.attr("name", "strict-ooxml");
    // The format scheme is required by the schema but contributes nothing the
    // cascade reads; a minimal fill/line/effect/font set is written so the part
    // validates.
    for name in [
        "a:fillStyleLst",
        "a:lnStyleLst",
        "a:effectStyleLst",
        "a:bgFillStyleLst",
    ] {
        xml.start(name);
        xml.end();
    }
    xml.end();
    xml.end();
    xml.end();
    xml.finish().expect("balanced")
}

fn font_collection(xml: &mut XmlWriter, name: &str, typeface: Option<&Arc<str>>) {
    if let Some(typeface) = typeface {
        xml.start(name);
        xml.attr("typeface", typeface.as_ref());
        xml.end();
    }
}

/// Writes `w:footnotes` or `w:endnotes`.
pub fn notes_part(ctx: &mut Ctx<'_>, table: &NoteTable, is_footnote: bool) -> String {
    let mut xml = XmlWriter::new();
    xml.start_root(
        if is_footnote {
            "w:footnotes"
        } else {
            "w:endnotes"
        },
        &WML_NAMESPACES,
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
    xml.finish().expect("balanced")
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
pub fn header_footer_part(ctx: &mut Ctx<'_>, header_footer: &HeaderFooter) -> String {
    let mut xml = XmlWriter::new();
    xml.start_root(
        if header_footer.is_header {
            "w:hdr"
        } else {
            "w:ftr"
        },
        &WML_NAMESPACES,
    );
    blocks(ctx, &mut xml, &header_footer.blocks);
    if header_footer.blocks.is_empty() {
        xml.start("w:p");
        xml.end();
    }
    xml.end();
    xml.finish().expect("balanced")
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
        let xml = styles_part(&mut ctx, &table);
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
