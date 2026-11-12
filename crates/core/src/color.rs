//! Converts pixels to sRGB and drops the source profile (SPEC 9.1 step 5,
//! 9.2). The output pixels are always sRGB, so the encoder needs no
//! profile from the source.

use image::RgbaImage;
use lcms2::{ColorSpaceSignature, Flags, Intent, PixelFormat, Profile, Transform};

use crate::Error;

/// Returns the pixels converted from `icc` to sRGB. Without a profile the
/// pixels are taken to be sRGB already and are returned unchanged. RGB and
/// gray profiles are supported; any other profile, or one lcms2 cannot
/// read, is an error.
pub fn to_srgb(mut image: RgbaImage, icc: Option<&[u8]>) -> Result<RgbaImage, Error> {
    let Some(icc) = icc else {
        return Ok(image);
    };
    let source = Profile::new_icc(icc).map_err(|_| Error::Color("cannot read the ICC profile"))?;
    let srgb = Profile::new_srgb();
    let failed = |_| Error::Color("cannot convert the ICC profile to sRGB");
    match source.color_space() {
        ColorSpaceSignature::RgbData => {
            let t = Transform::<u8, u8>::new_flags(
                &source,
                PixelFormat::RGBA_8,
                &srgb,
                PixelFormat::RGBA_8,
                Intent::Perceptual,
                Flags::COPY_ALPHA,
            )
            .map_err(failed)?;
            t.transform_in_place(&mut image);
        }
        ColorSpaceSignature::GrayData => {
            let t = Transform::<u8, [u8; 3]>::new(
                &source,
                PixelFormat::GRAY_8,
                &srgb,
                PixelFormat::RGB_8,
                Intent::Perceptual,
            )
            .map_err(failed)?;
            let gray: Vec<u8> = image.pixels().map(|p| p[0]).collect();
            let mut rgb = vec![[0u8; 3]; gray.len()];
            t.transform_pixels(&gray, &mut rgb);
            for (p, c) in image.pixels_mut().zip(rgb) {
                p.0[..3].copy_from_slice(&c);
            }
        }
        _ => {
            return Err(Error::Color(
                "ICC profiles of this color space are not supported",
            ));
        }
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    use lcms2::{CIExyY, CIExyYTRIPLE, ToneCurve};

    fn srgb_curve() -> ToneCurve {
        ToneCurve::new_parametric(4, &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045])
            .unwrap()
    }

    fn display_p3() -> Vec<u8> {
        let d65 = CIExyY {
            x: 0.3127,
            y: 0.3290,
            Y: 1.0,
        };
        let p = |x, y| CIExyY { x, y, Y: 1.0 };
        let primaries = CIExyYTRIPLE {
            Red: p(0.680, 0.320),
            Green: p(0.265, 0.690),
            Blue: p(0.150, 0.060),
        };
        let c = srgb_curve();
        Profile::new_rgb(&d65, &primaries, &[&c, &c, &c])
            .unwrap()
            .icc()
            .unwrap()
    }

    fn image(colors: &[[u8; 4]]) -> RgbaImage {
        RgbaImage::from_fn(colors.len() as u32, 1, |x, _| Rgba(colors[x as usize]))
    }

    fn assert_near(got: &RgbaImage, want: &[[u8; 4]], tolerance: u8) {
        for (p, w) in got.pixels().zip(want) {
            for (g, w) in p.0.iter().zip(w) {
                assert!(g.abs_diff(*w) <= tolerance, "got {:?}, want {w:?}", p.0);
            }
        }
    }

    #[test]
    fn p3_converts_to_srgb() {
        // Expected values from the standard P3 (D65) to sRGB matrix.
        let input = image(&[[255, 0, 0, 255], [200, 120, 80, 128], [0, 200, 0, 0]]);
        let out = to_srgb(input, Some(&display_p3())).unwrap();
        assert_near(
            &out,
            &[[255, 0, 0, 255], [213, 115, 70, 128], [0, 204, 0, 0]],
            2,
        );
    }

    #[test]
    fn no_profile_is_unchanged() {
        let input = image(&[[200, 120, 80, 255], [1, 2, 3, 4]]);
        assert_eq!(to_srgb(input.clone(), None).unwrap(), input);
    }

    #[test]
    fn srgb_profile_is_unchanged() {
        let colors: Vec<[u8; 4]> = (0..=255).map(|v| [v, 255 - v, v / 2, v]).collect();
        let input = image(&colors);
        let srgb = Profile::new_srgb().icc().unwrap();
        assert_near(&to_srgb(input, Some(&srgb)).unwrap(), &colors, 1);
    }

    #[test]
    fn gray_profile_gives_gray_srgb() {
        let d65 = CIExyY {
            x: 0.3127,
            y: 0.3290,
            Y: 1.0,
        };
        let gray = Profile::new_gray(&d65, &srgb_curve())
            .unwrap()
            .icc()
            .unwrap();
        let input = image(&[[0, 0, 0, 255], [128, 128, 128, 7], [255, 255, 255, 255]]);
        let out = to_srgb(input, Some(&gray)).unwrap();
        assert_near(
            &out,
            &[[0, 0, 0, 255], [128, 128, 128, 7], [255, 255, 255, 255]],
            1,
        );
    }

    #[test]
    fn garbage_profile_is_an_error() {
        let input = image(&[[1, 2, 3, 4]]);
        assert!(matches!(
            to_srgb(input, Some(b"not a profile")),
            Err(Error::Color(_))
        ));
    }
}
