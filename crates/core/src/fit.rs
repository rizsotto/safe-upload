//! Fit-to-size (SPEC 9.3, FR-3, FR-4). Predicts the scale and quality
//! from a small sample, encodes the image once, and corrects only when
//! that encode misses.
//!
//! The sample is a mosaic of tiles from a grid over the image, at the
//! output scale. JPEG codes each 8x8 block on its own, so the bytes per
//! pixel of the mosaic predict those of the whole image within a few
//! percent (decision 0002). The quality search runs on the mosaic, which
//! is about 1/40 of a 12 MP image, so the full image is usually encoded
//! once (NFR-1).

use std::collections::BTreeMap;

use image::RgbaImage;
use image::imageops::{self, FilterType};

use crate::Error;
use crate::encode::{Encoder, Pixels};

/// The long edge never goes below this when scaling down to fit.
pub const MIN_LONG_EDGE: u32 = 640;
/// The sample takes one tile from each cell of a GRID x GRID grid.
const GRID: u32 = 8;
/// Tile edge in output pixels: a multiple of the 16 px JPEG block.
const TILE: u32 = 64;
/// Aim a little below the target, so the prediction error (a few percent
/// for most photos, decision 0002) rarely pushes the output over it.
const AIM: f64 = 0.95;
/// FR-3: the output should be at least this share of the target.
const GOOD_ENOUGH: f64 = 0.85;
/// Output size grows about with scale^1.7, not with the pixel count,
/// since a smaller image has more detail per pixel (decision 0002).
const SCALE_EXPONENT: f64 = 1.7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fitted {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub quality: u8,
}

/// Encodes `px` so the output has at most `target_bytes` bytes. The long
/// edge is at most `max_long_edge` if given. Never returns more than the
/// target; if even the lowest quality at a 640 px long edge does not fit,
/// returns [`Error::CannotFit`].
pub fn fit<E: Encoder + ?Sized>(
    px: &Pixels,
    target_bytes: u64,
    max_long_edge: Option<u32>,
    encoder: &E,
) -> Result<Fitted, Error> {
    let (w, h) = px.image.dimensions();
    let long_edge = f64::from(w.max(h));
    if long_edge == 0.0 {
        return Err(Error::Encode("empty image"));
    }
    let mut scale = match max_long_edge {
        Some(max) => (f64::from(max) / long_edge).min(1.0),
        None => 1.0,
    };
    let floor = (f64::from(MIN_LONG_EDGE) / long_edge).min(scale);
    let (q_min, q_max) = encoder.quality_range();
    let q_max = q_max.max(q_min);
    let target = target_bytes as f64;
    let overhead = header_bytes(px, q_max, encoder)?;

    // Actual size over predicted size, learned from each full encode.
    let mut ratio = 1.0;
    // The highest quality not yet known to be too big at `scale`. None:
    // even the lowest is too big.
    let mut cap = Some(q_max);
    let mut sample = Sample::new(px, scale, overhead);
    loop {
        let budget = AIM * target / ratio;
        let guess = match cap {
            Some(cap) => sample.highest_fitting(q_min, cap, budget, encoder)?,
            None => None,
        };
        // Below the floor, or when the lowest quality may just fit, the
        // prediction is not trusted to rule it out: try it for real.
        let try_lowest = guess.is_none()
            && cap.is_some()
            && (scale <= floor || sample.predict(q_min, encoder)? * ratio <= target);
        let quality = match guess {
            Some(q) => q,
            None if try_lowest => q_min,
            None if scale <= floor => return Err(Error::CannotFit { target_bytes }),
            None => {
                let predicted = sample.predict(q_min, encoder)? * ratio;
                let step = (AIM * target / predicted).powf(1.0 / SCALE_EXPONENT);
                scale = (scale * step.min(0.95)).max(floor);
                sample = Sample::new(px, scale, overhead);
                cap = Some(q_max);
                continue;
            }
        };
        let first = sample.encode_full(px, quality, encoder)?;
        let len = first.bytes.len() as f64;
        ratio = len / sample.predict(quality, encoder)?;
        if len > target {
            cap = quality.checked_sub(1).filter(|&q| q >= q_min);
            continue;
        }
        let top = cap.unwrap_or(q_min);
        if len >= GOOD_ENOUGH * target || quality >= top {
            return Ok(first);
        }
        // Too small: one try with a higher quality and the corrected
        // prediction. Keep the first output if that try does not fit.
        let budget = AIM * target / ratio;
        let Some(q) = sample.highest_fitting(quality + 1, top, budget, encoder)? else {
            return Ok(first);
        };
        let better = sample.encode_full(px, q, encoder)?;
        if better.bytes.len() as u64 <= target_bytes {
            return Ok(better);
        }
        return Ok(first);
    }
}

