//! Lists as a word processor offers them: ready-made bullet and numbering
//! definitions, and what list a paragraph is in, through its styles.

use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;
use strict_ooxml_wml::model::numbering::{AbstractNum, Level, Num};
use strict_ooxml_wml::model::props::NumPr;
use strict_ooxml_wml::model::values::{Indentation, Justification, Twips};
use strict_ooxml_wml::model::{AbstractNumId, Document, Ilvl, NumId, Paragraph};

/// A list format of the kind a word processor's list gallery offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ListPreset {
    /// `•`, then `◦` and `▪` on deeper levels.
    Bullet,
    /// `◦` first.
    Circle,
    /// `▪` first.
    Square,
    /// `–` on every level.
    Dash,
    /// `1.`
    Decimal,
    /// `1)`
    DecimalParen,
    /// `a)`
    LowerLetter,
    /// `A.`
    UpperLetter,
    /// `i.`
    LowerRoman,
    /// `I.`
    UpperRoman,
}

/// The bullets of the bullet presets, by level.
const BULLETS: [&str; 3] = ["\u{2022}", "\u{25E6}", "\u{25AA}"];

impl ListPreset {
    /// Every preset, bullets first.
    pub const ALL: [Self; 10] = [
        Self::Bullet,
        Self::Circle,
        Self::Square,
        Self::Dash,
        Self::Decimal,
        Self::DecimalParen,
        Self::LowerLetter,
        Self::UpperLetter,
        Self::LowerRoman,
        Self::UpperRoman,
    ];

    /// Whether the preset marks items instead of numbering them.
    #[must_use]
    pub fn is_bullet(self) -> bool {
        matches!(
            self,
            Self::Bullet | Self::Circle | Self::Square | Self::Dash
        )
    }

    /// What the first item's marker shows: `•`, `1.`, `a)`.
    #[must_use]
    pub fn sample(self) -> &'static str {
        match self {
            Self::Bullet => "\u{2022}",
            Self::Circle => "\u{25E6}",
            Self::Square => "\u{25AA}",
            Self::Dash => "\u{2013}",
            Self::Decimal => "1.",
            Self::DecimalParen => "1)",
            Self::LowerLetter => "a)",
            Self::UpperLetter => "A.",
            Self::LowerRoman => "i.",
            Self::UpperRoman => "I.",
        }
    }

    /// The first level's number format (`w:numFmt`) and level text
    /// (`w:lvlText`).
    fn first_level(self) -> (&'static str, &'static str) {
        match self {
            Self::Bullet => ("bullet", BULLETS[0]),
            Self::Circle => ("bullet", BULLETS[1]),
            Self::Square => ("bullet", BULLETS[2]),
            Self::Dash => ("bullet", "\u{2013}"),
            Self::Decimal => ("decimal", "%1."),
            Self::DecimalParen => ("decimal", "%1)"),
            Self::LowerLetter => ("lowerLetter", "%1)"),
            Self::UpperLetter => ("upperLetter", "%1."),
            Self::LowerRoman => ("lowerRoman", "%1."),
            Self::UpperRoman => ("upperRoman", "%1."),
        }
    }

    /// The preset whose first level a list level looks like, if any.
    #[must_use]
    pub fn matching(level: &ListLevel) -> Option<Self> {
        Self::ALL.into_iter().find(|preset| {
            let (format, text) = preset.first_level();
            let text = text.replace("%1", &format!("%{}", level.ilvl + 1));
            level.format == format && level.text == text
        })
    }
}

