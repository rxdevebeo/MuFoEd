//! The order `xsd:sequence` gives the property containers, in ONE place.
//!
//! ISO/IEC 29500-1 declares the children of `w:pPr`, `w:settings`, `w:tblPr`,
//! `w:tcPr`, `w:trPr` and `w:style` as an `xsd:sequence`, not as a set: a child
//! that appears earlier than the schema says is "This element is not expected",
//! whatever it is and wherever else it would be legal.
//!
//! The writer used to emit them in the order of the MODEL'S FIELDS. That is the
//! same order in every fixture this project wrote itself, which is why 38 corpus
//! violations went unnoticed until the XSD gate (`STAGE-10G-TASK.md` XS-06,
//! XS-07) measured them against the official schema. The two orders differ in a
//! dozen places, and a property added to the model in the wrong spot is a new
//! violation - so fixing the two elements that happened to be caught would buy
//! nothing and the next property would break it again.
//!
//! So the order is stated here, once, transcribed from `strict/wml.xsd`, and
//! three things hold it to that statement:
//!
//! - `tests/schema_order.rs` walks every container of every package the writer
//!   produces from the whole corpus and fails on a child out of order, so the
//!   emission cannot drift;
//! - `xtool/xsd-gate/xsd_gate.py` reads these constants out of this file and
//!   compares them with the `xsd:sequence` in the published schema it validates
//!   against, so a transcription error cannot survive either;
//! - each constant names the type and the line it was taken from, so a reviewer
//!   can check the source rather than trust the table.
//!
//! The constants are `&[&str]` in schema order, and `rank` is what the emission
//! is written against: the writer asks where a child goes instead of remembering.

/// `CT_PPrBase` + `CT_PPr`, `strict/wml.xsd:2715` and `:2810`.
///
/// `w:rPr`, `w:sectPr` and `w:pPrChange` come from the `CT_PPr` extension, so
/// they come last - `w:rPr` in a `w:pPr` marks the paragraph mark, and it is
/// after every paragraph property, not among them.
pub const PPR: &[&str] = &[
    "pStyle",
    "keepNext",
    "keepLines",
    "pageBreakBefore",
    "framePr",
    "widowControl",
    "numPr",
    "suppressLineNumbers",
    "pBdr",
    "shd",
    "tabs",
    "suppressAutoHyphens",
    "kinsoku",
    "wordWrap",
    "overflowPunct",
    "topLinePunct",
    "autoSpaceDE",
    "autoSpaceDN",
    "bidi",
    "adjustRightInd",
    "snapToGrid",
    "spacing",
    "ind",
    "contextualSpacing",
    "mirrorIndents",
    "suppressOverlap",
    "jc",
    "textDirection",
    "textAlignment",
    "textboxTightWrap",
    "outlineLvl",
    "divId",
    "cnfStyle",
    "rPr",
    "sectPr",
    "pPrChange",
];

/// `CT_Settings`, `strict/wml.xsd:2733`.
///
/// Ninety-four children. Only the ones this writer can produce are named; the
/// point of the constant is the order of those, and a child the writer does not
/// write cannot be written out of order.
pub const SETTINGS: &[&str] = &[
    "writeProtection",
    "view",
    "zoom",
    "removePersonalInformation",
    "removeDateAndTime",
    "doNotDisplayPageBoundaries",
    "displayBackgroundShape",
    "printPostScriptOverText",
    "printFractionalCharacterWidth",
    "printFormsData",
    "embedTrueTypeFonts",
    "embedSystemFonts",
    "saveSubsetFonts",
    "saveFormsData",
    "mirrorMargins",
    "alignBordersAndEdges",
    "bordersDoNotSurroundHeader",
    "bordersDoNotSurroundFooter",
    "gutterAtTop",
    "hideSpellingErrors",
    "hideGrammaticalErrors",
    "activeWritingStyle",
    "proofState",
    "formsDesign",
    "attachedTemplate",
    "linkStyles",
    "stylePaneFormatFilter",
    "stylePaneSortMethod",
    "documentType",
    "mailMerge",
    "revisionView",
    "trackRevisions",
    "doNotTrackMoves",
    "doNotTrackFormatting",
    "documentProtection",
    "autoFormatOverride",
    "styleLockTheme",
    "styleLockQFSet",
    "defaultTabStop",
    "autoHyphenation",
    "consecutiveHyphenLimit",
    "hyphenationZone",
    "doNotHyphenateCaps",
    "showEnvelope",
    "summaryLength",
    "clickAndTypeStyle",
    "defaultTableStyle",
    "evenAndOddHeaders",
    "bookFoldRevPrinting",
    "bookFoldPrinting",
    "bookFoldPrintingSheets",
    "drawingGridHorizontalSpacing",
    "drawingGridVerticalSpacing",
    "displayHorizontalDrawingGridEvery",
    "displayVerticalDrawingGridEvery",
    "doNotUseMarginsForDrawingGridOrigin",
    "drawingGridHorizontalOrigin",
    "drawingGridVerticalOrigin",
    "doNotShadeFormData",
    "noPunctuationKerning",
    "characterSpacingControl",
    "printTwoOnOne",
    "strictFirstAndLastChars",
    "noLineBreaksAfter",
    "noLineBreaksBefore",
    "savePreviewPicture",
    "doNotValidateAgainstSchema",
    "saveInvalidXml",
    "ignoreMixedContent",
    "alwaysShowPlaceholderText",
    "doNotDemarcateInvalidXml",
    "saveXmlDataOnly",
    "useXSLTWhenSaving",
    "saveThroughXslt",
    "showXMLTags",
    "alwaysMergeEmptyNamespace",
    "updateFields",
    "footnotePr",
    "endnotePr",
    "compat",
    "docVars",
    "rsids",
    "attachedSchema",
    "themeFontLang",
    "clrSchemeMapping",
    "doNotIncludeSubdocsInStats",
    "doNotAutoCompressPictures",
    "forceUpgrade",
    "captions",
    "readModeInkLockDown",
    "smartTagType",
    "doNotEmbedSmartTags",
    "decimalSymbol",
    "listSeparator",
];