/// The bytes an encode costs whatever the image: headers, tables and the
/// profile. They do not grow with the pixel count.
fn header_bytes<E: Encoder + ?Sized>(px: &Pixels, quality: u8, encoder: &E) -> Result<f64, Error> {
    let (w, h) = px.image.dimensions();
    let tiny = imageops::crop_imm(&px.image, 0, 0, w.min(8), h.min(8)).to_image();
    Ok(encoder.encode(&like(px, tiny), quality)?.len() as f64)
}

/// The image at one scale, as a mosaic of tiles, or whole when it is
/// small. Remembers each quality it encoded.
struct Sample {
    scale: f64,
    pixels: Pixels,
    /// The sample is the whole scaled image, so its encodes are the
    /// output.
    whole: bool,
    /// Output pixels per sample pixel.
    factor: f64,
    overhead: f64,
    encoded: BTreeMap<u8, Vec<u8>>,
}

impl Sample {
    fn new(px: &Pixels, scale: f64, overhead: f64) -> Self {
        let (w, h) = scaled_size(px, scale);
        let mosaic = GRID * TILE;
        let short = w.min(h);
        let pixels =
            if u64::from(w) * u64::from(h) <= u64::from(mosaic * mosaic) || short < GRID * 8 {
                None
            } else {
                Some(mosaic_of(px, scale, TILE.min(short / GRID / 8 * 8)))
            };
        let whole = pixels.is_none();
        let pixels = pixels.unwrap_or_else(|| resize(px, scale));
        let (sw, sh) = pixels.image.dimensions();
        Self {
            scale,
            factor: f64::from(w) * f64::from(h) / (f64::from(sw) * f64::from(sh)),
            whole,
            pixels,
            overhead,
            encoded: BTreeMap::new(),
        }
    }

    fn bytes<E: Encoder + ?Sized>(&mut self, quality: u8, encoder: &E) -> Result<&[u8], Error> {
        if !self.encoded.contains_key(&quality) {
            let out = encoder.encode(&self.pixels, quality)?;
            self.encoded.insert(quality, out);
        }
        Ok(&self.encoded[&quality])
    }

    /// The predicted output size at this quality.
    fn predict<E: Encoder + ?Sized>(&mut self, quality: u8, encoder: &E) -> Result<f64, Error> {
        let (factor, overhead) = (self.factor, self.overhead);
        let len = self.bytes(quality, encoder)?.len() as f64;
        if self.whole {
            return Ok(len);
        }
        Ok(overhead + (len - overhead).max(0.0) * factor)
    }

    /// The highest quality in `lo..=hi` predicted to be at most `budget`
    /// bytes. Size grows with quality, so this is a binary search.
    fn highest_fitting<E: Encoder + ?Sized>(
        &mut self,
        lo: u8,
        hi: u8,
        budget: f64,
        encoder: &E,
    ) -> Result<Option<u8>, Error> {
        if lo > hi {
            return Ok(None);
        }
        if self.predict(hi, encoder)? <= budget {
            return Ok(Some(hi));
        }
        if self.predict(lo, encoder)? > budget {
            return Ok(None);
        }
        let (mut fits, mut too_big) = (lo, hi);
        while too_big - fits > 1 {
            let mid = fits + (too_big - fits) / 2;
            if self.predict(mid, encoder)? <= budget {
                fits = mid;
            } else {
                too_big = mid;
            }
        }
        Ok(Some(fits))
    }

    /// Encodes the whole image at this scale.
    fn encode_full<E: Encoder + ?Sized>(
        &mut self,
        px: &Pixels,
        quality: u8,
        encoder: &E,
    ) -> Result<Fitted, Error> {
        let (width, height) = scaled_size(px, self.scale);
        let bytes = if self.whole {
            self.bytes(quality, encoder)?.to_vec()
        } else if (width, height) == px.image.dimensions() {
            encoder.encode(px, quality)?
        } else {
            encoder.encode(&resize(px, self.scale), quality)?
        };
        Ok(Fitted {
            bytes,
            width,
            height,
            quality,
        })
    }
}

