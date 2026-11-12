//! Encoders (SPEC 8). An encoder gets only sRGB pixels plus, optionally,
//! our own sRGB profile. It cannot get anything from the source file, so
//! it cannot copy source metadata (SPEC 8, key rule): the types allow no
//! other profile. Its output passes [`crate::verify::verify`] with the
//! same profile.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;

use image::RgbaImage;

use crate::Error;
use crate::verify::{ICC_PROFILE_NAME, crc32};

/// The only ICC profiles an encoder can embed: our own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputProfile {
    /// moxcms's sRGB profile (SPEC 9.1 step 5).
    Srgb,
}

impl OutputProfile {
    /// The profile bytes. Pass the same bytes to `verify`.
    pub fn bytes(self) -> &'static [u8] {
        static SRGB: OnceLock<Vec<u8>> = OnceLock::new();
        match self {
            OutputProfile::Srgb => SRGB.get_or_init(|| {
                moxcms::ColorProfile::new_srgb()
                    .encode()
                    .expect("moxcms encodes its own sRGB profile")
            }),
        }
    }
}

/// sRGB pixels ready to encode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixels {
    pub image: RgbaImage,
    /// True if any pixel is not fully opaque.
    pub has_alpha: bool,
    /// The profile to embed, if any.
    pub icc: Option<OutputProfile>,
}

impl Pixels {
    /// Fully transparent pixels lose their color, so nothing hides there
    /// and resizing does not bleed it into visible pixels.
    pub fn new(mut image: RgbaImage, icc: Option<OutputProfile>) -> Self {
        let mut has_alpha = false;
        for p in image.pixels_mut() {
            has_alpha |= p[3] != 255;
            if p[3] == 0 {
                p.0 = [0; 4];
            }
        }
        Self {
            image,
            has_alpha,
            icc,
        }
    }
}

pub trait Encoder {
    /// Encodes at `quality`, 1 (smallest) to 100 (best).
    fn encode(&self, px: &Pixels, quality: u8) -> Result<Vec<u8>, Error>;
    /// File name extension, without the dot.
    fn extension(&self) -> &'static str;
    /// The qualities the fit-to-size search tries (SPEC 9.3).
    fn quality_range(&self) -> (u8, u8) {
        (40, 92)
    }
}

/// JPEG through mozjpeg. Transparent pixels are put on white.
#[derive(Debug, Clone, Copy, Default)]
pub struct JpegEncoder;

/// PNG. Quality 100 is lossless. Lower qualities quantize to a palette of
/// at most 256 colors with imagequant.
#[derive(Debug, Clone, Copy, Default)]
pub struct PngEncoder;

impl Encoder for JpegEncoder {
    fn encode(&self, px: &Pixels, quality: u8) -> Result<Vec<u8>, Error> {
        encode_jpeg(&px.image, quality, px.icc.map(OutputProfile::bytes))
    }

    fn extension(&self) -> &'static str {
        "jpg"
    }
}

/// Most APP2 chunks one ICC profile can use: the count is one byte.
const MAX_ICC_CHUNKS: usize = 255;
/// Profile bytes per APP2 chunk: the segment limit minus the length,
/// the tag and the two numbering bytes.
const ICC_CHUNK_DATA: usize = 65_535 - 2 - 14;

fn encode_jpeg(image: &RgbaImage, quality: u8, icc: Option<&[u8]>) -> Result<Vec<u8>, Error> {
    let (w, h) = image.dimensions();
    if w == 0 || h == 0 || w > 65_500 || h > 65_500 {
        return Err(Error::Encode("image size not possible in JPEG"));
    }
    if icc.is_some_and(|p| p.is_empty() || p.len().div_ceil(ICC_CHUNK_DATA) > MAX_ICC_CHUNKS) {
        return Err(Error::Encode("ICC profile size not possible in JPEG"));
    }
    let rgb: Vec<u8> = image
        .pixels()
        .flat_map(|p| {
            let a = u16::from(p[3]);
            let on_white = |c: u8| ((u16::from(c) * a + 255 * (255 - a) + 127) / 255) as u8;
            [on_white(p[0]), on_white(p[1]), on_white(p[2])]
        })
        .collect();
    let quality = quality.clamp(1, 100);
    // mozjpeg reports errors by unwinding.
    catch_unwind(AssertUnwindSafe(|| -> std::io::Result<Vec<u8>> {
        let mut c = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
        c.set_size(w as usize, h as usize);
        c.set_quality(f32::from(quality));
        // Trying many progressive scan scripts costs about 1.8x the time
        // for a few percent of size (NFR-1).
        c.set_optimize_scans(false);
        let mut c = c.start_compress(Vec::new())?;
        if let Some(icc) = icc {
            write_jpeg_icc(&mut c, icc);
        }
        c.write_scanlines(&rgb)?;
        c.finish()
    }))
    .ok()
    .and_then(Result::ok)
    .ok_or(Error::Encode("JPEG encoding failed"))
}

