#![allow(
    clippy::doc_markdown,
    clippy::useless_format,
    clippy::format_push_string,
    clippy::unreadable_literal
)]
//! Coverage tests for `styles.xml`, `settings.xml` and `numbering.xml` branches.

mod common;

use strict_ooxml_wml::model::ids::{NumId, StyleId};
use strict_ooxml_wml::model::support::SupportStatus;
use strict_ooxml_wml::model::values::{Justification, StyleType};

use common::{document_parts, parse_parts, rels, W_NS};

const STYLES: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/styles";
const NUMBERING: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/numbering";
const SETTINGS: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/settings";

fn parse_aux(
    styles: Option<&str>,
    numbering: Option<&str>,
    settings: Option<&str>,
) -> strict_ooxml_wml::model::Document {
    let mut parts = document_parts("<w:p/>", &[]);
    let mut rel_entries: Vec<(&str, &str, &str)> = Vec::new();
    if let Some(styles) = styles {
        parts.push(("word/styles.xml".to_owned(), styles.as_bytes().to_vec()));
        rel_entries.push(("rIdStyles", STYLES, "styles.xml"));
    }
    if let Some(numbering) = numbering {
        parts.push((
            "word/numbering.xml".to_owned(),
            numbering.as_bytes().to_vec(),
        ));
        rel_entries.push(("rIdNumbering", NUMBERING, "numbering.xml"));
    }
    if let Some(settings) = settings {
        parts.push(("word/settings.xml".to_owned(), settings.as_bytes().to_vec()));
        rel_entries.push(("rIdSettings", SETTINGS, "settings.xml"));
    }
    if !rel_entries.is_empty() {
        parts.push((
            "word/_rels/document.xml.rels".to_owned(),
            rels(&rel_entries),
        ));
    }
    parse_parts(&parts).expect("parse")
}

#[test]
fn parses_full_style_definition() {
    let styles = format!(
        "<w:styles xmlns:w=\"{W_NS}\">\
<w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val=\"22\"/></w:rPr></w:rPrDefault></w:docDefaults>\
<w:latentStyles w:count=\"3\"/>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading1\" w:default=\"1\" w:customStyle=\"1\">\
<w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:link w:val=\"Heading1Char\"/>\
<w:uiPriority w:val=\"9\"/><w:semiHidden/><w:qFormat/><w:unhideWhenUsed/>\
<w:pPr><w:keepNext/></w:pPr><w:rPr><w:b/></w:rPr><w:tblPr><w:tblStyle w:val=\"Grid\"/></w:tblPr>\
<w:unknownStyleChild w:val=\"x\"/>\
</w:style>\
<w:style w:type=\"character\" w:styleId=\"Emph\"><w:hidden/><w:name w:val=\"Emph\"/></w:style>\
<w:style w:type=\"paragraph\"><w:name w:val=\"no-id\"/></w:style>\
<w:style w:styleId=\"no-type\"><w:name w:val=\"no-type\"/></w:style>\
</w:styles>"
    );
    let document = parse_aux(Some(&styles), None, None);
    // `w:docDefaults` is parsed and becomes the root of the style cascade
    // (STAGE-5C-REWORK-1 C1: the real Strict corpus sets its line spacing and
    // font there), so it is no longer reported as a partial.
    assert!(document.support.get("w:docDefaults").is_none());
    let defaults = document.styles.defaults().expect("document defaults");
    assert_eq!(defaults.run.size.unwrap().value(), 22);
    assert_eq!(
        document.support.get("w:latentStyles").unwrap().status,
        SupportStatus::Ignored
    );
    let style = document.styles.get(&StyleId::new("Heading1")).unwrap();
    assert_eq!(style.style_type, StyleType::Paragraph);
    assert_eq!(style.name.as_deref(), Some("heading 1"));
    assert_eq!(style.based_on.as_ref().unwrap().as_str(), "Normal");
    assert_eq!(style.next.as_ref().unwrap().as_str(), "Normal");
    assert_eq!(style.link.as_ref().unwrap().as_str(), "Heading1Char");
    assert_eq!(style.ui_priority, Some(9));
    assert!(style.is_default && style.hidden);
    assert!(style.paragraph.keep_next);
    assert!(style.run.bold.is_on());
    assert_eq!(style.table.style.as_ref().unwrap().as_str(), "Grid");
    let emph = document.styles.get(&StyleId::new("Emph")).unwrap();
    assert_eq!(emph.style_type, StyleType::Character);
    assert!(emph.hidden);
    // Styles without a type/id are skipped and recorded.
    assert_eq!(
        document.support.get("w:style").unwrap().status,
        SupportStatus::Partial
    );
}