fn scaled_size(px: &Pixels, scale: f64) -> (u32, u32) {
    let (w, h) = px.image.dimensions();
    if scale >= 1.0 {
        return (w, h);
    }
    let size = |n: u32| ((f64::from(n) * scale).round() as u32).max(1);
    (size(w), size(h))
}

fn resize(px: &Pixels, scale: f64) -> Pixels {
    let (w, h) = scaled_size(px, scale);
    if (w, h) == px.image.dimensions() {
        return px.clone();
    }
    like(px, imageops::resize(&px.image, w, h, FilterType::Lanczos3))
}

/// GRID x GRID tiles of `tile` output pixels, each from the middle of its
/// grid cell, scaled like the output.
fn mosaic_of(px: &Pixels, scale: f64, tile: u32) -> Pixels {
    let (w, h) = px.image.dimensions();
    let (cell_w, cell_h) = (w / GRID, h / GRID);
    let src = ((f64::from(tile) / scale.min(1.0)).round() as u32).clamp(1, cell_w.min(cell_h));
    let mut image = RgbaImage::new(GRID * tile, GRID * tile);
    for gy in 0..GRID {
        for gx in 0..GRID {
            let x = gx * cell_w + (cell_w - src) / 2;
            let y = gy * cell_h + (cell_h - src) / 2;
            let part = imageops::crop_imm(&px.image, x, y, src, src).to_image();
            let part = if src == tile {
                part
            } else {
                imageops::resize(&part, tile, tile, FilterType::Lanczos3)
            };
            imageops::replace(
                &mut image,
                &part,
                i64::from(gx * tile),
                i64::from(gy * tile),
            );
        }
    }
    like(px, image)
}

