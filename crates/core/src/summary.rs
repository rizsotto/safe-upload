//! What was removed, for the user (FR-8, UC-7). Lists only the kinds of
//! metadata found in the input, never their values (S-5).
//!
//! The input is hostile, so the scan is plain bounds-checked Rust and
//! reads only a fixed set of places: JPEG segments, PNG chunks, WebP
//! chunks, and in EXIF only IFD0, the Exif IFD and whether IFD1 exists.

use std::fmt;

/// A kind of metadata found in the input and not in the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Removed {
    Location,
    Camera,
    Date,
    MakerNotes,
    Thumbnail,
    OtherExif,
    Xmp,
    Iptc,
    Comment,
    Text,
    ColorProfile,
}

impl fmt::Display for Removed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Removed::Location => "Location",
            Removed::Camera => "Camera",
            Removed::Date => "Date",
            Removed::MakerNotes => "Maker notes",
            Removed::Thumbnail => "Thumbnail",
            Removed::OtherExif => "Other EXIF",
            Removed::Xmp => "XMP",
            Removed::Iptc => "IPTC",
            Removed::Comment => "Comment",
            Removed::Text => "Text",
            Removed::ColorProfile => "Color profile",
        })
    }
}

/// Sizes, resolutions and removed metadata kinds for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub input_size: (u32, u32),
    pub output_size: (u32, u32),
    /// Sorted, no duplicates. `None` if the input format is not scanned
    /// (AVIF): metadata was still removed, but we cannot say which.
    pub removed: Option<Vec<Removed>>,
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (iw, ih) = self.input_size;
        let (ow, oh) = self.output_size;
        write!(
            f,
            "{} → {}, {iw}x{ih} → {ow}x{oh}, ",
            Bytes(self.input_bytes),
            Bytes(self.output_bytes)
        )?;
        match &self.removed {
            None => f.write_str("all metadata removed"),
            Some(r) if r.is_empty() => f.write_str("no metadata found"),
            Some(r) => {
                f.write_str("removed: ")?;
                for (i, kind) in r.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{kind}")?;
                }
                Ok(())
            }
        }
    }
}

struct Bytes(u64);

impl fmt::Display for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = self.0 as f64;
        match self.0 {
            1_000_000.. => write!(f, "{:.1} MB", b / 1e6),
            1_000.. => write!(f, "{:.0} KB", b / 1e3),
            _ => write!(f, "{} B", self.0),
        }
    }
}

/// The metadata kinds in an input file. `None` for formats not scanned.
pub fn scan(bytes: &[u8]) -> Option<Vec<Removed>> {
    let mut found = Vec::new();
    if bytes.starts_with(&[0xff, 0xd8]) {
        scan_jpeg(bytes, &mut found);
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        scan_png(bytes, &mut found);
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        scan_webp(bytes, &mut found);
    } else {
        return None;
    }
    found.sort();
    found.dedup();
    Some(found)
}

fn be16(b: &[u8], i: usize) -> Option<usize> {
    Some(u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?) as usize)
}

fn be32(b: &[u8], i: usize) -> Option<usize> {
    Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?) as usize)
}

fn scan_jpeg(b: &[u8], found: &mut Vec<Removed>) {
    let mut i = 2;
    while b.get(i) == Some(&0xff) {
        let Some(&marker) = b.get(i + 1) else { return };
        if marker == 0xff {
            i += 1;
            continue;
        }
        if marker == 0xd9 || marker == 0xda {
            return;
        }
        if marker == 0x01 || (0xd0..=0xd8).contains(&marker) {
            i += 2;
            continue;
        }
        let Some(len) = be16(b, i + 2).filter(|&l| l >= 2) else {
            return;
        };
        let seg = &b[(i + 4).min(b.len())..(i + 2 + len).min(b.len())];
        match marker {
            0xe1 if seg.starts_with(b"Exif\0\0") => scan_exif(&seg[6..], found),
            0xe1 if seg.starts_with(b"http://ns.adobe.com/") => found.push(Removed::Xmp),
            0xe2 if seg.starts_with(b"ICC_PROFILE\0") => found.push(Removed::ColorProfile),
            // Multi-Picture Format: extra images, usually a large preview.
            0xe2 if seg.starts_with(b"MPF\0") => found.push(Removed::Thumbnail),
            0xed => found.push(Removed::Iptc),
            0xfe => found.push(Removed::Comment),
            _ => {}
        }
        i += 2 + len;
    }
}

