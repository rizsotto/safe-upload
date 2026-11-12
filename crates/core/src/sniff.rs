//! Detects formats by content, never by file name. M1 refused HEIC/HEIF
//! here; since M2 it is decoded (decision 0001), so no format is refused.
//! [`Error::UnsupportedFormat`] stays for future refusals.

use std::fmt;
use std::io::Read;
use std::path::Path;

use crate::Error;

/// An input format named in errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Format {
    /// HEIF with no HEVC brand, so the codec it needs is not known.
    Heif,
    /// HEIF with an HEVC brand: the needed codec is HEVC.
    Heic,
    Avif,
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Format::Heif => f.write_str("HEIC/HEIF"),
            Format::Heic => f.write_str("HEIC"),
            Format::Avif => f.write_str("AVIF"),
        }
    }
}

const HEVC_BRANDS: &[&[u8; 4]] = &[b"heic", b"heix", b"heim", b"heis", b"hevc", b"hevx"];
const HEIF_BRANDS: &[&[u8; 4]] = &[b"mif1", b"mif2", b"msf1"];
const AVIF_BRANDS: &[&[u8; 4]] = &[b"avif", b"avis"];

/// How many bytes `check_file` reads from the start of a file.
const HEAD_LEN: u64 = 4096;

/// The HEIF family member in `b`, from the brands of its leading `ftyp`
/// box: AVIF if it has an AVIF brand, else HEIC if it has an HEVC brand,
/// else HEIF if it has a plain HEIF brand. `None` if there is no whole
/// `ftyp` box with a 32 bit size.
pub(crate) fn heif_family(b: &[u8]) -> Option<Format> {
    if b.get(4..8) != Some(b"ftyp") {
        return None;
    }
    let size = u32::from_be_bytes(b[0..4].try_into().unwrap()) as usize;
    if size < 16 {
        return None;
    }
    let ftyp = b.get(8..size)?;
    // Major brand, minor version, then compatible brands.
    let brands: Vec<&[u8; 4]> = ftyp[..4]
        .as_chunks::<4>()
        .0
        .iter()
        .chain(ftyp[8..].as_chunks::<4>().0)
        .collect();
    if brands.iter().any(|b| AVIF_BRANDS.contains(b)) {
        Some(Format::Avif)
    } else if brands.iter().any(|b| HEVC_BRANDS.contains(b)) {
        Some(Format::Heic)
    } else if brands.iter().any(|b| HEIF_BRANDS.contains(b)) {
        Some(Format::Heif)
    } else {
        None
    }
}

/// Refuses formats this build cannot process, from the start of the file.
/// None are refused in this version.
pub fn sniff(_head: &[u8]) -> Result<(), Error> {
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

    #[test]
    fn heif_is_no_longer_refused() {
        assert!(sniff(&ftyp(b"heic", &[b"mif1", b"heic"])).is_ok());
    }

    #[test]
    fn hevc_brands_are_heic() {
        for h in [
            ftyp(b"heic", &[b"mif1", b"heic"]),
            ftyp(b"heix", &[]),
            ftyp(b"mif1", &[b"heic"]),
            ftyp(b"msf1", &[b"hevc"]),
            ftyp(b"abcd", &[b"heim"]),
            ftyp(b"abcd", &[b"heis"]),
            ftyp(b"abcd", &[b"hevx"]),
        ] {
            assert_eq!(heif_family(&h), Some(Format::Heic), "{h:?}");
        }
    }

    #[test]
    fn other_heif_brands_are_heif() {
        for h in [
            ftyp(b"mif1", &[]),
            ftyp(b"msf1", &[b"mif1"]),
            // Major brand unknown, HEIF only in the compatible list.
            ftyp(b"abcd", &[b"mif2"]),
        ] {
            assert_eq!(heif_family(&h), Some(Format::Heif), "{h:?}");
        }
    }

    #[test]
    fn avif_brands_win() {
        assert_eq!(
            heif_family(&ftyp(b"avif", &[b"mif1", b"miaf"])),
            Some(Format::Avif)
        );
        assert_eq!(heif_family(&ftyp(b"mif1", &[b"avif"])), Some(Format::Avif));
        assert_eq!(heif_family(&ftyp(b"avis", &[b"msf1"])), Some(Format::Avif));
    }

    #[test]
    fn other_files_are_not_heif() {
        assert_eq!(heif_family(&ftyp(b"isom", &[b"mp41"])), None);
        assert_eq!(heif_family(&[0xff, 0xd8, 0xff, 0xe0]), None);
        assert_eq!(heif_family(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"), None);
        assert_eq!(heif_family(&[]), None);
    }

    #[test]
    fn cut_off_or_odd_ftyp_is_not_heif_and_does_not_panic() {
        let full = ftyp(b"heic", &[b"mif1", b"heic"]);
        for len in 0..full.len() {
            assert_eq!(heif_family(&full[..len]), None, "cut at {len}");
        }
        for size in [0u32, 1, 2, 7, 8, 9, 11, 15, 25, u32::MAX] {
            let mut v = ftyp(b"heic", &[b"mif1"]);
            v[0..4].copy_from_slice(&size.to_be_bytes());
            assert_eq!(heif_family(&v), None, "size {size}");
        }
        // Size not a multiple of four: the partial brand is ignored.
        let mut v = ftyp(b"abcd", &[b"heic"]);
        v.push(0);
        v[3] = 21;
        assert_eq!(heif_family(&v), Some(Format::Heic));
        v[3] = 19;
        assert_eq!(heif_family(&v), None);
    }
}
