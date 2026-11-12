//! Output verification (SPEC 9.1 step 8, S-6). Walks the JPEG markers or
//! PNG chunks of an output file and allows only image data and, if the
//! caller passes one, exactly the ICC profile we wrote ourselves. Anything
//! else is an error, so the caller fails closed.
//!
//! Every allowed segment or chunk is checked for its exact size, so no
//! bytes can hide in padding: JPEG tables are parsed, PNG chunks have fixed
//! lengths and a checked CRC. Spare channels are closed: JFIF, DRI, each
//! quantization table and every PNG chunk but IDAT appear at most once, and
//! the iCCP name is fixed. Entropy-coded data and IDAT are not decoded.

use crate::Error;

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
const ICC_TAG: &[u8] = b"ICC_PROFILE\0";

/// The only profile name allowed in a PNG iCCP chunk. The PNG encoder must
/// write exactly this name.
pub const ICC_PROFILE_NAME: &[u8] = b"icc";

/// Checks an encoded output file. Only JPEG and PNG are known; any other
/// format fails. `expected_icc` is the only ICC profile allowed, byte for
/// byte. With `None`, any ICC profile fails.
pub fn verify(bytes: &[u8], expected_icc: Option<&[u8]>) -> Result<(), Error> {
    if bytes.starts_with(&[0xff, 0xd8]) {
        verify_jpeg(bytes, expected_icc)
    } else if bytes.starts_with(PNG_SIGNATURE) {
        verify_png(bytes, expected_icc)
    } else {
        Err(fail("unknown output format"))
    }
}

fn fail(reason: impl Into<String>) -> Error {
    Error::Verify(reason.into())
}

fn check_icc(found: &[u8], expected: Option<&[u8]>) -> Result<(), Error> {
    match expected {
        Some(want) if want == found => Ok(()),
        Some(_) => Err(fail("ICC profile is not the one we wrote")),
        None => Err(fail("unexpected ICC profile")),
    }
}

fn verify_jpeg(b: &[u8], expected_icc: Option<&[u8]>) -> Result<(), Error> {
    let mut i = 2;
    let mut frames = 0;
    let mut scans = 0;
    // ICC chunks by sequence number (1-based), and the declared count.
    let mut icc: Vec<Option<&[u8]>> = Vec::new();
    let mut quant_tables = [false; 4];
    let mut seen_jfif = false;
    let mut seen_dri = false;
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
                if !icc.is_empty() {
                    let mut joined = Vec::new();
                    for part in &icc {
                        joined.extend_from_slice(
                            part.ok_or_else(|| fail("JPEG: ICC chunk missing"))?,
                        );
                    }
                    check_icc(&joined, expected_icc)?;
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
        let p = &b[i + 2..i + len];
        i += len;

        let before_frame = frames == 0;
        match marker {
            0xc0..=0xc2 => {
                frames += 1;
                let n = usize::from(*p.get(5).unwrap_or(&0));
                if n == 0 || p.len() != 6 + 3 * n {
                    return Err(fail("JPEG: bad frame header"));
                }
            }
            0xc4 => check_dht(p)?,
            0xdb => check_dqt(p, &mut quant_tables)?,
            0xdd if p.len() == 2 && !std::mem::replace(&mut seen_dri, true) => {}
            0xe0 if before_frame
                && is_plain_jfif(p)
                && !std::mem::replace(&mut seen_jfif, true) => {}
            0xe2 if before_frame && p.starts_with(ICC_TAG) => {
                let (seq, count, data) = match p[ICC_TAG.len()..] {
                    [seq, count, ref data @ ..] => (usize::from(seq), usize::from(count), data),
                    _ => return Err(fail("JPEG: short ICC chunk")),
                };
                if icc.is_empty() {
                    icc.resize(count, None);
                }
                if count == 0 || count != icc.len() || seq == 0 || seq > count {
                    return Err(fail("JPEG: bad ICC chunk numbering"));
                }
                if icc[seq - 1].replace(data).is_some() {
                    return Err(fail("JPEG: repeated ICC chunk"));
                }
            }
            0xda if before_frame => return Err(fail("JPEG: scan before the frame header")),
            0xda => {
                let n = usize::from(*p.first().unwrap_or(&0));
                if n == 0 || p.len() != 1 + 2 * n + 3 {
                    return Err(fail("JPEG: bad scan header"));
                }
                scans += 1;
                i = skip_entropy_data(b, i);
            }
            0xe2 if p.starts_with(ICC_TAG) => {
                return Err(fail("JPEG: ICC chunk after the frame header"));
            }
            0xe0..=0xef => return Err(fail(format!("JPEG: APP{} segment", marker - 0xe0))),
            0xfe => return Err(fail("JPEG: comment segment")),
            _ => return Err(fail(format!("JPEG: segment {marker:02X} not allowed"))),
        }
    }
}

