//! Numbering model (`numbering.xml`).

use std::collections::HashMap;
use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;

use super::drawing::Drawing;
use super::ids::{AbstractNumId, Ilvl, NumId, StyleId};
use super::props::{ParagraphProperties, RunProperties};
use super::values::Justification;

/// One numbered level of an abstract numbering definition (`w:lvl`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Level {
    /// Level index.
    pub ilvl: Ilvl,
    /// Starting value (`w:start`).
    pub start: Option<u32>,
    /// Number format (`w:numFmt`), for example `decimal`, `bullet`.
    pub format: Option<Arc<str>>,
    /// Level text (`w:lvlText`).
    pub text: Option<Arc<str>>,
    /// Picture bullet the level uses (`w:lvlPicBulletId`): the
    /// [`PictureBullet::id`] of a `w:numPicBullet` in the same part.
    pub pic_bullet_id: Option<u32>,
    /// Justification (`w:lvlJc`).
    pub justification: Option<Justification>,
    /// Paragraph style associated with the level (`w:pStyle`).
    pub paragraph_style: Option<StyleId>,
    /// Suffix after the number (`w:suff`).
    pub suffix: Option<Arc<str>>,
    /// Whether numbering restarts per higher level (`w:lvlRestart`).
    pub restart: Option<u32>,
    /// Paragraph properties of the level.
    pub paragraph: ParagraphProperties,
    /// Run properties of the level.
    pub run: RunProperties,
    /// Legal numbering (`w:isLgl`).
    pub is_legal: bool,
    /// Tentative level (`w:tentative="1"`).
    pub tentative: bool,
    /// Explicit `w:tentative="0"`. Absence of the attribute is neither flag.
    pub tentative_off: bool,
}

impl Level {
    /// Creates a level with the given index.
    #[must_use]
    pub fn new(ilvl: Ilvl) -> Self {
        Self {
            ilvl,
            ..Self::default()
        }
    }
}

/// An abstract numbering definition (`w:abstractNum`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbstractNum {
    /// Abstract definition id.
    pub id: AbstractNumId,
    /// The definition's name in the UI (`w:name`).
    pub name: Option<Arc<str>>,
    /// Multi-level numbering type (`w:multiLevelType`).
    pub multi_level_type: Option<Arc<str>>,
    /// Numbering style link (`w:numStyleLink`).
    pub num_style_link: Option<StyleId>,
    /// Style link (`w:styleLink`).
    pub style_link: Option<StyleId>,
    /// The nine levels, by index where present.
    pub levels: Vec<Level>,
    /// Source location.
    pub location: SourceLocation,
}

impl AbstractNum {
    /// Returns the level with the given index, if present.
    #[must_use]
    pub fn level(&self, ilvl: Ilvl) -> Option<&Level> {
        self.levels.iter().find(|level| level.ilvl == ilvl)
    }
}

/// A picture bullet definition (`w:numPicBullet`).
///
/// Strict `CT_NumPicBullet` is a choice of `w:pict` and `w:drawing`, and Strict
/// `w:pict` carries no VML, so the picture is a `w:drawing`; a Transitional VML
/// bullet arrives here already converted by the normalizer (T7). The image it
/// shows is a relationship of `numbering.xml`, resolved against that part and
/// indexed in the document's media like any other picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PictureBullet {
    /// `@w:numPicBulletId`, what `w:lvlPicBulletId` refers to.
    pub id: u32,
    /// The picture.
    pub drawing: Drawing,
    /// Source location.
    pub location: SourceLocation,
}

/// An override of one level within a numbering instance (`w:lvlOverride`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LevelOverride {
    /// Level index.
    pub ilvl: Ilvl,
    /// Start-value override (`w:startOverride`).
    pub start_override: Option<u32>,
    /// Full replacement level (`w:lvl`).
    pub level: Option<Level>,
}

/// A numbering instance (`w:num`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Num {
    /// Instance id (`w:numId`).
    pub num_id: NumId,
    /// Referenced abstract definition (`w:abstractNumId`).
    pub abstract_num_id: AbstractNumId,
    /// Level overrides.
    pub overrides: Vec<LevelOverride>,
    /// Source location.
    pub location: SourceLocation,
}

/// All numbering definitions declared by `numbering.xml`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NumberingTable {
    pic_bullets: Vec<PictureBullet>,
    abstracts: Vec<AbstractNum>,
    nums: Vec<Num>,
    by_abstract: HashMap<AbstractNumId, usize>,
    by_num: HashMap<NumId, usize>,
    /// Successful `numStyleLink` redirects: source abstract → terminal abstract
    /// that actually carries the levels (filled during resolve).
    link_targets: HashMap<AbstractNumId, AbstractNumId>,
}

impl NumberingTable {
    /// Creates an empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a picture bullet. The first definition for an id wins; returns
    /// `false` when a duplicate was skipped.
    pub fn insert_pic_bullet(&mut self, bullet: PictureBullet) -> bool {
        if self.pic_bullets.iter().any(|known| known.id == bullet.id) {
            return false;
        }
        self.pic_bullets.push(bullet);
        true
    }

