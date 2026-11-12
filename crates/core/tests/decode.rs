//! T06: the core decoder on every M1 fixture.

mod common;

use common::fixtures;
use safe_upload_core::decode::{Decoder, GlycinDecoder};
use safe_upload_core::{Error, Format, Limits, Orientation, orient};

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
fn keeps_the_icc_profile_for_conversion() {
    let want = common::fixture("display-p3.icc");
    assert_eq!(decode("p3.jpg").unwrap().icc, Some(want));
    assert_eq!(decode("exif-gps-thumb.jpg").unwrap().icc, None);
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
    let err = decode("huge-dims.jpg").unwrap_err();
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
fn heif_never_reaches_the_decoder() {
    assert!(matches!(
        decode("heif-header.heic"),
        Err(Error::UnsupportedFormat(Format::Heif))
    ));
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