#[test]
fn parses_all_settings_elements() {
    let settings = format!(
        "<w:settings xmlns:w=\"{W_NS}\">\
<w:defaultTabStop w:val=\"720\"/><w:zoom w:percent=\"120\" w:val=\"textFit\"/>\
<w:evenAndOddHeaders/><w:displayBackgroundShape/><w:hideSpellingErrors/><w:hideGrammaticalErrors/>\
<w:proofState w:spelling=\"clean\"/><w:trackRevisions/><w:doNotHyphenateCaps/><w:autoHyphenation/>\
<w:hyphenationZone w:val=\"360\"/><w:documentProtection w:edit=\"readOnly\"/>\
<w:decimalSymbol w:val=\".\"/><w:listSeparator w:val=\",\"/><w:themeFontLang w:val=\"en-US\"/>\
<w:mirrorMargins/>\
<w:compat><w:compatSetting w:name=\"compatibilityMode\" w:val=\"15\"/><w:compatSetting w:name=\"x\"/></w:compat>\
<w:mysterySetting/>\
</w:settings>"
    );
    let document = parse_aux(None, None, Some(&settings));
    let settings = &document.settings;
    assert_eq!(settings.default_tab_stop.unwrap().value(), 720);
    assert_eq!(settings.zoom.unwrap().percent, Some(120));
    assert!(settings.even_and_odd_headers);
    assert!(settings.display_background_shape);
    assert!(settings.hide_spelling_errors && settings.hide_grammatical_errors);
    assert!(settings.proofing && settings.track_revisions);
    assert!(settings.do_not_hyphenate_caps && settings.auto_hyphenation);
    assert_eq!(settings.hyphenation_zone.unwrap().value(), 360);
    assert_eq!(settings.document_protection.as_deref(), Some("readOnly"));
    assert_eq!(settings.decimal_symbol.as_deref(), Some("."));
    assert_eq!(settings.list_separator.as_deref(), Some(","));
    assert_eq!(settings.theme_font_lang.as_deref(), Some("en-US"));
    assert!(settings.mirror_margins);
    assert_eq!(settings.compatibility.len(), 1);
    assert!(document.support.get("w:mysterySetting").is_some());
}

#[test]
fn parses_full_numbering_definitions() {
    let numbering = format!(
        "<w:numbering xmlns:w=\"{W_NS}\">\
<w:abstractNum w:abstractNumId=\"0\">\
<w:multiLevelType w:val=\"multilevel\"/>\
<w:styleLink w:val=\"ListParagraph\"/><w:numStyleLink w:val=\"List\"/>\
<w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%1.\"/>\
<w:lvlJc w:val=\"start\"/><w:pStyle w:val=\"ListParagraph\"/><w:suff w:val=\"tab\"/><w:lvlRestart w:val=\"0\"/>\
<w:isLgl/><w:tentative/><w:pPr><w:ind w:start=\"720\"/></w:pPr><w:rPr><w:b/></w:rPr></w:lvl>\
<w:lvl w:ilvl=\"1\"/><w:unknownLvlChild/>\
</w:abstractNum>\
<w:abstractNum/>\
<w:num w:numId=\"5\"><w:abstractNumId w:val=\"0\"/>\
<w:lvlOverride w:ilvl=\"0\"><w:startOverride w:val=\"3\"/><w:lvl w:ilvl=\"0\"><w:numFmt w:val=\"lowerLetter\"/></w:lvl></w:lvlOverride>\
</w:num>\
<w:num w:numId=\"6\"/>\
</w:numbering>"
    );
    let document = parse_aux(None, Some(&numbering), None);
    let abstract_num = document.numbering.resolved_abstract(NumId(5)).unwrap();
    assert_eq!(abstract_num.multi_level_type.as_deref(), Some("multilevel"));
    assert_eq!(
        abstract_num.style_link.as_ref().unwrap().as_str(),
        "ListParagraph"
    );
    assert_eq!(
        abstract_num.num_style_link.as_ref().unwrap().as_str(),
        "List"
    );
    let level = abstract_num
        .level(strict_ooxml_wml::model::ids::Ilvl(0))
        .unwrap();
    assert_eq!(level.start, Some(1));
    assert_eq!(level.format.as_deref(), Some("decimal"));
    assert_eq!(level.text.as_deref(), Some("%1."));
    assert_eq!(level.justification, Some(Justification::Start));
    assert_eq!(
        level.paragraph_style.as_ref().unwrap().as_str(),
        "ListParagraph"
    );
    assert_eq!(level.suffix.as_deref(), Some("tab"));
    assert_eq!(level.restart, Some(0));
    assert!(level.is_legal && level.tentative);
    assert_eq!(
        level.paragraph.indentation.unwrap().start.unwrap().value(),
        720
    );
    assert!(level.run.bold.is_on());
    let num = document.numbering.num(NumId(5)).unwrap();
    assert_eq!(num.overrides.len(), 1);
    assert_eq!(num.overrides[0].start_override, Some(3));
    assert!(num.overrides[0].level.is_some());
    // `w:num` without abstractNumId is skipped and recorded.
    assert_eq!(
        document.support.get("w:num").unwrap().status,
        SupportStatus::Partial
    );
}

