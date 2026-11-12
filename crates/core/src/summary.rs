//! What was removed, for the user (FR-8, UC-7). Lists only the kinds of
//! metadata found in the input, never their values (S-5).
//!
//! The input is hostile, so the scan is plain bounds-checked Rust and
//! reads only a fixed set of places: JPEG segments, PNG chunks, WebP
//! chunks, HEIF `Exif` and XMP items, and in EXIF only IFD0, the Exif IFD
//! and whether IFD1 exists.

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
    } else if is_heif(bytes) {
        scan_heif(bytes, &mut found);
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

/// ISOBMFF boxes in `b` as (type, payload). Payloads are cut at the end
/// of `b`; a box with a bad size ends the walk.
fn boxes(b: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
    let mut i = 0usize;
    std::iter::from_fn(move || {
        let size = be32(b, i)?;
        let kind = b.get(i + 4..i + 8)?;
        let (start, size) = match size {
            0 => (i + 8, b.len() - i),
            1 => (i + 16, usize::try_from(be64(b, i + 8)?).ok()?),
            n => (i + 8, n),
        };
        let end = i.checked_add(size)?;
        if end < start {
            return None;
        }
        let data = &b[start.min(b.len())..end.min(b.len())];
        i = end;
        Some((kind, data))
    })
}

fn be64(b: &[u8], i: usize) -> Option<u64> {
    Some(u64::from_be_bytes(b.get(i..i + 8)?.try_into().ok()?))
}

/// An unsigned big-endian integer of 0, 2, 4 or 8 bytes.
fn uint(b: &[u8], i: usize, size: usize) -> Option<usize> {
    match size {
        0 => Some(0),
        2 => be16(b, i),
        4 => be32(b, i),
        8 => usize::try_from(be64(b, i)?).ok(),
        _ => None,
    }
}

const HEIF_BRANDS: &[&[u8]] = &[
    b"heic", b"heix", b"heim", b"heis", b"hevc", b"hevx", b"mif1", b"mif2", b"msf1",
];

/// An `ftyp` box with a HEIF brand and no AVIF brand (AVIF is not scanned).
fn is_heif(b: &[u8]) -> bool {
    let Some((b"ftyp", ftyp)) = boxes(b).next() else {
        return false;
    };
    let brands: Vec<&[u8]> = ftyp
        .get(..4)
        .into_iter()
        .chain(
            ftyp.get(8..)
                .unwrap_or(&[])
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| c.as_slice()),
        )
        .collect();
    !brands.iter().any(|b| *b == b"avif" || *b == b"avis")
        && brands.iter().any(|b| HEIF_BRANDS.contains(b))
}

/// Reads the top-level `meta` box: `Exif` items are scanned like other
/// EXIF, `mime` items with RDF/XML are XMP.
fn scan_heif(b: &[u8], found: &mut Vec<Removed>) {
    let Some((_, meta)) = boxes(b).find(|(kind, _)| *kind == b"meta") else {
        return;
    };
    let (mut exif_items, mut iloc, mut idat) = (Vec::new(), None, None);
    for (kind, data) in boxes(meta.get(4..).unwrap_or(&[])) {
        match kind {
            b"iinf" => scan_iinf(data, &mut exif_items, found),
            b"iloc" => iloc = Some(data),
            b"idat" => idat = Some(data),
            _ => {}
        }
    }
    for id in exif_items {
        let data = iloc.and_then(|l| item_data(b, l, idat.unwrap_or(&[]), id));
        // The payload starts with the offset of the TIFF header.
        let tiff = data
            .as_deref()
            .and_then(|d| d.get(be32(d, 0)?.checked_add(4)?..));
        match tiff {
            Some(t) => scan_exif(t, found),
            None => found.push(Removed::OtherExif),
        }
    }
}

/// Item infos (`infe` version 2 and 3): collects `Exif` item IDs and
/// reports XMP `mime` items.
fn scan_iinf(b: &[u8], exif_items: &mut Vec<usize>, found: &mut Vec<Removed>) {
    let start = if b.first() == Some(&0) { 6 } else { 8 };
    for (kind, e) in boxes(b.get(start..).unwrap_or(&[])).take(MAX_ENTRIES) {
        let version = e.first().copied().unwrap_or(0);
        if kind != b"infe" || version < 2 {
            continue;
        }
        let (id, at) = if version == 2 {
            (be16(e, 4), 8)
        } else {
            (be32(e, 4), 10)
        };
        match (id, e.get(at..at + 4)) {
            (Some(id), Some(b"Exif")) => exif_items.push(id),
            (_, Some(b"mime")) => {
                let content_type = e.get(at + 4..).unwrap_or(&[]);
                if content_type.starts_with(b"application/rdf+xml\0") {
                    found.push(Removed::Xmp);
                }
            }
            _ => {}
        }
    }
}

