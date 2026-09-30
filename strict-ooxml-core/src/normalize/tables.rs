//! The mapping tables normalization applies (`TZ-STRICT-OOXML-RUST.md`
//! §10.11).
//!
//! The spec has these generated from the official XSDs by `xtool xsd-diff`,
//! and carries entries flagged `verified = false` for the ones nobody had
//! checked. It is worth being explicit about why that caution is right rather
//! than ceremonial: the direction-neutral renames are the obvious suspect,
//! and checking them against our own parser showed `w:ind` accepts both
//! `start` and `left` while `w:pgMar` reads **only** `left`/`right`
//! (`strict-ooxml-wml/src/parse/props.rs`). Applying the ISO-correct rename to
//! `w:pgMar` would therefore have produced a document that validates as Strict
//! and lays out with zero page margins — a silent, total loss of layout on
//! every page.
//!
//! So the rule this module encodes is:
//!
//! * a table entry is applied only when [`Rename::verified`] is true;
//! * `verified` is set only by a test that shows the Strict-named form parses
//!   to the same model as the Transitional one.
//!
//! [`RENAMES`] therefore starts empty. That is a deliberate position, not an
//! omission: the parser is already tolerant of both spellings for the
//! attributes that matter, so renaming buys nothing there and risks
//! everything where the parser is not tolerant.

use crate::normalize::report::Severity;

/// An element or attribute that Strict spells differently.
#[derive(Clone, Copy, Debug)]
pub struct Rename {
    /// Namespace key of the owning domain, for grouping in the report.
    pub domain: &'static str,
    /// The Transitional local name.
    pub from: &'static str,
    /// The Strict local name.
    pub to: &'static str,
    /// Whether the downstream parser has been shown to accept `to`.
    pub verified: bool,
    /// Why it is (not) applied.
    pub note: &'static str,
}

/// The renames normalization may apply.
///
/// Empty. See the module docs: the entries `TZ` §10.4 lists are unverified,
/// and the one place the parser is *not* tolerant of both spellings
/// (`w:pgMar`) would lose every page margin if it were applied.
pub const RENAMES: &[Rename] = &[];

/// A value that Strict spells differently, in one attribute.
#[derive(Clone, Copy, Debug)]
pub struct ValueMap<'a> {
    /// The Strict-qualified attribute local name.
    pub attribute: &'static str,
    /// The Transitional value.
    pub from: &'a str,
    /// The Strict value.
    pub to: &'a str,
}

/// `ST_Jc` values that became direction-neutral in Strict (`TZ` §10.5).
///
/// `both` and `center` are unchanged and so are absent.
pub const JC_VALUES: &[ValueMap<'static>] = &[
    ValueMap {
        attribute: "jc",
        from: "left",
        to: "start",
    },
    ValueMap {
        attribute: "jc",
        from: "right",
        to: "end",
    },
];

/// Whether `attribute`/`value` has a Strict spelling in the tables above.
#[must_use]
pub fn map_value(attribute: &str, value: &str) -> Option<&'static str> {
    JC_VALUES
        .iter()
        .find(|entry| entry.attribute == attribute && entry.from == value)
        .map(|entry| entry.to)
}

/// An element Strict does not have, and what removing it costs.
#[derive(Clone, Copy, Debug)]
pub struct Removal {
    /// The namespace key, or `None` for the WML main namespace.
    pub domain: Option<&'static str>,
    /// The local name.
    pub local: &'static str,
    /// How much removing it matters.
    pub severity: Severity,
    /// Why it is classified that way.
    pub reason: &'static str,
}

const fn wml(local: &'static str, severity: Severity, reason: &'static str) -> Removal {
    Removal {
        domain: None,
        local,
        severity,
        reason,
    }
}