#[test]
fn auxiliary_parts_absent_by_default() {
    let document = parse_aux(None, None, None);
    assert!(document.styles.is_empty());
    assert!(document.numbering.is_empty());
    assert_eq!(
        document.settings,
        strict_ooxml_wml::model::settings::Settings::default()
    );
}

/// `m:mathPr` is the one child of `w:settings` in another namespace.
///
/// The settings parser drops everything that is not `wml`, and this block was
/// dropped with it - twelve elements gone from forty corpus documents, none of
/// them named. The test is named for the namespace rather than the element
/// because the element is fine; the filter was the defect.
#[test]
fn math_properties_survive_the_non_wml_filter() {
    let document = parse_aux(
        None,
        None,
        Some(
            r#"<w:settings xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main"
  xmlns:m="http://purl.oclc.org/ooxml/officeDocument/math">
  <m:mathPr>
    <m:mathFont m:val="Cambria Math"/>
    <m:brkBin m:val="before"/>
    <m:brkBinSub m:val="--"/>
    <m:smallFrac m:val="0"/>
    <m:dispDef/>
    <m:lMargin m:val="0"/>
    <m:rMargin m:val="0"/>
    <m:defJc m:val="centerGroup"/>
    <m:wrapIndent m:val="1440"/>
    <m:intLim m:val="subSup"/>
    <m:naryLim m:val="undOvr"/>
  </m:mathPr>
</w:settings>"#,
        ),
    );
    let mathematics = document
        .settings
        .math_properties
        .expect("m:mathPr is parsed, not skipped as foreign markup");
    assert_eq!(mathematics.math_font.as_deref(), Some("Cambria Math"));
    assert_eq!(mathematics.break_binary_operator.as_deref(), Some("before"));
    assert_eq!(
        mathematics.default_justification.as_deref(),
        Some("centerGroup")
    );
    assert_eq!(mathematics.integral_limit.as_deref(), Some("subSup"));
    // `m:wrapIndent` is nested in an `xsd:choice` inside the sequence, so a
    // model built from the sequence's direct children would have no field for it
    // and it would be dropped on the round trip - which is how 40 documents lost
    // it even after the block itself started being parsed.
    assert_eq!(
        mathematics.wrap_indent.as_deref(),
        Some("1440"),
        "the choice arm must be modelled, not just the direct children"
    );
    assert!(mathematics.pre_space.is_none());
    // A bare `<m:dispDef/>` is `CT_OnOff` and means on; recording it as absent
    // would turn a set flag off on the next write.
    assert_eq!(mathematics.display_default.as_deref(), Some("1"));
}

/// The seven on/off children of `w:compat`, in `CT_Compat`'s order.
///
/// They are bare `CT_OnOff` flags and every corpus document writes them with no
/// value at all, so a reader that looks for `@w:val` sees none of them. `w:compat`
/// is regenerated from the model, so a flag the model did not carry was deleted
/// from the document rather than left out of the output, and thirty-three of them
/// went that way with nothing in the report.
#[test]
fn compat_flags_survive_alongside_compat_settings() {
    let document = parse_aux(
        None,
        None,
        Some(
            r#"<w:settings xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main">
  <w:compat>
    <w:spaceForUL/>
    <w:doNotLeaveBackslashAlone/>
    <w:ulTrailSpace/>
    <w:doNotExpandShiftReturn/>
    <w:adjustLineHeightInTable/>
    <w:compatSetting w:name="compatibilityMode" w:val="15"/>
  </w:compat>
</w:settings>"#,
        ),
    );
    let settings = &document.settings;
    assert!(settings.compat_flags.space_for_underline);
    assert!(settings.compat_flags.do_not_leave_backslash_alone);
    assert!(settings.compat_flags.underline_trailing_space);
    assert!(settings.compat_flags.do_not_expand_shift_return);
    assert!(settings.compat_flags.adjust_line_height_in_table);
    assert!(
        !settings.compat_flags.apply_breaking_rules,
        "an absent flag is off, not unknown"
    );
    assert_eq!(
        settings.compatibility.len(),
        1,
        "compatSetting still parsed"
    );

    // Schema order, not corpus order: the corpus writes these alphabetically and
    // CT_Compat does not. `doNotLeaveBackslashAlone` is third, `ulTrailSpace`
    // fourth, and getting that pair backwards fails on xsd:sequence.
    assert_eq!(
        settings.compat_flags.set(),
        vec![
            "w:spaceForUL",
            "w:doNotLeaveBackslashAlone",
            "w:ulTrailSpace",
            "w:doNotExpandShiftReturn",
            "w:adjustLineHeightInTable",
        ]
    );
}
