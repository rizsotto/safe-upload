//! T13: the whole pipeline on every fixture, and fail closed (S-6).

mod common;

use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::fixtures;
use safe_upload_core::decode::GlycinDecoder;
use safe_upload_core::encode::{Encoder, JpegEncoder, Pixels};
use safe_upload_core::output::PrivateDir;
use safe_upload_core::pipeline::{default_encoder, prepare_with};
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
        assert_eq!(bytes.len() as u64, p.output_bytes, "{name}");
        assert!(p.output_bytes <= target, "{name}");
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
    assert_eq!(p.input_size, (64, 96));
    assert_eq!(p.output_size, (96, 64));
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
            assert!(
                matches!(err, Error::UnsupportedFormat(Format::Heif)),
                "{err}"
            );
        }
        assert!(files_in(dir.path()).is_empty(), "{name}");
    }
}

#[test]
fn heif_is_refused_before_the_decoder_runs() {
    struct Panics;
    impl safe_upload_core::decode::Decoder for Panics {
        fn decode(&self, _: &Path, _: &Limits) -> Result<safe_upload_core::decode::Decoded, Error> {
            panic!("the decoder must not see a HEIF file");
        }
    }
    let dir = private_dir("heif-first");
    let err = prepare_with(
        &fixtures().join("heif-header.heic"),
        &Options::new(1_000_000),
        one(&dir),
        &Limits::default(),
        &Panics,
        &default_encoder,
    )
    .unwrap_err();
    assert!(matches!(err, Error::UnsupportedFormat(Format::Heif)));
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
    assert_eq!(fs::read(&input).unwrap(), original);
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
    assert_eq!(p.input_size, (640, 480));
    assert_eq!(p.output_size, (320, 200));
}
