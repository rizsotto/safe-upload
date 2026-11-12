//! T06: the core decoder on every M1 fixture.

mod common;

use common::fixtures;
use safe_upload_core::color::{self, SourceColor};
use safe_upload_core::decode::{Decoder, GlycinDecoder};
use safe_upload_core::{Error, Limits, Orientation, orient};

fn decode(name: &str) -> Result<safe_upload_core::decode::Decoded, Error> {
    GlycinDecoder.decode(&fixtures().join(name), &Limits::default())
}

#[test]
fn decodes_every_fixture() {
    for (name, size) in [
        ("exif-gps-thumb.jpg", (640, 480)),
        ("png-text.png", (64, 48)),
        ("alpha.png", (64, 64)),
        ("p3.jpg", (64, 64)),
        ("orient-reference.png", (96, 64)),
    ] {
        let d = decode(name).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(d.pixels.dimensions(), size, "{name}");
    }
    let alpha = decode("alpha.png").unwrap();
    assert_eq!(alpha.pixels.get_pixel(0, 0)[3], 0);
    assert_eq!(alpha.pixels.get_pixel(32, 32)[3], 255);
}

#[test]
fn decodes_real_webp_and_avif_in_the_sandbox() {
    for (name, size, alpha) in [
        ("rgb.webp", (64, 48), false),
        ("alpha.webp", (64, 64), true),
        ("rgb.avif", (64, 48), false),
        ("alpha.avif", (64, 64), true),
    ] {
        let d = decode(name).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(d.pixels.dimensions(), size, "{name}");
        // Lossy AVIF alpha is not exactly 0.
        let corner = d.pixels.get_pixel(0, 0)[3];
        assert_eq!(corner < 8, alpha, "{name}: corner alpha {corner}");
        assert_eq!(d.pixels.get_pixel(32, 32)[3], 255, "{name}");
    }
}

#[test]
fn avif_with_cicp_only_converts_to_srgb() {
    let d = decode("p3-nclx.avif").unwrap();
    assert_eq!(d.color, Some(SourceColor::Cicp([12, 13, 0, 1])));
    let out = color::to_srgb(d.pixels, d.color.as_ref()).unwrap();
    // P3 rgb(200,120,80) is sRGB rgb(213,115,70).
    let p = out.get_pixel(48, 16);
    for (got, want) in p.0.iter().zip([213u8, 115, 70]) {
        assert!(got.abs_diff(want) <= 2, "got {p:?}");
    }
}

#[test]
fn keeps_the_icc_profile_for_conversion() {
    let want = common::fixture("display-p3.icc");
    assert_eq!(
        decode("p3.jpg").unwrap().color,
        Some(SourceColor::Icc(want))
    );
    assert_eq!(decode("exif-gps-thumb.jpg").unwrap().color, None);
}

#[test]
fn orientations_come_out_upright() {
    let reference = decode("orient-reference.png").unwrap().pixels;
    for n in 1..=8u16 {
        let d = decode(&format!("orient-{n}.jpg")).unwrap();
        assert_eq!(d.orientation as u16, n);
        assert_eq!(d.orientation, Orientation::from_exif(n));
        let upright = orient::apply(d.pixels, d.orientation);
        assert_eq!(upright.dimensions(), reference.dimensions(), "orient-{n}");
        // Quadrant centers: red, lime, blue, white.
        for (x, y) in [(24, 16), (72, 16), (24, 48), (72, 48)] {
            let (got, want) = (upright.get_pixel(x, y), reference.get_pixel(x, y));
            for c in 0..3 {
                assert!(
                    got[c].abs_diff(want[c]) < 20,
                    "orient-{n} at {x},{y}: {got:?}"
                );
            }
        }
    }
}

#[test]
fn huge_image_is_refused_before_decoding() {
    // No unwrap_err: printing a decoded 400 MP image takes minutes.
    let Err(err) = decode("huge-dims.jpg") else {
        panic!("huge-dims.jpg decoded");
    };
    assert!(
        matches!(
            err,
            Error::TooManyPixels {
                pixels: 400_000_000,
                ..
            }
        ),
        "{err}"
    );
}

#[test]
fn truncated_files_give_a_decode_error() {
    for name in ["truncated.jpg", "truncated.png"] {
        let err = decode(name).unwrap_err();
        assert!(matches!(err, Error::Decode(_)), "{name}: {err}");
    }
}

#[test]
fn heic_decodes_in_the_sandbox() {
    // GlycinDecoder fails unless the loader ran in bwrap.
    let d = decode("heic-gps.heic").unwrap();
    assert_eq!(d.pixels.dimensions(), (640, 480));
    // A red to blue gradient from top to bottom.
    let (top, bottom) = (d.pixels.get_pixel(320, 5), d.pixels.get_pixel(320, 474));
    assert!(top[0] > 200 && top[2] < 60, "{top:?}");
    assert!(bottom[2] > 200 && bottom[0] < 60, "{bottom:?}");
}

#[test]
fn heic_rotation_is_applied_once() {
    let d = decode("heic-rot90.heic").unwrap();
    let upright = orient::apply(d.pixels, d.orientation);
    assert_eq!(upright.dimensions(), (480, 640));
    // Red quadrant top-right, blue elsewhere.
    for ((x, y), red) in [
        ((360, 160), true),
        ((120, 160), false),
        ((120, 480), false),
        ((360, 480), false),
    ] {
        let p = upright.get_pixel(x, y);
        assert_eq!(p[0] > 200 && p[2] < 60, red, "{x},{y}: {p:?}");
        assert_eq!(p[2] > 200 && p[0] < 60, !red, "{x},{y}: {p:?}");
    }
}

#[test]
fn header_only_heif_is_a_decode_error() {
    let err = decode("heif-header.heic").unwrap_err();
    assert!(matches!(err, Error::Decode(_)), "{err}");
}

#[test]
fn file_size_limit() {
    let limits = Limits {
        max_file_bytes: 100,
        ..Limits::default()
    };
    let err = GlycinDecoder
        .decode(&fixtures().join("p3.jpg"), &limits)
        .unwrap_err();
    assert!(matches!(err, Error::FileTooLarge { .. }), "{err}");
}