/// An abstract numbering definition of nine levels in `preset`'s style, as
/// Word builds one: each level half an inch further in with a quarter inch
/// hanging, deeper levels of a numbered list counting `a.`, `i.`, `1.` in turn
/// and deeper bullets cycling `•`, `◦`, `▪`.
#[must_use]
pub fn list_definition(id: AbstractNumId, preset: ListPreset) -> AbstractNum {
    let levels = (0..=Ilvl::MAX)
        .map(|index| {
            let (format, text) = if index == 0 {
                let (format, text) = preset.first_level();
                (format.to_owned(), text.to_owned())
            } else if preset.is_bullet() {
                let bullet = if preset == ListPreset::Dash {
                    "\u{2013}"
                } else {
                    BULLETS
                        .get(usize::from(index) % BULLETS.len())
                        .copied()
                        .unwrap_or("\u{2022}")
                };
                ("bullet".to_owned(), bullet.to_owned())
            } else {
                let format = ["decimal", "lowerLetter", "lowerRoman"]
                    .get(usize::from(index) % 3)
                    .copied()
                    .unwrap_or("decimal");
                (format.to_owned(), format!("%{}.", index + 1))
            };
            let mut level = Level::new(Ilvl(index));
            level.start = Some(1);
            level.format = Some(Arc::from(format.as_str()));
            level.text = Some(Arc::from(text.as_str()));
            level.justification = Some(Justification::Start);
            level.paragraph.indentation = Some(Indentation {
                start: Some(Twips(720 * (i32::from(index) + 1))),
                hanging: Some(Twips(360)),
                ..Indentation::default()
            });
            level
        })
        .collect();
    AbstractNum {
        id,
        name: None,
        multi_level_type: Some(Arc::from("hybridMultilevel")),
        num_style_link: None,
        style_link: None,
        levels,
        location: SourceLocation::unknown(),
    }
}

/// A list instance of the abstract definition `abstract_id`.
#[must_use]
pub fn list_instance(id: NumId, abstract_id: AbstractNumId) -> Num {
    Num {
        num_id: id,
        abstract_num_id: abstract_id,
        overrides: Vec::new(),
        location: SourceLocation::unknown(),
    }
}

/// The list level a paragraph is at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListLevel {
    /// The list instance.
    pub num_id: NumId,
    /// The abstract definition the instance uses.
    pub abstract_id: AbstractNumId,
    /// Zero-based level.
    pub ilvl: u8,
    /// Number format (`w:numFmt`): `decimal`, `bullet`, `lowerLetter`...
    pub format: String,
    /// Level text (`w:lvlText`): `%1.`, `•`...
    pub text: String,
    /// Whether the numbering comes from the paragraph's style rather than
    /// the paragraph itself.
    pub from_style: bool,
}

impl ListLevel {
    /// Whether the level marks items instead of numbering them.
    #[must_use]
    pub fn is_bullet(&self) -> bool {
        self.format == "bullet" || self.format == "none"
    }
}

/// The list level `paragraph` is at: its own `w:numPr`, or its style's
/// (through `basedOn`), with the instance's level overrides applied. `None`
/// outside a list, `w:numId="0"` included.
#[must_use]
pub fn list_level(document: &Document, paragraph: &Paragraph) -> Option<ListLevel> {
    let mut from_style = false;
    let mut numbering: Option<NumPr> = paragraph.props.numbering;
    if numbering.as_ref().and_then(|n| n.num_id).is_none() {
        let style_id = paragraph.props.style.as_ref().or_else(|| {
            document
                .styles
                .default_for(strict_ooxml_wml::model::values::StyleType::Paragraph)
        });
        if let Some(style) = style_id.and_then(|id| document.styles.get(id)) {
            let chain = std::iter::once(&style.id).chain(style.based_on_chain.iter());
            let found = chain
                .filter_map(|id| document.styles.get(id))
                .find_map(|style| style.paragraph.numbering);
            if let Some(found) = found {
                let ilvl = numbering.as_ref().and_then(|n| n.ilvl).or(found.ilvl);
                numbering = Some(NumPr {
                    num_id: found.num_id,
                    ilvl,
                });
                from_style = true;
            }
        }
    }
    let numbering = numbering?;
    let num_id = numbering.num_id.filter(|id| id.0 != 0)?;
    let ilvl = numbering.ilvl.unwrap_or_default();
    let num = document.numbering.num(num_id)?;
    let abstract_num = document.numbering.resolved_abstract(num_id)?;
    let level = num
        .overrides
        .iter()
        .find(|o| o.ilvl == ilvl)
        .and_then(|o| o.level.as_ref())
        .or_else(|| abstract_num.level(ilvl))?;
    Some(ListLevel {
        num_id,
        abstract_id: num.abstract_num_id,
        ilvl: ilvl.0,
        format: level.format.as_deref().unwrap_or("decimal").to_owned(),
        text: level.text.as_deref().unwrap_or_default().to_owned(),
        from_style,
    })
}
