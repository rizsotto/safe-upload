//! T11: fit-to-size with the real JPEG encoder (SPEC 13 property test).

use image::{Rgba, RgbaImage};
use proptest::prelude::*;
use safe_upload_core::Error;
use safe_upload_core::encode::{JpegEncoder, Pixels};
use safe_upload_core::fit::fit;
use safe_upload_core::verify::verify;

/// A photo-like test image: gradients plus noise, so JPEG cannot squeeze
/// it to nothing.
fn photo() -> Pixels {
    let mut seed = 0x2545_f491_u32;
    let image = RgbaImage::from_fn(800, 600, |x, y| {
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
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(8))]

    #[test]
    fn output_is_under_target_or_a_clear_error(target in 100_000u64..=10_000_000) {
        match fit(&photo(), target, None, &JpegEncoder) {
            Ok(f) => {
                prop_assert!(f.bytes.len() as u64 <= target);
                verify(&f.bytes, None).unwrap();
            }
            Err(Error::CannotFit { target_bytes }) => prop_assert_eq!(target_bytes, target),
            Err(e) => prop_assert!(false, "unexpected error: {}", e),
        }
    }
}
