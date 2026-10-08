//! Document settings model (`settings.xml`).

use std::sync::Arc;

use super::ids::StyleId;
use super::notes::NoteProperties;
use super::values::Twips;

/// `w:themeFontLang` — the three script languages `CT_Language` carries.
///
/// `val` is the Latin/ascii language. `eastAsia` and `bidi` select the theme
/// font used for those scripts. Dropping either one changes which face a theme
/// reference resolves to.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ThemeFontLang {
    /// `w:val`.
    pub val: Option<Arc<str>>,
    /// `w:eastAsia`.
    pub east_asia: Option<Arc<str>>,
    /// `w:bidi`.
    pub bidi: Option<Arc<str>>,
}

impl ThemeFontLang {
    /// Returns `true` when none of the three languages is set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.val.is_none() && self.east_asia.is_none() && self.bidi.is_none()
    }
}

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
    /// Explicit `w:evenAndOddHeaders w:val="false"`. Absence is neither flag.
    pub even_and_odd_headers_off: bool,
    /// Display background shapes in print layout (`w:displayBackgroundShape`).
    pub display_background_shape: bool,
    /// Hide spelling errors (`w:hideSpellingErrors`).
    pub hide_spelling_errors: bool,
    /// Hide grammatical errors (`w:hideGrammaticalErrors`).
    pub hide_grammatical_errors: bool,
    /// Proofing state (`w:proofState` spelling/grammar), AUD-44.
    pub proof_state: Option<ProofState>,
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
    /// The on/off children of `w:compat` (`w:compatSetting` is above).
    ///
    /// The reader used to collect `w:compatSetting` and drop the flags, so five
    /// children that `CT_Compat` declares - thirty-three occurrences across the
    /// corpus - left the package without a word in the report. A `w:compat` that
    /// survives with its key/values and none of its switches is a document whose
    /// line breaking has silently changed.
    pub compat_flags: CompatFlags,
    /// Theme font languages (`w:themeFontLang`: `val`, `eastAsia`, `bidi`).
    pub theme_font_lang: Option<ThemeFontLang>,
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
    /// Theme colour mapping (`w:clrSchemeMapping`).
    ///
    /// The one `w:settings` child that changes how the page looks, and it was
    /// dropped from forty-eight of the fifty-eight corpus documents. `clrSchemeMapping`
    /// maps the colour names a document uses onto theme slots, so losing it loses
    /// a recolouring: a document whose links were mapped to `accent2` comes back
    /// with Word's defaults, and nothing in the report or in any schema message
    /// says so.
    pub color_scheme_mapping: Option<ColorSchemeMapping>,
    /// Revision save identifiers (`w:rsids`).
    pub revision_save_ids: Option<RevisionSaveIds>,
    /// Character spacing compression (`w:characterSpacingControl`).
    ///
    /// One of three values: `doNotCompress`, `compressPunctuation`,
    /// `compressPunctuationAndJapaneseKana`. All forty corpus documents set it
    /// and all forty lost it. It decides whether punctuation is compressed for
    /// CJK line breaking, so it is a layout setting and not editor state.
    pub character_spacing_control: Option<Arc<str>>,
    /// Document view (`w:view`): `none`, `print`, `outline`, `masterPages`,
    /// `normal`, `web`.
    pub view: Option<Arc<str>>,
    /// Line-breaking exceptions (`w:noLineBreaksAfter`, `w:noLineBreaksBefore`),
    /// as (language, characters) pairs.
    ///
    /// Japanese kinsoku rules: characters that may not end a line and characters
    /// that may not begin one. One corpus document carries both.
    pub no_line_breaks_after: Vec<(Arc<str>, Arc<str>)>,
    /// Characters that may not begin a line, per language.
    pub no_line_breaks_before: Vec<(Arc<str>, Arc<str>)>,
    /// Document variables (`w:docVars`), as (name, value) pairs.
    pub document_variables: Vec<(Arc<str>, Arc<str>)>,
    /// Flags this document sets and the corpus would otherwise lose.
    ///
    /// One flat map rather than thirty named fields, because they are thirty
    /// instances of ONE shape: a `CT_OnOff` child of `w:settings` with no payload
    /// beyond on/off. `CT_OnOff` is `union(xsd:boolean)` with an `ST_OnOff`
    /// enumeration layered on it, so three spellings mean "on" - a bare element,
    /// `w:val="true"` and `w:val="1"` - and two mean "off".
    ///
    /// The value is kept as the document wrote it, because writing a bare element
    /// for an input that said `w:val="0"` turns an off flag ON. That is not a
    /// cosmetic difference for `w:embedTrueTypeFonts`, which decides whether the
    /// embedded font parts exist at all.
    pub on_off_flags: Vec<(Arc<str>, Option<Arc<str>>)>,
    /// Numeric settings this document sets: name to the string as written.
    ///
    /// `w:drawingGridHorizontalSpacing`, `w:displayHorizontalDrawingGridEvery`
    /// and their vertical counterparts are four different types -
    /// `ST_TwipsMeasure`, `ST_DecimalNumber`, and two more - and they all round
    /// trip as the producer's own text. Keeping the text means a value this
    /// project does not model is still carried rather than rounded away.
    pub numeric_settings: Vec<(Arc<str>, Arc<str>)>,
    /// `w:documentProtection`'s attributes beyond `w:edit`.
    ///
    /// `@w:edit` is the mode and lives on its own field; the corpus writes
    /// `@w:enforcement="0"` instead, which is a different attribute with a
    /// different meaning - whether the protection is in force at all. A model
    /// that reads only `w:edit` sees an element with nothing on it and writes
    /// nothing, which is how ten documents lost the element.
    pub document_protection_attributes: Vec<(Arc<str>, Arc<str>)>,
    /// `w:stylePaneFormatFilter`'s fourteen boolean attributes.
    ///
    /// It is a filter over what the style pane lists, so it is editor state by
    /// any reading - and it is kept, for the reason `REMOVALS` documents.
    pub style_pane_filter: Vec<(Arc<str>, Arc<str>)>,
    /// `w:revisionView`'s four boolean attributes: `markup`, `comments`,
    /// `insDel`, `formatting`.
    pub revision_view: Vec<(Arc<str>, Arc<str>)>,
    /// `w:attachedTemplate/@r:id`, the relationship to the template part.
    ///
    /// Kept as the relationship id rather than resolved: the writer has to emit
    /// the relationship as well, and a template part the package does not carry
    /// is a part-level question this model does not own.
    pub attached_template: Option<Arc<str>>,
}

