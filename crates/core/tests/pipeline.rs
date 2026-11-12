//! T13: the whole pipeline on every fixture, and fail closed (S-6).

mod common;

use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::fixtures;
use safe_upload_core::decode::{Decoder, GlycinDecoder};
use safe_upload_core::encode::{Encoder, JpegEncoder, Pixels};
use safe_upload_core::output::PrivateDir;
use safe_upload_core::pipeline::{default_encoder, prepare_with, verify_output};
use safe_upload_core::verify::verify;
use safe_upload_core::{Error, Format, Limits, Options, OutputFormat, Target, prepare};

fn private_dir(test: &str) -> PrivateDir {
    let rt = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("pipeline")
        .join(test);
    let _ = fs::remove_dir_all(&rt);
    fs::create_dir_all(&rt).unwrap();
    fs::set_permissions(&rt, Permissions::from_mode(0o700)).unwrap();
    PrivateDir::create_in(&rt).unwrap()
}

fn files_in(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect()
}

fn one(dir: &PrivateDir) -> Target<'_> {
    Target {
        dir,
        position: 1,
        total: 1,
    }
}

#[test]
fn every_fixture_prepares() {
    let mut cases: Vec<(String, &str)> = [
        ("exif-gps-thumb.jpg", "jpg"),
        ("png-text.png", "jpg"),
        ("alpha.png", "png"),
        ("p3.jpg", "jpg"),
        ("orient-reference.png", "jpg"),
        ("rgb.webp", "jpg"),
        ("alpha.webp", "png"),
        ("rgb.avif", "jpg"),
        ("alpha.avif", "png"),
        ("p3-nclx.avif", "jpg"),
        ("heic-gps.heic", "jpg"),
        ("heic-rot90.heic", "jpg"),
        ("heic-mirror.heic", "jpg"),
    ]
    .into_iter()
    .map(|(n, e)| (n.to_string(), e))
    .collect();
    cases.extend((1..=8).map(|n| (format!("orient-{n}.jpg"), "jpg")));
    for (i, (name, ext)) in cases.iter().enumerate() {
        let dir = private_dir(&format!("fixture-{i}"));
        let target = 200_000;
        let p = prepare(&fixtures().join(name), &Options::new(target), one(&dir))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(p.path, dir.path().join(format!("image.{ext}")), "{name}");
        let bytes = fs::read(&p.path).unwrap();
        assert_eq!(bytes.len() as u64, p.summary.output_bytes, "{name}");
        assert!(p.summary.output_bytes <= target, "{name}");
        verify(&bytes, None).unwrap_or_else(|e| panic!("{name}: {e}"));
        let mode = fs::metadata(&p.path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{name}");
    }
}

#[test]
fn orientation_6_swaps_the_size() {
    let dir = private_dir("orient6");
    let p = prepare(
        &fixtures().join("orient-6.jpg"),
        &Options::new(1_000_000),
        one(&dir),
    )
    .unwrap();
    assert_eq!(p.summary.input_size, (64, 96));
    assert_eq!(p.summary.output_size, (96, 64));
}

#[test]
fn heic_rotation_swaps_the_size() {
    let dir = private_dir("heic-rot90");
    let p = prepare(
        &fixtures().join("heic-rot90.heic"),
        &Options::new(1_000_000),
        one(&dir),
    )
    .unwrap();
    // glycin applies the irot box while decoding.
    assert_eq!(p.summary.input_size, (480, 640));
    assert_eq!(p.summary.output_size, (480, 640));
}

#[test]
fn heic_mirror_is_applied_once() {
    let dir = private_dir("heic-mirror");
    let p = prepare(
        &fixtures().join("heic-mirror.heic"),
        &Options::new(1_000_000),
        one(&dir),
    )
    .unwrap();
    assert_eq!(p.summary.output_size, (640, 480));
    let out = GlycinDecoder.decode(&p.path, &Limits::default()).unwrap();
    // Red bottom-left, as libheif shows it (fixtures/check.sh): imir
    // applied, EXIF Orientation 2 not applied on top.
    for ((x, y), red) in [
        ((160, 120), false),
        ((480, 120), false),
        ((160, 360), true),
        ((480, 360), false),
    ] {
        let px = out.pixels.get_pixel(x, y);
        assert_eq!(px[0] > 200 && px[2] < 60, red, "{x},{y}: {px:?}");
    }
}

#[test]
fn bad_inputs_leave_no_file() {
    for (i, name) in [
        "heif-header.heic",
        "truncated.jpg",
        "truncated.png",
        "huge-dims.jpg",
    ]
    .into_iter()
    .enumerate()
    {
        let dir = private_dir(&format!("bad-{i}"));
        let err =
            prepare(&fixtures().join(name), &Options::new(1_000_000), one(&dir)).expect_err(name);
        if name == "heif-header.heic" {
            assert!(matches!(err, Error::Decode(_)), "{err}");
        }
        assert!(files_in(dir.path()).is_empty(), "{name}");
    }
}

#[test]
fn missing_decoder_is_passed_on_and_leaves_no_file() {
    struct NoCodec;
    impl safe_upload_core::decode::Decoder for NoCodec {
        fn decode(&self, _: &Path, _: &Limits) -> Result<safe_upload_core::decode::Decoded, Error> {
            Err(Error::MissingDecoder(Format::Heic))
        }
    }
    let dir = private_dir("heif-no-codec");
    let err = prepare_with(
        &fixtures().join("heic-gps.heic"),
        &Options::new(1_000_000),
        one(&dir),
        &Limits::default(),
        &NoCodec,
        &default_encoder,
    )
    .unwrap_err();
    assert!(matches!(err, Error::MissingDecoder(Format::Heic)), "{err}");
    assert!(files_in(dir.path()).is_empty());
}

/// Adds an APP1 Exif segment after SOI, like an encoder that leaks.
struct Leaky;

impl Encoder for Leaky {
    fn encode(&self, px: &Pixels, quality: u8) -> Result<Vec<u8>, Error> {
        let clean = JpegEncoder.encode(px, quality)?;
        let payload = b"Exif\0\0SECRET";
        let mut out = clean[..2].to_vec();
        out.extend_from_slice(&[0xff, 0xe1]);
        out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(payload);
        out.extend_from_slice(&clean[2..]);
        Ok(out)
    }

    fn extension(&self) -> &'static str {
        "jpg"
    }
}