/// APP2 ICC chunks with 1-based sequence numbers (ICC.1, annex B).
/// mozjpeg's own `write_icc_profile` numbers them from 0. The caller
/// checks that the profile fits in 255 chunks.
fn write_jpeg_icc<W>(c: &mut mozjpeg::compress::CompressStarted<W>, icc: &[u8]) {
    let chunks: Vec<&[u8]> = icc.chunks(ICC_CHUNK_DATA).collect();
    for (i, chunk) in chunks.iter().enumerate() {
        let mut buf = b"ICC_PROFILE\0".to_vec();
        buf.extend([i as u8 + 1, chunks.len() as u8]);
        buf.extend_from_slice(chunk);
        c.write_marker(mozjpeg::Marker::APP(2), &buf);
    }
}

impl Encoder for PngEncoder {
    fn encode(&self, px: &Pixels, quality: u8) -> Result<Vec<u8>, Error> {
        let (w, h) = px.image.dimensions();
        // Fully transparent pixels keep no color, so nothing hides there.
        let rgba: Vec<[u8; 4]> = px
            .image
            .pixels()
            .map(|p| if p[3] == 0 { [0; 4] } else { p.0 })
            .collect();
        let failed = |_| Error::Encode("PNG encoding failed");
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_depth(png::BitDepth::Eight);
        let data: Vec<u8> = if quality >= 100 {
            if px.has_alpha {
                enc.set_color(png::ColorType::Rgba);
                rgba.concat()
            } else {
                enc.set_color(png::ColorType::Rgb);
                rgba.iter().flat_map(|p| [p[0], p[1], p[2]]).collect()
            }
        } else {
            let (palette, indexes) = quantize(&rgba, w, h, quality)?;
            enc.set_color(png::ColorType::Indexed);
            enc.set_palette(
                palette
                    .iter()
                    .flat_map(|p| if p.a == 0 { [0; 3] } else { [p.r, p.g, p.b] })
                    .collect::<Vec<_>>(),
            );
            let mut trns: Vec<u8> = palette.iter().map(|p| p.a).collect();
            while trns.last() == Some(&255) {
                trns.pop();
            }
            if !trns.is_empty() {
                enc.set_trns(trns);
            }
            indexes
        };
        let mut writer = enc.write_header().map_err(failed)?;
        writer.write_image_data(&data).map_err(failed)?;
        writer.finish().map_err(failed)?;
        match px.icc {
            Some(icc) => Ok(insert_iccp(out, icc.bytes())),
            None => Ok(out),
        }
    }

    fn extension(&self) -> &'static str {
        "png"
    }

    /// The top of the range is lossless.
    fn quality_range(&self) -> (u8, u8) {
        (40, 100)
    }
}

fn quantize(
    rgba: &[[u8; 4]],
    w: u32,
    h: u32,
    quality: u8,
) -> Result<(Vec<imagequant::RGBA>, Vec<u8>), Error> {
    let failed = |_| Error::Encode("PNG quantization failed");
    let pixels: Vec<imagequant::RGBA> = rgba
        .iter()
        .map(|&[r, g, b, a]| imagequant::RGBA::new(r, g, b, a))
        .collect();
    let mut attr = imagequant::new();
    attr.set_quality(0, quality).map_err(failed)?;
    let mut img = attr
        .new_image(pixels, w as usize, h as usize, 0.0)
        .map_err(failed)?;
    let mut result = attr.quantize(&mut img).map_err(failed)?;
    result.set_dithering_level(1.0).map_err(failed)?;
    result.remapped(&mut img).map_err(failed)
}

