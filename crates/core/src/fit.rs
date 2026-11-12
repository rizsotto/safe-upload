//! Fit-to-size search (SPEC 9.3, FR-3, FR-4). Finds a high quality that
//! fits the target, and scales the image down when no quality fits.
//!
//! JPEG size grows about exponentially with quality, so the search
//! interpolates on log(size) between a quality that fits and one that does
//! not (regula falsi), and runs the encodes of each round in parallel.
//! That usually needs two rounds instead of the 6 or 7 sequential encodes
//! of a binary search (NFR-1).

use image::imageops::{self, FilterType};

use crate::Error;
use crate::encode::{Encoder, Pixels};

/// The long edge never goes below this when scaling down to fit.
pub const MIN_LONG_EDGE: u32 = 640;
/// At most this many rounds of encodes per scale: one with the highest
/// and the lowest quality, then up to two with three guesses each.
const MAX_ROUNDS: u32 = 3;
/// Stop searching once the output reaches this share of the target. FR-3
/// asks for at least 85%.
const GOOD_ENOUGH: f64 = 0.9;
/// The interpolation aims a little below the target, so the guess fits
/// more often.
const AIM: f64 = 0.97;
/// Margin on the scale jump, since size is not exactly proportional to
/// the pixel count.
const SCALE_MARGIN: f64 = 0.95;

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
pub fn fit<E: Encoder + Sync + ?Sized>(
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
    loop {
        let scaled = resize(px, scale);
        let smallest = match search(&scaled, target_bytes, encoder)? {
            Search::Fits(best) => return Ok(best),
            Search::TooBig { smallest } => smallest,
        };
        if scale <= floor {
            return Err(Error::CannotFit { target_bytes });
        }
        // Size is about proportional to the pixel count, so jump straight
        // to the scale that should fit at the lowest quality.
        let jump = (target_bytes as f64 / smallest as f64).sqrt() * SCALE_MARGIN;
        scale = (scale * jump.min(SCALE_MARGIN)).max(floor);
    }
}

enum Search {
    Fits(Fitted),
    /// Even the lowest quality is too big; this is its size.
    TooBig {
        smallest: u64,
    },
}

/// Finds a high quality that fits at this size. Round one encodes the
/// highest and the lowest quality; each later round interpolates a guess
/// and encodes it with two neighbors. The encodes of a round run in
/// parallel.
fn search<E: Encoder + Sync + ?Sized>(
    px: &Pixels,
    target: u64,
    encoder: &E,
) -> Result<Search, Error> {
    let (q_min, q_max) = encoder.quality_range();
    let (q_min, q_max) = (i32::from(q_min), i32::from(q_max.max(q_min)));
    let fits = |f: &Fitted| f.bytes.len() as u64 <= target;
    let ln = |f: &Fitted| (f.bytes.len().max(1) as f64).ln();

    let mut round = encode_all(px, encoder, &[q_max, q_min])?;
    let lowest = round.pop().unwrap();
    let top = round.pop().unwrap();
    if fits(&top) {
        return Ok(Search::Fits(top));
    }
    if !fits(&lowest) {
        return Ok(Search::TooBig {
            smallest: lowest.bytes.len() as u64,
        });
    }
    let (mut lo_ln, mut hi_ln) = (ln(&lowest), ln(&top));
    let mut hi = q_max;
    let mut best = lowest;
    let aim = (target as f64 * AIM).ln();
    for _ in 1..MAX_ROUNDS {
        let lo = i32::from(best.quality);
        if hi - lo <= 1 || best.bytes.len() as f64 >= GOOD_ENOUGH * target as f64 {
            break;
        }
        let guess = if hi_ln > lo_ln {
            f64::from(lo) + (aim - lo_ln) / (hi_ln - lo_ln) * f64::from(hi - lo)
        } else {
            f64::from(lo + hi) / 2.0
        };
        let guess = guess.floor() as i32;
        let spread = ((hi - lo) / 8).max(1);
        let mut qs: Vec<i32> = [guess - spread, guess, guess + spread]
            .into_iter()
            .map(|q| q.clamp(lo + 1, hi - 1))
            .collect();
        qs.dedup();
        for out in encode_all(px, encoder, &qs)? {
            let q = i32::from(out.quality);
            if fits(&out) {
                if q > i32::from(best.quality) {
                    lo_ln = ln(&out);
                    best = out;
                }
            } else if q < hi {
                hi = q;
                hi_ln = ln(&out);
            }
        }
    }
    // A quality above a failing one may still have fit by chance; only
    // the size counts.
    Ok(Search::Fits(best))
}

