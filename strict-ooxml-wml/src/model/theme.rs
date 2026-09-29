//! Document theme model (`theme/theme1.xml`).

use std::collections::HashMap;
use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;

/// The three typefaces of one font scheme role.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FontSet {
    /// Latin typeface (`a:latin/@typeface`).
    pub latin: Option<Arc<str>>,
    /// East-Asian typeface (`a:ea/@typeface`).
    pub east_asia: Option<Arc<str>>,
    /// Complex-script typeface (`a:cs/@typeface`).
    pub cs: Option<Arc<str>>,
}

/// The major/minor font scheme (`a:fontScheme`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ThemeFonts {
    /// Major (heading) fonts.
    pub major: FontSet,
    /// Minor (body) fonts.
    pub minor: FontSet,
}

impl ThemeFonts {
    /// Resolves a `w:*Theme` reference such as `minorHAnsi`/`majorEastAsia`.
    #[must_use]
    pub fn font(&self, reference: &str) -> Option<&Arc<str>> {
        let set = if reference.starts_with("major") {
            &self.major
        } else if reference.starts_with("minor") {
            &self.minor
        } else {
            return None;
        };
        if reference.ends_with("EastAsia") {
            set.east_asia.as_ref()
        } else if reference.ends_with("Bidi") {
            set.cs.as_ref()
        } else {
            set.latin.as_ref()
        }
    }
}

/// The theme colour scheme (`a:clrScheme`), keyed by canonical slot name.
#[derive(Clone, Debug, Default)]
pub struct ThemeColors {
    slots: HashMap<String, Arc<str>>,
}

impl ThemeColors {
    /// Inserts a canonical slot colour (`#rrggbb`).
    pub fn insert(&mut self, slot: impl Into<String>, color: impl Into<Arc<str>>) {
        self.slots.insert(slot.into(), color.into());
    }

    /// Returns a canonical slot colour (`dk1`, `lt1`, `accent1`, ...).
    #[must_use]
    pub fn slot(&self, slot: &str) -> Option<&str> {
        self.slots.get(slot).map(AsRef::as_ref)
    }

    /// Resolves a `w:themeColor` reference to `#rrggbb`.
    #[must_use]
    pub fn resolve(&self, reference: &str) -> Option<&str> {
        let slot = match reference {
            "dark1" | "text1" => "dk1",
            "light1" | "background1" => "lt1",
            "dark2" | "text2" => "dk2",
            "light2" | "background2" => "lt2",
            "hyperlink" => "hlink",
            "followedHyperlink" => "folHlink",
            "accent1" => "accent1",
            "accent2" => "accent2",
            "accent3" => "accent3",
            "accent4" => "accent4",
            "accent5" => "accent5",
            "accent6" => "accent6",
            other => other,
        };
        self.slot(slot)
    }
}

/// A parsed document theme.
#[derive(Clone, Debug)]
pub struct Theme {
    /// Major/minor font scheme.
    pub fonts: ThemeFonts,
    /// Colour scheme.
    pub colors: ThemeColors,
    /// Source location of the theme root.
    pub location: SourceLocation,
}

impl Theme {
    /// Resolves a theme font reference.
    #[must_use]
    pub fn font(&self, reference: &str) -> Option<&Arc<str>> {
        self.fonts.font(reference)
    }

    /// Resolves a theme colour reference.
    #[must_use]
    pub fn color(&self, reference: &str) -> Option<&str> {
        self.colors.resolve(reference)
    }
}

#[cfg(test)]
mod tests {
    use super::{FontSet, ThemeColors, ThemeFonts};
    use std::sync::Arc;

    #[test]
    fn font_references_resolve_to_sets() {
        let fonts = ThemeFonts {
            major: FontSet {
                latin: Some(Arc::from("Calibri Light")),
                east_asia: Some(Arc::from("MS Mincho")),
                cs: Some(Arc::from("Arial")),
            },
            minor: FontSet {
                latin: Some(Arc::from("Calibri")),
                ..FontSet::default()
            },
        };
        assert_eq!(fonts.font("minorHAnsi").map(AsRef::as_ref), Some("Calibri"));
        assert_eq!(
            fonts.font("majorAscii").map(AsRef::as_ref),
            Some("Calibri Light")
        );
        assert_eq!(
            fonts.font("majorEastAsia").map(AsRef::as_ref),
            Some("MS Mincho")
        );
        assert_eq!(fonts.font("minorBidi").map(AsRef::as_ref), None);
        assert_eq!(fonts.font("bogus"), None);
    }

    #[test]
    fn color_references_resolve_aliases() {
        let mut colors = ThemeColors::default();
        colors.insert("dk1", "#000000");
        colors.insert("lt1", "#ffffff");
        colors.insert("accent1", "#4472c4");
        colors.insert("hlink", "#0563c1");
        assert_eq!(colors.resolve("dark1"), Some("#000000"));
        assert_eq!(colors.resolve("text1"), Some("#000000"));
        assert_eq!(colors.resolve("background1"), Some("#ffffff"));
        assert_eq!(colors.resolve("accent1"), Some("#4472c4"));
        assert_eq!(colors.resolve("hyperlink"), Some("#0563c1"));
        assert_eq!(colors.resolve("accent6"), None);
    }
}
