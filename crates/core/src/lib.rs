//! Image pipeline: decode, orient, redact, strip, fit to size. No GTK, no D-Bus.

pub mod color;
pub mod decode;
pub mod edit;
pub mod encode;
mod error;
pub mod fit;
mod limits;
mod options;
pub mod orient;
pub mod output;
pub mod sniff;
pub mod verify;

pub use error::Error;
pub use limits::Limits;
pub use options::{Options, OutputFormat, parse_size};
pub use orient::Orientation;
pub use sniff::Format;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_set() {
        assert!(!super::version().is_empty());
    }
}
