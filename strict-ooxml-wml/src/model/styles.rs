//! Style table model (`styles.xml`).

use std::collections::HashMap;
use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;

use super::ids::StyleId;
use super::props::{ParagraphProperties, RunProperties, TableProperties};
use super::values::StyleType;

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
    /// Whether the style is hidden (`w:semiHidden`/`w:hidden`).
    pub hidden: bool,
    /// UI priority (`w:uiPriority`).
    pub ui_priority: Option<i32>,
    /// Table style conditional formatting (reserved).
    pub table: TableProperties,
    /// Paragraph properties of the style.
    pub paragraph: ParagraphProperties,
    /// Run properties of the style.
    pub run: RunProperties,
    /// Resolved `basedOn` chain, nearest ancestor first (excluding self).
    pub based_on_chain: Vec<StyleId>,
    /// Source location.
    pub location: SourceLocation,
}

/// All styles declared by `styles.xml`.
#[derive(Clone, Debug, Default)]
pub struct StyleTable {
    styles: Vec<Style>,
    by_id: HashMap<StyleId, usize>,
}

impl StyleTable {
    /// Creates an empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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
