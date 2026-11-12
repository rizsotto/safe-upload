use std::time::Duration;

use crate::Error;

/// Limits for untrusted input (S-4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    pub max_pixels: u64,
    pub max_file_bytes: u64,
    pub decode_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_pixels: 200_000_000,
            max_file_bytes: 200_000_000,
            decode_timeout: Duration::from_secs(30),
        }
    }
}

impl Limits {
    pub fn check_dimensions(&self, width: u32, height: u32) -> Result<(), Error> {
        let pixels = u64::from(width) * u64::from(height);
        if pixels > self.max_pixels {
            return Err(Error::TooManyPixels {
                pixels,
                limit: self.max_pixels,
            });
        }
        Ok(())
    }

    pub fn check_file_size(&self, bytes: u64) -> Result<(), Error> {
        if bytes > self.max_file_bytes {
            return Err(Error::FileTooLarge {
                bytes,
                limit: self.max_file_bytes,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_allows_200_megapixels() {
        let l = Limits::default();
        assert!(l.check_dimensions(20_000, 10_000).is_ok());
        assert!(matches!(
            l.check_dimensions(20_000, 10_001),
            Err(Error::TooManyPixels { .. })
        ));
    }

    #[test]
    fn huge_dimensions_do_not_overflow() {
        let l = Limits::default();
        assert!(l.check_dimensions(u32::MAX, u32::MAX).is_err());
    }

    #[test]
    fn file_size_limit() {
        let l = Limits::default();
        assert!(l.check_file_size(200_000_000).is_ok());
        assert!(matches!(
            l.check_file_size(200_000_001),
            Err(Error::FileTooLarge { .. })
        ));
    }
}