/// Puts an iCCP chunk with our fixed profile name right after IHDR. The
/// png crate would name it `_`, which `verify` refuses.
fn insert_iccp(png: Vec<u8>, icc: &[u8]) -> Vec<u8> {
    const AFTER_IHDR: usize = 8 + 25;
    let mut data = ICC_PROFILE_NAME.to_vec();
    data.extend([0, 0]);
    data.extend(miniz_oxide::deflate::compress_to_vec_zlib(icc, 9));
    let mut chunk = (data.len() as u32).to_be_bytes().to_vec();
    chunk.extend(b"iCCP");
    chunk.extend(&data);
    chunk.extend(crc32(&chunk[4..]).to_be_bytes());

    let mut out = Vec::with_capacity(png.len() + chunk.len());
    out.extend_from_slice(&png[..AFTER_IHDR]);
    out.extend(chunk);
    out.extend_from_slice(&png[AFTER_IHDR..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::verify;
    use image::Rgba;

    fn gradient(w: u32, h: u32, alpha: bool) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            let a = if alpha {
                (x * 255 / w.max(2)) as u8
            } else {
                255
            };
            Rgba([(x * 4) as u8, (y * 4) as u8, 128, a])
        })
    }

    /// Profile-sized bytes, to test chunking beyond our small profile.
    fn icc(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    fn decode_jpeg(bytes: &[u8]) -> RgbaImage {
        let d = mozjpeg::Decompress::new_mem(bytes).unwrap();
        let mut d = d.rgba().unwrap();
        let (w, h) = (d.width() as u32, d.height() as u32);
        let data = d.read_scanlines::<[u8; 4]>().unwrap().concat();
        d.finish().unwrap();
        RgbaImage::from_raw(w, h, data).unwrap()
    }

    fn decode_png(bytes: &[u8]) -> RgbaImage {
        let mut d = png::Decoder::new(std::io::Cursor::new(bytes));
        d.set_transformations(png::Transformations::ALPHA | png::Transformations::EXPAND);
        let mut r = d.read_info().unwrap();
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        let info = r.next_frame(&mut buf).unwrap();
        buf.truncate(info.buffer_size());
        RgbaImage::from_raw(info.width, info.height, buf).unwrap()
    }

    fn mean_error(a: &RgbaImage, b: &RgbaImage) -> f64 {
        assert_eq!(a.dimensions(), b.dimensions());
        let sum: u64 = a
            .iter()
            .zip(b.iter())
            .map(|(x, y)| u64::from(x.abs_diff(*y)))
            .sum();
        sum as f64 / a.len() as f64
    }

    /// mozjpeg-sys silently builds without SIMD when nasm is missing,
    /// which makes every encode several times slower (NFR-1).
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn mozjpeg_is_built_with_simd() {
        unsafe extern "C" {
            fn jsimd_can_fdct_islow() -> std::ffi::c_int;
        }
        assert_eq!(unsafe { jsimd_can_fdct_islow() }, 1, "is nasm installed?");
    }

    #[test]
    fn jpeg_round_trip() {
        let px = Pixels::new(gradient(64, 48, false), None);
        let out = JpegEncoder.encode(&px, 90).unwrap();
        verify(&out, None).unwrap();
        let back = decode_jpeg(&out);
        assert!(mean_error(&back, &px.image) < 3.0);
    }

    #[test]
    fn jpeg_lower_quality_is_smaller() {
        let px = Pixels::new(gradient(128, 128, false), None);
        let hi = JpegEncoder.encode(&px, 95).unwrap();
        let lo = JpegEncoder.encode(&px, 40).unwrap();
        assert!(lo.len() < hi.len());
    }

    #[test]
    fn jpeg_puts_transparent_pixels_on_white() {
        let px = Pixels::new(RgbaImage::from_pixel(16, 16, Rgba([0, 0, 0, 0])), None);
        let back = decode_jpeg(&JpegEncoder.encode(&px, 90).unwrap());
        assert!(
            back.pixels()
                .all(|p| p[0] > 250 && p[1] > 250 && p[2] > 250)
        );
    }

    #[test]
    fn jpeg_with_profile_passes_verify_only_with_that_profile() {
        let srgb = OutputProfile::Srgb.bytes();
        let px = Pixels::new(gradient(32, 32, false), Some(OutputProfile::Srgb));
        let out = JpegEncoder.encode(&px, 80).unwrap();
        verify(&out, Some(srgb)).unwrap();
        assert!(verify(&out, None).is_err());
    }

    #[test]
    fn jpeg_profile_chunks_are_numbered_from_one() {
        let image = gradient(8, 8, false);
        for len in [ICC_CHUNK_DATA, ICC_CHUNK_DATA + 1, 3 * ICC_CHUNK_DATA] {
            let profile = icc(len);
            let out = encode_jpeg(&image, 80, Some(&profile)).unwrap();
            verify(&out, Some(&profile)).unwrap();
        }
    }

    #[test]
    fn jpeg_profile_over_255_chunks_is_an_error() {
        let image = gradient(8, 8, false);
        let fits = icc(MAX_ICC_CHUNKS * ICC_CHUNK_DATA);
        let out = encode_jpeg(&image, 80, Some(&fits)).unwrap();
        verify(&out, Some(&fits)).unwrap();
        let too_big = icc(MAX_ICC_CHUNKS * ICC_CHUNK_DATA + 1);
        assert!(matches!(
            encode_jpeg(&image, 80, Some(&too_big)),
            Err(Error::Encode(_))
        ));
    }

    #[test]
    fn output_profile_is_srgb_and_stable() {
        let bytes = OutputProfile::Srgb.bytes();
        let parsed = moxcms::ColorProfile::new_from_slice(bytes).unwrap();
        assert_eq!(parsed.color_space, moxcms::DataColorSpace::Rgb);
        assert_eq!(bytes, moxcms::ColorProfile::new_srgb().encode().unwrap());
    }

    #[test]
    fn new_clears_the_color_of_transparent_pixels() {
        let mut image = RgbaImage::from_pixel(2, 1, Rgba([9, 8, 7, 0]));
        image.put_pixel(1, 0, Rgba([9, 8, 7, 1]));
        let px = Pixels::new(image, None);
        assert_eq!(px.image.get_pixel(0, 0).0, [0, 0, 0, 0]);
        assert_eq!(px.image.get_pixel(1, 0).0, [9, 8, 7, 1]);
    }

    #[test]
    fn jpeg_too_large_is_an_error() {
        let px = Pixels::new(RgbaImage::new(70_000, 1), None);
        assert!(matches!(JpegEncoder.encode(&px, 80), Err(Error::Encode(_))));
    }

    #[test]
    fn png_lossless_round_trip() {
        for alpha in [false, true] {
            let mut image = gradient(40, 30, alpha);
            image.put_pixel(0, 0, Rgba([0, 0, 0, if alpha { 0 } else { 255 }]));
            let px = Pixels::new(image, None);
            assert_eq!(px.has_alpha, alpha);
            let out = PngEncoder.encode(&px, 100).unwrap();
            verify(&out, None).unwrap();
            assert_eq!(decode_png(&out), px.image);
        }
    }

    #[test]
    fn png_transparent_pixels_lose_their_color() {
        let px = Pixels::new(RgbaImage::from_pixel(4, 4, Rgba([9, 8, 7, 0])), None);
        for q in [100, 50] {
            let back = decode_png(&PngEncoder.encode(&px, q).unwrap());
            assert!(back.pixels().all(|p| p.0 == [0, 0, 0, 0]), "quality {q}");
        }
    }

    #[test]
    fn png_quantized_round_trip() {
        let px = Pixels::new(gradient(64, 64, true), None);
        let out = PngEncoder.encode(&px, 70).unwrap();
        verify(&out, None).unwrap();
        assert!(out.windows(4).any(|w| w == b"PLTE"));
        assert!(mean_error(&decode_png(&out), &px.image) < 8.0);
    }

    #[test]
    fn png_with_profile_passes_verify_only_with_that_profile() {
        let srgb = OutputProfile::Srgb.bytes();
        for q in [100, 60] {
            let px = Pixels::new(gradient(16, 16, true), Some(OutputProfile::Srgb));
            let out = PngEncoder.encode(&px, q).unwrap();
            verify(&out, Some(srgb)).unwrap();
            assert!(verify(&out, None).is_err());
            assert_eq!(
                decode_png(&out),
                decode_png(&PngEncoder.encode(&Pixels { icc: None, ..px }, q).unwrap())
            );
        }
    }
}
