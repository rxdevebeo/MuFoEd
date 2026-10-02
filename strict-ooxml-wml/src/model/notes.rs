//! Footnote/endnote model (`footnotes.xml`, `endnotes.xml`).

use std::collections::HashMap;
use std::sync::Arc;

use strict_ooxml_core::error::SourceLocation;

use super::block::Block;

/// Which role a note definition plays (`w:footnote/@w:type`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum NoteKind {
    /// A normal note (`normal`, the default).
    #[default]
    Normal,
    /// The separator drawn between the body and the notes (`separator`).
    Separator,
    /// The separator drawn for notes continued from the previous page
    /// (`continuationSeparator`).
    ContinuationSeparator,
    /// A continuation notice (`continuationNotice`).
    ContinuationNotice,
}

impl NoteKind {
    /// Parses a Strict note-type value (absent means `normal`).
    #[must_use]
    pub fn from_strict(value: &str) -> Option<Self> {
        match value {
            "normal" => Some(Self::Normal),
            "separator" => Some(Self::Separator),
            "continuationSeparator" => Some(Self::ContinuationSeparator),
            "continuationNotice" => Some(Self::ContinuationNotice),
            _ => None,
        }
    }

    /// Returns the Strict lexical value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Separator => "separator",
            Self::ContinuationSeparator => "continuationSeparator",
            Self::ContinuationNotice => "continuationNotice",
        }
    }
}

/// One note definition (`w:footnote`/`w:endnote`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// Numeric id (`w:id`). Normal notes are positive; the separator notes use
    /// the reserved ids `-1` (separator) and `0` (continuation separator).
    pub id: i32,
    /// Role of this definition.
    pub kind: NoteKind,
    /// Block content.
    pub blocks: Vec<Block>,
    /// Source location.
    pub location: SourceLocation,
}

/// All note definitions declared by a notes part.
#[derive(Clone, Debug, Default)]
pub struct NoteTable {
    notes: Vec<Note>,
    by_id: HashMap<i32, usize>,
}

impl NoteTable {
    /// Creates an empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a note. A later definition with the same id replaces the former.
    pub fn insert(&mut self, note: Note) {
        if let Some(&index) = self.by_id.get(&note.id) {
            self.notes[index] = note;
            return;
        }
        let index = self.notes.len();
        self.by_id.insert(note.id, index);
        self.notes.push(note);
    }

    /// Returns a note by id.
    #[must_use]
    pub fn get(&self, id: i32) -> Option<&Note> {
        self.by_id.get(&id).and_then(|&index| self.notes.get(index))
    }

    /// Returns the separator definition, if present.
    #[must_use]
    pub fn separator(&self) -> Option<&Note> {
        self.notes
            .iter()
            .find(|note| note.kind == NoteKind::Separator)
    }

    /// Returns the continuation-separator definition, if present.
    #[must_use]
    pub fn continuation_separator(&self) -> Option<&Note> {
        self.notes
            .iter()
            .find(|note| note.kind == NoteKind::ContinuationSeparator)
    }

    /// Returns the continuation-notice definition, if present.
    #[must_use]
    pub fn continuation_notice(&self) -> Option<&Note> {
        self.notes
            .iter()
            .find(|note| note.kind == NoteKind::ContinuationNotice)
    }

    /// Iterates over notes in declaration order.
    pub fn iter(&self) -> impl Iterator<Item = &Note> {
        self.notes.iter()
    }

    /// Returns the number of notes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.notes.len()
    }

    /// Returns `true` if no notes are defined.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }
}

/// Numbering and placement options for footnotes or endnotes
/// (`w:footnotePr`/`w:endnotePr` in `settings.xml` or `w:sectPr`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct NoteProperties {
    /// Placement (`w:pos`), for example `pageBottom`, `docEnd`.
    pub position: Option<Arc<str>>,
    /// Number format (`w:numFmt`), for example `decimal`, `lowerRoman`.
    pub num_format: Option<Arc<str>>,
    /// Starting number (`w:numStart`).
    pub num_start: Option<u32>,
    /// Restart mode (`w:numRestart`): `continuous`, `eachSect`, `eachPage`.
    pub num_restart: Option<Arc<str>>,
    /// `w:footnote`/`w:endnote` references inside the document's `w:footnotePr`.
    ///
    /// Not the notes themselves: ids, naming the separator (`-1`) and the
    /// continuation separator (`0`). Those two are what draws the rule above a
    /// footnote block, so they are page content. The writer emitted
    /// `w:footnotePr` whenever there was a position or a format and nothing at
    /// all when there was not, so a document whose only settings are the two
    /// separator ids lost both.
    pub separator_ids: Vec<u32>,
}

impl NoteProperties {
    /// Returns `true` if no option is set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.position.is_none()
            && self.num_format.is_none()
            && self.num_start.is_none()
            && self.num_restart.is_none()
            && self.separator_ids.is_empty()
    }

    /// Overlays `other` on top of `self` (values in `other` win).
    pub fn merge_from(&mut self, other: &Self) {
        if other.position.is_some() {
            self.position.clone_from(&other.position);
        }
        if other.num_format.is_some() {
            self.num_format.clone_from(&other.num_format);
        }
        if other.num_start.is_some() {
            self.num_start = other.num_start;
        }
        if other.num_restart.is_some() {
            self.num_restart.clone_from(&other.num_restart);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Note, NoteKind, NoteProperties, NoteTable};
    use strict_ooxml_core::error::SourceLocation;
    use strict_ooxml_core::part::PartId;

    fn location() -> SourceLocation {
        SourceLocation::new(PartId::new("/word/footnotes.xml"), 1, 1, 0)
    }

    fn note(id: i32, kind: NoteKind) -> Note {
        Note {
            id,
            kind,
            blocks: Vec::new(),
            location: location(),
        }
    }

    #[test]
    fn table_indexes_notes_by_id_and_kind() {
        let mut table = NoteTable::new();
        assert!(table.is_empty());
        table.insert(note(-1, NoteKind::Separator));
        table.insert(note(0, NoteKind::ContinuationSeparator));
        table.insert(note(1, NoteKind::Normal));
        table.insert(note(2, NoteKind::ContinuationNotice));
        // Replacing an id keeps a single entry.
        table.insert(note(1, NoteKind::Normal));
        assert_eq!(table.len(), 4);
        assert!(table.get(1).is_some());
        assert!(table.get(-1).is_some());
        assert!(table.separator().is_some());
        assert!(table.continuation_separator().is_some());
        assert!(table.continuation_notice().is_some());
        assert_eq!(table.iter().count(), 4);
    }

    #[test]
    fn note_kind_round_trips() {
        for kind in [
            NoteKind::Normal,
            NoteKind::Separator,
            NoteKind::ContinuationSeparator,
            NoteKind::ContinuationNotice,
        ] {
            assert_eq!(NoteKind::from_strict(kind.as_str()), Some(kind));
        }
        assert_eq!(NoteKind::from_strict("nope"), None);
    }

    #[test]
    fn properties_merge_and_emptiness() {
        let mut props = NoteProperties::default();
        assert!(props.is_empty());
        props.num_format = Some("decimal".into());
        assert!(!props.is_empty());
        let overlay = NoteProperties {
            num_start: Some(5),
            ..NoteProperties::default()
        };
        props.merge_from(&overlay);
        assert_eq!(props.num_start, Some(5));
        assert_eq!(props.num_format.as_deref(), Some("decimal"));
    }
}
