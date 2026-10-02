//! Document settings model (`settings.xml`).

use std::sync::Arc;

use super::ids::StyleId;
use super::notes::NoteProperties;
use super::values::Twips;

/// The document zoom level (`w:zoom`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Zoom {
    /// Zoom percentage.
    pub percent: Option<u16>,
    /// Zoom preset kind (`w:val`), for example `fullPage`.
    pub kind: Option<DocumentZoom>,
}

/// `ST_Zoom` kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DocumentZoom {
    /// `none`.
    None,
    /// `fullPage`.
    FullPage,
    /// `bestFit`.
    BestFit,
    /// `textFit`.
    TextFit,
}

impl DocumentZoom {
    /// Parses a Strict zoom kind.
    #[must_use]
    pub fn from_strict(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "fullPage" => Some(Self::FullPage),
            "bestFit" => Some(Self::BestFit),
            "textFit" => Some(Self::TextFit),
            _ => None,
        }
    }
}

/// The subset of `settings.xml` retained by Stage 2.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Settings {
    /// Default tab stop in twips (`w:defaultTabStop`).
    pub default_tab_stop: Option<Twips>,
    /// Zoom (`w:zoom`).
    pub zoom: Option<Zoom>,
    /// Different headers/footers for even pages (`w:evenAndOddHeaders`).
    pub even_and_odd_headers: bool,
    /// Display background shapes in print layout (`w:displayBackgroundShape`).
    pub display_background_shape: bool,
    /// Hide spelling errors (`w:hideSpellingErrors`).
    pub hide_spelling_errors: bool,
    /// Hide grammatical errors (`w:hideGrammaticalErrors`).
    pub hide_grammatical_errors: bool,
    /// Show proofing marks (`w:proofState`).
    pub proofing: bool,
    /// Track revisions (`w:trackRevisions`).
    pub track_revisions: bool,
    /// Do not hyphenate automatically (`w:doNotHyphenateCaps`).
    pub do_not_hyphenate_caps: bool,
    /// Auto-hyphenation zone in twips (`w:autoHyphenation` / `w:hyphenationZone`).
    pub auto_hyphenation: bool,
    /// Hyphenation zone in twips (`w:hyphenationZone`).
    pub hyphenation_zone: Option<Twips>,
    /// Document protection mode (`w:documentProtection/@w:edit`).
    pub document_protection: Option<Arc<str>>,
    /// Default paragraph style (`w:stylePaneFormatFilter` is unrelated; from the
    /// default-styles cascade this is `w:defaultTabStop`'s sibling).
    pub default_style: Option<StyleId>,
    /// Decimal symbol used for lists (`w:decimalSymbol`).
    pub decimal_symbol: Option<Arc<str>>,
    /// List separator (`w:listSeparator`).
    pub list_separator: Option<Arc<str>>,
    /// Compatibility settings (`w:compat`), recorded as raw key/value pairs.
    pub compatibility: Vec<(Arc<str>, Arc<str>)>,
    /// Theme font languages (`w:themeFontLang`).
    pub theme_font_lang: Option<Arc<str>>,
    /// Footnote properties (`w:footnotePr`).
    pub footnote_properties: NoteProperties,
    /// Endnote properties (`w:endnotePr`).
    pub endnote_properties: NoteProperties,
    /// Click-and-type settings are ignored in Stage 2.
    pub click_and_type: bool,
    /// Recognise the document as having a "mirror margins" preference.
    pub mirror_margins: bool,
    /// Put the gutter at the top of the page rather than the side (`w:gutterAtTop`).
    ///
    /// **A document setting, not a section one.** Strict's `EG_SectPrContents`
    /// has no `gutterAtTop` slot - `w:settings` declares it, at position 20 of
    /// `CT_Settings` - while Transitional's `EG_SectPrContents` does. So a
    /// Transitional document carries it inside `w:sectPr`, and a Strict one carries
    /// it in `settings.xml`; it is the same flag and the parser accepts both
    /// spellings, which is why this field is on [`Settings`] and not on
    /// [`crate::model::props::SectionProperties`]. The writer emitting it into
    /// `w:sectPr` produced a document that fails on
    /// "This element is not expected" for a flag that has a legal Strict home.
    pub gutter_at_top: bool,
    /// OMML maths properties (`m:mathPr`).
    ///
    /// This is the one child of `w:settings` that lives in another namespace,
    /// and the settings parser skips everything that is not `wml` — so the block
    /// was recorded as foreign markup and dropped. Forty corpus documents lost
    /// twelve elements each, none of them named. Queue item 16 wrote this down
    /// twice and measured it on the corpus neither time, because the instrument
    /// it used filtered by the `wml` namespace and could not see what it was
    /// looking for.
    pub math_properties: Option<MathProperties>,
}

