//! Converts pixels to sRGB and drops the source color description (SPEC
//! 9.1 step 5, 9.2). The output pixels are always sRGB, so the encoder
//! needs nothing from the source.
//!
//! Uses moxcms, which is pure Rust: the ICC profile comes from an
//! untrusted file and is parsed in our process (NFR-2). We do not use
//! glycin's `color_convert_icc_srgb(true)`: it runs the same code in our
//! process, and on a bad profile it only warns and returns the pixels
//! unconverted, labelled sRGB.

use std::panic::{AssertUnwindSafe, catch_unwind};

use image::RgbaImage;
use moxcms::{
    CicpColorPrimaries, CicpProfile, ColorProfile, DataColorSpace, Layout, MatrixCoefficients,
    TransferCharacteristics, TransformOptions,
};

use crate::Error;

/// How the source file describes its colors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceColor {
    /// An embedded ICC profile.
    Icc(Vec<u8>),
    /// CICP code points (primaries, transfer, matrix, full range flag), as
    /// in an AVIF `nclx` box.
    Cicp([u8; 4]),
}

/// Returns the pixels converted to sRGB. Without a color description the
/// pixels are taken to be sRGB already and are returned unchanged. RGB and
/// gray ICC profiles and CICP with known primaries are supported; anything
/// else is an error.
pub fn to_srgb(image: RgbaImage, source: Option<&SourceColor>) -> Result<RgbaImage, Error> {
    let Some(source) = source else {
        return Ok(image);
    };
    catch_unwind(AssertUnwindSafe(|| convert(image, source)))
        .unwrap_or(Err(Error::Color("the color conversion crashed")))
}

fn convert(mut image: RgbaImage, source: &SourceColor) -> Result<RgbaImage, Error> {
    let profile = match source {
        SourceColor::Icc(icc) => ColorProfile::new_from_slice(icc)
            .map_err(|_| Error::Color("cannot read the ICC profile"))?,
        SourceColor::Cicp(cicp) => match cicp_profile(*cicp)? {
            Some(p) => p,
            None => return Ok(image),
        },
    };
    let srgb = ColorProfile::new_srgb();
    let failed = |_| Error::Color("cannot convert the colors to sRGB");
    let options = TransformOptions::default();
    match profile.color_space {
        DataColorSpace::Rgb => {
            let t = profile
                .create_transform_8bit(Layout::Rgba, &srgb, Layout::Rgba, options)
                .map_err(failed)?;
            let src = image.as_raw().clone();
            t.transform(&src, &mut image).map_err(failed)?;
        }
        DataColorSpace::Gray => {
            let t = profile
                .create_transform_8bit(Layout::Gray, &srgb, Layout::Rgb, options)
                .map_err(failed)?;
            let gray: Vec<u8> = image.pixels().map(|p| p[0]).collect();
            let mut rgb = vec![0u8; gray.len() * 3];
            t.transform(&gray, &mut rgb).map_err(failed)?;
            for (p, c) in image.pixels_mut().zip(rgb.as_chunks::<3>().0) {
                p.0[..3].copy_from_slice(c);
            }
        }
        _ => return Err(Error::Color("this color space is not supported")),
    }
    Ok(image)
}

