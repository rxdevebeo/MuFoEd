//! Document theme model (`theme/theme1.xml`).

use std::collections::HashMap;
use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;

/// One DrawingML typeface (`CT_TextFont`): `a:latin`, `a:ea`, or `a:cs`.
///
/// `charset` defaults to `1` and `pitchFamily` to `0` when the attributes are
/// absent. An explicit value, including those defaults, is kept: omitting it
/// is a different document to the census even when a consumer would paint the
/// same face.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ThemeTypeface {
    /// `typeface`. Empty means the script has no face.
    pub name: Option<Arc<str>>,
    /// `panose`.
    pub panose: Option<Arc<str>>,
    /// `pitchFamily`.
    pub pitch_family: Option<Arc<str>>,
    /// `charset`.
    pub charset: Option<Arc<str>>,
}

impl ThemeTypeface {
    /// A typeface that carries only a name.
    #[must_use]
    pub fn named(name: impl Into<Arc<str>>) -> Self {
        Self {
            name: Some(name.into()),
            ..Self::default()
        }
    }

    /// Returns `true` when no attribute was stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.panose.is_none()
            && self.pitch_family.is_none()
            && self.charset.is_none()
    }
}

/// The three typefaces of one font scheme role.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FontSet {
    /// Latin typeface (`a:latin`).
    pub latin: ThemeTypeface,
    /// East-Asian typeface (`a:ea`).
    pub east_asia: ThemeTypeface,
    /// Complex-script typeface (`a:cs`).
    pub cs: ThemeTypeface,
    /// Per-script faces (`a:font` `script`, `typeface`), in source order.
    pub scripts: Vec<(Arc<str>, Arc<str>)>,
}

/// `a:defRPr` latin/ea/cs under theme object defaults.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ThemeRunFonts {
    /// `a:latin`.
    pub latin: ThemeTypeface,
    /// `a:ea`.
    pub east_asia: ThemeTypeface,
    /// `a:cs`.
    pub cs: ThemeTypeface,
}

impl ThemeRunFonts {
    /// Returns `true` when none of the three faces carries an attribute.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.latin.is_empty() && self.east_asia.is_empty() && self.cs.is_empty()
    }
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
            set.east_asia.name.as_ref()
        } else if reference.ends_with("Bidi") {
            set.cs.name.as_ref()
        } else {
            set.latin.name.as_ref()
        }
    }
}

/// The theme colour scheme (`a:clrScheme`), keyed by canonical slot name.
#[derive(Clone, Debug, Default)]
pub struct ThemeColors {
    slots: HashMap<String, Arc<str>>,
    /// Slots written as `a:sysClr`, with its `val` (`windowText`, `window`).
    system: HashMap<String, Arc<str>>,
}

impl ThemeColors {
    /// Marks `slot` as the system colour `name` (`a:sysClr/@val`); its value
    /// is the last resolved colour, inserted as for any other slot.
    pub fn set_system(&mut self, slot: impl Into<String>, name: impl Into<Arc<str>>) {
        self.system.insert(slot.into(), name.into());
    }

    /// The system colour a slot names, when it was written as `a:sysClr`.
    #[must_use]
    pub fn system(&self, slot: &str) -> Option<&str> {
        self.system.get(slot).map(AsRef::as_ref)
    }

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
            // WML `w:themeColor` names and DrawingML `a:schemeClr` aliases.
            "dark1" | "text1" | "tx1" | "dk1" => "dk1",
            "light1" | "background1" | "bg1" | "lt1" => "lt1",
            "dark2" | "text2" | "tx2" | "dk2" => "dk2",
            "light2" | "background2" | "bg2" | "lt2" => "lt2",
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
    /// `a:objectDefaults/a:spDef` run fonts, when the theme has them.
    pub shape_defaults: Option<ThemeRunFonts>,
    /// `a:objectDefaults/a:txDef` run fonts, when the theme has them.
    pub text_defaults: Option<ThemeRunFonts>,
    /// `a:objectDefaults` as parsed, so the writer can put the element back.
    ///
    /// The faces above are the part the model understands. The element also
    /// carries shape properties, list styles and `a:lnDef`, and rewriting it
    /// as an empty shell would drop those. `None` means the theme had no
    /// object defaults (a hand-built theme may still set the faces).
    pub object_defaults_xml: Option<String>,
    /// `a:fmtScheme` as read (fill, line, effect and background styles that a
    /// shape's `wps:style` refers to), written back in place of the placeholder.
    /// `None` when the theme had none, or when it carried something this
    /// writer may not write (ADR-0014).
    pub format_scheme_xml: Option<String>,
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
    use super::{FontSet, ThemeColors, ThemeFonts, ThemeTypeface};

    #[test]
    fn font_references_resolve_to_sets() {
        let fonts = ThemeFonts {
            major: FontSet {
                latin: ThemeTypeface::named("Calibri Light"),
                east_asia: ThemeTypeface::named("MS Mincho"),
                cs: ThemeTypeface::named("Arial"),
                scripts: Vec::new(),
            },
            minor: FontSet {
                latin: ThemeTypeface::named("Calibri"),
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
        assert_eq!(colors.resolve("tx1"), Some("#000000"));
        assert_eq!(colors.resolve("background1"), Some("#ffffff"));
        assert_eq!(colors.resolve("bg1"), Some("#ffffff"));
        assert_eq!(colors.resolve("accent1"), Some("#4472c4"));
        assert_eq!(colors.resolve("hyperlink"), Some("#0563c1"));
        assert_eq!(colors.resolve("accent6"), None);
    }
}