/// Quantization tables must fill the segment exactly. Each table id is
/// defined once; an empty segment fails.
fn check_dqt(mut p: &[u8], defined: &mut [bool; 4]) -> Result<(), Error> {
    if p.is_empty() {
        return Err(fail("JPEG: empty quantization table segment"));
    }
    while let Some((&pq_tq, rest)) = p.split_first() {
        let size = match pq_tq >> 4 {
            0 => 64,
            1 => 128,
            _ => return Err(fail("JPEG: bad quantization table")),
        };
        let id = usize::from(pq_tq & 0x0f);
        if id > 3 || rest.len() < size || std::mem::replace(&mut defined[id], true) {
            return Err(fail("JPEG: bad or repeated quantization table"));
        }
        p = &rest[size..];
    }
    Ok(())
}

/// Huffman tables must fill the segment exactly.
fn check_dht(mut p: &[u8]) -> Result<(), Error> {
    if p.is_empty() {
        return Err(fail("JPEG: empty Huffman table segment"));
    }
    while let Some((&tc_th, rest)) = p.split_first() {
        if tc_th >> 4 > 1 || tc_th & 0x0f > 3 || rest.len() < 16 {
            return Err(fail("JPEG: bad Huffman table"));
        }
        let symbols: usize = rest[..16].iter().map(|&c| usize::from(c)).sum();
        if rest.len() < 16 + symbols {
            return Err(fail("JPEG: bad Huffman table"));
        }
        p = &rest[16 + symbols..];
    }
    Ok(())
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

/// CRC-32 as used by PNG (ISO 3309, polynomial 0xEDB88320).
pub(crate) fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (crc & 1).wrapping_neg());
        }
    }
    !crc
}

#[derive(PartialEq, PartialOrd, Clone, Copy)]
enum PngStage {
    Header,
    Palette,
    Data,
    AfterData,
}

fn verify_png(b: &[u8], expected_icc: Option<&[u8]>) -> Result<(), Error> {
    use PngStage::*;
    let mut i = PNG_SIGNATURE.len();
    let mut stage = None;
    let mut seen: Vec<[u8; 4]> = Vec::new();
    loop {
        let Some(header) = b.get(i..i + 8) else {
            return Err(fail("PNG: missing end chunk"));
        };
        let len = u32::from_be_bytes(header[0..4].try_into().unwrap()) as usize;
        let kind: &[u8; 4] = header[4..8].try_into().unwrap();
        let end = match len.checked_add(12).and_then(|n| n.checked_add(i)) {
            Some(end) if len <= 0x7fff_ffff && end <= b.len() => end,
            _ => return Err(fail("PNG: bad chunk length")),
        };
        let data = &b[i + 8..i + 8 + len];
        let crc = u32::from_be_bytes(b[end - 4..end].try_into().unwrap());
        if crc != crc32(&b[i + 4..i + 8 + len]) {
            return Err(fail("PNG: bad chunk CRC"));
        }
        i = end;

        // Every chunk but IDAT may appear only once.
        if kind != b"IDAT" {
            if seen.contains(kind) {
                return Err(fail(format!(
                    "PNG: repeated {} chunk",
                    String::from_utf8_lossy(kind)
                )));
            }
            seen.push(*kind);
        }

        // Chunk, the stage it moves to, and whether it may appear there.
        let (next, ok) = match (kind, stage) {
            (b"IHDR", None) => (Header, len == 13),
            (_, None) => return Err(fail("PNG: first chunk is not IHDR")),
            (b"iCCP", Some(Header)) => (Header, true),
            (b"sRGB", Some(Header)) => (Header, len == 1),
            (b"gAMA", Some(Header)) => (Header, len == 4),
            (b"cHRM", Some(Header)) => (Header, len == 32),
            (b"PLTE", Some(Header)) => (Palette, len > 0 && len <= 768 && len.is_multiple_of(3)),
            (b"tRNS", Some(Header | Palette)) => (Palette, len <= 256),
            (b"IDAT", Some(Header | Palette | Data)) => (Data, true),
            (b"IEND", Some(Data)) => (AfterData, len == 0),
            (
                b"IHDR" | b"iCCP" | b"sRGB" | b"gAMA" | b"cHRM" | b"PLTE" | b"tRNS" | b"IDAT"
                | b"IEND",
                _,
            ) => (Header, false),
            _ => {
                return Err(fail(format!(
                    "PNG: {} chunk not allowed",
                    String::from_utf8_lossy(kind)
                )));
            }
        };
        if !ok {
            return Err(fail(format!(
                "PNG: {} chunk in the wrong place or with a bad size",
                String::from_utf8_lossy(kind)
            )));
        }
        stage = Some(next);

        if kind == b"iCCP" {
            check_iccp(data, expected_icc)?;
        }
        if next == AfterData {
            if i != b.len() {
                return Err(fail("PNG: data after end chunk"));
            }
            return Ok(());
        }
    }
}

