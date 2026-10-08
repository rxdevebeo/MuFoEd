//! Editing addresses and structural transactions across document stories.
mod address;
mod apply;
mod command;
mod editor;
mod ids;
mod notes;
mod revisions;
mod validate;

pub use address::{Address, Container, Story};
#[cfg(feature = "visual")]
pub(crate) use apply::complex_fields;
pub use command::{Edit, EditFailure};
pub use editor::Editor;
pub(crate) use revisions::has_revisions;