#[test]
fn leaky_encoder_fails_closed_and_leaves_no_file() {
    let dir = private_dir("leaky");
    let input = fixtures().join("exif-gps-thumb.jpg");
    let original = common::fixture("exif-gps-thumb.jpg");
    // An old mtime on the folder shows if a file was created and deleted.
    let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    fs::File::open(dir.path())
        .unwrap()
        .set_modified(old)
        .unwrap();
    let err = prepare_with(
        &input,
        &Options::new(1_000_000),
        one(&dir),
        &Limits::default(),
        &GlycinDecoder,
        &|_| Ok(Box::new(Leaky)),
    )
    .unwrap_err();
    assert!(matches!(err, Error::Verify(_)), "{err}");
    assert!(files_in(dir.path()).is_empty());
    let mtime = fs::metadata(dir.path()).unwrap().modified().unwrap();
    assert_eq!(mtime, old, "the leaky bytes were never written");
    assert_eq!(fs::read(&input).unwrap(), original);
}

#[test]
fn verify_output_accepts_outputs_and_refuses_metadata() {
    let dir = private_dir("verify-output");
    let p = prepare(
        &fixtures().join("exif-gps-thumb.jpg"),
        &Options::new(1_000_000),
        one(&dir),
    )
    .unwrap();
    let bytes = fs::read(&p.path).unwrap();
    verify_output(&bytes).unwrap();
    let original = common::fixture("exif-gps-thumb.jpg");
    assert!(matches!(verify_output(&original), Err(Error::Verify(_))));
}