/// iCCP: our fixed name, NUL, method 0, zlib stream with nothing after.
fn check_iccp(data: &[u8], expected_icc: Option<&[u8]>) -> Result<(), Error> {
    let want = expected_icc.ok_or_else(|| fail("unexpected ICC profile"))?;
    let nul = ICC_PROFILE_NAME.len();
    if !data.starts_with(ICC_PROFILE_NAME) || data.get(nul) != Some(&0) {
        return Err(fail("PNG: iCCP profile name is not ours"));
    }
    if data.get(nul + 1) != Some(&0) {
        return Err(fail("PNG: bad iCCP compression method"));
    }
    let stream = &data[nul + 2..];
    let mut inflater = miniz_oxide::inflate::core::DecompressorOxide::new();
    let mut out = vec![0u8; want.len() + 1];
    let flags = miniz_oxide::inflate::core::inflate_flags::TINFL_FLAG_PARSE_ZLIB_HEADER
        | miniz_oxide::inflate::core::inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
    let (status, used, written) =
        miniz_oxide::inflate::core::decompress(&mut inflater, stream, &mut out, 0, flags);
    if status != miniz_oxide::inflate::TINFLStatus::Done || used != stream.len() {
        return Err(fail("PNG: iCCP data is not one complete zlib stream"));
    }
    check_icc(&out[..written], Some(want))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICC: &[u8] = b"our minimal sRGB profile bytes";

    fn seg(marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![0xff, marker];
        v.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        v.extend_from_slice(payload);
        v
    }

    fn icc_seg(seq: u8, count: u8, data: &[u8]) -> Vec<u8> {
        seg(
            0xe2,
            &[b"ICC_PROFILE\0".as_slice(), &[seq, count], data].concat(),
        )
    }

    const JFIF: &[u8] = b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0";

    fn frame() -> Vec<u8> {
        seg(0xc0, &[8, 0, 16, 0, 16, 1, 1, 0x11, 0])
    }

    fn scan() -> Vec<u8> {
        let mut v = seg(0xda, &[1, 1, 0, 0, 63, 0]);
        v.extend([
            0x12, 0xff, 0x00, 0x34, 0xff, 0xd0, 0x56, 0xff, 0xff, 0xd1, 0x78,
        ]);
        v
    }

    /// A JPEG marker skeleton. `head` goes after APP0, `mid` after SOF.
    fn jpeg_with(head: &[Vec<u8>], mid: &[Vec<u8>]) -> Vec<u8> {
        let mut v = vec![0xff, 0xd8];
        v.extend(seg(0xe0, JFIF));
        v.extend(head.concat());
        v.extend(seg(0xdb, &[0; 65]));
        v.extend(frame());
        v.extend(mid.concat());
        v.extend(seg(0xc4, &[0; 17]));
        v.extend(seg(0xdd, &[0, 4]));
        v.extend(scan());
        v.extend([0xff, 0xd9]);
        v
    }

    fn jpeg(head: &[Vec<u8>]) -> Vec<u8> {
        jpeg_with(head, &[])
    }

    fn rejects(bytes: &[u8], icc: Option<&[u8]>) -> bool {
        matches!(verify(bytes, icc), Err(Error::Verify(_)))
    }

    #[test]
    fn clean_jpeg_passes() {
        verify(&jpeg(&[]), None).unwrap();
        verify(&jpeg(&[]), Some(ICC)).unwrap();
    }

    #[test]
    fn jpeg_with_expected_icc_passes() {
        verify(&jpeg(&[icc_seg(1, 1, ICC)]), Some(ICC)).unwrap();
        let (a, b) = ICC.split_at(10);
        verify(&jpeg(&[icc_seg(1, 2, a), icc_seg(2, 2, b)]), Some(ICC)).unwrap();
        verify(&jpeg(&[icc_seg(2, 2, b), icc_seg(1, 2, a)]), Some(ICC)).unwrap();
    }

    #[test]
    fn jpeg_icc_payload_must_match() {
        let secret = b"Exif\0\0MM\0*GPS 47.5N SECRET";
        assert!(rejects(&jpeg(&[icc_seg(1, 1, secret)]), None));
        assert!(rejects(&jpeg(&[icc_seg(1, 1, secret)]), Some(ICC)));
        assert!(rejects(&jpeg(&[icc_seg(1, 1, ICC)]), None));
        let with_extra = [ICC, b"SECRET"].concat();
        assert!(rejects(&jpeg(&[icc_seg(1, 1, &with_extra)]), Some(ICC)));
    }

    #[test]
    fn jpeg_icc_chunks_must_be_complete_and_unique() {
        let (a, b) = ICC.split_at(10);
        let cases = [
            vec![icc_seg(1, 2, a)],
            vec![icc_seg(1, 2, a), icc_seg(1, 2, b)],
            vec![icc_seg(1, 2, a), icc_seg(2, 3, b)],
            vec![icc_seg(1, 1, ICC), icc_seg(1, 1, ICC)],
            vec![icc_seg(0, 1, ICC)],
            vec![icc_seg(2, 1, ICC)],
            vec![seg(0xe2, b"ICC_PROFILE\0\x01")],
        ];
        for c in cases {
            assert!(rejects(&jpeg(&c), Some(ICC)));
        }
    }

    #[test]
    fn jpeg_icc_after_frame_or_between_scans_fails() {
        assert!(rejects(&jpeg_with(&[], &[icc_seg(1, 1, ICC)]), Some(ICC)));
        let mut v = jpeg(&[]);
        let eoi = v.len() - 2;
        let mut extra = icc_seg(1, 1, ICC);
        extra.extend(scan());
        v.splice(eoi..eoi, extra);
        assert!(rejects(&v, Some(ICC)));
        let secret = icc_seg(1, 1, b"Exif GPS SECRET");
        let mut w = jpeg(&[]);
        let eoi = w.len() - 2;
        w.splice(eoi..eoi, [secret, scan()].concat());
        assert!(rejects(&w, None));
    }

    #[test]
    fn progressive_jpeg_with_two_scans_passes() {
        let mut v = jpeg(&[]);
        let eoi = v.len() - 2;
        let mut more = seg(
            0xc4,
            &[0x10; 1]
                .iter()
                .chain(&[0; 16])
                .copied()
                .collect::<Vec<_>>(),
        );
        more.extend(scan());
        v.splice(eoi..eoi, more);
        verify(&v, None).unwrap();
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
            assert!(
                rejects(&jpeg(std::slice::from_ref(&c)), None),
                "{:02X?}",
                &c[..6]
            );
        }
    }

    #[test]
    fn jpeg_tables_with_padding_fail() {
        let padded = |marker, payload: &[u8]| {
            let mut v = jpeg(&[]);
            let eoi = v.len() - 2;
            v.splice(eoi - scan().len()..eoi - scan().len(), seg(marker, payload));
            v
        };
        assert!(rejects(
            &padded(0xdb, &[[0u8; 65].as_slice(), b"SECRET"].concat()),
            None
        ));
        assert!(rejects(
            &padded(0xc4, &[[0u8; 17].as_slice(), b"SECRET"].concat()),
            None
        ));
        assert!(rejects(&padded(0xdd, &[0, 4, b'S']), None));
        // DHT that declares 3 symbols but has 2.
        let mut dht = vec![0u8; 17];
        dht[1] = 3;
        dht.extend([1, 2]);
        assert!(rejects(&padded(0xc4, &dht), None));
        // SOF with extra bytes.
        let mut v = jpeg(&[]);
        let sof = v.windows(2).position(|w| w == [0xff, 0xc0]).unwrap();
        v.splice(
            sof..sof + frame().len(),
            seg(0xc0, &[8, 0, 16, 0, 16, 1, 1, 0x11, 0, 9]),
        );
        assert!(rejects(&v, None));
    }

    #[test]
    fn jfif_with_thumbnail_fails() {
        let mut v = vec![0xff, 0xd8];
        v.extend(seg(0xe0, b"JFIF\0\x01\x01\0\0\x01\0\x01\x01\x01\0\0\0"));
        v.extend(&jpeg(&[])[2 + 18..]);
        assert!(rejects(&v, None));
    }

    #[test]
    fn second_soi_fails() {
        let mut v = jpeg(&[]);
        v.splice(2..2, [0xff, 0xd8]);
        assert!(rejects(&v, None));
        let mut two = jpeg(&[]);
        two.extend(jpeg(&[]));
        assert!(rejects(&two, None));
    }

    #[test]
    fn broken_jpeg_structure_fails() {
        let good = jpeg(&[]);
        assert!(rejects(&good[..good.len() - 2], None), "no EOI");
        assert!(rejects(&[good.as_slice(), b"trailer"].concat(), None));
        for len in 2..good.len() - 1 {
            assert!(rejects(&good[..len], None), "cut at {len}");
        }
        assert!(rejects(&[0xff, 0xd8, 0xff, 0xd9], None), "no frame");
        let mut bad_len = good.clone();
        bad_len[5] = 1;
        assert!(rejects(&bad_len, None));
    }

    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut v = (data.len() as u32).to_be_bytes().to_vec();
        v.extend_from_slice(kind);
        v.extend_from_slice(data);
        let crc = crc32(&v[4..]);
        v.extend_from_slice(&crc.to_be_bytes());
        v
    }

    fn iccp(profile: &[u8]) -> Vec<u8> {
        let z = miniz_oxide::deflate::compress_to_vec_zlib(profile, 6);
        chunk(
            b"iCCP",
            &[ICC_PROFILE_NAME, b"\0\0".as_slice(), &z].concat(),
        )
    }

    fn png_with(head: &[Vec<u8>], tail: &[Vec<u8>]) -> Vec<u8> {
        let mut v = PNG_SIGNATURE.to_vec();
        v.extend(chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]));
        v.extend(head.concat());
        v.extend(chunk(b"IDAT", b"pixels"));
        v.extend(chunk(b"IDAT", b"more"));
        v.extend(tail.concat());
        v.extend(chunk(b"IEND", b""));
        v
    }

    fn png(head: &[Vec<u8>]) -> Vec<u8> {
        png_with(head, &[])
    }

    #[test]
    fn clean_png_passes() {
        verify(&png(&[]), None).unwrap();
        verify(
            &png(&[
                iccp(ICC),
                chunk(b"sRGB", &[0]),
                chunk(b"gAMA", &[0; 4]),
                chunk(b"cHRM", &[0; 32]),
                chunk(b"PLTE", &[0; 3]),
                chunk(b"tRNS", &[0]),
            ]),
            Some(ICC),
        )
        .unwrap();
    }

    #[test]
    fn png_icc_must_match() {
        assert!(rejects(&png(&[iccp(ICC)]), None));
        assert!(rejects(&png(&[iccp(b"Exif GPS SECRET")]), Some(ICC)));
        assert!(rejects(
            &png(&[iccp(&[ICC, b"SECRET"].concat())]),
            Some(ICC)
        ));
        assert!(rejects(
            &png(&[chunk(b"iCCP", b"icc\0\0not zlib")]),
            Some(ICC)
        ));
        assert!(rejects(&png(&[iccp(ICC), iccp(ICC)]), Some(ICC)));
        let z = miniz_oxide::deflate::compress_to_vec_zlib(ICC, 6);
        let trailing = chunk(b"iCCP", &[b"icc\0\0".as_slice(), &z, b"SECRET"].concat());
        assert!(rejects(&png(&[trailing]), Some(ICC)));
    }

    #[test]
    fn png_icc_after_idat_fails() {
        assert!(rejects(&png_with(&[], &[iccp(ICC)]), Some(ICC)));
        assert!(rejects(&png_with(&[], &[chunk(b"iCCP", b"junk")]), None));
        assert!(rejects(
            &png(&[chunk(b"PLTE", &[0; 3]), iccp(ICC)]),
            Some(ICC)
        ));
    }

    #[test]
    fn png_known_chunks_with_wrong_length_fail() {
        for c in [
            chunk(b"sRGB", &[0, 1]),
            chunk(b"gAMA", &[0; 5]),
            chunk(b"cHRM", &[0; 33]),
            chunk(b"PLTE", &[0; 4]),
            chunk(b"tRNS", &[0; 257]),
        ] {
            assert!(rejects(&png(&[c]), None));
        }
        let mut v = PNG_SIGNATURE.to_vec();
        v.extend(chunk(b"IHDR", &[0; 14]));
        v.extend(chunk(b"IDAT", b"x"));
        v.extend(chunk(b"IEND", b""));
        assert!(rejects(&v, None));
        let mut w = png(&[]);
        let n = w.len();
        w.splice(n - 12..n, chunk(b"IEND", b"x"));
        assert!(rejects(&w, None));
    }

    #[test]
    fn png_bad_crc_fails() {
        let mut v = png(&[]);
        let n = v.len();
        v[n - 1] ^= 1;
        assert!(rejects(&v, None));
    }

    #[test]
    fn png_metadata_chunks_fail() {
        for kind in [
            b"tEXt", b"zTXt", b"iTXt", b"eXIf", b"tIME", b"pHYs", b"caBX",
        ] {
            assert!(rejects(&png(&[chunk(kind, b"data")]), None), "{kind:?}");
        }
    }

    #[test]
    fn broken_png_structure_fails() {
        let good = png(&[]);
        assert!(rejects(&[good.as_slice(), b"trailer"].concat(), None));
        for len in 8..good.len() {
            assert!(rejects(&good[..len], None), "cut at {len}");
        }
        let mut no_ihdr = PNG_SIGNATURE.to_vec();
        no_ihdr.extend(chunk(b"IEND", b""));
        assert!(rejects(&no_ihdr, None));
        let mut huge = PNG_SIGNATURE.to_vec();
        huge.extend([0xff, 0xff, 0xff, 0xff]);
        huge.extend(b"IHDR");
        assert!(rejects(&huge, None));
        // IDAT runs must be consecutive.
        let mut v = PNG_SIGNATURE.to_vec();
        v.extend(chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]));
        v.extend(chunk(b"IDAT", b"a"));
        v.extend(chunk(b"gAMA", &[0; 4]));
        v.extend(chunk(b"IDAT", b"b"));
        v.extend(chunk(b"IEND", b""));
        assert!(rejects(&v, None));
    }

    #[test]
    fn crc_matches_known_value() {
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
    }

    #[test]
    fn unknown_format_fails() {
        assert!(rejects(b"RIFF\0\0\0\0WEBPVP8 ", None));
        assert!(rejects(&[], None));
    }

    #[test]
    fn png_iccp_name_must_be_ours() {
        let z = miniz_oxide::deflate::compress_to_vec_zlib(ICC, 6);
        let named = |name: &[u8]| chunk(b"iCCP", &[name, b"\0\0".as_slice(), &z].concat());
        verify(&png(&[named(ICC_PROFILE_NAME)]), Some(ICC)).unwrap();
        assert!(rejects(&png(&[named(b"GPS 47.5N Jane Doe")]), Some(ICC)));
        assert!(rejects(&png(&[named(b"ICC")]), Some(ICC)));
    }

    #[test]
    fn png_repeated_chunks_fail() {
        for c in [
            chunk(b"tRNS", &[0]),
            chunk(b"gAMA", &[0; 4]),
            chunk(b"cHRM", &[0; 32]),
            chunk(b"sRGB", &[0]),
            chunk(b"PLTE", &[0; 3]),
        ] {
            assert!(rejects(&png(&[c.clone(), c]), None));
        }
    }

    #[test]
    fn jpeg_repeated_or_empty_segments_fail() {
        assert!(rejects(&jpeg(&[seg(0xe0, JFIF)]), None));
        let mut v = jpeg(&[]);
        let sof = v.windows(2).position(|w| w == [0xff, 0xc0]).unwrap();
        v.splice(sof..sof, seg(0xdb, &[]));
        assert!(rejects(&v, None));
        let mut w = jpeg(&[]);
        w.splice(sof..sof, [seg(0xdb, &[]), seg(0xdb, &[])].concat());
        assert!(rejects(&w, None));
    }

    #[test]
    fn jpeg_scan_before_frame_fails() {
        // SOS first, then ICC and JFIF "before the frame", then SOF.
        let mut v = vec![0xff, 0xd8];
        v.extend(seg(0xdb, &[0; 65]));
        v.extend(seg(0xc4, &[0; 17]));
        v.extend(scan());
        v.extend(seg(0xe0, JFIF));
        v.extend(icc_seg(1, 1, ICC));
        v.extend(frame());
        v.extend(scan());
        v.extend([0xff, 0xd9]);
        assert!(rejects(&v, Some(ICC)));
    }

    #[test]
    fn huge_png_length_does_not_overflow() {
        let mut v = png(&[]);
        let at = PNG_SIGNATURE.len() + 25;
        v.splice(at..at, [0x7f, 0xff, 0xff, 0xff, b'I', b'D', b'A', b'T']);
        assert!(rejects(&v, None));
    }
}