/// The profile for CICP code points, or `None` if they mean sRGB.
/// Unspecified primaries mean sRGB; an unspecified transfer with known
/// primaries is taken to be the sRGB curve.
fn cicp_profile([primaries, transfer, _, _]: [u8; 4]) -> Result<Option<ColorProfile>, Error> {
    const SRGB_PRIMARIES: u8 = 1;
    const UNSPECIFIED: u8 = 2;
    const SRGB_TRANSFER: u8 = 13;
    let transfer = if transfer == UNSPECIFIED {
        SRGB_TRANSFER
    } else {
        transfer
    };
    if primaries == UNSPECIFIED || (primaries == SRGB_PRIMARIES && transfer == SRGB_TRANSFER) {
        return Ok(None);
    }
    let unsupported = |_| Error::Color("these CICP values are not supported");
    let cicp = CicpProfile {
        color_primaries: CicpColorPrimaries::try_from(primaries).map_err(unsupported)?,
        transfer_characteristics: TransferCharacteristics::try_from(transfer)
            .map_err(unsupported)?,
        // The decoder already gave RGB, so the matrix does not matter.
        matrix_coefficients: MatrixCoefficients::Identity,
        full_range: true,
    };
    let profile = ColorProfile::new_from_cicp(cicp);
    // moxcms leaves the curves empty when it cannot use the values.
    if profile.red_trc.is_none() {
        return Err(Error::Color("these CICP values are not supported"));
    }
    Ok(Some(profile))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

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

    const P3_INPUT: [[u8; 4]; 3] = [[255, 0, 0, 255], [200, 120, 80, 128], [0, 200, 0, 0]];
    // From the standard P3 (D65) to sRGB matrix.
    const P3_AS_SRGB: [[u8; 4]; 3] = [[255, 0, 0, 255], [213, 115, 70, 128], [0, 204, 0, 0]];

    #[test]
    fn p3_profile_converts_to_srgb() {
        let icc = ColorProfile::new_display_p3().encode().unwrap();
        let out = to_srgb(image(&P3_INPUT), Some(&SourceColor::Icc(icc))).unwrap();
        assert_near(&out, &P3_AS_SRGB, 2);
    }

    #[test]
    fn p3_cicp_converts_to_srgb() {
        // Primaries 12 (Display P3), transfer 13 (sRGB), matrix 6, full range.
        let out = to_srgb(image(&P3_INPUT), Some(&SourceColor::Cicp([12, 13, 6, 1]))).unwrap();
        assert_near(&out, &P3_AS_SRGB, 2);
    }

    #[test]
    fn srgb_is_unchanged() {
        let colors: Vec<[u8; 4]> = (0..=255).map(|v| [v, 255 - v, v / 2, v]).collect();
        let input = image(&colors);
        assert_eq!(to_srgb(input.clone(), None).unwrap(), input);
        for cicp in [[1, 13, 6, 1], [2, 2, 2, 0], [1, 2, 0, 1]] {
            let out = to_srgb(input.clone(), Some(&SourceColor::Cicp(cicp))).unwrap();
            assert_eq!(out, input, "{cicp:?}");
        }
        let icc = ColorProfile::new_srgb().encode().unwrap();
        let out = to_srgb(input, Some(&SourceColor::Icc(icc))).unwrap();
        assert_near(&out, &colors, 1);
    }

    #[test]
    fn gray_profile_gives_gray_srgb() {
        let gray = ColorProfile::new_gray_with_gamma(2.2).encode().unwrap();
        let input = image(&[[0, 0, 0, 255], [128, 128, 128, 7], [255, 255, 255, 255]]);
        let out = to_srgb(input, Some(&SourceColor::Icc(gray))).unwrap();
        for (p, a) in out.pixels().zip([255, 7, 255]) {
            assert!(p[0] == p[1] && p[1] == p[2] && p[3] == a, "{p:?}");
        }
        assert_eq!(out.get_pixel(0, 0)[0], 0);
        assert_eq!(out.get_pixel(2, 0)[0], 255);
    }

    #[test]
    fn bad_input_is_an_error() {
        let px = || image(&[[1, 2, 3, 4]]);
        let bad_icc = SourceColor::Icc(b"not a profile".to_vec());
        assert!(matches!(
            to_srgb(px(), Some(&bad_icc)),
            Err(Error::Color(_))
        ));
        for cicp in [[3, 13, 0, 1], [12, 3, 0, 1], [200, 13, 0, 1]] {
            assert!(
                matches!(
                    to_srgb(px(), Some(&SourceColor::Cicp(cicp))),
                    Err(Error::Color(_))
                ),
                "{cicp:?}"
            );
        }
    }
}