    /// Returns a picture bullet by id.
    #[must_use]
    pub fn pic_bullet(&self, id: u32) -> Option<&PictureBullet> {
        self.pic_bullets.iter().find(|bullet| bullet.id == id)
    }

    /// Iterates over picture bullets in document order.
    pub fn pic_bullets(&self) -> impl Iterator<Item = &PictureBullet> {
        self.pic_bullets.iter()
    }

    /// Inserts an abstract definition. The first definition for an id wins
    /// (Word behaviour); returns `false` when a duplicate was skipped.
    pub fn insert_abstract(&mut self, abstract_num: AbstractNum) -> bool {
        if self.by_abstract.contains_key(&abstract_num.id) {
            return false;
        }
        let index = self.abstracts.len();
        self.by_abstract.insert(abstract_num.id, index);
        self.abstracts.push(abstract_num);
        true
    }

    /// Inserts a numbering instance. The first definition for an id wins;
    /// returns `false` when a duplicate was skipped.
    pub fn insert_num(&mut self, num: Num) -> bool {
        if self.by_num.contains_key(&num.num_id) {
            return false;
        }
        let index = self.nums.len();
        self.by_num.insert(num.num_id, index);
        self.nums.push(num);
        true
    }

    /// Replaces the abstract definition with the same id; returns `false`
    /// (and changes nothing) when there is none.
    pub fn replace_abstract(&mut self, abstract_num: AbstractNum) -> bool {
        let Some(slot) = self
            .by_abstract
            .get(&abstract_num.id)
            .and_then(|&i| self.abstracts.get_mut(i))
        else {
            return false;
        };
        *slot = abstract_num;
        true
    }

    /// Removes an abstract definition, keeping the others in order.
    pub fn remove_abstract(&mut self, id: AbstractNumId) -> Option<AbstractNum> {
        let index = self.by_abstract.remove(&id)?;
        if index >= self.abstracts.len() {
            return None;
        }
        let removed = self.abstracts.remove(index);
        for slot in self.by_abstract.values_mut() {
            if *slot > index {
                *slot -= 1;
            }
        }
        Some(removed)
    }

    /// Replaces the numbering instance with the same id; returns `false` (and
    /// changes nothing) when there is none.
    pub fn replace_num(&mut self, num: Num) -> bool {
        let Some(slot) = self
            .by_num
            .get(&num.num_id)
            .and_then(|&i| self.nums.get_mut(i))
        else {
            return false;
        };
        *slot = num;
        true
    }

    /// Removes a numbering instance, keeping the others in order.
    pub fn remove_num(&mut self, id: NumId) -> Option<Num> {
        let index = self.by_num.remove(&id)?;
        if index >= self.nums.len() {
            return None;
        }
        let removed = self.nums.remove(index);
        for slot in self.by_num.values_mut() {
            if *slot > index {
                *slot -= 1;
            }
        }
        Some(removed)
    }

    /// Forgets every `numStyleLink` redirect, before they are resolved again.
    pub fn clear_link_targets(&mut self) {
        self.link_targets.clear();
    }

    /// Records a successful `numStyleLink` redirect (resolve phase).
    pub fn set_link_target(&mut self, from: AbstractNumId, to: AbstractNumId) {
        self.link_targets.insert(from, to);
    }

    /// Returns an abstract definition by id.
    #[must_use]
    pub fn abstract_num(&self, id: AbstractNumId) -> Option<&AbstractNum> {
        self.by_abstract
            .get(&id)
            .and_then(|&i| self.abstracts.get(i))
    }

    /// Returns a numbering instance by id.
    #[must_use]
    pub fn num(&self, id: NumId) -> Option<&Num> {
        self.by_num.get(&id).and_then(|&i| self.nums.get(i))
    }

    /// Resolves a numbering instance to its abstract definition, following any
    /// `numStyleLink` redirect recorded during resolve.
    #[must_use]
    pub fn resolved_abstract(&self, id: NumId) -> Option<&AbstractNum> {
        let num = self.num(id)?;
        let start = num.abstract_num_id;
        if let Some(&target) = self.link_targets.get(&start) {
            return self
                .abstract_num(target)
                .or_else(|| self.abstract_num(start));
        }
        self.abstract_num(start)
    }

    /// Iterates over numbering instances.
    pub fn nums(&self) -> impl Iterator<Item = &Num> {
        self.nums.iter()
    }

    /// Iterates over abstract definitions.
    pub fn abstracts(&self) -> impl Iterator<Item = &AbstractNum> {
        self.abstracts.iter()
    }

    /// Returns the number of numbering instances.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nums.len()
    }

    /// Returns `true` if there are no numbering definitions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.abstracts.is_empty() && self.nums.is_empty() && self.pic_bullets.is_empty()
    }
}
