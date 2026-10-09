//! The mapping tables normalization applies (`TZ-STRICT-OOXML-RUST.md`
//! §10.11).
//!
//! The spec has these generated from the official XSDs by `xtool xsd-diff`,
//! and carries entries flagged `verified = false` for the ones nobody had
//! checked. It is worth being explicit about why that caution is right rather
//! than ceremonial: **the direction-neutral rename is conditional on the parent,
//! and getting the condition wrong loses layout on every page.**
//!
//! The four containers whose edges Strict made direction-neutral are
//! `CT_TblBorders`, `CT_TcBorders`, `CT_TblCellMar` and `CT_TcMar`, which
//! declare `start`/`end`. The two that did not are `CT_PBdr` and
//! `CT_PageBorders`, which still declare **`left`/`right`** — and
//! `CT_PageMar` likewise still declares `@w:left`/`@w:right`, plus
//! `@w:header`/`@w:footer`/`@w:gutter` as `use="required"`. `CT_Ind`, by
//! contrast, declares only `@w:start`/`@w:end`.
//!
//! So a *global* `left → start` rename is not a renaming at all: it is a way of
//! producing a document that validates as Strict and draws every paragraph
//! border, every page border and all four page margins as nothing. That is a
//! silent, total loss of layout, and it is the reason the table is keyed by
//! `(parent, name)` and why `verified` is a gate rather than a comment.
//!
//! The rule this module encodes:
//!
//! * a table entry is applied only when [`Rename::verified`] is true;
//! * `verified` is set only by a test that shows the Strict-named form parses
//!   to the same model as the Transitional one;
//! * an entry names the parent it is conditional on, because "is this `left` an
//!   edge or a margin?" is a question about the container, not the name.

use crate::normalize::report::Severity;

// AUD-22: the relationship-type table lives in `opc::rels`, not here — `opc`
// already depends on `normalize` (for `RawNormalizer`), and `RelType` itself
// lives in `opc`, so putting the table here would pull `normalize` into
// `opc`'s own module in the other direction too. Re-exported so code that
// already reaches for `tables::` finds it anyway.
pub use crate::opc::rels::{RelTypeRow, REL_TYPES};

