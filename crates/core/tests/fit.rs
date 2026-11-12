//! T11: fit-to-size with the real JPEG encoder (SPEC 13 property test).

use std::sync::OnceLock;

use image::{Rgba, RgbaImage};
use proptest::prelude::*;
use safe_upload_core::Error;
use safe_upload_core::encode::{JpegEncoder, Pixels};
use safe_upload_core::fit::fit;
use safe_upload_core::verify::verify;

const WIDTH: u32 = 1600;

/// A photo-like test image: gradients plus noise, so JPEG cannot squeeze
/// it to nothing. At quality 92 it is about 1 MB, so small targets need
/// the quality search and the scale-down.
fn photo() -> &'static Pixels {
    static PHOTO: OnceLock<Pixels> = OnceLock::new();
    PHOTO.get_or_init(|| {
        let mut seed = 0x2545_f491_u32;
        let image = RgbaImage::from_fn(WIDTH, WIDTH * 3 / 4, |x, y| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let n = seed % 64;
            Rgba([
                ((x / 7 + n) % 256) as u8,
                ((y / 5 + n) % 256) as u8,
                ((x + y) / 13 % 256) as u8,
                255,
            ])
        });
        Pixels::new(image, None)
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(8))]

    // Log-uniform, so about half the targets are below 1 MB.
    #[test]
    fn output_is_under_target_or_a_clear_error(
        target in (0.0f64..=2.0).prop_map(|e| (100_000.0 * 10f64.powf(e)) as u64)
    ) {
        match fit(photo(), target, None, &JpegEncoder) {
            Ok(f) => {
                let len = f.bytes.len() as u64;
                prop_assert!(len <= target);
                // FR-3: at least 85% when the best quality at full size
                // does not fit anyway.
                if f.quality < 92 || f.width < WIDTH {
                    prop_assert!(len * 100 >= target * 85, "{} for {}", len, target);
                }
                verify(&f.bytes, None).unwrap();
            }
            Err(Error::CannotFit { target_bytes }) => prop_assert_eq!(target_bytes, target),
            Err(e) => prop_assert!(false, "unexpected error: {}", e),
        }
    }
}
