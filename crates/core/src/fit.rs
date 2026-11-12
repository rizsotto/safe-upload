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
/// Tile edge in output pixels: a multiple of the 16 px macroblock of
/// 4:2:0 JPEG, so tile edges fall on block edges.
const TILE: u32 = 64;
/// Aim a little below the target, so the prediction error (a few percent
/// for most photos, decision 0002) rarely pushes the output over it.
const AIM: f64 = 0.95;
/// Below this share of the target, one higher quality is tried. FR-3
/// asks for 85% when possible; 80% saves a full encode for one quality
/// step in many photos (decision 0002).
const GOOD_ENOUGH: f64 = 0.8;
/// The most full encodes at one scale: the guess, one correction, and a
/// binary search over at most 52 qualities.
const MAX_ENCODES_PER_SCALE: usize = 8;
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

    // Actual size over predicted size, learned from the full encodes.
    let mut ratio = 1.0;
    let mut sample = Sample::new(px, scale, overhead);
    loop {
        let guess = sample.highest_fitting(q_min, q_max, AIM * target / ratio, encoder)?;
        // Below the floor, or when the lowest quality may just fit, the
        // prediction is not trusted to rule it out: try it for real.
        let quality = match guess {
            Some(q) => Some(q),
            None if scale <= floor || sample.predict(q_min, encoder)? * ratio <= target => {
                Some(q_min)
            }
            None => None,
        };
        if let Some(quality) = quality {
            match at_scale(px, &mut sample, quality, target, &mut ratio, encoder)? {
                Some(fitted) => return Ok(fitted),
                None if scale <= floor => return Err(Error::CannotFit { target_bytes }),
                None => {}
            }
        }
        let predicted = sample.predict(q_min, encoder)? * ratio;
        let step = (AIM * target / predicted).powf(1.0 / SCALE_EXPONENT);
        scale = (scale * step.min(0.95)).max(floor);
        sample = Sample::new(px, scale, overhead);
    }
}

