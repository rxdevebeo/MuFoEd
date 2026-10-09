//! Serialization of the auxiliary parts: `styles.xml`, `numbering.xml`,
//! `settings.xml`, `fontTable.xml`, `theme1.xml`, `footnotes.xml`,
//! `endnotes.xml` and the header/footer parts.
//!
//! A part is written only when the model carries content for it, so a document
//! that had no `numbering.xml` does not acquire one — that is what keeps a
//! round trip from growing parts it did not have (SC-3).

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_core::part::PartId;
use strict_ooxml_wml::model::block::{Block, Paragraph};
use strict_ooxml_wml::model::document::HeaderFooter;
use strict_ooxml_wml::model::ids::Ilvl;
use strict_ooxml_wml::model::inline::{Inline, Run, RunContent};
use strict_ooxml_wml::model::notes::{Note, NoteKind, NoteTable};
use strict_ooxml_wml::model::numbering::{AbstractNum, Level, NumberingTable};
use strict_ooxml_wml::model::settings::{MathProperties, Settings};
use strict_ooxml_wml::model::styles::{DocDefaults, Style, StyleTable};
use strict_ooxml_wml::model::theme::{Theme, ThemeRunFonts, ThemeTypeface};
use strict_ooxml_wml::model::values::StyleType;

use crate::body::blocks;
use crate::ctx::{Ctx, NoteRole};
use crate::props::{
    cell_properties, note_properties, paragraph_properties, row_properties, run_properties,
    table_properties,
};
use crate::xml::{WriteError, XmlWriter, NS_A, NS_M, NS_PIC, NS_R, NS_W, NS_WP};
use strict_ooxml_wml::model::fonts::{EmbedKind, FontEntry, FontTable};
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
    if let Some(defaults) = table.declared_defaults() {
        doc_defaults(ctx, &mut xml, defaults);
    }
    for style in table.iter() {
        style_element(ctx, &mut xml, style);
    }
    xml.end();
    ctx.finish_xml(xml)
}

fn doc_defaults(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, defaults: &DocDefaults) {
    xml.start("w:docDefaults");
    xml.start("w:rPrDefault");
    run_properties(xml, &defaults.run);
    xml.end();
    xml.start("w:pPrDefault");
    paragraph_properties(ctx, xml, &defaults.paragraph, None);
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
    if style.custom_style {
        xml.attr_w("customStyle", "1");
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
            "autoRedefine" if style.auto_redefine => xml.empty("w:autoRedefine"),
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
            "semiHidden" if style.semi_hidden => xml.empty("w:semiHidden"),
            "hidden" if style.hidden => xml.empty("w:hidden"),
            "qFormat" if style.q_format => xml.empty("w:qFormat"),
            "locked" if style.locked => xml.empty("w:locked"),
            "unhideWhenUsed" if style.unhide_when_used => xml.empty("w:unhideWhenUsed"),
            "pPr" => paragraph_properties(ctx, xml, &style.paragraph, None),
            "rPr" => run_properties(xml, &style.run),
            "tblPr" if style.style_type == StyleType::Table => {
                table_properties(xml, &style.table);
            }
            "trPr" if style.style_type == StyleType::Table => {
                row_properties(xml, &style.row);
            }
            "tcPr" if style.style_type == StyleType::Table => {
                cell_properties(xml, &style.cell);
            }
            "tblStylePr" => {
                for condition in &style.conditions {
                    xml.start("w:tblStylePr");
                    xml.attr_w("type", condition.kind.as_ref());
                    for child in crate::order::TBLSTYLEPR {
                        match *child {
                            "pPr" => paragraph_properties(ctx, xml, &condition.paragraph, None),
                            "rPr" => run_properties(xml, &condition.run),
                            "tblPr" => table_properties(xml, &condition.table),
                            "trPr" => row_properties(xml, &condition.row),
                            "tcPr" => cell_properties(xml, &condition.cell),
                            _ => {}
                        }
                    }
                    xml.end();
                }
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
    ctx.finish_xml(xml)
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
    // `CT_Lvl` order is [`crate::order::LVL`]: start, numFmt, lvlRestart,
    // pStyle, isLgl, suff, lvlText, lvlPicBulletId, lvlJc, pPr, rPr.
    xml.start("w:lvl");
    ilvl_attr(ctx, xml, &level.ilvl, "w:lvl");
    // Strict `CT_Lvl` carries `tentative` as an attribute, not a child.
    if level.tentative {
        xml.attr_w("tentative", "1");
    } else if level.tentative_off {
        xml.attr_w("tentative", "0");
    }
    if let Some(start) = level.start {
        xml.empty_attr_w("w:start", "val", start);
    }
    if let Some(format) = &level.format {
        xml.empty_attr_w("w:numFmt", "val", format.as_ref());
    }
    if let Some(restart) = level.restart {
        xml.empty_attr_w("w:lvlRestart", "val", restart);
    }
    if let Some(style) = &level.paragraph_style {
        xml.empty_attr_w("w:pStyle", "val", style.as_str());
    }
    if level.is_legal {
        xml.empty("w:isLgl");
    }
    if let Some(suffix) = &level.suffix {
        match suffix.as_ref() {
            "tab" | "space" | "nothing" => {
                xml.empty_attr_w("w:suff", "val", suffix.as_ref());
            }
            other => ctx.report_unsupported(
                "w:suff",
                &format!("suffix '{other}' is not tab, space, or nothing"),
                &strict_ooxml_core::error::SourceLocation::unknown(),
            ),
        }
    }
    if let Some(text) = &level.text {
        xml.empty_attr_w("w:lvlText", "val", text.as_ref());
    }
    if let Some(justification) = &level.justification {
        xml.empty_attr_w("w:lvlJc", "val", justification.as_str());
    }
    paragraph_properties(ctx, xml, &level.paragraph, None);
    run_properties(xml, &level.run);
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
    ctx.finish_xml(xml)
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
fn math_properties(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, properties: &MathProperties) {
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
            if name == "m:smallFrac" {
                match strict_on_off(value.as_ref()) {
                    Some(word) => xml.empty_attr(name, "m:val", word),
                    None => ctx.report_unsupported(
                        "m:smallFrac",
                        &format!(
                            "value '{value}' is not an on/off token (on, off, 1, 0, true, false)"
                        ),
                        &strict_ooxml_core::error::SourceLocation::unknown(),
                    ),
                }
            } else {
                xml.empty_attr(name, "m:val", value.as_ref());
            }
        }
    }
    xml.end();
}

/// Maps an OMML on/off token onto the Strict boolean spelling.
fn strict_on_off(value: &str) -> Option<&'static str> {
    match value.trim() {
        "on" | "1" | "true" => Some("true"),
        "off" | "0" | "false" => Some("false"),
        _ => None,
    }
}