/// Spelling/grammar cleanliness from `w:proofState` (AUD-44).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ProofState {
    /// Spelling state (`@w:spelling`).
    pub spelling: Option<ProofCleanliness>,
    /// Grammar state (`@w:grammar`).
    pub grammar: Option<ProofCleanliness>,
}

/// `clean` / `dirty` for [`ProofState`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProofCleanliness {
    /// `clean`.
    Clean,
    /// `dirty`.
    Dirty,
}

impl ProofCleanliness {
    /// Parses a Strict lexical value.
    #[must_use]
    pub fn from_strict(value: &str) -> Option<Self> {
        match value {
            "clean" => Some(Self::Clean),
            "dirty" => Some(Self::Dirty),
            _ => None,
        }
    }

    /// Lexical form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Dirty => "dirty",
        }
    }
}

/// The seven on/off children `CT_Compat` declares, in the order it declares them.
///
/// `CT_Compat` has exactly eight children and the eighth is `w:compatSetting`,
/// which [`Settings::compatibility`] carries as key/value pairs because a reader
/// records it that way. These seven are bare `CT_OnOff` flags: the corpus writes
/// every one of them with no `@w:val` at all, so there is no value to keep and a
/// `bool` per flag is the whole of it.
///
/// The order is the schema's and not the corpus's, and it matters for the same
/// reason as `MathProperties`: `CT_Compat` is an `xsd:sequence`, so a child that
/// appears earlier than the schema says is "This element is not expected". The
/// corpus happens to write them alphabetically, which is not the schema's order
/// - `doNotLeaveBackslashAlone` before `ulTrailSpace`, not after.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CompatFlags {
    /// `w:spaceForUL` — add space for underlining.
    pub space_for_underline: bool,
    /// `w:balanceSingleByteDoubleByteWidth` — balance single and double byte widths.
    pub balance_single_byte_double_byte_width: bool,
    /// `w:doNotLeaveBackslashAlone` — do not leave a backslash alone at line end.
    pub do_not_leave_backslash_alone: bool,
    /// `w:ulTrailSpace` — underline trailing spaces.
    pub underline_trailing_space: bool,
    /// `w:doNotExpandShiftReturn` — do not expand a soft return to a full line.
    pub do_not_expand_shift_return: bool,
    /// `w:adjustLineHeightInTable` — add document grid line pitch to line height.
    pub adjust_line_height_in_table: bool,
    /// `w:applyBreakingRules` — apply East Asian breaking rules.
    pub apply_breaking_rules: bool,
}