/// Elements removed by T5, classified by whether the page changes.
///
/// The split is the whole point of the table. A real Word document puts
/// dozens of editor-state elements in `settings.xml` that no renderer can act
/// on; treating those as blockers is what made `check` useless on real files
/// and would make normalization useless here. Conversely `w:object` and
/// `w:pict` carry pictures, and dropping them silently is exactly the failure
/// this stage must not have.
pub const REMOVALS: &[Removal] = &[
    // ---- editor state: cannot change a rendered page -------------------
    wml(
        "stylePaneFormatFilter",
        Severity::Ignorable,
        "word processor UI state, not document content",
    ),
    wml(
        "stylePaneSortMethod",
        Severity::Ignorable,
        "word processor UI state, not document content",
    ),
    wml(
        "drawingGridHorizontalSpacing",
        Severity::Ignorable,
        "grid pitch only matters with a snapped document grid",
    ),
    wml(
        "drawingGridVerticalSpacing",
        Severity::Ignorable,
        "grid pitch only matters with a snapped document grid",
    ),
    wml(
        "displayHorizontalDrawingGridEvery",
        Severity::Ignorable,
        "grid display interval, no effect on the page",
    ),
    wml(
        "displayVerticalDrawingGridEvery",
        Severity::Ignorable,
        "grid display interval, no effect on the page",
    ),
    wml(
        "doNotUseMarginsForDrawingGridOrigin",
        Severity::Ignorable,
        "grid origin, no effect on the page",
    ),
    wml(
        "drawingGridHorizontalOrigin",
        Severity::Ignorable,
        "grid origin, no effect on the page",
    ),
    wml(
        "drawingGridVerticalOrigin",
        Severity::Ignorable,
        "grid origin, no effect on the page",
    ),
    wml(
        "bordersDoNotSurroundHeader",
        Severity::Ignorable,
        "affects page borders only, which the renderer draws itself",
    ),
    wml(
        "bordersDoNotSurroundFooter",
        Severity::Ignorable,
        "affects page borders only, which the renderer draws itself",
    ),
    wml(
        "noPunctuationKerning",
        Severity::Ignorable,
        "CJK punctuation kerning hint",
    ),
    wml(
        "characterSpacingControl",
        Severity::Ignorable,
        "CJK character spacing control",
    ),
    wml(
        "printTwoOnOne",
        Severity::Ignorable,
        "print setting, not a page property",
    ),
    wml(
        "savePreviewPicture",
        Severity::Ignorable,
        "thumbnail setting",
    ),
    wml(
        "doNotValidateAgainstSchema",
        Severity::Ignorable,
        "editor setting",
    ),
    wml("saveInvalidXml", Severity::Ignorable, "editor setting"),
    wml("ignoreMixedContent", Severity::Ignorable, "editor setting"),
    wml(
        "alwaysShowPlaceholderText",
        Severity::Ignorable,
        "editor setting",
    ),
    wml(
        "doNotDemarcateInvalidXml",
        Severity::Ignorable,
        "editor setting",
    ),
    wml("saveXmlDataOnly", Severity::Ignorable, "editor setting"),
    wml("useXSLTWhenSaving", Severity::Ignorable, "editor setting"),
    wml("saveThroughXslt", Severity::Ignorable, "editor setting"),
    wml("showXMLTags", Severity::Ignorable, "editor setting"),
    wml(
        "alwaysMergeEmptyNamespace",
        Severity::Ignorable,
        "editor setting",
    ),
    wml(
        "updateFields",
        Severity::Ignorable,
        "field refresh is the application's job at render time",
    ),
    wml(
        "mailMerge",
        Severity::Ignorable,
        "mail merge settings, no page content",
    ),
    wml(
        "revisionView",
        Severity::Ignorable,
        "tracked-change display setting",
    ),
    wml(
        "rsids",
        Severity::Ignorable,
        "revision save ids, no page content",
    ),
    wml(
        "embedSystemFonts",
        Severity::Ignorable,
        "which font set to embed on save, not a page property",
    ),
    wml(
        "embedTrueTypeFonts",
        Severity::Ignorable,
        "font embedding setting, not a page property",
    ),
    wml(
        "saveSubsetFonts",
        Severity::Ignorable,
        "font embedding setting, not a page property",
    ),
    wml(
        "attachedTemplate",
        Severity::Ignorable,
        "a path to a template on the author's machine",
    ),
    wml(
        "linkStyles",
        Severity::Ignorable,
        "template linkage, no page content",
    ),
    wml(
        "documentProtection",
        Severity::Ignorable,
        "editing restriction, no page content",
    ),
    wml(
        "defaultTabStop",
        Severity::Ignorable,
        "carried by the style hierarchy, not by a removable setting",
    ),
    // ---- compatibility switches: the page CAN move ----------------------
    wml(
        "compat",
        Severity::Lossy,
        "layout compatibility flags can change line breaking and spacing",
    ),
    wml(
        "useFELayout",
        Severity::Ignorable,
        "Far East layout mode, no effect on the scripts in this toolkit",
    ),
    wml(
        "doNotExpandShiftReturn",
        Severity::Ignorable,
        "no effect without soft returns",
    ),
    // ---- content-bearing: removing these changes the page ---------------
    wml(
        "object",
        Severity::Lossy,
        "OLE object; the embedded preview image cannot be substituted",
    ),
    wml(
        "pict",
        Severity::Lossy,
        "VML picture; no raster substitute available",
    ),
    wml(
        "legacyDrawing",
        Severity::Lossy,
        "VML drawing; no raster substitute available",
    ),
    wml(
        "legacyDrawingHF",
        Severity::Lossy,
        "VML drawing in a header or footer; no raster substitute available",
    ),
    wml(
        "ptab",
        Severity::Lossy,
        "absolute position tab; a real layout construct with no substitute",
    ),
    wml(
        "objectEmbed",
        Severity::Lossy,
        "embedded OLE object without a preview image",
    ),
    wml(
        "objectLink",
        Severity::Lossy,
        "linked OLE object without a preview image",
    ),
];