/// Other pixels with the same alpha flag and profile as `px`.
fn like(px: &Pixels, image: RgbaImage) -> Pixels {
    Pixels {
        image,
        has_alpha: px.has_alpha,
        icc: px.icc,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;
    use std::sync::Mutex;

    /// Output size is width * height * quality / 10 bytes, times
    /// `(long edge / 512) ^ skew`: with a skew the sample mosaic (512 px)
    /// misleads the prediction for larger images.
    #[derive(Default)]
    struct Fake {
        skew: f64,
        calls: Mutex<Vec<(u32, u32, u8)>>,
    }

    impl Fake {
        fn skewed(skew: f64) -> Self {
            Self {
                skew,
                ..Self::default()
            }
        }

        /// The encodes of the full image, not of the sample.
        fn full_encodes(&self) -> Vec<(u32, u32, u8)> {
            let mosaic = GRID * TILE;
            let calls = self.calls.lock().unwrap();
            calls
                .iter()
                .copied()
                .filter(|&(w, h, _)| (w, h) != (mosaic, mosaic) && w * h > 16 * 16)
                .collect()
        }
    }

    impl Encoder for Fake {
        fn encode(&self, px: &Pixels, quality: u8) -> Result<Vec<u8>, Error> {
            let (w, h) = px.image.dimensions();
            self.calls.lock().unwrap().push((w, h, quality));
            let skew = (f64::from(w.max(h)) / 512.0).powf(self.skew);
            let len = f64::from(w * h) * f64::from(quality) / 10.0 * skew;
            Ok(vec![0; len as usize])
        }
        fn extension(&self) -> &'static str {
            "fake"
        }
    }

    fn px(w: u32, h: u32) -> Pixels {
        Pixels::new(RgbaImage::new(w, h), None)
    }

    #[test]
    fn under_target_and_at_least_85_percent() {
        let area = 1000 * 800;
        for target in [area * 4, area * 5, area * 7 + 123, area * 9] {
            let fake = Fake::default();
            let f = fit(&px(1000, 800), target as u64, None, &fake).unwrap();
            let len = f.bytes.len() as u64;
            assert!(len <= target as u64);
            assert!(len as f64 >= 0.85 * target as f64, "{len} for {target}");
            assert_eq!((f.width, f.height), (1000, 800));
            // The sample predicts this encoder exactly.
            assert_eq!(fake.full_encodes().len(), 1, "{target}");
        }
    }

    #[test]
    fn best_quality_when_everything_fits() {
        let fake = Fake::default();
        let f = fit(&px(1000, 800), 100_000_000, None, &fake).unwrap();
        assert_eq!(f.quality, 92);
        assert_eq!(fake.full_encodes(), [(1000, 800, 92)]);
    }

    #[test]
    fn small_images_are_their_own_sample() {
        // No encode is repeated: the sample is the image itself.
        let fake = Fake::default();
        let f = fit(&px(400, 300), 10_000_000, None, &fake).unwrap();
        assert_eq!((f.width, f.height, f.quality), (400, 300, 92));
        let calls = fake.calls.lock().unwrap();
        let full: Vec<_> = calls.iter().filter(|c| c.0 == 400).collect();
        assert_eq!(full, [&(400, 300, 92)]);
    }

    #[test]
    fn one_correction_when_the_sample_is_too_low() {
        // The full image is about 14% bigger than the sample predicts.
        let fake = Fake::skewed(0.2);
        let target = 5_000_000;
        let f = fit(&px(1000, 800), target, None, &fake).unwrap();
        let len = f.bytes.len() as u64;
        assert!(len <= target);
        assert!(len as f64 >= 0.85 * target as f64, "{len}");
        assert_eq!(fake.full_encodes().len(), 2, "{:?}", fake.full_encodes());
    }

    #[test]
    fn one_correction_when_the_sample_is_too_high() {
        // The full image is about 12% smaller than predicted, so the first
        // guess lands under 85% and one higher quality is tried.
        let fake = Fake::skewed(-0.2);
        let target = 4_500_000;
        let f = fit(&px(1000, 800), target, None, &fake).unwrap();
        let len = f.bytes.len() as u64;
        assert!(len <= target);
        assert!(len as f64 >= 0.85 * target as f64, "{len}");
        assert!(fake.full_encodes().len() <= 2, "{:?}", fake.full_encodes());
    }

    #[test]
    fn scales_down_until_it_fits() {
        // At full size even quality 40 is 3.2 MB.
        let fake = Fake::default();
        let target = 2_000_000;
        let f = fit(&px(1000, 800), target, None, &fake).unwrap();
        let len = f.bytes.len() as u64;
        assert!(len <= target);
        assert!(len as f64 >= 0.85 * target as f64, "{len}");
        assert!(f.width < 1000 && f.width >= MIN_LONG_EDGE);
        assert!((f.width * 4).abs_diff(f.height * 5) <= 5);
        assert_eq!(fake.full_encodes().len(), 1, "{:?}", fake.full_encodes());
    }

    #[test]
    fn a_jump_below_the_floor_still_tries_the_floor() {
        // Quality 40 fits exactly at 640x512.
        let fake = Fake::default();
        let target = 640 * 512 * 4;
        let f = fit(&px(1000, 800), target, None, &fake).unwrap();
        assert_eq!((f.width, f.height, f.quality), (640, 512, 40));
    }

    #[test]
    fn floor_of_640_px_gives_an_error() {
        let fake = Fake::default();
        let err = fit(&px(1000, 800), 1000, None, &fake).unwrap_err();
        assert!(matches!(err, Error::CannotFit { target_bytes: 1000 }));
        // The error comes from a real encode at the floor, not only from
        // the prediction.
        assert_eq!(fake.full_encodes(), [(640, 512, 40)]);
        // An image already smaller than the floor is not scaled at all.
        let fake = Fake::default();
        assert!(fit(&px(300, 200), 10, None, &fake).is_err());
        assert!(
            fake.calls
                .lock()
                .unwrap()
                .iter()
                .all(|c| c.0 == 300 || c.0 <= 16)
        );
    }

    #[test]
    fn max_long_edge_scales_first() {
        let f = fit(&px(2000, 1000), 100_000_000, Some(500), &Fake::default()).unwrap();
        assert_eq!((f.width, f.height), (500, 250));
        // A limit above the size does not enlarge.
        let f = fit(&px(200, 100), 100_000_000, Some(500), &Fake::default()).unwrap();
        assert_eq!((f.width, f.height), (200, 100));
    }
}