/// An element or attribute that Strict spells differently.
///
/// The `parent` field is what makes the entry safe to apply. `left` is an edge
/// in four containers and a page margin in three, and the two sets of Strict
/// names do not overlap.
#[derive(Clone, Copy, Debug)]
pub struct Rename {
    /// Namespace key of the owning domain, for grouping in the report.
    pub domain: &'static str,
    /// Local name the rename is conditional on.
    ///
    /// **The key means different things for the two kinds of entry, and the
    /// reason is that Strict renamed the two halves independently.** For an
    /// element entry it is the **container**: `w:left` is an edge of
    /// `w:tblBorders` (renamed to `w:start`) and a border of `w:pBdr` (not
    /// renamed). For an attribute entry it is the **holder**: `w:left` is an
    /// attribute of `w:ind` (renamed to `@w:start`) and of `w:pgMar` (not
    /// renamed). Both halves are load-bearing, and using the container for an
    /// attribute entry would make `w:pgMar/@w:left` follow the `w:ind` rule and
    /// zero all four page margins on every page.
    pub parent: &'static str,
    /// Whether the entry renames an attribute rather than an element.
    pub attribute: bool,
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
/// **Every entry is keyed by parent, and the parents with no entry are the
/// point.** `w:pBdr`, `w:pgBorders` and `w:pgMar` are absent on purpose: Strict
/// keeps `left` and `right` in all three, and `TZ-STRICT-OOXML-RUST.md` §10.4
/// says otherwise. So are `w:tblpPr/@w:leftFromText` and `@w:rightFromText`,
/// which the same section wants renamed and which Strict's `CT_TblPPr` still
/// spells `leftFromText`/`rightFromText` — eight occurrences in two corpus
/// documents, and renaming them would have been the same class of defect as
/// zeroing the page margins. The negative entries are the ones that need the
/// gate, and the gate that holds them is
/// `no_direction_neutral_rename_without_its_parent`.
pub const RENAMES: &[Rename] = &[
    // ---- edges: `CT_TblBorders` and `CT_TcBorders` declare start/end ------
    edge("tblBorders", "left", "start", "CT_TblBorders"),
    edge("tblBorders", "right", "end", "CT_TblBorders"),
    edge("tcBorders", "left", "start", "CT_TcBorders"),
    edge("tcBorders", "right", "end", "CT_TcBorders"),
    // ---- `CT_Charset` declares one attribute and it is not `w:val` -------
    //
    // Three hundred and eighteen occurrences in forty-one corpus documents, all
    // of them in `w:font`, which IS a regenerated part - so the writer's font
    // table was always conformant and the normalizer was never asked. A pass-
    // through part that carried a `w:charset` would not be, which is the same
    // blind spot as the four value forms.
    attribute("charset", "val", "characterSet", "CT_Charset"),
    // ---- margins: `CT_TblCellMar` and `CT_TcMar` declare start/end -------
    edge("tblCellMar", "left", "start", "CT_TblCellMar"),
    edge("tblCellMar", "right", "end", "CT_TblCellMar"),
    edge("tcMar", "left", "start", "CT_TcMar"),
    edge("tcMar", "right", "end", "CT_TcMar"),
    // ---- `CT_Ind` has attributes, and only start/end --------------------
    attribute(
        "ind",
        "left",
        "start",
        "CT_Ind declares @w:start and has no @w:left; the parser reads both \
         spellings and prefers start",
    ),
    attribute(
        "ind",
        "right",
        "end",
        "CT_Ind declares @w:end and has no @w:right",
    ),
];

const fn edge(
    parent: &'static str,
    from: &'static str,
    to: &'static str,
    note: &'static str,
) -> Rename {
    Rename {
        domain: "wml",
        parent,
        attribute: false,
        from,
        to,
        verified: true,
        note,
    }
}

const fn attribute(
    parent: &'static str,
    from: &'static str,
    to: &'static str,
    note: &'static str,
) -> Rename {
    Rename {
        domain: "wml",
        parent,
        attribute: true,
        from,
        to,
        verified: true,
        note,
    }
}

/// The Strict local name for a WML child element of `parent`, or `from` itself.
#[must_use]
pub fn rename_element(parent: &str, from: &str) -> Option<&'static str> {
    RENAMES
        .iter()
        .find(|entry| {
            !entry.attribute && entry.verified && entry.parent == parent && entry.from == from
        })
        .map(|entry| entry.to)
}

/// The Strict local name for an attribute of `parent`, or `from` itself.
#[must_use]
pub fn rename_attribute(parent: &str, from: &str) -> Option<&'static str> {
    RENAMES
        .iter()
        .find(|entry| {
            entry.attribute && entry.verified && entry.parent == parent && entry.from == from
        })
        .map(|entry| entry.to)
}

/// A value that Strict spells differently, in one attribute.
#[derive(Clone, Copy, Debug)]
pub struct ValueMap<'a> {
    /// Local name of the element carrying the attribute.
    ///
    /// Required, and the reason is a counter-example rather than a principle:
    /// `left` and `right` are *legal Strict values* of `ST_PTabAlignment`, which
    /// is `w:ptab/@w:alignment`, and of `w:pBdr`'s edge elements. A table keyed
    /// by the attribute alone would rename a `w:ptab` alignment that must keep
    /// its spelling, and the value would be gone. The original key - the
    /// attribute's own local name - was the opposite mistake: `map_value("val",
    /// "left")` can never match an entry keyed `jc`, so the rule was dead.
    /// Both halves are load-bearing, and the test that holds them is
    /// `the_value_map_only_applies_to_its_own_element`.
    pub element: &'static str,
    /// The attribute's local name.
    pub attribute: &'static str,
    /// The Transitional value.
    pub from: &'a str,
    /// The Strict value.
    pub to: &'a str,
}