/// Encodes at each quality, in parallel.
fn encode_all<E: Encoder + Sync + ?Sized>(
    px: &Pixels,
    encoder: &E,
    qualities: &[i32],
) -> Result<Vec<Fitted>, Error> {
    let encode = |q: i32| -> Result<Fitted, Error> {
        Ok(Fitted {
            bytes: encoder.encode(px, q as u8)?,
            width: px.image.width(),
            height: px.image.height(),
            quality: q as u8,
        })
    };
    std::thread::scope(|s| {
        let handles: Vec<_> = qualities
            .iter()
            .map(|&q| s.spawn(move || encode(q)))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or(Err(Error::Encode("encoder crashed"))))
            .collect()
    })
}

fn resize(px: &Pixels, scale: f64) -> Pixels {
    if scale >= 1.0 {
        return px.clone();
    }
    let (w, h) = px.image.dimensions();
    let size = |n: u32| ((f64::from(n) * scale).round() as u32).max(1);
    Pixels {
        image: imageops::resize(&px.image, size(w), size(h), FilterType::Lanczos3),
        has_alpha: px.has_alpha,
        icc: px.icc,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;
    use std::sync::Mutex;

    /// Output size is width * height * quality / 10 bytes.
    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<(u32, u8)>>,
    }

    impl Encoder for Fake {
        fn encode(&self, px: &Pixels, quality: u8) -> Result<Vec<u8>, Error> {
            let (w, h) = px.image.dimensions();
            self.calls.lock().unwrap().push((w.max(h), quality));
            Ok(vec![0; (w * h * u32::from(quality) / 10) as usize])
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
            let f = fit(&px(1000, 800), target as u64, None, &Fake::default()).unwrap();
            let len = f.bytes.len() as u64;
            assert!(len <= target as u64);
            assert!(len as f64 >= 0.85 * target as f64, "{len} for {target}");
            assert_eq!((f.width, f.height), (1000, 800));
        }
    }

    #[test]
    fn best_quality_when_everything_fits() {
        let fake = Fake::default();
        let f = fit(&px(100, 100), 10_000_000, None, &fake).unwrap();
        assert_eq!(f.quality, 92);
        // One round: the highest and the lowest quality.
        assert_eq!(fake.calls.lock().unwrap().len(), 2);
    }

    #[test]
    fn two_rounds_when_it_fits_at_full_size() {
        let fake = Fake::default();
        fit(&px(1000, 800), 6_000_000, None, &fake).unwrap();
        let calls = fake.calls.lock().unwrap();
        // The highest and the lowest, then one round of three guesses.
        let mut first = calls[..2].to_vec();
        first.sort();
        assert_eq!(first, [(1000, 40), (1000, 92)]);
        assert!(calls.len() <= 5, "{calls:?}");
        assert!(calls.iter().all(|&(_, q)| (40..=92).contains(&q)));
    }

    #[test]
    fn scales_down_until_it_fits() {
        // At full size even quality 40 is 3.2 MB.
        let fake = Fake::default();
        let target = 2_000_000;
        let f = fit(&px(1000, 800), target, None, &fake).unwrap();
        assert!(f.bytes.len() as u64 <= target);
        assert!(f.width < 1000 && f.width >= MIN_LONG_EDGE);
        // One jump: sqrt(2.0 / 3.2) * 0.95 of the size.
        let edges: Vec<u32> = fake.calls.lock().unwrap().iter().map(|c| c.0).collect();
        let mut distinct = edges.clone();
        distinct.dedup();
        assert_eq!(distinct, [1000, 751]);
        assert_eq!((f.width, f.height), (751, 601));
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
        let edges: Vec<u32> = fake.calls.lock().unwrap().iter().map(|c| c.0).collect();
        assert_eq!(edges, [1000, 1000, 640, 640]);
        // An image already smaller than the floor is not scaled at all.
        let fake = Fake::default();
        assert!(fit(&px(300, 200), 10, None, &fake).is_err());
        assert!(fake.calls.lock().unwrap().iter().all(|c| c.0 == 300));
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
