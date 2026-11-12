//! Image pipeline: decode, orient, redact, strip, fit to size. No GTK, no D-Bus.

mod error;
mod limits;
mod options;
pub mod output;
pub mod sniff;
pub mod verify;

pub use error::Error;
pub use limits::Limits;
pub use options::{Options, OutputFormat, parse_size};
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