/// Legacy VML namespaces and how touching them is classified.
pub const VML_NAMESPACES: &[&str] = &[
    "urn:schemas-microsoft-com:vml",
    "urn:schemas-microsoft-com:office:office",
    "urn:schemas-microsoft-com:office:word",
];

/// Extension namespaces whose whole content is decoration or editor state.
///
/// Narrow on purpose. `w14` and `w15` carry text effects — a gradient fill on
/// a run, a glow, a shadow — that a renderer either implements or cannot
/// approximate, and an extension is by construction outside the standard:
/// Markup Compatibility exists precisely so a consumer may skip what it does
/// not understand. Dropping them is a no-op for the page and is what lets a
/// fifth of the corpus open at all; 476 `w14:textFill` elements in a single
/// document is not a defect, it is Word 2010's default text styling.
///
/// The drawing extensions are deliberately **absent**. `wps`, `wpg`, `wpc`,
/// `wpi` and `wp14` carry shapes, groups, canvases and ink — content the
/// renderer does implement (Stage 5B) — so dropping them would be losing
/// pictures, not decoration.
pub const IGNORABLE_EXTENSION_NAMESPACES: &[&str] = &[
    "http://schemas.microsoft.com/office/word/2010/wordml",
    "http://schemas.microsoft.com/office/word/2012/wordml",
];

/// Looks up a removal by local name.
#[must_use]
pub fn removal_for(local: &str) -> Option<&'static Removal> {
    REMOVALS.iter().find(|entry| entry.local == local)
}

/// Whether a namespace holds only decoration and editor state.
#[must_use]
pub fn is_ignorable_extension(uri: &str) -> bool {
    IGNORABLE_EXTENSION_NAMESPACES.contains(&uri)
}

#[cfg(test)]
mod tests {
    use super::{map_value, removal_for, REMOVALS};
    use crate::normalize::report::Severity;

    #[test]
    fn no_rename_is_verified_yet() {
        // The trap from the module docs: `w:pgMar` is read only as
        // left/right by the parser, so an applied rename would zero the page
        // margins. The table is empty until a test proves otherwise.
        for rename in super::RENAMES {
            assert!(
                !rename.verified,
                "{} -> {} is applied but unverified",
                rename.from, rename.to
            );
        }
    }

    #[test]
    fn direction_neutral_values_are_mapped() {
        assert_eq!(map_value("jc", "left"), Some("start"));
        assert_eq!(map_value("jc", "right"), Some("end"));
    }

    #[test]
    fn values_strict_keeps_are_not_mapped() {
        assert_eq!(map_value("jc", "center"), None);
        assert_eq!(map_value("jc", "both"), None);
        assert_eq!(map_value("jc", "distribute"), None);
    }

    #[test]
    fn the_value_map_only_applies_to_its_own_attribute() {
        // `left` is also a legal value elsewhere; the table is keyed by
        // attribute so an unrelated attribute is never rewritten.
        assert_eq!(map_value("textAlignment", "left"), None);
    }

    #[test]
    fn editor_state_is_ignorable_and_content_is_lossy() {
        assert_eq!(
            removal_for("stylePaneFormatFilter").map(|entry| entry.severity),
            Some(Severity::Ignorable)
        );
        assert_eq!(
            removal_for("pict").map(|entry| entry.severity),
            Some(Severity::Lossy)
        );
        assert_eq!(
            removal_for("object").map(|entry| entry.severity),
            Some(Severity::Lossy)
        );
    }

    #[test]
    fn an_unlisted_element_is_not_removed() {
        assert!(removal_for("w:sectPr").is_none());
        assert!(removal_for("notInTheTable").is_none());
    }

    #[test]
    fn the_removal_table_has_no_duplicates() {
        let mut seen = std::collections::BTreeSet::new();
        for entry in REMOVALS {
            assert!(
                seen.insert(entry.local),
                "duplicate removal {}",
                entry.local
            );
        }
    }
}
