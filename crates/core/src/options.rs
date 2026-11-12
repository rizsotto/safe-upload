use crate::Error;
use crate::edit::{Rect, Redaction};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Any input with real transparency becomes PNG. Everything else
    /// becomes JPEG.
    #[default]
    Auto,
    Jpeg,
    Png,
    Webp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub target_bytes: u64,
    pub max_long_edge: Option<u32>,
    pub format: OutputFormat,
    pub keep_name: bool,
    /// Crop, in coordinates after orientation (SPEC 9.4).
    pub crop: Option<Rect>,
    /// Redactions, in coordinates after orientation and before the crop.
    pub redactions: Vec<Redaction>,
}

impl Options {
    pub fn new(target_bytes: u64) -> Self {
        Self {
            target_bytes,
            max_long_edge: None,
            format: OutputFormat::Auto,
            keep_name: false,
            crop: None,
            redactions: Vec::new(),
        }
    }
}

/// Parses a size like `2MB`, `500 KB`, `1.5MiB` or `2000000` into bytes.
///
/// `KB` and `MB` are decimal (1000, 1000000). `KiB` and `MiB` are binary.
/// A bare number is bytes. Units are not case sensitive. The result is
/// rounded down to whole bytes.
pub fn parse_size(input: &str) -> Result<u64, Error> {
    let s = input.trim();
    let split = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(s.len());
    let (number, unit) = s.split_at(split);
    let multiplier: u128 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "kb" => 1_000,
        "mb" => 1_000_000,
        "kib" => 1 << 10,
        "mib" => 1 << 20,
        _ => return Err(Error::InvalidSize("unknown unit, use KB, MB, KiB or MiB")),
    };

    let (int_part, frac_part) = number.split_once('.').unwrap_or((number, ""));
    if int_part.is_empty() && frac_part.is_empty() {
        return Err(Error::InvalidSize("expected a number, for example 2MB"));
    }
    if int_part.len() + frac_part.len() > 30 || frac_part.contains('.') {
        return Err(Error::InvalidSize("not a valid number"));
    }
    let digits = |p: &str| -> u128 {
        p.bytes()
            .fold(0u128, |acc, d| acc * 10 + u128::from(d - b'0'))
    };
    let scale = 10u128.pow(frac_part.len() as u32);
    let scaled = digits(int_part) * scale + digits(frac_part);
    let bytes = scaled * multiplier / scale;

    match u64::try_from(bytes) {
        Ok(0) => Err(Error::InvalidSize("must be at least 1 byte")),
        Ok(b) => Ok(b),
        Err(_) => Err(Error::InvalidSize("too large")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_units() {
        assert_eq!(parse_size("2MB").unwrap(), 2_000_000);
        assert_eq!(parse_size("500KB").unwrap(), 500_000);
        assert_eq!(parse_size("500 kb").unwrap(), 500_000);
        assert_eq!(parse_size("0.5MB").unwrap(), 500_000);
    }

    #[test]
    fn binary_units() {
        assert_eq!(parse_size("1.5MiB").unwrap(), 1_572_864);
        assert_eq!(parse_size("1KiB").unwrap(), 1024);
    }

    #[test]
    fn bare_number_is_bytes() {
        assert_eq!(parse_size("2000000").unwrap(), 2_000_000);
        assert_eq!(parse_size("10B").unwrap(), 10);
    }

    #[test]
    fn rounds_down() {
        assert_eq!(parse_size("1.0005KB").unwrap(), 1000);
    }

    #[test]
    fn bad_input() {
        for bad in [
            "",
            "MB",
            "abc",
            "2XB",
            "2 M B",
            "-2MB",
            "1.2.3MB",
            ".",
            "0MB",
            "0.0001B",
            "99999999999999999999MB",
        ] {
            assert!(
                matches!(parse_size(bad), Err(Error::InvalidSize(_))),
                "{bad:?} should fail"
            );
        }
    }

    #[test]
    fn options_default_to_auto_format() {
        let o = Options::new(2_000_000);
        assert_eq!(o.format, OutputFormat::Auto);
        assert!(!o.keep_name && o.max_long_edge.is_none());
    }
}