impl CompatFlags {
    /// Every flag that is set, in `CT_Compat`'s order.
    #[must_use]
    pub fn set(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        for (on, name) in [
            (self.space_for_underline, "w:spaceForUL"),
            (
                self.balance_single_byte_double_byte_width,
                "w:balanceSingleByteDoubleByteWidth",
            ),
            (
                self.do_not_leave_backslash_alone,
                "w:doNotLeaveBackslashAlone",
            ),
            (self.underline_trailing_space, "w:ulTrailSpace"),
            (self.do_not_expand_shift_return, "w:doNotExpandShiftReturn"),
            (
                self.adjust_line_height_in_table,
                "w:adjustLineHeightInTable",
            ),
            (self.apply_breaking_rules, "w:applyBreakingRules"),
        ] {
            if on {
                names.push(name);
            }
        }
        names
    }
}

/// `w:clrSchemeMapping`: which theme slot each colour name resolves to.
///
/// All twelve attributes are `use="optional"` and all twelve are `ST_WmlColorSchemeIndex`,
/// a restriction of `xsd:string` over exactly twelve values. They stay strings
/// here for the same reason as `MathProperties`: a value outside the twelve has
/// to survive to be named by the XSD gate, and an enum would have to either drop
/// it or invent a variant for it.
///
/// Every corpus document carries all twelve, mapped to themselves - Word writes
/// the identity mapping unless a document has been recoloured. That is why the
/// element looked worthless to keep and is not: an identity mapping is the
/// default, and the *absence* of the element means the same thing to Word, so the
/// corpus proves nothing about whether it can differ. A document that maps
/// `w:hyperlink` to `w:accent2` renders its links in a theme colour, and losing
/// that element loses the recolouring while leaving a document that validates.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ColorSchemeMapping {
    /// `w:bg1` — background 1.
    pub background_1: Option<Arc<str>>,
    /// `w:t1` — text 1.
    pub text_1: Option<Arc<str>>,
    /// `w:bg2` — background 2.
    pub background_2: Option<Arc<str>>,
    /// `w:t2` — text 2.
    pub text_2: Option<Arc<str>>,
    /// `w:accent1`.
    pub accent_1: Option<Arc<str>>,
    /// `w:accent2`.
    pub accent_2: Option<Arc<str>>,
    /// `w:accent3`.
    pub accent_3: Option<Arc<str>>,
    /// `w:accent4`.
    pub accent_4: Option<Arc<str>>,
    /// `w:accent5`.
    pub accent_5: Option<Arc<str>>,
    /// `w:accent6`.
    pub accent_6: Option<Arc<str>>,
    /// `w:hyperlink`.
    pub hyperlink: Option<Arc<str>>,
    /// `w:followedHyperlink`.
    pub followed_hyperlink: Option<Arc<str>>,
}

impl ColorSchemeMapping {
    /// The attributes in `CT_ColorSchemeMapping`'s declaration order.
    #[must_use]
    pub fn slots(&self) -> [(&'static str, &Option<Arc<str>>); 12] {
        [
            ("w:bg1", &self.background_1),
            ("w:t1", &self.text_1),
            ("w:bg2", &self.background_2),
            ("w:t2", &self.text_2),
            ("w:accent1", &self.accent_1),
            ("w:accent2", &self.accent_2),
            ("w:accent3", &self.accent_3),
            ("w:accent4", &self.accent_4),
            ("w:accent5", &self.accent_5),
            ("w:accent6", &self.accent_6),
            ("w:hyperlink", &self.hyperlink),
            ("w:followedHyperlink", &self.followed_hyperlink),
        ]
    }
}

/// `w:rsids`: the revision-save identifiers a document has accumulated.
///
/// `w:rsids` holds one `w:rsidRoot` and then one `w:rsid` per editing session.
/// They are editor state - nothing renders from them - and they were dropped from
/// all forty corpus documents, unnamed.
///
/// They are kept rather than named away, for the reason `REMOVALS` documents:
/// "we do not act on it" is a support question the Feature Report answers, and
/// deleting the element converts a declared feature into a loss that is not one.
/// The list is a `Vec` in document order and the root is kept apart, because
/// `CT_DocRsids` is a sequence of exactly one root followed by an unbounded list
/// and re-sorting them would be a change nobody asked for.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RevisionSaveIds {
    /// `w:rsidRoot/@w:val` - the session the document was created in.
    pub root: Option<Arc<str>>,
    /// Each `w:rsid/@w:val`, in document order.
    pub entries: Vec<Arc<str>>,
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