/// Encodes the full image at the sample's scale, starting at `quality`.
/// Returns the best output that fits, or None if even the lowest quality
/// is too big. At most [`MAX_ENCODES_PER_SCALE`] encodes: the guess, one
/// correction with the measured ratio (one try up when the guess is under
/// [`GOOD_ENOUGH`], one try down when it is over), then a binary search
/// between the best quality that fits and the lowest that does not.
fn at_scale<E: Encoder + ?Sized>(
    px: &Pixels,
    sample: &mut Sample,
    quality: u8,
    target: f64,
    ratio: &mut f64,
    encoder: &E,
) -> Result<Option<Fitted>, Error> {
    let (q_min, q_max) = encoder.quality_range();
    let q_max = q_max.max(q_min);
    let fits = |f: &Fitted| f.bytes.len() as f64 <= target;
    let good = |f: &Fitted| f.bytes.len() as f64 >= GOOD_ENOUGH * target;

    let first = sample.encode_full(px, quality, encoder)?;
    *ratio = first.bytes.len() as f64 / sample.predict(quality, encoder)?;
    let budget = AIM * target / *ratio;
    if fits(&first) {
        if good(&first) || quality == q_max {
            return Ok(Some(first));
        }
        // One try up. Keep the first output if it does not fit.
        let Some(q) = sample.highest_fitting(quality + 1, q_max, budget, encoder)? else {
            return Ok(Some(first));
        };
        let up = sample.encode_full(px, q, encoder)?;
        return Ok(Some(if fits(&up) { up } else { first }));
    }
    if quality == q_min {
        return Ok(None);
    }
    // One try down with the corrected prediction, then a binary search.
    let mut best: Option<Fitted> = None;
    let mut too_big = quality;
    let mut next = sample
        .highest_fitting(q_min, quality - 1, budget, encoder)?
        .unwrap_or(q_min);
    let mut encodes = 1;
    loop {
        let out = sample.encode_full(px, next, encoder)?;
        encodes += 1;
        debug_assert!(encodes <= MAX_ENCODES_PER_SCALE);
        if fits(&out) {
            let done = good(&out) || too_big - next <= 1;
            best = Some(out);
            if done {
                return Ok(best);
            }
        } else {
            too_big = next;
        }
        let below = best
            .as_ref()
            .map_or(i32::from(q_min) - 1, |b| i32::from(b.quality));
        if i32::from(too_big) - below <= 1 {
            return Ok(best);
        }
        next = ((below + i32::from(too_big)) / 2).max(i32::from(q_min)) as u8;
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
                Some(mosaic_of(px, scale, tile_edge(short)))
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

/// [`TILE`], or less for a short edge under GRID * TILE: a multiple of 16
/// if possible, else of 8.
fn tile_edge(short: u32) -> u32 {
    let cell = short / GRID;
    match cell / 16 * 16 {
        0 => cell / 8 * 8,
        t => TILE.min(t),
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
        // The full image is about 24% smaller than predicted, so the first
        // guess lands under 80% and one higher quality is tried.
        let fake = Fake::skewed(-0.4);
        let target = 4_500_000;
        let f = fit(&px(1000, 800), target, None, &fake).unwrap();
        let len = f.bytes.len() as u64;
        assert!(len <= target);
        assert!(len as f64 >= 0.8 * target as f64, "{len}");
        let full = fake.full_encodes();
        assert_eq!(full.len(), 2, "{full:?}");
        assert!(full[1].2 > full[0].2, "{full:?}");
        assert_eq!(f.quality, full[1].2);
    }

    /// The sample is linear in quality, like [`Fake`]; the full image
    /// costs `bytes_per_pixel(quality)`. So the prediction can be made as
    /// wrong as a test needs.
    struct Curve<F> {
        bytes_per_pixel: F,
        inner: Fake,
    }

    impl<F: Fn(u8) -> f64> Curve<F> {
        fn new(bytes_per_pixel: F) -> Self {
            Self {
                bytes_per_pixel,
                inner: Fake::default(),
            }
        }
    }

    impl<F: Fn(u8) -> f64> Encoder for Curve<F> {
        fn encode(&self, px: &Pixels, quality: u8) -> Result<Vec<u8>, Error> {
            let out = self.inner.encode(px, quality)?;
            let (w, h) = px.image.dimensions();
            if self.inner.full_encodes().last() != Some(&(w, h, quality)) {
                return Ok(out);
            }
            let len = f64::from(w * h) * (self.bytes_per_pixel)(quality);
            Ok(vec![0; len as usize])
        }
        fn extension(&self) -> &'static str {
            "fake"
        }
    }

    #[test]
    fn bisects_when_the_correction_is_still_over() {
        // Everything from quality 45 up is far too big; below, the
        // prediction is right.
        let curve = Curve::new(|q| if q >= 45 { 7.0 } else { f64::from(q) / 10.0 });
        let target = 5_000_000;
        let f = fit(&px(1000, 800), target, None, &curve).unwrap();
        assert!(f.bytes.len() as u64 <= target);
        assert_eq!((f.width, f.quality), (1000, 44));
        let full = curve.inner.full_encodes();
        assert!(full.len() <= MAX_ENCODES_PER_SCALE, "{full:?}");
        assert!(full.len() > 2, "{full:?}");
    }

    #[test]
    fn encodes_per_scale_are_bounded() {
        // Only the lowest quality fits, and the prediction is useless.
        let curve = Curve::new(|q| if q == 40 { 4.0 } else { 100.0 });
        let target = 5_000_000;
        let f = fit(&px(1000, 800), target, None, &curve).unwrap();
        assert_eq!((f.width, f.quality), (1000, 40));
        let full = curve.inner.full_encodes();
        assert!(full.len() <= MAX_ENCODES_PER_SCALE, "{full:?}");
    }

    #[test]
    fn keeps_the_first_output_when_the_try_up_does_not_fit() {
        // 30% smaller than predicted up to quality 60, far too big above.
        let curve = Curve::new(|q| if q <= 60 { f64::from(q) * 0.07 } else { 10.0 });
        let target = 5_000_000;
        let f = fit(&px(1000, 800), target, None, &curve).unwrap();
        let full = curve.inner.full_encodes();
        assert_eq!(full.len(), 2, "{full:?}");
        assert!(full[1].2 > 60, "{full:?}");
        assert_eq!(f.quality, full[0].2);
        assert!(f.bytes.len() as u64 <= target);
    }

    #[test]
    fn cannot_fit_after_encodes_over_the_target() {
        // The sample says it fits; every real encode is far too big.
        let curve = Curve::new(|_| 100.0);
        let err = fit(&px(1000, 800), 5_000_000, None, &curve).unwrap_err();
        assert!(matches!(err, Error::CannotFit { .. }));
        let full = curve.inner.full_encodes();
        assert_eq!(full.last(), Some(&(640, 512, 40)), "{full:?}");
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
