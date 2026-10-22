//! A two-panel file manager drawn with egui, after Total Commander.
//!
//! [`FileManager`] is an embeddable widget: two file panels with drive
//! buttons, a path bar, sortable columns and selection; the function-key
//! bar (F3 view, F4 edit, F5 copy, F6 move, F7 new folder, F8 delete);
//! archives browsed as folders and packed or unpacked; file search; the
//! quick view (Ctrl+Q) in the other panel; space counting; and a command
//! line with an expandable log under the panels.
//!
//! The work happens on worker threads ([`ops`], [`search`], [`entry`]),
//! so large files and folders of many thousands of files keep the
//! interface responsive. The host answers the [`Event`]s, such as opening a
//! document in its editor.
//!
//! The core modules have no egui dependency and can be used alone.

pub mod archive;
pub mod documents;
pub mod drives;
pub mod entry;
pub mod locale;
pub mod mask;
#[cfg(not(feature = "oxideav"))]
pub mod media;
#[cfg(feature = "oxideav")]
#[path = "media_oxideav.rs"]
pub mod media;
pub mod ops;
pub mod preview;
pub mod progress;
pub mod search;
pub mod terminal;

mod app;
#[cfg(all(windows, not(feature = "oxideav")))]
mod native_audio;

pub use app::{
    Event, FileManager, FolderTabSettings, PanelTabSettings, Settings, EDITOR_EXTENSIONS,
};
pub use locale::Locale;
