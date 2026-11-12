//! Detects formats this build refuses, by content and never by file name.
//! In M1 that is HEIC/HEIF (decision 0001).

use std::fmt;
use std::io::Read;
use std::path::Path;

use crate::Error;

/// An input format that this build cannot process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Format {
    Heif,
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Format::Heif => f.write_str("HEIC/HEIF"),
        }
    }
}

const HEIF_BRANDS: &[&[u8; 4]] = &[
    b"heic", b"heix", b"heim", b"heis", b"hevc", b"hevx", b"mif1", b"mif2", b"msf1",
];
const AVIF_BRANDS: &[&[u8; 4]] = &[b"avif", b"avis"];

/// How many bytes `check_file` reads from the start of a file.
const HEAD_LEN: u64 = 4096;

/// Refuses HEIF unless it is AVIF. `head` is the start of the file.
pub fn sniff(head: &[u8]) -> Result<(), Error> {
    if head.get(4..8) != Some(b"ftyp") {
        return Ok(());
    }
    let declared = u32::from_be_bytes(head[0..4].try_into().unwrap()) as usize;
    let (brands_start, box_end) = match declared {
        0 => (8, head.len().max(16)),
        1 => match head.get(8..16) {
            Some(large) => (16, u64::from_be_bytes(large.try_into().unwrap()) as usize),
            None => return Err(Error::UnsupportedFormat(Format::Heif)),
        },
        n => (8, n),
    };
    // A box too small for the major brand and minor version is malformed.
    // Treat it as HEIF, so it is refused before any decoder sees it.
    if box_end < brands_start + 8 {
        return Err(Error::UnsupportedFormat(Format::Heif));
    }
    let truncated = box_end > head.len();
    let ftyp = &head[..box_end.min(head.len())];

    // Major brand at brands_start, minor version after it, then compatible
    // brands to the end of the box.
    let mut brands: Vec<&[u8; 4]> = Vec::new();
    if let Some(major) = ftyp.get(brands_start..brands_start + 4) {
        brands.extend(major.as_chunks::<4>().0);
    }
    if let Some(rest) = ftyp.get(brands_start + 8..) {
        brands.extend(rest.as_chunks::<4>().0);
    }

    let is = |list: &[&[u8; 4]], b: &[u8; 4]| list.contains(&b);
    if brands.iter().any(|b| is(AVIF_BRANDS, b)) {
        return Ok(());
    }
    if truncated || brands.iter().any(|b| is(HEIF_BRANDS, b)) {
        return Err(Error::UnsupportedFormat(Format::Heif));
    }
    Ok(())
}

/// Reads the start of the file at `path` and runs [`sniff`] on it.
pub fn check_file(path: &Path) -> Result<(), Error> {
    let mut head = Vec::with_capacity(HEAD_LEN as usize);
    std::fs::File::open(path)?
        .take(HEAD_LEN)
        .read_to_end(&mut head)?;
    sniff(&head)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ftyp(major: &[u8; 4], compatible: &[&[u8; 4]]) -> Vec<u8> {
        let size = 16 + 4 * compatible.len() as u32;
        let mut v = size.to_be_bytes().to_vec();
        v.extend_from_slice(b"ftyp");
        v.extend_from_slice(major);
        v.extend_from_slice(&[0, 0, 0, 0]);
        for b in compatible {
            v.extend_from_slice(*b);
        }
        v
    }

    fn refused(bytes: &[u8]) -> bool {
        matches!(sniff(bytes), Err(Error::UnsupportedFormat(Format::Heif)))
    }

    #[test]
    fn heif_brands_are_refused() {
        assert!(refused(&ftyp(b"heic", &[b"mif1", b"heic"])));
        assert!(refused(&ftyp(b"heix", &[])));
        assert!(refused(&ftyp(b"mif1", &[b"heic"])));
        assert!(refused(&ftyp(b"msf1", &[b"hevc"])));
        // Major brand unknown, HEIF only in the compatible list.
        assert!(refused(&ftyp(b"abcd", &[b"mif2"])));
    }

    #[test]
    fn avif_passes() {
        assert!(sniff(&ftyp(b"avif", &[b"mif1", b"miaf"])).is_ok());
        assert!(sniff(&ftyp(b"mif1", &[b"avif"])).is_ok());
        assert!(sniff(&ftyp(b"avis", &[b"msf1"])).is_ok());
    }

    #[test]
    fn other_ftyp_brands_pass() {
        assert!(sniff(&ftyp(b"isom", &[b"mp41"])).is_ok());
    }

    #[test]
    fn jpeg_and_png_pass() {
        assert!(sniff(&[0xff, 0xd8, 0xff, 0xe0, 0, 16, b'J', b'F', b'I', b'F']).is_ok());
        assert!(sniff(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").is_ok());
        assert!(sniff(&[]).is_ok());
    }

    #[test]
    fn cut_off_headers_are_refused_without_panic() {
        let full = ftyp(b"heic", &[b"mif1", b"heic"]);
        for len in 8..full.len() {
            assert!(refused(&full[..len]), "cut at {len}");
        }
        assert!(refused(b"\0\0\0\x18ftyp"));
        assert!(refused(b"\0\0\0\x01ftypheic"));
        // Shorter than a box header: not HEIF.
        assert!(sniff(b"\0\0\0\x18fty").is_ok());
    }

    #[test]
    fn odd_box_sizes_are_refused_without_panic() {
        for size in [0u32, 1, 2, 7, 8, 9, 11, 15, 16, 17, u32::MAX] {
            let mut v = ftyp(b"heic", &[b"mif1"]);
            v[0..4].copy_from_slice(&size.to_be_bytes());
            assert!(refused(&v), "size {size}");
        }
        for large_size in [0u64, 1, 8, 15, 16, 23, 24, 28, u64::MAX] {
            let mut large = ftyp(b"heic", &[b"mif1"]);
            large[0..4].copy_from_slice(&1u32.to_be_bytes());
            large.splice(8..8, large_size.to_be_bytes());
            assert!(refused(&large), "largesize {large_size}");
        }
    }
}
