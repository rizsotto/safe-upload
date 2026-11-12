//! Output verification (SPEC 9.1 step 8, S-6). Walks the JPEG markers or
//! PNG chunks of an output file and allows only image data and an optional
//! ICC profile. Anything else is an error, so the caller fails closed.

use crate::Error;

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Checks an encoded output file. Only JPEG and PNG are known; any other
/// format fails.
pub fn verify(bytes: &[u8]) -> Result<(), Error> {
    if bytes.starts_with(&[0xff, 0xd8]) {
        verify_jpeg(bytes)
    } else if bytes.starts_with(PNG_SIGNATURE) {
        verify_png(bytes)
    } else {
        Err(fail("unknown output format"))
    }
}

fn fail(reason: impl Into<String>) -> Error {
    Error::Verify(reason.into())
}

fn verify_jpeg(b: &[u8]) -> Result<(), Error> {
    let mut i = 2;
    let mut frames = 0;
    let mut scans = 0;
    loop {
        if b.get(i) != Some(&0xff) {
            return Err(fail("JPEG: missing marker or end of image"));
        }
        while b.get(i) == Some(&0xff) {
            i += 1;
        }
        let Some(&marker) = b.get(i) else {
            return Err(fail("JPEG: missing end of image"));
        };
        i += 1;

        match marker {
            0xd9 => {
                if i != b.len() {
                    return Err(fail("JPEG: data after end of image"));
                }
                if frames != 1 || scans == 0 {
                    return Err(fail("JPEG: needs one frame and at least one scan"));
                }
                return Ok(());
            }
            0xd8 => return Err(fail("JPEG: second start of image (embedded image)")),
            0x00 | 0x01 | 0xd0..=0xd7 => {
                return Err(fail(format!("JPEG: unexpected marker {marker:02X}")));
            }
            _ => {}
        }

        let len = match b.get(i..i + 2) {
            Some(l) => usize::from(u16::from_be_bytes([l[0], l[1]])),
            None => return Err(fail("JPEG: truncated segment")),
        };
        if len < 2 || i + len > b.len() {
            return Err(fail("JPEG: bad segment length"));
        }
        let payload = &b[i + 2..i + len];
        i += len;

        match marker {
            0xc0..=0xc2 => frames += 1,
            0xc4 | 0xdb | 0xdd => {}
            0xe0 if is_plain_jfif(payload) => {}
            0xe2 if payload.starts_with(b"ICC_PROFILE\0") => {}
            0xda => {
                scans += 1;
                i = skip_entropy_data(b, i);
            }
            0xe0..=0xef => return Err(fail(format!("JPEG: APP{} segment", marker - 0xe0))),
            0xfe => return Err(fail("JPEG: comment segment")),
            _ => return Err(fail(format!("JPEG: segment {marker:02X} not allowed"))),
        }
    }
}

/// A JFIF APP0 segment with no thumbnail. JFXX (thumbnail) segments fail.
fn is_plain_jfif(p: &[u8]) -> bool {
    p.len() == 14 && p.starts_with(b"JFIF\0") && p[12] == 0 && p[13] == 0
}

/// Returns the index of the next real marker after entropy-coded data.
/// Stuffed bytes (FF 00), restart markers (FF D0 to D7) and fill bytes are
/// part of the data.
fn skip_entropy_data(b: &[u8], mut i: usize) -> usize {
    while i < b.len() {
        if b[i] == 0xff {
            let mut j = i + 1;
            while b.get(j) == Some(&0xff) {
                j += 1;
            }
            match b.get(j) {
                Some(0x00) | Some(0xd0..=0xd7) => i = j + 1,
                _ => return i,
            }
        } else {
            i += 1;
        }
    }
    i
}