/// The direction-neutral enumeration values Strict spells `start`/`end`
/// (`TZ` §10.5).
///
/// `both`, `center` and `clear` are unchanged and so are absent. The element
/// column is what keeps `w:ptab/@w:alignment="left"` out of this table - see
/// [`ValueMap::element`].
const fn jc(element: &'static str, from: &'static str, to: &'static str) -> ValueMap<'static> {
    ValueMap {
        element,
        attribute: "val",
        from,
        to,
    }
}

/// Direction-neutral values, by the element that carries them.
///
/// Three elements and three simple types, checked against `strict/wml.xsd` and
/// not against recollection:
///
/// * `w:jc/@w:val` is `ST_Jc` inside `w:pPr` and `w:rPr` and `ST_JcTable`
///   inside `w:tblPr`. Both lost `left`/`right` — `ST_Jc` is
///   `{start, center, end, both, mediumKashida, distribute, numTab, highKashida,
///   lowKashida, thaiDistribute}` and `ST_JcTable` is `{center, end, start}` —
///   so one entry serves both, and the audit's claim that `ST_JcTable` needs a
///   "separate entry keyed by `(element, type)`" was wrong for the same reason
///   it was right about the key needing the element.
/// * `w:tab/@w:val` is `ST_TabJc`, which is `{clear, start, center, end,
///   decimal, bar, num}` — no `left`, no `right`.
/// * `w:lvlJc/@w:val` is `ST_Jc`, same as `w:jc`.
pub const JC_VALUES: &[ValueMap<'static>] = &[
    jc("jc", "left", "start"),
    jc("jc", "right", "end"),
    jc("tab", "left", "start"),
    jc("tab", "right", "end"),
    jc("lvlJc", "left", "start"),
    jc("lvlJc", "right", "end"),
];

/// Whether `attribute`/`value` has a Strict spelling in the tables above.
#[must_use]
pub fn map_value(element: &str, attribute: &str, value: &str) -> Option<&'static str> {
    JC_VALUES
        .iter()
        .find(|entry| {
            entry.element == element && entry.attribute == attribute && entry.from == value
        })
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
/// The rule is the one above the table: only what Strict does not declare.
///
/// The split the table used to draw - "editor state" against "content" - is a
/// question about **rendering**, and answering it by deletion answered it
/// wrongly. A real Word document puts dozens of editor-state elements into
/// `settings.xml`; that is an argument for not *failing* on them, which the
/// severity scale already provides, and not for deleting them.
pub const REMOVALS: &[Removal] = &[
    // ---- Transitional-only: Strict does not declare these at all ----------
    //
    // The rule is narrow on purpose: **an element is removed only when the
    // Strict schema does not declare it.** Anything Strict does declare stays,
    // whether or not a renderer acts on it - "we do not act on it" is a support
    // question the Feature Report answers, and deleting the element converts a
    // declared feature into a loss that is not one.
    //
    // The previous version of this table broke that rule for 42 of its 46
    // entries, at a measurable cost: `w:compat` is declared by Strict as
    // `CT_Settings` position 89 with eight legal children, and removing it put
    // 56 of 58 corpus documents into `lossy` for nothing. `w:object` and `w:ptab`
    // are declared in Strict's `EG_RunInnerContent`, so removing them was
    // losing content. `w:defaultTabStop` affects the page and was filed under
    // "editor state". Every entry below was checked against the official
    // `wml.xsd` of ECMA-376 5th edition Part 1, not from recollection.
    wml(
        "pict",
        Severity::Lossy,
        "VML picture; Strict declares no w:pict, and the picture is lost with it",
    ),
    wml(
        "legacyDrawing",
        Severity::Lossy,
        "VML drawing reference; not in Strict",
    ),
    wml(
        "legacyDrawingHF",
        Severity::Lossy,
        "VML header/footer drawing reference; not in Strict",
    ),
    wml(
        "useFELayout",
        Severity::Ignorable,
        "CJK layout switch; Strict's CT_Compat does not declare it",
    ),
    // The other two children the corpus puts inside `w:compat` that Strict's
    // `CT_Compat` does not declare. Same shape as `useFELayout` above, same
    // reason, and they were absent from this table while being absent from the
    // model too - so they were not removed AND not named, which is the defect
    // reaudit П-2 measured: seven documents each, disappearing silently.
    wml(
        "doNotWrapTextWithPunct",
        Severity::Ignorable,
        "CJK punctuation wrapping switch; Strict's CT_Compat does not declare it",
    ),
    wml(
        "doNotUseEastAsianBreakRules",
        Severity::Ignorable,
        "CJK line-break rules switch; Strict's CT_Compat does not declare it",
    ),
    // ---- the three that were missing altogether ---------------------------
    wml(
        "shapeDefaults",
        Severity::Ignorable,
        "Transitional-only default shape properties; Strict does not declare it",
    ),
    wml(
        "hdrShapeDefaults",
        Severity::Ignorable,
        "Transitional-only header shape defaults; Strict does not declare it",
    ),
    wml(
        "relyOnVML",
        Severity::Ignorable,
        "Transitional-only VML reliance flag in webSettings; not in Strict",
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

/// The Transitional `w:tblLook/@w:val` bit mask, bit for bit.
///
/// Strict's `CT_TblLook` declares six `s:ST_OnOff` attributes and **no `w:val`**
/// at all, so the whole bit mask is a Transitional spelling. 838 occurrences in
/// 14 corpus documents, and the conversion is arithmetic, not a judgement: each
/// bit is a documented flag, and the flags are the same six the attributes are
/// named after.
pub const TBL_LOOK_BITS: &[(u32, &str)] = &[
    (0x0020, "firstRow"),
    (0x0040, "lastRow"),
    (0x0080, "firstColumn"),
    (0x0100, "lastColumn"),
    (0x0200, "noHBand"),
    (0x0400, "noVBand"),
];

/// Vendor namespaces the writer **reproduces from the model** rather than drops.
///
/// The complement of [`IGNORABLE_EXTENSION_NAMESPACES`], and the reason T6's
/// "does this consumer understand the namespace" question has an answer that is
/// not "is it in ECMA-376". `wps` and `wpg` carry shapes and groups that this
/// project reads and writes back — ADR-0014 keeps them as the `XS-18`/`XS-19`
/// debt on purpose rather than dropping the content — so an `mc:Choice
/// Requires="wps"` names something we handle, and MCE says to take it.
///
/// Every namespace here is declared in `VENDOR_NAMESPACES`'s sense: it is not
/// ECMA-376, and it is not Transitional either, so a package that carries one is
/// neither Strict nor Transitional until MCE has run. That is exactly the
/// situation `mc:AlternateContent` exists for.
pub const REPRODUCED_EXTENSION_NAMESPACES: &[&str] = &[
    "http://schemas.microsoft.com/office/word/2010/wordprocessingShape",
    "http://schemas.microsoft.com/office/word/2010/wordprocessingGroup",
    "http://schemas.microsoft.com/office/word/2010/wordprocessingCanvas",
    "http://schemas.microsoft.com/office/word/2010/wordprocessingInk",
    "http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing",
    "http://schemas.microsoft.com/office/word/2012/wordprocessingDrawing",
    "http://schemas.microsoft.com/office/drawing/2010/main",
    "http://schemas.microsoft.com/office/drawing/2014/main",
];

/// `ST_MeasurementOrPercent` carriers: the element, and whether it is a bare
/// `w:@w:w` or an edge of a margin or border container.
///
/// Fifteen thousand bare twips in the corpus, and Strict's
/// `ST_MeasurementOrPercent` is `union(ST_DecimalNumberOrPercent,
/// s:ST_UniversalMeasure)` while `ST_UniversalMeasure`'s pattern is
/// `-?[0-9]+(\.[0-9]+)?(mm|cm|in|pt|pc|pi)` - **twip is not a unit it accepts**.
/// So there is no spelling to add; the number has to be converted. One twip is
/// 1/20 of a point, which is exact in two decimals, so the conversion loses
/// nothing: 2200 becomes 110pt and 17663 becomes 883.15pt.
///
/// The four containers are listed by name because an edge is only a measure in
/// one of them. `w:left` inside `w:ind` is `ST_TwipsMeasure` and stays a bare
/// number; `w:left` inside `w:tblCellMar` is not.
pub const MEASURE_OR_PERCENT: &[(&str, bool)] = &[
    // (name, is_container): a container is matched against the PARENT, because
    // the measure is on one of its edges; the rest are matched against the element
    // that holds `@w:w` directly. The flag is `is_container`, and it was written
    // backwards the first time - which is why the rule fired nine times on the
    // corpus and the nine were the only cases where an edge happened to be named
    // like a container's child.
    ("tblW", false),
    ("tcW", false),
    ("tblInd", false),
    ("wBefore", false),
    ("wAfter", false),
    ("tblCellSpacing", false),
    ("tblBorders", true),
    ("tcBorders", true),
    ("tblCellMar", true),
    ("tcMar", true),
];

/// Twips to the universal measure Strict accepts, or `None` when the value is
/// already conformant.
///
/// Returns `None` for anything carrying a unit or a `%`, so a producer that got
/// this right is left alone, and for a value that is not a bare number at all -
/// which the XSD gate then names, rather than this guessing.
#[must_use]
pub fn twips_to_universal(value: &str) -> Option<String> {
    if value.is_empty()
        || value.contains('%')
        || value.contains(|c: char| c.is_ascii_alphabetic() || c == ':')
    {
        return None;
    }
    let twips: i64 = value.parse().ok()?;
    // Exactly two decimal places, because one twip is 1/20 of a point and 1/100
    // of a point is 1/500 twip - so hundredths are the finest unit that can
    // represent every twip value without loss. The trailing zero is then trimmed,
    // because `5.40pt` and `5.4pt` are the same number and a document that reads
    // `5.4pt` is the one a producer would have written.
    let hundredths = twips * 5;
    let whole = hundredths / 100;
    let rest = (hundredths % 100).abs();
    if rest == 0 {
        return Some(format!("{whole}pt"));
    }
    let fraction = format!("{rest:02}");
    Some(format!("{whole}.{}pt", fraction.trim_end_matches('0')))
}

/// `ST_TextScale`: a bare number where Strict's pattern wants a `%`.
///
/// `ST_TextScalePercent` is `0*(600|([0-5]?[0-9]?[0-9]))%`, so 82 and 135 both
/// need the sign and 601 would need clamping - which this does not do, because a
/// value outside the set is a measurement for the XSD gate and not a repair.
#[must_use]
pub fn text_scale_percent(value: &str) -> Option<String> {
    if value.ends_with('%') {
        return None;
    }
    let number: i64 = value.parse().ok()?;
    number.checked_mul(0)?;
    Some(format!("{number}%"))
}

/// `ST_DecimalNumberOrPercent` on `w:zoom/@w:percent`, the same shape as
/// [`text_scale_percent`] and kept separate because they are different types on
/// different elements and a rule that silently grew to cover both would stop
/// being checkable against either.
#[must_use]
pub fn decimal_or_percent(value: &str) -> Option<String> {
    text_scale_percent(value)
}

/// `DrawingML` `ST_Percentage` / `ST_PositiveFixedPercentage` as Transitional
/// thousandths of a percent → Strict percentage with a `%` sign.
///
/// `65000` becomes `65%`, `10` becomes `0.010%`, `0` becomes `0%`. Values that
/// already carry `%` are left alone.
#[must_use]
pub fn drawingml_thousandths_percent(value: &str) -> Option<String> {
    if value.ends_with('%') {
        return None;
    }
    let number: i64 = value.parse().ok()?;
    if number == 0 {
        return Some(String::from("0%"));
    }
    let sign = if number < 0 { "-" } else { "" };
    let absolute = number.unsigned_abs();
    let whole = absolute / 1000;
    let fraction = absolute % 1000;
    if fraction == 0 {
        Some(format!("{sign}{whole}%"))
    } else {
        Some(format!("{sign}{whole}.{fraction:03}%"))
    }
}

/// `EG_ColorTransform` members whose schema type has required `@val` of a
/// percentage (`CT_Percentage` / `CT_PositivePercentage` /
/// `CT_FixedPercentage` / `CT_PositiveFixedPercentage`).
///
/// `a:hue` is `CT_PositiveFixedAngle` and `a:hueOff` is `CT_Angle`: those
/// stay integers in 60,000ths of a degree. `comp` / `inv` / `gray` / `gamma` /
/// `invGamma` are empty types and have no `@val`.
const COLOR_TRANSFORM_PERCENT_VAL: &[&str] = &[
    "tint", "shade", "alpha", "alphaOff", "alphaMod", "hueMod", "sat", "satOff", "satMod", "lum",
    "lumOff", "lumMod", "red", "redOff", "redMod", "green", "greenOff", "greenMod", "blue",
    "blueOff", "blueMod",
];

/// Whether `(element, attribute)` carries a `DrawingML` percentage stored as
/// thousandths in Transitional packages.
///
/// The decision is the schema type of that pair, not a shared name. `a:hue/@val`
/// and `a:lin/@ang` are angles even though the digits look like thousandths.
#[must_use]
pub fn is_drawingml_percentage_attr(element: &str, attribute: &str) -> bool {
    if attribute == "val" && COLOR_TRANSFORM_PERCENT_VAL.contains(&element) {
        return true;
    }
    if element == "gs" && attribute == "pos" {
        return true;
    }
    // `CT_RelativeRect`: fillToRect, fillRect, srcRect, tileRect.
    if matches!(element, "fillToRect" | "fillRect" | "srcRect" | "tileRect")
        && matches!(attribute, "l" | "t" | "r" | "b")
    {
        return true;
    }
    // `CT_TextSpacingPercent/@val` (`a:spcPct` in line and paragraph spacing),
    // `CT_TextBulletSizePercent/@val` (`a:buSzPct`) and
    // `CT_LineJoinMiterProperties/@lim` (`a:miter`): thousandths in
    // Transitional as well - 1161 of the census's schema violations were
    // `a:spcPct` alone.
    if matches!(
        (element, attribute),
        ("spcPct" | "buSzPct", "val") | ("miter", "lim")
    ) {
        return true;
    }
    // The blip and shape effects the writer now keeps as markup: luminance,
    // alpha, bi-level, HSL and tint amounts, and the shadow and reflection
    // scales and stops. Their angles (`hue`, `kx`, `dir`, `fadeDir`) are not
    // percentages.
    if matches!(
        (element, attribute),
        ("lum", "bright" | "contrast")
            | ("alphaModFix" | "tint", "amt")
            | ("alphaBiLevel" | "biLevel", "thresh")
            | ("alphaRepl", "a")
            | ("hsl", "sat" | "lum")
            | ("outerShdw", "sx" | "sy")
            | (
                "reflection",
                "stA" | "stPos" | "endA" | "endPos" | "sx" | "sy"
            )
    ) {
        return true;
    }
    // `CT_TextCharacterProperties/@baseline` is `ST_Percentage`.
    matches!(element, "defRPr" | "rPr" | "endParaRPr") && attribute == "baseline"
}

/// Whether a chart attribute holds a whole percent - `150`, not thousandths -
/// that Strict spells as a percent string (`150%`): the gap, overlap, label
/// offset, hole, pie, bubble and 3-D depth/height amounts of `c:` charts.
#[must_use]
pub fn is_chart_whole_percent_attr(element: &str, attribute: &str) -> bool {
    attribute == "val"
        && matches!(
            element,
            "gapWidth"
                | "gapDepth"
                | "overlap"
                | "lblOffset"
                | "holeSize"
                | "secondPieSize"
                | "bubbleScale"
                | "depthPercent"
                | "hPercent"
        )
}

/// `150` as `150%`; `None` for a value that already has the sign or is no
/// integer.
#[must_use]
pub fn whole_percent(value: &str) -> Option<String> {
    if value.ends_with('%') {
        return None;
    }
    let number: i64 = value.trim().parse().ok()?;
    Some(format!("{number}%"))
}

/// Whether an element/parent pair carries a bare `ST_MeasurementOrPercent`.
#[must_use]
pub fn is_measure_carrier(element: &str, parent: &str) -> bool {
    MEASURE_OR_PERCENT.iter().any(|(name, is_container)| {
        if *is_container {
            *name == parent
        } else {
            *name == element
        }
    })
}

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
    use super::{
        drawingml_thousandths_percent, is_chart_whole_percent_attr, is_drawingml_percentage_attr,
        map_value, removal_for, rename_attribute, rename_element, whole_percent, REMOVALS, RENAMES,
    };
    use crate::normalize::report::Severity;

    /// The negative half of the T3 table, and the half that needs the gate.
    ///
    /// Strict's own `wml.xsd` is the oracle, and it disagrees with
    /// `TZ-STRICT-OOXML-RUST.md` §10.4 on three containers whose names are the
    /// same `left`/`right` the other four containers renamed. Applying the
    /// rename there would produce a document that validates as Strict and lays
    /// out with no paragraph borders, no page borders and no page margins at all
    /// — a silent, total loss of layout on every page, which is a far worse
    /// defect than the invalid document it replaced.
    #[test]
    fn no_direction_neutral_rename_without_its_parent() {
        for parent in [
            // CT_PBdr: top, left, bottom, right, between, bar.
            "pBdr",
            // CT_PageBorders: top, left, bottom, right.
            "pgBorders",
            // CT_PageMar: @w:top/@w:right/@w:bottom/@w:left, all use="required".
            "pgMar",
        ] {
            assert_eq!(
                rename_element(parent, "left"),
                None,
                "{parent}/w:left is still `left` in Strict's CT_ table"
            );
            assert_eq!(
                rename_element(parent, "right"),
                None,
                "{parent}/w:right is still `right` in Strict's CT_ table"
            );
        }
        // CT_TblPPr keeps `leftFromText`/`rightFromText` too, which the same
        // section of the spec wants renamed.
        assert_eq!(rename_attribute("tblpPr", "leftFromText"), None);
        assert_eq!(rename_attribute("tblpPr", "rightFromText"), None);
    }

    /// Every applied rename is a *conditional* one, and every container whose
    /// `left`/`right` Strict did rename has an entry. The second half is what
    /// makes the first meaningful: a table that renamed nothing would also pass
    /// the test above, and that is the state the project was in until 2026-10-01.
    #[test]
    fn the_four_renamed_containers_all_have_both_edges() {
        for parent in ["tblBorders", "tcBorders", "tblCellMar", "tcMar"] {
            assert_eq!(rename_element(parent, "left"), Some("start"), "{parent}");
            assert_eq!(rename_element(parent, "right"), Some("end"), "{parent}");
        }
        assert_eq!(rename_attribute("ind", "left"), Some("start"));
        assert_eq!(rename_attribute("ind", "right"), Some("end"));
        assert!(
            RENAMES.iter().all(|entry| entry.verified),
            "an applied entry that no test verified"
        );
    }

    /// `verified` is a gate, not a comment, and it is only meaningful if the
    /// table can hold an unverified entry. The point of the test is that the
    /// field exists and is read by `rename_element`/`rename_attribute` - which is
    /// why those two are the only way to ask the table anything.
    #[test]
    fn an_unverified_entry_is_never_applied() {
        for entry in RENAMES {
            let applied = if entry.attribute {
                rename_attribute(entry.parent, entry.from)
            } else {
                rename_element(entry.parent, entry.from)
            };
            assert_eq!(applied, Some(entry.to), "{} is applied", entry.note);
        }
    }

    #[test]
    fn direction_neutral_values_are_mapped() {
        assert_eq!(map_value("jc", "val", "left"), Some("start"));
        assert_eq!(map_value("jc", "val", "right"), Some("end"));
        assert_eq!(map_value("tab", "val", "left"), Some("start"));
        assert_eq!(map_value("lvlJc", "val", "left"), Some("start"));
    }

    #[test]
    fn values_strict_keeps_are_not_mapped() {
        assert_eq!(map_value("jc", "val", "center"), None);
        assert_eq!(map_value("jc", "val", "both"), None);
        assert_eq!(map_value("jc", "val", "distribute"), None);
        assert_eq!(map_value("tab", "val", "clear"), None);
    }

    #[test]
    fn the_value_map_only_applies_to_its_own_element() {
        // `left` is a legal Strict value of `ST_PTabAlignment`, so a table keyed
        // without the element would rename a `w:ptab` alignment into something
        // outside its own simple type. This is the test that holds the element
        // column; the key-on-attribute bug of queue item 2 is held by
        // `t4_reaches_st_jc_values_and_writes_the_strict_spelling`.
        assert_eq!(map_value("ptab", "alignment", "left"), None);
        assert_eq!(map_value("jc", "textAlignment", "left"), None);
    }

    #[test]
    fn only_what_strict_does_not_declare_is_removed() {
        // This is the rule, and it is the one the old table broke 42 times.
        // `w:compat` is declared by Strict (`CT_Settings`, position 89) and
        // carries eight legal children; `w:object` and `w:ptab` are declared
        // in `EG_RunInnerContent`; `w:defaultTabStop` changes the page.
        for legal in [
            "compat",
            "object",
            "ptab",
            "defaultTabStop",
            "stylePaneFormatFilter",
            "drawingGridHorizontalSpacing",
            "bordersDoNotSurroundHeader",
            "rsids",
            "mailMerge",
            "documentProtection",
            "attachedTemplate",
            "updateFields",
            "embedTrueTypeFonts",
            "doNotExpandShiftReturn",
        ] {
            assert!(
                removal_for(legal).is_none(),
                "{legal} is declared by Strict and must not be removed"
            );
        }
        // And what Strict does not declare is removed, classified by whether the
        // page moves.
        assert_eq!(
            removal_for("pict").map(|entry| entry.severity),
            Some(Severity::Lossy),
            "a VML picture is content"
        );
        assert_eq!(
            removal_for("legacyDrawing").map(|entry| entry.severity),
            Some(Severity::Lossy)
        );
        assert_eq!(
            removal_for("useFELayout").map(|entry| entry.severity),
            Some(Severity::Ignorable),
            "a CJK layout switch is not content"
        );
        assert_eq!(
            removal_for("relyOnVML").map(|entry| entry.severity),
            Some(Severity::Ignorable)
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

    #[test]
    fn the_census_carriers_are_drawingml_percentages() {
        assert!(is_drawingml_percentage_attr("spcPct", "val"));
        assert!(is_drawingml_percentage_attr("buSzPct", "val"));
        assert!(is_drawingml_percentage_attr("miter", "lim"));
        assert!(!is_drawingml_percentage_attr("spcPts", "val"));
        assert!(is_drawingml_percentage_attr("lum", "contrast"));
        assert!(is_drawingml_percentage_attr("reflection", "endPos"));
        assert!(is_drawingml_percentage_attr("outerShdw", "sx"));
        assert!(!is_drawingml_percentage_attr("outerShdw", "kx"));
        assert!(!is_drawingml_percentage_attr("hsl", "hue"));
        assert_eq!(
            drawingml_thousandths_percent("20000").as_deref(),
            Some("20%")
        );
        assert_eq!(
            drawingml_thousandths_percent("95000").as_deref(),
            Some("95%")
        );
        assert_eq!(
            drawingml_thousandths_percent("800000").as_deref(),
            Some("800%")
        );
    }

    #[test]
    fn chart_amounts_are_whole_percents() {
        assert!(is_chart_whole_percent_attr("gapWidth", "val"));
        assert!(is_chart_whole_percent_attr("overlap", "val"));
        assert!(is_chart_whole_percent_attr("lblOffset", "val"));
        assert!(!is_chart_whole_percent_attr("gapWidth", "lim"));
        assert_eq!(whole_percent("150").as_deref(), Some("150%"));
        assert_eq!(whole_percent("-100").as_deref(), Some("-100%"));
        assert_eq!(whole_percent("100%"), None);
        assert_eq!(whole_percent("wide"), None);
    }

    #[test]
    fn drawingml_thousandths_percent_converts_bare_thousandths() {
        assert_eq!(drawingml_thousandths_percent("0").as_deref(), Some("0%"));
        assert_eq!(
            drawingml_thousandths_percent("65000").as_deref(),
            Some("65%")
        );
        assert_eq!(
            drawingml_thousandths_percent("1253").as_deref(),
            Some("1.253%")
        );
        assert_eq!(drawingml_thousandths_percent("65%"), None);
        assert_eq!(
            drawingml_thousandths_percent("-65000").as_deref(),
            Some("-65%")
        );
        assert_eq!(drawingml_thousandths_percent("nope"), None);
        assert_eq!(drawingml_thousandths_percent("999999999999999999999"), None);
        assert!(is_drawingml_percentage_attr("lumMod", "val"));
        assert!(is_drawingml_percentage_attr("sat", "val"));
        assert!(is_drawingml_percentage_attr("hueMod", "val"));
        assert!(is_drawingml_percentage_attr("gs", "pos"));
        assert!(is_drawingml_percentage_attr("tileRect", "l"));
        assert!(is_drawingml_percentage_attr("defRPr", "baseline"));
        assert!(!is_drawingml_percentage_attr("defRPr", "sz"));
        assert!(
            !is_drawingml_percentage_attr("hue", "val"),
            "a:hue/@val is ST_PositiveFixedAngle"
        );
        assert!(
            !is_drawingml_percentage_attr("hueOff", "val"),
            "a:hueOff/@val is ST_Angle"
        );
        assert!(!is_drawingml_percentage_attr("comp", "val"));
        assert!(!is_drawingml_percentage_attr("gamma", "val"));
        assert!(
            !is_drawingml_percentage_attr("lin", "ang"),
            "a:lin/@ang is an angle, not a percentage"
        );
        assert!(!is_drawingml_percentage_attr("hslClr", "hue"));
    }
}