/// Legacy `w:stylePaneFormatFilter/@w:val` bits, in declaration order.
///
/// `0x0010` is reserved. A value that sets it, or that is not hexadecimal, is
/// refused instead of being copied into Strict XML.
const STYLE_PANE_FLAGS: &[(u16, &str)] = &[
    (0x0001, "allStyles"),
    (0x0002, "customStyles"),
    (0x0004, "latentStyles"),
    (0x0008, "stylesInUse"),
    (0x0020, "headingStyles"),
    (0x0040, "numberingStyles"),
    (0x0080, "tableStyles"),
    (0x0100, "directFormattingOnRuns"),
    (0x0200, "directFormattingOnParagraphs"),
    (0x0400, "directFormattingOnNumbering"),
    (0x0800, "directFormattingOnTables"),
    (0x1000, "clearFormatting"),
    (0x2000, "top3HeadingStyles"),
    (0x4000, "visibleStyles"),
    (0x8000, "alternateStyleNames"),
];

fn write_style_pane_filter(
    ctx: &mut Ctx<'_>,
    xml: &mut XmlWriter,
    pairs: &[(std::sync::Arc<str>, std::sync::Arc<str>)],
) {
    if pairs.is_empty() {
        return;
    }
    let named: Vec<_> = pairs
        .iter()
        .filter(|(name, _)| name.as_ref() != "val")
        .collect();
    if !named.is_empty() {
        xml.start("w:stylePaneFormatFilter");
        for (attribute, value) in named {
            xml.attr_w(attribute.as_ref(), value.as_ref());
        }
        xml.end();
        return;
    }
    let Some((_, value)) = pairs.iter().find(|(name, _)| name.as_ref() == "val") else {
        return;
    };
    match decode_style_pane(value.as_ref()) {
        Ok(bits) => {
            xml.start("w:stylePaneFormatFilter");
            for (name, enabled) in bits {
                if enabled {
                    xml.attr_w(name, "true");
                }
            }
            xml.end();
        }
        Err(reason) => ctx.report_unsupported(
            "w:stylePaneFormatFilter",
            reason,
            &strict_ooxml_core::error::SourceLocation::unknown(),
        ),
    }
}

