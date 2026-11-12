//! Reusable GTK4 redaction editor widget.

mod canvas;
pub mod editor;
pub mod model;
pub mod preview;
pub mod view;

pub use editor::RedactionEditor;
pub use model::{Area, AreaKind, EditModel, Tool};
pub use safe_upload_core::edit::{Rect, Redaction};
pub use view::ViewTransform;
