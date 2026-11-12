//! Reusable GTK4 redaction editor widget.

pub mod a11y;
mod canvas;
pub mod editor;
pub mod keys;
pub mod model;
pub mod preview;
pub mod view;

pub use editor::RedactionEditor;
pub use model::{Area, AreaKind, EditModel, Tool};
pub use safe_upload_core::edit::{Rect, Redaction};
pub use view::ViewTransform;
