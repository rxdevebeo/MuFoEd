//! Tracked-change revisions (`w:ins` / `w:del` / `w:moveFrom` / `w:moveTo`, ADR-0018).

use std::sync::Arc;

/// Kind of a tracked-change container.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RevisionKind {
    /// Insertion (`w:ins`).
    Insert,
    /// Deletion (`w:del`).
    Delete,
    /// Move source (`w:moveFrom`).
    MoveFrom,
    /// Move destination (`w:moveTo`).
    MoveTo,
}

impl RevisionKind {
    /// Element local name written for this kind.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Insert => "ins",
            Self::Delete => "del",
            Self::MoveFrom => "moveFrom",
            Self::MoveTo => "moveTo",
        }
    }

    /// Feature id recorded in the support model (`w:ins`, …).
    #[must_use]
    pub const fn feature_id(self) -> &'static str {
        match self {
            Self::Insert => "w:ins",
            Self::Delete => "w:del",
            Self::MoveFrom => "w:moveFrom",
            Self::MoveTo => "w:moveTo",
        }
    }

    /// Whether this kind is treated as deleted content in the Final view.
    #[must_use]
    pub const fn is_deletion(self) -> bool {
        matches!(self, Self::Delete | Self::MoveFrom)
    }

    /// Whether this kind is treated as inserted content in the Final view.
    #[must_use]
    pub const fn is_insertion(self) -> bool {
        matches!(self, Self::Insert | Self::MoveTo)
    }

    /// Maps a WML local name to a revision kind.
    #[must_use]
    pub fn from_local(local: &str) -> Option<Self> {
        match local {
            "ins" => Some(Self::Insert),
            "del" => Some(Self::Delete),
            "moveFrom" => Some(Self::MoveFrom),
            "moveTo" => Some(Self::MoveTo),
            _ => None,
        }
    }
}

/// A tracked-change marker attached to a run or a paragraph mark (ADR-0018).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revision {
    /// Kind of change.
    pub kind: RevisionKind,
    /// Revision id (`w:id`).
    pub id: u32,
    /// Author (`w:author`), when present.
    pub author: Option<Arc<str>>,
    /// Date (`w:date`), when present.
    pub date: Option<Arc<str>>,
}

impl Revision {
    /// Creates a revision with the given kind and id.
    #[must_use]
    pub fn new(kind: RevisionKind, id: u32) -> Self {
        Self {
            kind,
            id,
            author: None,
            date: None,
        }
    }
}
