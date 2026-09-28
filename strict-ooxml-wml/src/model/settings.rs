//! Document settings model (`settings.xml`).

use std::sync::Arc;

use super::ids::StyleId;
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
    /// Click-and-type settings are ignored in Stage 2.
    pub click_and_type: bool,
    /// Recognise the document as having a "mirror margins" preference.
    pub mirror_margins: bool,
}
