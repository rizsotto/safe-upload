//! Encoders (SPEC 8). An encoder gets only sRGB pixels plus an optional
//! ICC profile, never anything from the source file, so it cannot copy
//! source metadata (SPEC 8, key rule). Its output passes
//! [`crate::verify::verify`] with the same profile.

use std::panic::{AssertUnwindSafe, catch_unwind};

use image::RgbaImage;

use crate::Error;
use crate::verify::{ICC_PROFILE_NAME, crc32};

/// sRGB pixels ready to encode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixels {
    pub image: RgbaImage,
    /// True if any pixel is not fully opaque.
    pub has_alpha: bool,
    /// The profile to embed. Only ever our own profile, never the source's.
    pub icc: Option<Vec<u8>>,
}

impl Pixels {
    pub fn new(image: RgbaImage, icc: Option<Vec<u8>>) -> Self {
        let has_alpha = image.pixels().any(|p| p[3] != 255);
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
        let (w, h) = px.image.dimensions();
        if w == 0 || h == 0 || w > 65_500 || h > 65_500 {
            return Err(Error::Encode("image size not possible in JPEG"));
        }
        let rgb: Vec<u8> = px
            .image
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
            let mut c = c.start_compress(Vec::new())?;
            if let Some(icc) = &px.icc {
                write_jpeg_icc(&mut c, icc);
            }
            c.write_scanlines(&rgb)?;
            c.finish()
        }))
        .ok()
        .and_then(Result::ok)
        .ok_or(Error::Encode("JPEG encoding failed"))
    }

    fn extension(&self) -> &'static str {
        "jpg"
    }
}

/// APP2 ICC chunks with 1-based sequence numbers (ICC.1, annex B).
/// mozjpeg's own `write_icc_profile` numbers them from 0.
fn write_jpeg_icc<W>(c: &mut mozjpeg::compress::CompressStarted<W>, icc: &[u8]) {
    const MAX_DATA: usize = 65_533 - 14;
    let chunks: Vec<&[u8]> = icc.chunks(MAX_DATA).collect();
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
        match &px.icc {
            Some(icc) => Ok(insert_iccp(out, icc)),
            None => Ok(out),
        }
    }

    fn extension(&self) -> &'static str {
        "png"
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

    /// A stand-in for our profile. Large enough for two JPEG chunks.
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
        for len in [500, 70_000] {
            let profile = icc(len);
            let px = Pixels::new(gradient(32, 32, false), Some(profile.clone()));
            let out = JpegEncoder.encode(&px, 80).unwrap();
            verify(&out, Some(&profile)).unwrap();
            assert!(verify(&out, None).is_err());
        }
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
            let mut want = px.image.clone();
            want.pixels_mut()
                .filter(|p| p[3] == 0)
                .for_each(|p| p.0 = [0; 4]);
            assert_eq!(decode_png(&out), want);
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
        let profile = icc(3000);
        for q in [100, 60] {
            let px = Pixels::new(gradient(16, 16, true), Some(profile.clone()));
            let out = PngEncoder.encode(&px, q).unwrap();
            verify(&out, Some(&profile)).unwrap();
            assert!(verify(&out, None).is_err());
            assert_eq!(
                decode_png(&out),
                decode_png(&PngEncoder.encode(&Pixels { icc: None, ..px }, q).unwrap())
            );
        }
    }
}