#[test]
fn format_choice_and_batch_names() {
    let dir = private_dir("formats");
    let mut opts = Options::new(1_000_000);
    opts.format = OutputFormat::Jpeg;
    let at = |position| Target {
        dir: &dir,
        position,
        total: 2,
    };
    let p = prepare(&fixtures().join("alpha.png"), &opts, at(1)).unwrap();
    assert_eq!(p.path, dir.path().join("image-1.jpg"));
    opts.format = OutputFormat::Png;
    let p = prepare(&fixtures().join("p3.jpg"), &opts, at(2)).unwrap();
    assert_eq!(p.path, dir.path().join("image-2.png"));
    opts.format = OutputFormat::Webp;
    let err = prepare(&fixtures().join("p3.jpg"), &opts, at(1)).unwrap_err();
    assert!(matches!(err, Error::Encode(_)));
}

#[test]
fn max_long_edge_and_redaction_are_applied() {
    use safe_upload_core::edit::{Rect, Redaction};
    let dir = private_dir("edits");
    let mut opts = Options::new(1_000_000);
    opts.max_long_edge = Some(320);
    opts.crop = Some(Rect::new(0, 0, 640, 400));
    opts.redactions = vec![Redaction::black_box(Rect::new(0, 0, 640, 400))];
    let p = prepare(&fixtures().join("exif-gps-thumb.jpg"), &opts, one(&dir)).unwrap();
    assert_eq!(p.summary.input_size, (640, 480));
    assert_eq!(p.summary.output_size, (320, 200));
}

#[test]
fn summary_names_removed_kinds_without_values() {
    use safe_upload_core::summary::Removed::*;
    let dir = private_dir("summary");
    let input = fixtures().join("exif-gps-thumb.jpg");
    let p = prepare(&input, &Options::new(1_000_000), one(&dir)).unwrap();
    let removed = p.summary.removed.clone().unwrap();
    for kind in [Location, Camera, Date, MakerNotes, Thumbnail, Xmp, Iptc] {
        assert!(removed.contains(&kind), "{kind:?} missing from {removed:?}");
    }
    assert_eq!(p.summary.input_bytes, fs::metadata(&input).unwrap().len());
    let text = p.summary.to_string();
    assert!(text.contains("Location"), "{text}");
    for secret in [
        "FixtureCam",
        "FixtureModel",
        "SN12345",
        "2026",
        "Budapest",
        "Fixture Person",
        "SECRET",
        "47.5",
        "19.04",
        "exif-gps-thumb",
        "image.jpg",
    ] {
        assert!(!text.contains(secret), "{secret} in {text}");
    }

    let p = prepare(
        &fixtures().join("png-text.png"),
        &Options::new(1_000_000),
        Target {
            dir: &dir,
            position: 2,
            total: 2,
        },
    )
    .unwrap();
    let removed = p.summary.removed.unwrap();
    for kind in [Location, Camera, Text, Xmp] {
        assert!(removed.contains(&kind), "{kind:?} missing from {removed:?}");
    }
    let p = prepare(
        &fixtures().join("p3.jpg"),
        &Options::new(1_000_000),
        Target {
            dir: &dir,
            position: 3,
            total: 3,
        },
    )
    .unwrap();
    assert_eq!(p.summary.removed.unwrap(), [ColorProfile]);
}

/// SPEC 9.1 step 5: P3 input comes out as sRGB, from an ICC profile and
/// from CICP. The mid-tone P3 rgb(200,120,80) is sRGB rgb(213,115,70).
#[test]
fn p3_output_is_converted_to_srgb() {
    use safe_upload_core::decode::Decoder;
    for (i, name) in ["p3.jpg", "p3-nclx.avif"].into_iter().enumerate() {
        let dir = private_dir(&format!("p3-{i}"));
        let p = prepare(&fixtures().join(name), &Options::new(1_000_000), one(&dir)).unwrap();
        let out = GlycinDecoder.decode(&p.path, &Limits::default()).unwrap();
        assert_eq!(out.color, None, "{name}: no profile in the output");
        let px = out.pixels.get_pixel(48, 16).0;
        for (got, want) in px.iter().zip([213u8, 115, 70]) {
            assert!(got.abs_diff(want) <= 6, "{name}: got {px:?}");
        }
    }
}