fn decode_style_pane(value: &str) -> Result<Vec<(&'static str, bool)>, &'static str> {
    let bits = u16::from_str_radix(value.trim(), 16)
        .map_err(|_| "style pane filter is not hexadecimal")?;
    if bits & 0x0010 != 0 {
        return Err("style pane filter sets reserved bit 0x0010");
    }
    Ok(STYLE_PANE_FLAGS
        .iter()
        .map(|(mask, name)| (*name, bits & mask != 0))
        .collect())
}

/// Maps a `w:documentProtection` attribute set onto the attributes Strict's
/// `CT_DocProtect` has, dropping the ones it does not.
///
/// `@w:edit` is handled by the caller; everything else arrives here as
/// whatever the source wrote. A Strict source already used Strict's own
/// names (`enforcement`, `formatting`, `spinCount`, `hashValue`, `saltValue`,
/// `algorithmName`) and those pass through unchanged. A Transitional source
/// used the crypto group instead: `cryptSpinCount`, `hash` and `salt` rename
/// 1:1 to their Strict counterparts, and `cryptAlgorithmSid` maps through
/// [`hash_algorithm_name`] to `algorithmName` when the SID names one of the
/// hash algorithms ECMA-376 lists - an unrecognised SID has no Strict
/// spelling and is skipped rather than guessed. The rest of the crypto group
/// (`cryptProviderType`, `cryptAlgorithmClass`, `cryptAlgorithmType`,
/// `cryptProvider`, `algIdExt`, `algIdExtSource`, `cryptProviderTypeExt`,
/// `cryptProviderTypeExtSource`) names a crypto provider `CT_DocProtect` has
/// no attribute for at all in Strict, and writing any of them back would make
/// the element invalid - they are dropped.
fn strict_document_protection_attrs(
    pairs: &[(std::sync::Arc<str>, std::sync::Arc<str>)],
) -> Vec<(&'static str, std::sync::Arc<str>)> {
    let mut enforcement = None;
    let mut formatting = None;
    let mut spin_count = None;
    let mut hash_value = None;
    let mut salt_value = None;
    let mut algorithm_name = None;
    let mut sid = None;
    for (name, value) in pairs {
        match name.as_ref() {
            "enforcement" => enforcement = Some(value.clone()),
            "formatting" => formatting = Some(value.clone()),
            "spinCount" => spin_count = Some(value.clone()),
            "cryptSpinCount" => {
                spin_count.get_or_insert_with(|| value.clone());
            }
            "hashValue" => hash_value = Some(value.clone()),
            "hash" => {
                hash_value.get_or_insert_with(|| value.clone());
            }
            "saltValue" => salt_value = Some(value.clone()),
            "salt" => {
                salt_value.get_or_insert_with(|| value.clone());
            }
            "algorithmName" => algorithm_name = Some(value.clone()),
            "cryptAlgorithmSid" => sid = Some(value.clone()),
            // Transitional's remaining crypto-provider attributes have no
            // Strict home at all (see above) and are dropped silently here;
            // nothing in `CT_DocProtect` could ever be named for them.
            _ => {}
        }
    }
    if algorithm_name.is_none() {
        if let Some(name) = sid
            .as_deref()
            .and_then(|value| value.trim().parse::<u32>().ok())
            .and_then(hash_algorithm_name)
        {
            algorithm_name = Some(std::sync::Arc::from(name));
        }
    }
    let mut out = Vec::new();
    if let Some(value) = enforcement {
        out.push(("enforcement", value));
    }
    if let Some(value) = formatting {
        out.push(("formatting", value));
    }
    if let Some(value) = spin_count {
        out.push(("spinCount", value));
    }
    if let Some(value) = hash_value {
        out.push(("hashValue", value));
    }
    if let Some(value) = salt_value {
        out.push(("saltValue", value));
    }
    if let Some(value) = algorithm_name {
        out.push(("algorithmName", value));
    }
    out
}

/// Transitional protection attributes that `CT_DocProtect` cannot carry.
///
/// Dropping them keeps the element schema-valid. Each one is still a named
/// loss: a clean report would claim the parameter survived.
const UNMAPPED_PROTECTION_ATTRS: &[&str] = &[
    "cryptProviderType",
    "cryptAlgorithmClass",
    "cryptAlgorithmType",
    "cryptProvider",
    "algIdExt",
    "algIdExtSource",
    "cryptProviderTypeExt",
    "cryptProviderTypeExtSource",
];

fn report_unmapped_protection(
    ctx: &mut Ctx<'_>,
    pairs: &[(std::sync::Arc<str>, std::sync::Arc<str>)],
) {
    let mut location = SourceLocation::unknown();
    location.part = PartId::new(crate::package::SETTINGS_PART);
    for (name, value) in pairs {
        if UNMAPPED_PROTECTION_ATTRS.contains(&name.as_ref()) {
            ctx.report_lossy(
                &format!("w:documentProtection/@w:{name}"),
                &format!(
                    "w:{name} has no CT_DocProtect attribute; the value {value:?} is dropped \
                     and password-provider identity is not preserved"
                ),
                &location,
            );
        }
        if name.as_ref() == "cryptAlgorithmSid"
            && value
                .trim()
                .parse::<u32>()
                .ok()
                .and_then(hash_algorithm_name)
                .is_none()
        {
            ctx.report_lossy(
                "w:documentProtection/@w:cryptAlgorithmSid",
                &format!(
                    "cryptAlgorithmSid {value:?} is not a supported hash algorithm; \
                     algorithmName is omitted and password verification is not preserved"
                ),
                &location,
            );
        }
    }
}

/// `w:cryptAlgorithmSid`'s hash-algorithm SIDs, mapped to the lexical value
/// Strict's `w:algorithmName` takes for the same algorithm.
///
/// The common OOXML SID table names more algorithms than these seven, but
/// these are the ones `w:documentProtection`'s hash actually uses in
/// practice; an unlisted SID is not guessed at and the caller skips
/// `algorithmName` entirely rather than writing a name that does not match.
fn hash_algorithm_name(sid: u32) -> Option<&'static str> {
    match sid {
        1 => Some("MD2"),
        2 => Some("MD4"),
        3 => Some("MD5"),
        4 => Some("SHA-1"),
        12 => Some("SHA-256"),
        13 => Some("SHA-384"),
        14 => Some("SHA-512"),
        _ => None,
    }
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
        "proofState" => {
            if let Some(state) = &settings.proof_state {
                xml.start("w:proofState");
                if let Some(spelling) = state.spelling {
                    xml.attr_w("spelling", spelling.as_str());
                }
                if let Some(grammar) = state.grammar {
                    xml.attr_w("grammar", grammar.as_str());
                }
                xml.end();
            }
        }
        "trackRevisions" if settings.track_revisions => xml.empty("w:trackRevisions"),
        "documentProtection" => {
            // `@w:edit` is the mode and the corpus usually writes `@w:enforcement`
            // instead, so writing only `edit` produced NO element for ten corpus
            // documents rather than a partial one. The remaining attributes go
            // through [`strict_document_protection_attrs`], which keeps only the
            // ones `CT_DocProtect` has in Strict and drops Transitional's crypto
            // group - writing them back made the element invalid outright. An
            // element with nothing left to say is not written, but `edit` or
            // `enforcement` alone is still reason enough to keep it.
            report_unmapped_protection(ctx, &settings.document_protection_attributes);
            let strict_attrs =
                strict_document_protection_attrs(&settings.document_protection_attributes);
            let has_edit = settings.document_protection.is_some();
            if has_edit || !strict_attrs.is_empty() {
                xml.start("w:documentProtection");
                if let Some(edit) = &settings.document_protection {
                    xml.attr_w("edit", edit.as_ref());
                }
                for (attribute, value) in &strict_attrs {
                    xml.attr_w(attribute, value.as_ref());
                }
                xml.end();
            }
        }
        "defaultTabStop" => {
            if let Some(stop) = settings.default_tab_stop {
                xml.empty_attr_w("w:defaultTabStop", "val", stop.0);
            }
        }
        "autoHyphenation" if settings.auto_hyphenation => {
            xml.empty_attr_w("w:autoHyphenation", "val", "true");
        }
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
        "evenAndOddHeaders" if settings.even_and_odd_headers_off => {
            xml.empty_attr_w("w:evenAndOddHeaders", "val", "false");
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
            // `CT_Compat`'s sequence puts the seven on/off flags FIRST and
            // `w:compatSetting` LAST, so the two are written in that order rather
            // than in the order they were read. The corpus writes them
            // alphabetically, which is not this order, so a round trip through a
            // producer that sorted them would otherwise move four of the seven.
            let flags = settings.compat_flags.set();
            if settings.compat_present || !flags.is_empty() || !settings.compatibility.is_empty() {
                xml.start("w:compat");
                for flag in flags {
                    xml.empty(flag);
                }
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
        "rsids" => {
            if let Some(ids) = &settings.revision_save_ids {
                xml.start("w:rsids");
                // `CT_DocRsids` is one root then an unbounded list, so the root
                // goes first whatever order the input had them in.
                if let Some(root) = &ids.root {
                    xml.empty_attr_w("w:rsidRoot", "val", root.as_ref());
                }
                for entry in &ids.entries {
                    xml.empty_attr_w("w:rsid", "val", entry.as_ref());
                }
                xml.end();
            }
        }
        "characterSpacingControl" => {
            if let Some(value) = &settings.character_spacing_control {
                xml.empty_attr_w("w:characterSpacingControl", "val", value.as_ref());
            }
        }
        "view" => {
            if let Some(value) = &settings.view {
                xml.empty_attr_w("w:view", "val", value.as_ref());
            }
        }
        "docVars" => {
            if !settings.document_variables.is_empty() {
                xml.start("w:docVars");
                for (name, value) in &settings.document_variables {
                    xml.start("w:docVar");
                    xml.attr_w("name", name.as_ref());
                    xml.attr_w("val", value.as_ref());
                    xml.end();
                }
                xml.end();
            }
        }
        "noLineBreaksAfter" | "noLineBreaksBefore" => {
            let pairs = if name == "noLineBreaksAfter" {
                &settings.no_line_breaks_after
            } else {
                &settings.no_line_breaks_before
            };
            for (language, characters) in pairs {
                let element = if name == "noLineBreaksAfter" {
                    "w:noLineBreaksAfter"
                } else {
                    "w:noLineBreaksBefore"
                };
                xml.start(element);
                xml.attr_w("lang", language.as_ref());
                xml.attr_w("val", characters.as_ref());
                xml.end();
            }
        }
        "attachedTemplate" => {
            // AUD-61: `r:id` must resolve through `settings.xml.rels`, not the
            // document part. The package writer binds the source id into this
            // part's allocator; without a binding the element is omitted.
            if let Some(id) = &settings.attached_template {
                if let Some(remapped) = ctx.foreign_rel(id.as_ref()) {
                    xml.start("w:attachedTemplate");
                    xml.attr("r:id", remapped);
                    xml.end();
                }
            }
        }
        "stylePaneFormatFilter" => {
            write_style_pane_filter(ctx, xml, &settings.style_pane_filter);
        }
        "revisionView" => {
            if !settings.revision_view.is_empty() {
                xml.start("w:revisionView");
                for (attribute, value) in &settings.revision_view {
                    xml.attr_w(attribute, value.as_ref());
                }
                xml.end();
            }
        }
        "clrSchemeMapping" => {
            // `CT_ColorSchemeMapping` has no `xsd:sequence`, so attribute order is
            // free - it is written in declaration order so two settings parts diff
            // legibly. Every attribute is optional and only the ones the producer
            // set are written: emitting all twelve defaults would claim a mapping
            // the document never made.
            if let Some(mapping) = &settings.color_scheme_mapping {
                xml.start("w:clrSchemeMapping");
                for (name, value) in mapping.slots() {
                    if let Some(value) = value {
                        xml.attr_w(name.trim_start_matches("w:"), value.as_ref());
                    }
                }
                xml.end();
            }
        }
        "mathPr" => {
            if let Some(mathematics) = &settings.math_properties {
                math_properties(ctx, xml, mathematics);
            }
        }
        "themeFontLang" => {
            if let Some(language) = &settings.theme_font_lang {
                if !language.is_empty() {
                    xml.start("w:themeFontLang");
                    xml.attr_w_opt("val", language.val.as_deref());
                    xml.attr_w_opt("eastAsia", language.east_asia.as_deref());
                    xml.attr_w_opt("bidi", language.bidi.as_deref());
                    xml.end();
                }
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
        // The `CT_OnOff` and numeric children are name-keyed maps in the model, so
        // the DATA says whether this child is one of them and a second list of names
        // here would be a second place to forget to update. A name in neither map
        // writes nothing, which is what "not read" has always meant. This arm is
        // last so it cannot shadow a named one.
        name => {
            if let Some((_, value)) = settings
                .on_off_flags
                .iter()
                .find(|(flag, _)| flag.as_ref() == name)
            {
                xml.start(&format!("w:{name}"));
                if let Some(value) = value {
                    xml.attr_w("val", value.as_ref());
                }
                xml.end();
            } else if let Some((_, value)) = settings
                .numeric_settings
                .iter()
                .find(|(setting, _)| setting.as_ref() == name)
            {
                xml.empty_attr_w(&format!("w:{name}"), "val", value.as_ref());
            }
        }
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
/// Children follow `CT_Font`'s sequence: `altName`…`family`, `notTrueType`,
/// `pitch`, `sig`, then embeds. Hint elements are written only with their required
/// attributes — never as empty tags (`XS-01`). `w:charset` uses Strict
/// `@w:characterSet`. `ctx.font_rel` gives the id **this part's own** `.rels`
/// will carry.
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
        write_font_entry(ctx, &mut xml, entry);
    }
    for family in families {
        if seen.contains(&family.as_str()) {
            continue;
        }
        xml.empty_attr_w("w:font", "name", family);
    }
    xml.end();
    let _ = ctx;
    ctx.finish_xml(xml)
}

/// One `w:font` with hints and embeds in schema order.
fn write_font_entry(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, entry: &FontEntry) {
    xml.start("w:font");
    xml.attr_w("name", entry.name.as_ref());
    let hints = &entry.hints;
    if let Some(alt) = hints.alt_name.as_deref() {
        xml.empty_attr_w("w:altName", "val", alt);
    }
    if let Some(panose) = hints.panose1.as_deref() {
        xml.empty_attr_w("w:panose1", "val", panose);
    }
    if let Some(charset) = hints.charset.as_deref() {
        // Strict `CT_Charset` names the attribute `characterSet`, not `val`.
        xml.empty_attr_w("w:charset", "characterSet", charset);
    }
    if let Some(family) = hints.family.as_deref() {
        xml.empty_attr_w("w:family", "val", family);
    }
    if hints.not_true_type {
        xml.empty_attr_w("w:notTrueType", "val", "true");
    }
    if let Some(pitch) = hints.pitch.as_deref() {
        xml.empty_attr_w("w:pitch", "val", pitch);
    }
    if let Some(sig) = &hints.sig {
        xml.start("w:sig");
        xml.attr_w_opt("usb0", sig.usb0.as_deref());
        xml.attr_w_opt("usb1", sig.usb1.as_deref());
        xml.attr_w_opt("usb2", sig.usb2.as_deref());
        xml.attr_w_opt("usb3", sig.usb3.as_deref());
        xml.attr_w_opt("csb0", sig.csb0.as_deref());
        xml.attr_w_opt("csb1", sig.csb1.as_deref());
        xml.end();
    }
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
    ctx.report_partial(
        "a:theme@name",
        "theme name is rewritten to strict-ooxml",
        &theme.location,
    );
    xml.attr("name", "strict-ooxml");
    xml.start("a:themeElements");
    xml.start("a:clrScheme");
    ctx.report_partial(
        "a:clrScheme@name",
        "color scheme name is rewritten to strict-ooxml",
        &theme.location,
    );
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
    ctx.report_partial(
        "a:fontScheme@name",
        "font scheme name is rewritten to strict-ooxml",
        &theme.location,
    );
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
        font_collection(&mut xml, "a:latin", &set.latin);
        font_collection(&mut xml, "a:ea", &set.east_asia);
        font_collection(&mut xml, "a:cs", &set.cs);
        xml.end();
    }
    xml.end();
    format_scheme(&mut xml);
    xml.end();
    if let Some(markup) = &theme.object_defaults_xml {
        // The parsed element, including `a:lnDef`, list styles and `a:sym`.
        // Rewriting only the `defRPr` faces would drop the rest of the subtree.
        xml.raw_markup(markup, &["a"]);
    } else {
        let shape = theme
            .shape_defaults
            .as_ref()
            .filter(|fonts| !fonts.is_empty());
        let text = theme
            .text_defaults
            .as_ref()
            .filter(|fonts| !fonts.is_empty());
        if shape.is_some() || text.is_some() {
            // `CT_DefaultShapeDefinition` requires `spPr`, `bodyPr` and `lstStyle`.
            // A hand-built theme has no source element to copy, so the empty
            // properties are the smallest schema-valid shell around the faces.
            xml.start("a:objectDefaults");
            if let Some(fonts) = shape {
                write_object_default(&mut xml, "a:spDef", fonts);
            }
            if let Some(fonts) = text {
                write_object_default(&mut xml, "a:txDef", fonts);
            }
            xml.end();
        }
    }
    xml.end();
    ctx.finish_xml(xml)
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

fn font_collection(xml: &mut XmlWriter, name: &str, face: &ThemeTypeface) {
    xml.start(name);
    write_text_font_attrs(xml, face);
    xml.end();
}

fn write_text_font_attrs(xml: &mut XmlWriter, face: &ThemeTypeface) {
    xml.attr("typeface", face.name.as_deref().unwrap_or(""));
    xml.attr_opt("panose", face.panose.as_deref());
    xml.attr_opt("pitchFamily", face.pitch_family.as_deref());
    xml.attr_opt("charset", face.charset.as_deref());
}

fn write_object_default(xml: &mut XmlWriter, name: &str, fonts: &ThemeRunFonts) {
    xml.start(name);
    xml.empty("a:spPr");
    xml.empty("a:bodyPr");
    xml.start("a:lstStyle");
    xml.start("a:defPPr");
    xml.start("a:defRPr");
    for (element, face) in [
        ("a:latin", &fonts.latin),
        ("a:ea", &fonts.east_asia),
        ("a:cs", &fonts.cs),
    ] {
        if face.is_empty() {
            continue;
        }
        xml.start(element);
        write_text_font_attrs(xml, face);
        xml.end();
    }
    xml.end();
    xml.end();
    xml.end();
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
    ctx.finish_xml(xml)
}

/// Whether a note's leading paragraph already carries the reference marker.
///
/// Word's separator notes put a run holding `w:footnoteRef`/`w:endnoteRef` at
/// the start of their first paragraph, and the parser records it as
/// [`RunContent::NoteRef`]. Emitting it only when the model does not already
/// carry it keeps a note Word wrote byte-identical and still gives a hand-built
/// note the marker it needs (AUD-60).
fn leading_paragraph_has_reference(note: &Note) -> bool {
    let Some(Block::Paragraph(paragraph)) = note.blocks.first() else {
        return false;
    };
    paragraph_has_reference(paragraph)
}

fn paragraph_has_reference(paragraph: &Paragraph) -> bool {
    paragraph.inlines.iter().any(
        |inline| matches!(inline, Inline::Run(run) if run.content.contains(&RunContent::NoteRef)),
    )
}

/// Ensures the first run of `paragraph` carries [`RunContent::NoteRef`].
///
/// Returns a clone only when the marker must be inserted; otherwise borrows.
fn paragraph_with_note_ref(paragraph: &Paragraph) -> Paragraph {
    if paragraph_has_reference(paragraph) {
        return paragraph.clone();
    }
    let mut out = paragraph.clone();
    match out.inlines.first_mut() {
        Some(Inline::Run(run)) => {
            run.content.insert(0, RunContent::NoteRef);
        }
        _ => {
            out.inlines.insert(
                0,
                Inline::Run(Run {
                    props: Default::default(),
                    content: vec![RunContent::NoteRef],
                    revision: None,
                    location: strict_ooxml_core::error::SourceLocation::unknown(),
                }),
            );
        }
    }
    out
}

fn note_element(ctx: &mut Ctx<'_>, xml: &mut XmlWriter, note: &Note, is_footnote: bool) {
    let element = if is_footnote {
        "w:footnote"
    } else {
        "w:endnote"
    };
    xml.start(element);
    xml.attr_w("type", note.kind.as_str());
    xml.attr_w("id", note.id);

    // AUD-60: each block is its own child of the note. The reference marker
    // lives in the first run of the first paragraph; if the note starts with a
    // table (or is empty), a paragraph holding only the marker is inserted.
    let needs_ref = !leading_paragraph_has_reference(note);
    let first_is_paragraph = matches!(note.blocks.first(), Some(Block::Paragraph(_)));
    if needs_ref && !first_is_paragraph {
        let marker = Paragraph {
            props: Default::default(),
            inlines: vec![Inline::Run(Run {
                props: Default::default(),
                content: vec![RunContent::NoteRef],
                revision: None,
                location: strict_ooxml_core::error::SourceLocation::unknown(),
            })],
            rsids: Default::default(),
            revision: None,
            para_id: None,
            text_id: None,
            location: strict_ooxml_core::error::SourceLocation::unknown(),
        };
        crate::body::paragraph_element(ctx, xml, &marker);
    }
    for (index, block) in note.blocks.iter().enumerate() {
        match block {
            Block::Paragraph(paragraph) if index == 0 && needs_ref => {
                let with_ref = paragraph_with_note_ref(paragraph);
                crate::body::paragraph_element(ctx, xml, &with_ref);
            }
            other => crate::body::block_item(ctx, xml, other),
        }
    }
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
                        revision: None,
                        location: strict_ooxml_core::error::SourceLocation::unknown(),
                    },
                )],
                rsids: Default::default(),
                para_id: None,
                text_id: None,
                revision: None,
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
    ctx.finish_xml(xml)
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
            // Empty slots (`w:cs=""`, `w:eastAsia=""`) are not font names: the
            // writer would emit `<w:font w:name=""/>`, the parser drops it, and
            // the next write appends `""` again — not a fixed point
            // (`doc-with-toc.docx`).
            if family.is_empty() {
                return;
            }
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
    use strict_ooxml_core::normalize::report::{NormalizationReport, Severity};
    use strict_ooxml_wml::model::ids::StyleId;
    use strict_ooxml_wml::model::props::RunProperties;
    use strict_ooxml_wml::model::settings::Settings;
    use strict_ooxml_wml::model::styles::{Style, StyleTable};
    use strict_ooxml_wml::model::values::{StyleType, TriState};

    use super::{font_families, settings_part, strict_document_protection_attrs, styles_part, Ctx};

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
            custom_style: false,
            auto_redefine: false,
            semi_hidden: false,
            hidden: false,
            q_format: false,
            locked: false,
            unhide_when_used: false,
            ui_priority: Some(9),
            table: strict_ooxml_wml::model::props::TableProperties::default(),
            row: strict_ooxml_wml::model::props::RowProperties::default(),
            cell: strict_ooxml_wml::model::props::CellProperties::default(),
            paragraph: Default::default(),
            run: RunProperties {
                bold: TriState::On,
                size: Some(strict_ooxml_wml::model::values::HalfPoints(32)),
                ..RunProperties::default()
            },
            conditions: Vec::new(),
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
                custom_style: false,
                auto_redefine: false,
                semi_hidden: false,
                hidden: false,
                q_format: false,
                locked: false,
                unhide_when_used: false,
                ui_priority: None,
                table: strict_ooxml_wml::model::props::TableProperties::default(),
                row: strict_ooxml_wml::model::props::RowProperties::default(),
                cell: strict_ooxml_wml::model::props::CellProperties::default(),
                paragraph: Default::default(),
                run: RunProperties {
                    fonts: Some(strict_ooxml_wml::model::values::Fonts {
                        ascii: Some((*family).into()),
                        ..strict_ooxml_wml::model::values::Fonts::default()
                    }),
                    ..RunProperties::default()
                },
                conditions: Vec::new(),
                based_on_chain: Vec::new(),
                location: strict_ooxml_core::error::SourceLocation::unknown(),
            });
        }
        assert_eq!(font_families(&table), vec!["Arial", "Times"]);
    }

    #[test]
    fn font_families_skip_empty_style_slots() {
        let mut table = StyleTable::new();
        table.insert(Style {
            id: StyleId::new("Normal"),
            style_type: StyleType::Paragraph,
            name: None,
            based_on: None,
            next: None,
            link: None,
            is_default: true,
            custom_style: false,
            auto_redefine: false,
            semi_hidden: false,
            hidden: false,
            q_format: false,
            locked: false,
            unhide_when_used: false,
            ui_priority: None,
            table: strict_ooxml_wml::model::props::TableProperties::default(),
            row: strict_ooxml_wml::model::props::RowProperties::default(),
            cell: strict_ooxml_wml::model::props::CellProperties::default(),
            paragraph: Default::default(),
            run: RunProperties {
                fonts: Some(strict_ooxml_wml::model::values::Fonts {
                    ascii: Some("Liberation Sans".into()),
                    complex_script: Some("".into()),
                    east_asia: Some("".into()),
                    ..strict_ooxml_wml::model::values::Fonts::default()
                }),
                ..RunProperties::default()
            },
            conditions: Vec::new(),
            based_on_chain: Vec::new(),
            location: strict_ooxml_core::error::SourceLocation::unknown(),
        });
        assert_eq!(font_families(&table), vec!["Liberation Sans"]);
    }

    /// Transitional's crypto-group attribute names rename onto Strict's
    /// `CT_DocProtect` attributes; the SID maps through the hash-algorithm
    /// table, and the names Strict has no attribute for at all are dropped.
    #[test]
    fn document_protection_maps_transitional_crypto_attrs() {
        let pairs: Vec<(std::sync::Arc<str>, std::sync::Arc<str>)> = vec![
            ("enforcement".into(), "1".into()),
            ("cryptProviderType".into(), "rsaFull".into()),
            ("cryptAlgorithmClass".into(), "hash".into()),
            ("cryptAlgorithmType".into(), "typeAny".into()),
            ("cryptAlgorithmSid".into(), "4".into()),
            ("cryptSpinCount".into(), "100000".into()),
            ("hash".into(), "abcd".into()),
            ("salt".into(), "ef01".into()),
        ];
        let attrs = strict_document_protection_attrs(&pairs);
        let find = |name: &str| {
            attrs
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| v.as_ref())
        };
        assert_eq!(find("enforcement"), Some("1"));
        assert_eq!(find("spinCount"), Some("100000"));
        assert_eq!(find("hashValue"), Some("abcd"));
        assert_eq!(find("saltValue"), Some("ef01"));
        assert_eq!(find("algorithmName"), Some("SHA-1"));
        // Transitional-only names with no Strict equivalent must not survive.
        assert!(find("cryptProviderType").is_none());
        assert!(find("cryptAlgorithmClass").is_none());
        assert!(find("cryptAlgorithmType").is_none());
        assert!(find("cryptAlgorithmSid").is_none());
        assert!(find("cryptSpinCount").is_none());
        assert!(find("hash").is_none());
        assert!(find("salt").is_none());
    }

    /// An unrecognised SID has no Strict spelling and is skipped rather than
    /// guessed at. The skip is a named loss: the write is not Clean.
    #[test]
    fn document_protection_skips_algorithm_name_for_unknown_sid() {
        let pairs: Vec<(std::sync::Arc<str>, std::sync::Arc<str>)> =
            vec![("cryptAlgorithmSid".into(), "9999".into())];
        let attrs = strict_document_protection_attrs(&pairs);
        assert!(attrs.iter().all(|(name, _)| *name != "algorithmName"));

        let settings = Settings {
            document_protection_attributes: pairs,
            ..Settings::default()
        };
        let mut report = NormalizationReport::new();
        let mut ctx = Ctx::new(&mut report);
        let xml = settings_part(&mut ctx, &settings).expect("settings");
        assert!(!xml.contains("algorithmName"), "{xml}");
        assert!(!xml.contains("9999"), "{xml}");
        let losses = report.losses();
        assert!(
            losses.iter().any(|loss| {
                loss.feature_id == "w:documentProtection/@w:cryptAlgorithmSid"
                    && loss.severity == Severity::Lossy
                    && loss
                        .reason
                        .contains("password verification is not preserved")
            }),
            "{losses:?}"
        );
    }

    /// A Strict source's own attribute names pass through unchanged.
    #[test]
    fn document_protection_passes_through_strict_names() {
        let pairs: Vec<(std::sync::Arc<str>, std::sync::Arc<str>)> = vec![
            ("formatting".into(), "1".into()),
            ("spinCount".into(), "50".into()),
            ("algorithmName".into(), "SHA-512".into()),
        ];
        let attrs = strict_document_protection_attrs(&pairs);
        let find = |name: &str| {
            attrs
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| v.as_ref())
        };
        assert_eq!(find("formatting"), Some("1"));
        assert_eq!(find("spinCount"), Some("50"));
        assert_eq!(find("algorithmName"), Some("SHA-512"));
    }

    /// `w:documentProtection` is kept (never silently dropped) when only
    /// `enforcement` survived the mapping, and never carries a
    /// Transitional-only attribute.
    #[test]
    fn settings_part_keeps_protection_with_only_enforcement() {
        let settings = Settings {
            document_protection_attributes: vec![
                ("enforcement".into(), "1".into()),
                ("cryptProviderType".into(), "rsaFull".into()),
            ],
            ..Settings::default()
        };
        let mut report = NormalizationReport::new();
        let mut ctx = Ctx::new(&mut report);
        let xml = settings_part(&mut ctx, &settings).expect("settings_part balances");
        assert!(xml.contains("<w:documentProtection"), "{xml}");
        assert!(xml.contains(r#"w:enforcement="1""#), "{xml}");
        assert!(!xml.contains("cryptProviderType"), "{xml}");
        assert!(
            report.losses().iter().any(|loss| {
                loss.feature_id == "w:documentProtection/@w:cryptProviderType"
                    && loss.severity == Severity::Lossy
            }),
            "{report:?}"
        );
    }

    /// Word's `<w:compat/>` carries no switch and still comes back.
    #[test]
    fn an_empty_compat_is_written_back() {
        let settings = Settings {
            compat_present: true,
            ..Settings::default()
        };
        let mut report = NormalizationReport::new();
        let mut ctx = Ctx::new(&mut report);
        let xml = settings_part(&mut ctx, &settings).expect("settings");
        assert!(xml.contains("<w:compat/>"), "{xml}");
        let mut report = NormalizationReport::new();
        let mut ctx = Ctx::new(&mut report);
        let xml = settings_part(&mut ctx, &Settings::default()).expect("settings");
        assert!(!xml.contains("w:compat"), "{xml}");
    }

    #[test]
    fn explicit_false_headers_and_auto_hyphenation_keep_their_values() {
        let settings = Settings {
            even_and_odd_headers_off: true,
            auto_hyphenation: true,
            ..Settings::default()
        };
        let mut report = NormalizationReport::new();
        let mut ctx = Ctx::new(&mut report);
        let xml = settings_part(&mut ctx, &settings).expect("settings_part balances");
        assert!(
            xml.contains(r#"<w:evenAndOddHeaders w:val="false"/>"#),
            "{xml}"
        );
        assert!(
            xml.contains(r#"<w:autoHyphenation w:val="true"/>"#),
            "{xml}"
        );
    }

    #[test]
    fn an_empty_doc_defaults_element_is_written() {
        let mut table = StyleTable::new();
        table.set_defaults(Default::default());
        let mut report = NormalizationReport::new();
        let mut ctx = Ctx::new(&mut report);
        let xml = styles_part(&mut ctx, &table).expect("styles_part balances");
        assert!(xml.contains("<w:docDefaults>"), "{xml}");
        assert!(xml.contains("<w:rPrDefault"), "{xml}");
        assert!(xml.contains("<w:pPrDefault"), "{xml}");
    }

    #[test]
    fn table_style_band_sizes_are_written() {
        let mut table = StyleTable::new();
        table.insert(Style {
            id: StyleId::new("TableGrid"),
            style_type: StyleType::Table,
            name: None,
            based_on: None,
            next: None,
            link: None,
            is_default: false,
            custom_style: false,
            auto_redefine: false,
            semi_hidden: false,
            hidden: false,
            q_format: false,
            locked: false,
            unhide_when_used: false,
            ui_priority: None,
            table: strict_ooxml_wml::model::props::TableProperties {
                style_row_band_size: Some(3),
                style_col_band_size: Some(2),
                ..Default::default()
            },
            row: strict_ooxml_wml::model::props::RowProperties::default(),
            cell: strict_ooxml_wml::model::props::CellProperties::default(),
            paragraph: Default::default(),
            run: Default::default(),
            conditions: Vec::new(),
            based_on_chain: Vec::new(),
            location: strict_ooxml_core::error::SourceLocation::unknown(),
        });
        let mut report = NormalizationReport::new();
        let mut ctx = Ctx::new(&mut report);
        let xml = styles_part(&mut ctx, &table).expect("styles_part balances");
        assert!(
            xml.contains(r#"<w:tblStyleRowBandSize w:val="3"/>"#),
            "{xml}"
        );
        assert!(
            xml.contains(r#"<w:tblStyleColBandSize w:val="2"/>"#),
            "{xml}"
        );
    }
}