/// `m:mathPr` and its children, in the order `CT_MathPr` declares them.
///
/// Order is not decoration here. The type is an `xsd:sequence`, so the writer has
/// to emit these in exactly this order or the part fails on "This element is not
/// expected" — the same failure as `XS-15`, which is OMML order elsewhere. The
/// fields are therefore in schema order rather than in a `BTreeMap`, because a
/// map would sort them alphabetically and `brkBinSub` would precede `brkBin`.
///
/// `m:wrapIndent` and `m:wrapRight` sit between `m:intraSp` and `m:intLim`
/// because `CT_MathPr` nests them in an `xsd:choice` at that point, which a
/// scan of the sequence's direct children does not reach.
///
/// The corpus carries eleven of the sixteen: `m:preSp`, `m:postSp`, `m:interSp`,
/// `m:intraSp` and `m:wrapRight` never appear. All sixteen are modelled so a
/// document that does carry them keeps them.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct MathProperties {
    /// `m:mathFont` — the maths font, `Cambria Math` on every corpus document.
    pub math_font: Option<Arc<str>>,
    /// `m:brkBin` — where a binary operator may break.
    pub break_binary_operator: Option<Arc<str>>,
    /// `m:brkBinSub` — how a binary operator breaks in a subscript.
    pub break_binary_sub: Option<Arc<str>>,
    /// `m:smallFrac` — render fractions smaller than a line at line height.
    pub small_fraction: Option<Arc<str>>,
    /// `m:dispDef` — display the equation by default.
    pub display_default: Option<Arc<str>>,
    /// `m:lMargin` — left margin of an equation, twips.
    pub left_margin: Option<Arc<str>>,
    /// `m:rMargin` — right margin of an equation, twips.
    pub right_margin: Option<Arc<str>>,
    /// `m:defJc` — default justification of an equation.
    pub default_justification: Option<Arc<str>>,
    /// `m:preSp` — space before the equation, twips.
    pub pre_space: Option<Arc<str>>,
    /// `m:postSp` — space after the equation, twips.
    pub post_space: Option<Arc<str>>,
    /// `m:interSp` — space between equation lines, twips.
    pub inter_space: Option<Arc<str>>,
    /// `m:intraSp` — space between equation lines when they wrap.
    pub intra_space: Option<Arc<str>>,
    /// `m:wrapIndent` — indent the wrapped part of an equation, twips.
    ///
    /// Declared inside an `xsd:choice` with [`wrap_right`](Self::wrap_right) and
    /// between `m:intraSp` and `m:intLim`. Two things about that are worth
    /// writing down: it is a CHOICE, so the two are mutually exclusive, and it is
    /// NESTED one level below the sequence, so a scan that reads the sequence's
    /// direct children misses it - which is how the model first declared
    /// `m:intLim` as the thirteenth child and would have written a valid
    /// document with the wrong content.
    pub wrap_indent: Option<Arc<str>>,
    /// `m:wrapRight` — wrap an equation to the right instead of indenting.
    ///
    /// The other arm of the same choice, so a document may carry this or
    /// [`wrap_indent`](Self::wrap_indent) but never both.
    pub wrap_right: Option<Arc<str>>,
    /// `m:intLim` — where an n-ary limit goes by default.
    pub integral_limit: Option<Arc<str>>,
    /// `m:naryLim` — where a non-integral n-ary limit goes by default.
    pub nary_limit: Option<Arc<str>>,
}
