//! Style table model (`styles.xml`).

use std::collections::HashMap;
use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;

use super::ids::StyleId;
use super::props::{ParagraphProperties, RunProperties, TableProperties};
use super::values::StyleType;

/// One `w:tblStylePr` conditional format inside a table style.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TableStyleCondition {
    /// `w:type` (`firstRow`, `band1Horz`, ...).
    pub kind: Arc<str>,
    /// Paragraph properties of the condition.
    pub paragraph: ParagraphProperties,
    /// Run properties of the condition.
    pub run: RunProperties,
}

/// A single style definition (`w:style`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Style {
    /// Style id (`w:styleId`).
    pub id: StyleId,
    /// Style kind (`w:type`).
    pub style_type: StyleType,
    /// Display name (`w:name`).
    pub name: Option<Arc<str>>,
    /// Parent style (`w:basedOn`).
    pub based_on: Option<StyleId>,
    /// Style applied to the next paragraph (`w:next`).
    pub next: Option<StyleId>,
    /// Linked style (`w:link`).
    pub link: Option<StyleId>,
    /// Whether this is a default style (`w:default`).
    pub is_default: bool,
    /// Semi-hidden in the UI (`w:semiHidden`), AUD-46.
    pub semi_hidden: bool,
    /// Fully hidden (`w:hidden`), AUD-46.
    pub hidden: bool,
    /// Show in the recommended list (`w:qFormat`), AUD-46.
    pub q_format: bool,
    /// Locked against editing (`w:locked`), AUD-46.
    pub locked: bool,
    /// Unhide when used (`w:unhideWhenUsed`), AUD-46.
    pub unhide_when_used: bool,
    /// UI priority (`w:uiPriority`).
    pub ui_priority: Option<i32>,
    /// Table style conditional formatting (reserved).
    pub table: TableProperties,
    /// Paragraph properties of the style.
    pub paragraph: ParagraphProperties,
    /// Run properties of the style.
    pub run: RunProperties,
    /// `w:tblStylePr` conditions, in document order.
    pub conditions: Vec<TableStyleCondition>,
    /// Resolved `basedOn` chain, nearest ancestor first (excluding self).
    pub based_on_chain: Vec<StyleId>,
    /// Source location.
    pub location: SourceLocation,
}

/// Document-wide default properties (`w:docDefaults`).
///
/// ISO/IEC 29500-1 §17.7.1: `w:docDefaults` is the root of the style cascade —
/// every paragraph and run inherits from it before any style is applied.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocDefaults {
    /// Default run properties (`w:rPrDefault/w:rPr`).
    pub run: RunProperties,
    /// Default paragraph properties (`w:pPrDefault/w:pPr`).
    pub paragraph: ParagraphProperties,
}

/// All styles declared by `styles.xml`.
#[derive(Clone, Debug, Default)]
pub struct StyleTable {
    styles: Vec<Style>,
    by_id: HashMap<StyleId, usize>,
    defaults: DocDefaults,
}

impl StyleTable {
    /// Creates an empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the document-wide default properties (`w:docDefaults`).
    pub fn set_defaults(&mut self, defaults: DocDefaults) {
        self.defaults = defaults;
    }

    /// Returns the document-wide default properties, if any were declared.
    #[must_use]
    pub fn defaults(&self) -> Option<&DocDefaults> {
        let empty = DocDefaults::default();
        if self.defaults == empty {
            None
        } else {
            Some(&self.defaults)
        }
    }

    /// Inserts a style. A later definition with the same id replaces the former.
    pub fn insert(&mut self, style: Style) {
        if let Some(&index) = self.by_id.get(&style.id) {
            self.styles[index] = style;
            return;
        }
        let index = self.styles.len();
        self.by_id.insert(style.id.clone(), index);
        self.styles.push(style);
    }

    /// Returns a style by id.
    #[must_use]
    pub fn get(&self, id: &StyleId) -> Option<&Style> {
        self.by_id.get(id).and_then(|&index| self.styles.get(index))
    }

    /// Returns a mutable style by id (used by the resolve phase).
    pub fn get_mut(&mut self, id: &StyleId) -> Option<&mut Style> {
        let index = self.by_id.get(id).copied()?;
        self.styles.get_mut(index)
    }

    /// Iterates over style ids in declaration order.
    pub fn ids(&self) -> impl Iterator<Item = &StyleId> {
        self.styles.iter().map(|style| &style.id)
    }

    /// Returns `true` if the style id is defined.
    #[must_use]
    pub fn contains(&self, id: &StyleId) -> bool {
        self.by_id.contains_key(id)
    }

    /// Iterates over styles in declaration order.
    pub fn iter(&self) -> impl Iterator<Item = &Style> {
        self.styles.iter()
    }

    /// Returns the number of styles.
    #[must_use]
    pub fn len(&self) -> usize {
        self.styles.len()
    }

    /// Returns `true` if no styles are defined.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.styles.is_empty()
    }

    /// Returns the default style id for a kind, if declared.
    #[must_use]
    pub fn default_for(&self, style_type: StyleType) -> Option<&StyleId> {
        self.styles
            .iter()
            .find(|style| style.is_default && style.style_type == style_type)
            .map(|style| &style.id)
    }
}