/// `CT_TblPrBase` + `CT_TblPr`, `strict/wml.xsd:2632` and `:2721`.
pub const TBLPR: &[&str] = &[
    "tblStyle",
    "tblpPr",
    "tblOverlap",
    "bidiVisual",
    "tblStyleRowBandSize",
    "tblStyleColBandSize",
    "tblW",
    "jc",
    "tblCellSpacing",
    "tblInd",
    "tblBorders",
    "shd",
    "tblLayout",
    "tblCellMar",
    "tblLook",
    "tblCaption",
    "tblDescription",
    "tblPrChange",
];

/// `CT_TcPrBase` + `CT_TcPr`, `strict/wml.xsd:2555` and `:2749`.
pub const TCPR: &[&str] = &[
    "cnfStyle",
    "tcW",
    "gridSpan",
    "vMerge",
    "tcBorders",
    "shd",
    "noWrap",
    "tcMar",
    "textDirection",
    "tcFitText",
    "vAlign",
    "hideMark",
    "headers",
    "tcPrChange",
];

/// `CT_TrPrBase` + `CT_TrPr`, `strict/wml.xsd:2477` and `:2745`.
pub const TRPR: &[&str] = &[
    "cnfStyle",
    "divId",
    "gridBefore",
    "gridAfter",
    "wBefore",
    "wAfter",
    "cantSplit",
    "trHeight",
    "tblHeader",
    "tblCellSpacing",
    "jc",
    "hidden",
    "ins",
    "del",
    "trPrChange",
];

/// `CT_Style`, `strict/wml.xsd:1913`.
///
/// The order here is the one that bites: `w:rPr` comes before `w:tblPr`, and the
/// writer emitted the paragraph properties between them, so a style with a run
/// default and a table default produced `w:pPr` after `w:tblPr` and the schema
/// called it "not expected".
pub const STYLE: &[&str] = &[
    "name",
    "aliases",
    "basedOn",
    "next",
    "link",
    "autoRedefine",
    "hidden",
    "uiPriority",
    "semiHidden",
    "unhideWhenUsed",
    "qFormat",
    "locked",
    "personal",
    "personalCompose",
    "personalReply",
    "rsid",
    "pPr",
    "rPr",
    "tblPr",
    "trPr",
    "tcPr",
    "tblStylePr",
];

/// `CT_Lvl`, `strict/wml.xsd:1521`.
///
/// The numbering level's own properties, then the paragraph and run defaults
/// that belong to the level's text. `w:legacy` is a Transitional-only child and
/// is not in this table: the XSD gate found it here on the first run of its
/// comparison, which is the whole reason that comparison exists.
pub const LVL: &[&str] = &[
    "start",
    "numFmt",
    "lvlRestart",
    "pStyle",
    "isLgl",
    "suff",
    "lvlText",
    "lvlPicBulletId",
    "lvlJc",
    "pPr",
    "rPr",
];

/// Where `name` sits in `sequence`.
///
/// `usize::MAX` for a child the sequence does not name, so an unknown child
/// sorts last and the caller can report it as unknown rather than mis-ordered -
/// the two are different defects and only one of them is this module's business.
#[must_use]
pub fn rank(sequence: &[&str], name: &str) -> usize {
    sequence
        .iter()
        .position(|entry| *entry == name)
        .unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::{rank, PPR, SETTINGS, STYLE, TBLPR, TCPR, TRPR};

    #[test]
    fn a_child_ranks_by_its_position() {
        assert_eq!(rank(PPR, "pStyle"), 0);
        assert!(rank(PPR, "keepNext") < rank(PPR, "spacing"));
        assert!(rank(PPR, "spacing") < rank(PPR, "ind"));
        assert!(rank(PPR, "outlineLvl") < rank(PPR, "rPr"));
    }

    #[test]
    fn an_unknown_child_ranks_last_rather_than_guessed() {
        assert_eq!(rank(PPR, "w:invented"), usize::MAX);
    }

    #[test]
    fn no_constant_names_a_child_twice() {
        for (name, sequence) in [
            ("PPR", PPR),
            ("SETTINGS", SETTINGS),
            ("TBLPR", TBLPR),
            ("TCPR", TCPR),
            ("TRPR", TRPR),
            ("STYLE", STYLE),
        ] {
            let mut seen: Vec<&str> = sequence.to_vec();
            seen.sort_unstable();
            let count = seen.len();
            seen.dedup();
            assert_eq!(seen.len(), count, "{name} names a child twice");
        }
    }
}