fn verify_png(b: &[u8]) -> Result<(), Error> {
    const ALLOWED: &[&[u8; 4]] = &[
        b"IHDR", b"PLTE", b"tRNS", b"IDAT", b"IEND", b"iCCP", b"sRGB", b"gAMA", b"cHRM",
    ];
    let mut i = PNG_SIGNATURE.len();
    let mut first = true;
    loop {
        let Some(header) = b.get(i..i + 8) else {
            return Err(fail("PNG: missing end chunk"));
        };
        let len = u32::from_be_bytes(header[0..4].try_into().unwrap()) as usize;
        let kind: &[u8; 4] = header[4..8].try_into().unwrap();
        let end = i + 8 + len + 4;
        if len > 0x7fff_ffff || end > b.len() {
            return Err(fail("PNG: bad chunk length"));
        }
        i = end;

        if first && kind != b"IHDR" {
            return Err(fail("PNG: first chunk is not IHDR"));
        }
        first = false;
        if !ALLOWED.contains(&kind) {
            return Err(fail(format!(
                "PNG: {} chunk not allowed",
                String::from_utf8_lossy(kind)
            )));
        }
        if kind == b"IEND" {
            if i != b.len() {
                return Err(fail("PNG: data after end chunk"));
            }
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![0xff, marker];
        v.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        v.extend_from_slice(payload);
        v
    }

    const JFIF: &[u8] = b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0";

    /// A JPEG marker skeleton. `extra` goes after APP0. The entropy data
    /// has stuffed bytes and restart markers.
    fn jpeg(extra: &[Vec<u8>]) -> Vec<u8> {
        let mut v = vec![0xff, 0xd8];
        v.extend(seg(0xe0, JFIF));
        for e in extra {
            v.extend(e);
        }
        v.extend(seg(0xdb, &[0; 65]));
        v.extend(seg(0xc0, &[8, 0, 16, 0, 16, 1, 1, 0x11, 0]));
        v.extend(seg(0xc4, &[0; 20]));
        v.extend(seg(0xdd, &[0, 4]));
        v.extend(seg(0xda, &[1, 1, 0, 0, 63, 0]));
        v.extend([
            0x12, 0xff, 0x00, 0x34, 0xff, 0xd0, 0x56, 0xff, 0xff, 0xd1, 0x78,
        ]);
        v.extend([0xff, 0xd9]);
        v
    }

    fn rejects(bytes: &[u8]) -> bool {
        matches!(verify(bytes), Err(Error::Verify(_)))
    }

    #[test]
    fn clean_jpeg_passes() {
        verify(&jpeg(&[])).unwrap();
    }

    #[test]
    fn jpeg_with_icc_passes() {
        verify(&jpeg(&[seg(0xe2, b"ICC_PROFILE\0\x01\x01profile")])).unwrap();
    }

    #[test]
    fn progressive_jpeg_with_two_scans_passes() {
        let mut v = jpeg(&[]);
        let eoi = v.len() - 2;
        let mut scan = seg(0xc4, &[0; 5]);
        scan.extend(seg(0xda, &[1, 1, 0, 1, 63, 0]));
        scan.extend([0x9a, 0xbc]);
        v.splice(eoi..eoi, scan);
        verify(&v).unwrap();
    }

    #[test]
    fn jpeg_metadata_segments_fail() {
        let cases = [
            seg(0xe1, b"Exif\0\0MM\0*"),
            seg(0xe1, b"http://ns.adobe.com/xap/1.0/\0<x:xmpmeta/>"),
            seg(0xed, b"Photoshop 3.0\08BIM"),
            seg(0xfe, b"a comment"),
            seg(0xe0, b"JFXX\0\x10thumb"),
            seg(0xe2, b"MPF\0II*\0"),
            seg(0xeb, b"JP"),
        ];
        for c in cases {
            assert!(rejects(&jpeg(std::slice::from_ref(&c))), "{:02X?}", &c[..6]);
        }
    }

    #[test]
    fn jfif_with_thumbnail_fails() {
        let mut v = vec![0xff, 0xd8];
        v.extend(seg(0xe0, b"JFIF\0\x01\x01\0\0\x01\0\x01\x01\x01\0\0\0"));
        v.extend(&jpeg(&[])[2 + 18..]);
        assert!(rejects(&v));
    }

    #[test]
    fn second_soi_fails() {
        let mut v = jpeg(&[]);
        v.splice(2..2, [0xff, 0xd8]);
        assert!(rejects(&v));
        // A whole second image after the first EOI.
        let mut two = jpeg(&[]);
        two.extend(jpeg(&[]));
        assert!(rejects(&two));
    }

    #[test]
    fn broken_jpeg_structure_fails() {
        let good = jpeg(&[]);
        assert!(rejects(&good[..good.len() - 2]), "no EOI");
        assert!(
            rejects(&[good.as_slice(), b"trailer"].concat()),
            "data after EOI"
        );
        for len in 2..good.len() - 1 {
            assert!(rejects(&good[..len]), "cut at {len}");
        }
        assert!(rejects(&[0xff, 0xd8, 0xff, 0xd9]), "no frame");
        let mut bad_len = good.clone();
        bad_len[5] = 1; // APP0 length 1
        assert!(rejects(&bad_len));
    }

    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut v = (data.len() as u32).to_be_bytes().to_vec();
        v.extend_from_slice(kind);
        v.extend_from_slice(data);
        v.extend_from_slice(&[0; 4]);
        v
    }

    fn png(extra: &[Vec<u8>]) -> Vec<u8> {
        let mut v = PNG_SIGNATURE.to_vec();
        v.extend(chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]));
        for e in extra {
            v.extend(e);
        }
        v.extend(chunk(b"IDAT", b"pixels"));
        v.extend(chunk(b"IEND", b""));
        v
    }

    #[test]
    fn clean_png_passes() {
        verify(&png(&[])).unwrap();
        verify(&png(&[
            chunk(b"iCCP", b"icc\0\0data"),
            chunk(b"sRGB", &[0]),
            chunk(b"gAMA", &[0; 4]),
            chunk(b"cHRM", &[0; 32]),
            chunk(b"PLTE", &[0; 3]),
            chunk(b"tRNS", &[0]),
        ]))
        .unwrap();
    }

    #[test]
    fn png_metadata_chunks_fail() {
        for kind in [
            b"tEXt", b"zTXt", b"iTXt", b"eXIf", b"tIME", b"pHYs", b"caBX",
        ] {
            assert!(rejects(&png(&[chunk(kind, b"data")])), "{kind:?}");
        }
    }

    #[test]
    fn broken_png_structure_fails() {
        let good = png(&[]);
        assert!(
            rejects(&[good.as_slice(), b"trailer"].concat()),
            "data after IEND"
        );
        for len in 8..good.len() {
            assert!(rejects(&good[..len]), "cut at {len}");
        }
        let mut no_ihdr = PNG_SIGNATURE.to_vec();
        no_ihdr.extend(chunk(b"IEND", b""));
        assert!(rejects(&no_ihdr));
        let mut huge = PNG_SIGNATURE.to_vec();
        huge.extend([0xff, 0xff, 0xff, 0xff]);
        huge.extend(b"IHDR");
        assert!(rejects(&huge));
    }

    #[test]
    fn unknown_format_fails() {
        assert!(rejects(b"RIFF\0\0\0\0WEBPVP8 "));
        assert!(rejects(&[]));
    }
}
