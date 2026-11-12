//! Reusable GTK4 redaction editor widget.

pub mod model;
pub mod view;

pub use model::{Area, AreaKind, EditModel, Tool};
pub use safe_upload_core::edit::{Rect, Redaction};
pub use view::ViewTransform;
