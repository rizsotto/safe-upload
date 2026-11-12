//! Fit-to-size search (SPEC 9.3, FR-3, FR-4). Finds the best quality that
//! fits the target, and scales the image down when no quality fits.

use image::imageops::{self, FilterType};

use crate::Error;
use crate::encode::{Encoder, Pixels};

/// The long edge never goes below this when scaling down to fit.
pub const MIN_LONG_EDGE: u32 = 640;
/// Each scale-down step multiplies the scale by this.
const SCALE_STEP: f64 = 0.85;
/// At most this many encodes per scale.
const MAX_STEPS: u32 = 7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fitted {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub quality: u8,
}

/// Encodes `px` so the output has at most `target_bytes` bytes. The long
/// edge is at most `max_long_edge` if given. Never returns more than the
/// target; if even the smallest allowed size does not fit, returns
/// [`Error::CannotFit`].
pub fn fit<E: Encoder + ?Sized>(
    px: &Pixels,
    target_bytes: u64,
    max_long_edge: Option<u32>,
    encoder: &E,
) -> Result<Fitted, Error> {
    let (w, h) = px.image.dimensions();
    let long_edge = w.max(h);
    if long_edge == 0 {
        return Err(Error::Encode("empty image"));
    }
    let mut scale = match max_long_edge {
        Some(max) => (f64::from(max) / f64::from(long_edge)).min(1.0),
        None => 1.0,
    };
    let (q_min, q_max) = encoder.quality_range();
    loop {
        let scaled = resize(px, scale);
        if let Some(best) = search(&scaled, target_bytes, q_min, q_max, encoder)? {
            return Ok(best);
        }
        scale *= SCALE_STEP;
        if f64::from(long_edge) * scale < f64::from(MIN_LONG_EDGE) {
            return Err(Error::CannotFit { target_bytes });
        }
    }
}

/// Binary search for the highest quality that fits.
fn search<E: Encoder + ?Sized>(
    px: &Pixels,
    target: u64,
    q_min: u8,
    q_max: u8,
    encoder: &E,
) -> Result<Option<Fitted>, Error> {
    let (mut lo, mut hi) = (i32::from(q_min), i32::from(q_max));
    let mut best = None;
    let mut steps = 0;
    while lo <= hi && steps < MAX_STEPS {
        steps += 1;
        let q = (lo + hi) / 2;
        let bytes = encoder.encode(px, q as u8)?;
        if bytes.len() as u64 <= target {
            best = Some(Fitted {
                bytes,
                width: px.image.width(),
                height: px.image.height(),
                quality: q as u8,
            });
            lo = q + 1;
        } else {
            hi = q - 1;
        }
    }
    Ok(best)
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
        icc: px.icc.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;
    use std::cell::RefCell;

    /// Output size is width * height * quality / 10 bytes.
    #[derive(Default)]
    struct Fake {
        calls: RefCell<Vec<(u32, u8)>>,
    }

    impl Encoder for Fake {
        fn encode(&self, px: &Pixels, quality: u8) -> Result<Vec<u8>, Error> {
            let (w, h) = px.image.dimensions();
            self.calls.borrow_mut().push((w.max(h), quality));
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
        let f = fit(&px(100, 100), 10_000_000, None, &Fake::default()).unwrap();
        assert_eq!(f.quality, 92);
    }

    #[test]
    fn at_most_7_encodes_per_scale() {
        let fake = Fake::default();
        fit(&px(1000, 800), 6_000_000, None, &fake).unwrap();
        let calls = fake.calls.borrow();
        assert!(calls.len() <= 7);
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
        // Each step is 85% of the previous scale.
        let edges: Vec<u32> = fake.calls.borrow().iter().map(|c| c.0).collect();
        let mut distinct = edges.clone();
        distinct.dedup();
        assert_eq!(distinct, [1000, 850, 722]);
    }

    #[test]
    fn floor_of_640_px_gives_an_error() {
        let fake = Fake::default();
        let err = fit(&px(1000, 800), 1000, None, &fake).unwrap_err();
        assert!(matches!(err, Error::CannotFit { target_bytes: 1000 }));
        assert!(fake.calls.borrow().iter().all(|c| c.0 >= MIN_LONG_EDGE));
        // An image already smaller than the floor is not scaled at all.
        let fake = Fake::default();
        assert!(fit(&px(300, 200), 10, None, &fake).is_err());
        assert!(fake.calls.borrow().iter().all(|c| c.0 == 300));
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
