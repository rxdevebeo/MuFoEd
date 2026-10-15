//! Metric profiles of proprietary families without an open metric twin.
//!
//! Carlito, Caladea, Arimo, Tinos and Cousine stand in for Calibri, Cambria,
//! Arial, Times New Roman and Courier New because they share their metrics.
//! Verdana, Tahoma, Lucida Sans Unicode, Candara and the other Windows
//! families have no such twin: drawn with Carlito, a Lucida Sans Unicode book
//! set 15.4 pt lines at 11.5 pt and lost a quarter of its pages. A profile
//! carries the family's measure - its Windows line box and the advance widths
//! of the Latin, Greek, Cyrillic and punctuation characters - and a bundled
//! face draws the glyphs at those advances. Lines then break and stack where
//! Word puts them; the letter shapes are the substitute's.
//!
//! The numbers come from the fonts themselves
//! (`xtool/fonts/build_metric_profiles.py`); no outline is carried. A profile
//! answers to its own family name, `"<substitute> for <family>"`, so SVG and
//! PDF name what they actually draw.

#[rustfmt::skip]
#[path = "profiles_data.rs"]
mod data;

/// One family's measure.
pub(crate) struct MetricProfile {
    /// The family name the profile is delivered under.
    pub(crate) family: &'static str,
    /// The requested names (lower-cased) it answers to.
    pub(crate) requested: &'static [&'static str],
    /// The bundled face that draws the glyphs.
    pub(crate) substitute: &'static str,
    /// The styles the family has.
    pub(crate) faces: &'static [ProfileFace],
}

/// One style of a profiled family.
pub(crate) struct ProfileFace {
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) units_per_em: u16,
    /// `OS/2` Windows ascent, font units.
    pub(crate) ascent: u16,
    /// `OS/2` Windows descent, font units.
    pub(crate) descent: u16,
    /// Advances of consecutive code points: `(first, advances)`.
    pub(crate) runs: &'static [(u32, &'static [u16])],
}

impl ProfileFace {
    /// The advance of `ch` in em, when the family has the character.
    pub(crate) fn advance_em(&self, ch: char) -> Option<f64> {
        let code = u32::from(ch);
        let index = self.runs.partition_point(|(start, _)| *start <= code);
        let (start, advances) = self.runs.get(index.checked_sub(1)?)?;
        let advance = advances.get(usize::try_from(code - start).ok()?)?;
        Some(f64::from(*advance) / f64::from(self.units_per_em))
    }

    /// Ascent and descent in em.
    pub(crate) fn line_box(&self) -> (f64, f64) {
        let units = f64::from(self.units_per_em);
        (
            f64::from(self.ascent) / units,
            f64::from(self.descent) / units,
        )
    }
}

/// The profile a requested (trimmed, lower-cased) family name selects, or the
/// profile's own delivered name.
pub(crate) fn for_name(name: &str) -> Option<&'static MetricProfile> {
    data::PROFILES.iter().find(|profile| {
        profile.requested.contains(&name) || profile.family.eq_ignore_ascii_case(name)
    })
}

/// The profile delivered under `family` (an exact mapped name).
pub(crate) fn for_family(family: &str) -> Option<&'static MetricProfile> {
    data::PROFILES
        .iter()
        .find(|profile| profile.family == family)
}

/// Every profile, for the bundle's face list.
pub(crate) fn all() -> &'static [MetricProfile] {
    data::PROFILES
}

impl MetricProfile {
    /// The style that measures a `(bold, italic)` request: the exact one, else
    /// the one with the same weight, else the regular. Word synthesizes the
    /// missing style from the regular's advances.
    pub(crate) fn face(&self, bold: bool, italic: bool) -> Option<&'static ProfileFace> {
        let faces = self.faces;
        faces
            .iter()
            .find(|face| face.bold == bold && face.italic == italic)
            .or_else(|| faces.iter().find(|face| face.bold == bold && !face.italic))
            .or_else(|| faces.iter().find(|face| !face.bold && !face.italic))
            .or_else(|| faces.first())
    }
}

#[cfg(test)]
mod tests {
    use super::{for_family, for_name};

    #[test]
    fn a_profile_measures_its_family() {
        let verdana = for_name("verdana").expect("Verdana");
        assert_eq!(verdana.substitute, "Arimo");
        assert_eq!(
            for_family(verdana.family).map(|p| p.family),
            Some(verdana.family)
        );
        let regular = verdana.face(false, false).expect("regular");
        // Verdana's `a` is wider than Arial's (1139/2048).
        let a = regular.advance_em('a').expect("a");
        assert!(a > 1139.0 / 2048.0, "{a}");
        assert_eq!(regular.advance_em('\u{4E00}'), None);
        // Tahoma has no italic: the upright face measures it.
        let tahoma = for_name("tahoma").expect("Tahoma");
        assert!(!tahoma.face(false, true).expect("face").italic);
    }
}
