//! Embeddable native diagram editor. Coordinates are logical pixels at 96 DPI.
//! JSON is the editable source; SVG is a portable visual export.
mod commands;
#[cfg(feature = "docx")]
mod docx;
mod editor;
mod locale;
mod model;
mod routing;
mod svg;
mod widget;
#[cfg(feature = "docx")]
pub use docx::{DocxExport, DocxImport};

pub use commands::{Alignment, Axis};
pub use editor::{DiagramEditor, Target, Tool};
pub use locale::Locale;
pub use model::{
    Connection, Dash, Diagram, Group, Node, NodeStyle, Point, Port, Routing, ShapeKind, TextAlign,
};