/// The bytes of item `id` from `iloc`: in the file (construction method
/// 0) or in `idat` (method 1). `None` for anything else or bad offsets.
fn item_data(file: &[u8], iloc: &[u8], idat: &[u8], id: usize) -> Option<Vec<u8>> {
    let version = *iloc.first()?;
    let sizes = be16(iloc, 4)?;
    let (off_size, len_size) = (sizes >> 12, (sizes >> 8) & 0xf);
    let (base_size, index_size) = ((sizes >> 4) & 0xf, sizes & 0xf);
    let id_size = if version < 2 { 2 } else { 4 };
    let mut i = 6;
    let count = uint(iloc, i, id_size)?;
    i += id_size;
    for _ in 0..count.min(MAX_ENTRIES) {
        let item = uint(iloc, i, id_size)?;
        i += id_size;
        let method = if version == 0 {
            0
        } else {
            i += 2;
            be16(iloc, i - 2)? & 0xf
        };
        i += 2;
        let base = uint(iloc, i, base_size)?;
        i += base_size;
        let extents = be16(iloc, i)?;
        i += 2;
        let mut data = Vec::new();
        for _ in 0..extents.min(MAX_ENTRIES) {
            if version > 0 {
                i += index_size;
            }
            let off = uint(iloc, i, off_size)?;
            i += off_size;
            let len = uint(iloc, i, len_size)?;
            i += len_size;
            if item == id {
                let src = match method {
                    0 => file,
                    1 => idat,
                    _ => return None,
                };
                // All extents together are never larger than their source.
                if data.len().saturating_add(len) > src.len() {
                    return None;
                }
                let start = base.checked_add(off)?;
                data.extend_from_slice(src.get(start..start.checked_add(len)?)?);
            }
        }
        if item == id {
            return Some(data);
        }
    }
    None
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

    fn bx(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        [
            &((data.len() + 8) as u32).to_be_bytes(),
            kind.as_slice(),
            data,
        ]
        .concat()
    }

    /// HEIF with an Exif item stored in `idat` (construction method 1) and
    /// an XMP `mime` item.
    fn heif() -> Vec<u8> {
        let exif = [&6u32.to_be_bytes(), b"Exif\0\0".as_slice(), &tiff()].concat();
        let infe = |id: u8, kind: &[u8; 4], rest: &[u8]| {
            bx(
                b"infe",
                &[&[2, 0, 0, 0, 0, id, 0, 0], kind.as_slice(), rest].concat(),
            )
        };
        let iinf = bx(
            b"iinf",
            &[
                [0, 0, 0, 0, 0, 2].as_slice(),
                &infe(1, b"Exif", b""),
                &infe(2, b"mime", b"application/rdf+xml\0"),
            ]
            .concat(),
        );
        // Version 1, offset and length 4 bytes, no base offset, one item:
        // id 1, method 1, one extent at 0 of the Exif length.
        let iloc = bx(
            b"iloc",
            &[
                [1, 0, 0, 0, 0x44, 0x00, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1].as_slice(),
                &0u32.to_be_bytes(),
                &(exif.len() as u32).to_be_bytes(),
            ]
            .concat(),
        );
        let meta = bx(
            b"meta",
            &[[0, 0, 0, 0].as_slice(), &iinf, &iloc, &bx(b"idat", &exif)].concat(),
        );
        [bx(b"ftyp", b"heic\0\0\0\0mif1heic"), meta].concat()
    }

    #[test]
    fn heif_kinds() {
        assert_eq!(
            scan(&heif()).unwrap(),
            [
                Removed::Location,
                Removed::Camera,
                Removed::Thumbnail,
                Removed::Xmp
            ]
        );
    }

    #[test]
    fn heif_without_meta_has_nothing() {
        assert_eq!(scan(&bx(b"ftyp", b"heic\0\0\0\0mif1heic")).unwrap(), []);
    }

    #[test]
    fn heif_garbage_does_not_panic() {
        let h = heif();
        for n in 0..h.len() {
            scan(&h[..n]);
        }
        let mut bad = h.clone();
        for i in 0..bad.len() {
            bad[i] ^= 0xff;
            scan(&bad);
            bad[i] ^= 0xff;
        }
        let huge = [
            bx(b"ftyp", b"mif1\0\0\0\0heic"),
            b"\xff\xff\xff\xffmeta\0".to_vec(),
        ]
        .concat();
        assert!(scan(&huge).is_some());
        let large = [
            bx(b"ftyp", b"heic\0\0\0\0"),
            b"\0\0\0\x01meta\xff\xff\xff\xff\xff\xff\xff\xff".to_vec(),
        ]
        .concat();
        assert!(scan(&large).is_some());
    }

    #[test]
    fn avif_is_not_scanned_as_heif() {
        assert_eq!(scan(&bx(b"ftyp", b"avif\0\0\0\0mif1avif")), None);
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