fn scan_png(b: &[u8], found: &mut Vec<Removed>) {
    let mut i = 8;
    while let (Some(len), Some(kind)) = (be32(b, i), b.get(i + 4..i + 8)) {
        let data = &b[(i + 8).min(b.len())..(i + 8).saturating_add(len).min(b.len())];
        match kind {
            b"iTXt" if data.starts_with(b"XML:com.adobe.xmp\0") => found.push(Removed::Xmp),
            b"tEXt" | b"zTXt" | b"iTXt" => found.push(Removed::Text),
            b"eXIf" => scan_exif(data, found),
            b"iCCP" => found.push(Removed::ColorProfile),
            b"tIME" => found.push(Removed::Date),
            b"IEND" => return,
            _ => {}
        }
        i = i.saturating_add(12).saturating_add(len);
    }
}

fn scan_webp(b: &[u8], found: &mut Vec<Removed>) {
    let mut i = 12;
    while let (Some(kind), Some(len)) = (b.get(i..i + 4), b.get(i + 4..i + 8)) {
        let len = u32::from_le_bytes(len.try_into().unwrap()) as usize;
        let data = &b[(i + 8).min(b.len())..(i + 8).saturating_add(len).min(b.len())];
        match kind {
            b"EXIF" => scan_exif(data.strip_prefix(b"Exif\0\0").unwrap_or(data), found),
            b"XMP " => found.push(Removed::Xmp),
            b"ICCP" => found.push(Removed::ColorProfile),
            _ => {}
        }
        i = i.saturating_add(8).saturating_add(len + (len & 1));
    }
}

const MAX_ENTRIES: usize = 1000;

/// Reads a TIFF structure: tags in IFD0 and the Exif IFD, and whether
/// IFD1 (the thumbnail) exists. Bad data just ends the scan.
fn scan_exif(t: &[u8], found: &mut Vec<Removed>) {
    let little = match t.get(0..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return,
    };
    let u16_at = |i: usize| -> Option<usize> {
        let b: [u8; 2] = t.get(i..i + 2)?.try_into().ok()?;
        Some(if little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        } as usize)
    };
    let u32_at = |i: usize| -> Option<usize> {
        let b: [u8; 4] = t.get(i..i + 4)?.try_into().ok()?;
        Some(if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        } as usize)
    };
    // Returns the tags of the IFD at `off` and the offset of the next IFD.
    let ifd = |off: usize| -> Option<(Vec<(usize, usize)>, usize)> {
        let n = u16_at(off)?.min(MAX_ENTRIES);
        let mut tags = Vec::with_capacity(n);
        for k in 0..n {
            let e = off + 2 + 12 * k;
            tags.push((u16_at(e)?, u32_at(e + 8)?));
        }
        Some((tags, u32_at(off + 2 + 12 * n).unwrap_or(0)))
    };
    let Some(ifd0_off) = u32_at(4) else { return };
    let Some((ifd0, next)) = ifd(ifd0_off) else {
        return;
    };
    let mut exif_off = None;
    for &(tag, value) in &ifd0 {
        match tag {
            0x010f | 0x0110 => found.push(Removed::Camera),
            0x0132 => found.push(Removed::Date),
            0x8825 => found.push(Removed::Location),
            0x8769 => exif_off = Some(value),
            // Orientation, resolution and YCbCr layout say nothing private.
            0x0112 | 0x011a | 0x011b | 0x0128 | 0x0213 => {}
            _ => found.push(Removed::OtherExif),
        }
    }
    if let Some((exif, _)) = exif_off.filter(|&o| o != ifd0_off).and_then(ifd) {
        for (tag, _) in exif {
            match tag {
                0x9003 | 0x9004 | 0x9010 | 0x9011 => found.push(Removed::Date),
                0x927c => found.push(Removed::MakerNotes),
                0x9286 => found.push(Removed::Comment),
                _ => found.push(Removed::Camera),
            }
        }
    }
    if next != 0 && next != ifd0_off && ifd(next).is_some_and(|(tags, _)| !tags.is_empty()) {
        found.push(Removed::Thumbnail);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Big-endian TIFF: IFD0 with Make and a GPS pointer, then IFD1.
    fn tiff() -> Vec<u8> {
        let mut t = b"MM\0\x2a\0\0\0\x08".to_vec();
        t.extend_from_slice(&2u16.to_be_bytes());
        t.extend_from_slice(&[0x01, 0x0f, 0, 2, 0, 0, 0, 4, b'A', b'B', b'C', 0]);
        t.extend_from_slice(&[0x88, 0x25, 0, 4, 0, 0, 0, 1, 0, 0, 0, 0]);
        t.extend_from_slice(&38u32.to_be_bytes());
        t.extend_from_slice(&1u16.to_be_bytes());
        t.extend_from_slice(&[0x01, 0x03, 0, 3, 0, 0, 0, 1, 0, 6, 0, 0]);
        t.extend_from_slice(&0u32.to_be_bytes());
        t
    }

    fn jpeg(segments: &[(u8, &[u8])]) -> Vec<u8> {
        let mut j = vec![0xff, 0xd8];
        for (m, data) in segments {
            j.extend_from_slice(&[0xff, *m]);
            j.extend_from_slice(&((data.len() + 2) as u16).to_be_bytes());
            j.extend_from_slice(data);
        }
        j.extend_from_slice(&[0xff, 0xd9]);
        j
    }

    #[test]
    fn jpeg_kinds() {
        let exif = [b"Exif\0\0".as_slice(), &tiff()].concat();
        let j = jpeg(&[
            (0xe0, b"JFIF\0\x01\x02\0\0\x01\0\x01\0\0"),
            (0xe1, &exif),
            (0xe1, b"http://ns.adobe.com/xap/1.0/\0<x/>"),
            (0xed, b"Photoshop 3.0\0"),
            (0xfe, b"hello"),
        ]);
        assert_eq!(
            scan(&j).unwrap(),
            [
                Removed::Location,
                Removed::Camera,
                Removed::Thumbnail,
                Removed::Xmp,
                Removed::Iptc,
                Removed::Comment
            ]
        );
    }

    #[test]
    fn mpf_preview_is_a_thumbnail() {
        let j = jpeg(&[(0xe2, b"MPF\0MM\0\x2a\0\0\0\x08")]);
        assert_eq!(scan(&j).unwrap(), [Removed::Thumbnail]);
    }

    #[test]
    fn clean_jpeg_has_nothing() {
        assert_eq!(scan(&jpeg(&[(0xe0, b"JFIF\0")])).unwrap(), []);
    }

    #[test]
    fn png_kinds() {
        let mut p = b"\x89PNG\r\n\x1a\n".to_vec();
        for (kind, data) in [
            (b"tEXt", b"Author\0x".as_slice()),
            (b"eXIf", &tiff()),
            (b"IEND", b""),
        ] {
            p.extend_from_slice(&(data.len() as u32).to_be_bytes());
            p.extend_from_slice(kind);
            p.extend_from_slice(data);
            p.extend_from_slice(&[0; 4]);
        }
        assert_eq!(
            scan(&p).unwrap(),
            [
                Removed::Location,
                Removed::Camera,
                Removed::Thumbnail,
                Removed::Text
            ]
        );
    }

    #[test]
    fn garbage_does_not_panic() {
        let exif = [b"Exif\0\0MM\0\x2a\xff\xff\xff\xff".as_slice()].concat();
        assert_eq!(scan(&jpeg(&[(0xe1, &exif)])).unwrap(), []);
        assert!(scan(&[0xff, 0xd8, 0xff, 0xe1, 0xff]).is_some());
        assert!(scan(b"\x89PNG\r\n\x1a\n\xff\xff\xff\xfftEXt").is_some());
        assert!(scan(b"RIFF\0\0\0\0WEBPEXIF\xff\xff\xff\xff").is_some());
        let mut t = tiff();
        t.truncate(20);
        assert_eq!(
            scan(&jpeg(&[(0xe1, &[b"Exif\0\0".as_slice(), &t].concat())])).unwrap(),
            []
        );
        assert_eq!(scan(b"ftypavif"), None);
    }

    #[test]
    fn display_has_sizes_and_kinds_only() {
        let s = Summary {
            input_bytes: 4_100_000,
            output_bytes: 1_900_000,
            input_size: (4000, 3000),
            output_size: (1600, 1200),
            removed: Some(vec![Removed::Location, Removed::Camera]),
        };
        assert_eq!(
            s.to_string(),
            "4.1 MB → 1.9 MB, 4000x3000 → 1600x1200, removed: Location, Camera"
        );
        let none = Summary {
            removed: Some(vec![]),
            input_bytes: 900,
            output_bytes: 2_500,
            ..s.clone()
        };
        assert!(none.to_string().starts_with("900 B → 2 KB"));
        assert!(none.to_string().ends_with("no metadata found"));
    }
}
