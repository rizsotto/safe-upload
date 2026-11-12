//! M2 T05: `pipeline::preview` decodes, orients and converts like the
//! pipeline, downscales, and writes nothing.

mod common;

use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::fixtures;
use safe_upload_core::decode::{Decoded, Decoder, GlycinDecoder};
use safe_upload_core::output::PrivateDir;
use safe_upload_core::pipeline::{preview, preview_with};
use safe_upload_core::{Error, Format, Limits, Options, OutputFormat, Target, prepare};

fn preview_of(name: &str, max_edge: u32) -> safe_upload_core::pipeline::Preview {
    preview(&fixtures().join(name), &Limits::default(), max_edge)
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn near(got: [u8; 4], want: [u8; 3], tolerance: u8) -> bool {
    got.iter()
        .zip(want)
        .all(|(g, w)| g.abs_diff(w) <= tolerance)
}

#[test]
fn orientation_6_is_upright_with_swapped_size() {
    let p = preview_of("orient-6.jpg", 2048);
    assert_eq!(p.full_size, (96, 64));
    assert_eq!(p.pixels.dimensions(), (96, 64));
    // Quadrant centers of orient-reference.png: red, lime, blue, white.
    for ((x, y), want) in [
        ((24, 16), [255, 0, 0]),
        ((72, 16), [0, 255, 0]),
        ((24, 48), [0, 0, 255]),
        ((72, 48), [255, 255, 255]),
    ] {
        let got = p.pixels.get_pixel(x, y).0;
        assert!(near(got, want, 20), "at {x},{y}: {got:?}");
    }
}

#[test]
fn every_orientation_matches_the_reference() {
    let reference = preview_of("orient-reference.png", 2048);
    for n in 1..=8 {
        let p = preview_of(&format!("orient-{n}.jpg"), 2048);
        assert_eq!(p.full_size, reference.full_size, "orient-{n}");
        for (x, y) in [(24, 16), (72, 16), (24, 48), (72, 48)] {
            let want = reference.pixels.get_pixel(x, y).0;
            let got = p.pixels.get_pixel(x, y).0;
            assert!(
                near(got, [want[0], want[1], want[2]], 20),
                "orient-{n} at {x},{y}: {got:?}"
            );
        }
    }
}

/// The preview and the prepared output come from the same pixels.
#[test]
fn preview_matches_the_prepared_output() {
    let rt = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("preview-vs-prepare");
    let _ = fs::remove_dir_all(&rt);
    fs::create_dir_all(&rt).unwrap();
    fs::set_permissions(&rt, Permissions::from_mode(0o700)).unwrap();
    let dir = PrivateDir::create_in(&rt).unwrap();
    let mut opts = Options::new(1_000_000);
    opts.format = OutputFormat::Png;
    for (i, name) in ["orient-6.jpg", "orient-7.jpg", "p3.jpg"]
        .iter()
        .enumerate()
    {
        let target = Target {
            dir: &dir,
            position: i + 1,
            total: 3,
        };
        let out = prepare(&fixtures().join(name), &opts, target).unwrap();
        let out = GlycinDecoder.decode(&out.path, &Limits::default()).unwrap();
        let p = preview_of(name, 2048);
        assert_eq!(out.pixels.dimensions(), p.pixels.dimensions(), "{name}");
        for (a, b) in out.pixels.pixels().zip(p.pixels.pixels()) {
            assert!(near(a.0, [b[0], b[1], b[2]], 2), "{name}: {a:?} vs {b:?}");
        }
    }
}

/// SPEC 9.2: P3 rgb(200,120,80) is sRGB rgb(213,115,70), from an ICC
/// profile and from CICP.
#[test]
fn p3_preview_is_srgb() {
    for name in ["p3.jpg", "p3-nclx.avif"] {
        let p = preview_of(name, 2048);
        let got = p.pixels.get_pixel(48, 16).0;
        assert!(near(got, [213, 115, 70], 6), "{name}: {got:?}");
    }
}

#[test]
fn max_edge_is_respected_and_full_size_reported() {
    let p = preview_of("exif-gps-thumb.jpg", 320);
    assert_eq!(p.full_size, (640, 480));
    assert_eq!(p.pixels.dimensions(), (320, 240));
    let p = preview_of("orient-6.jpg", 48);
    assert_eq!(p.full_size, (96, 64));
    assert_eq!(p.pixels.dimensions(), (48, 32));
    let p = preview_of("orient-6.jpg", 1000);
    assert_eq!(p.pixels.dimensions(), (96, 64), "never upscaled");
}

#[test]
fn hostile_inputs_fail_like_the_pipeline() {
    let limits = Limits::default();
    // No unwrap_err: printing a decoded 400 MP image takes minutes.
    let Err(err) = preview(&fixtures().join("huge-dims.jpg"), &limits, 2048) else {
        panic!("huge-dims.jpg decoded");
    };
    assert!(matches!(err, Error::TooManyPixels { .. }), "{err}");
    for name in ["truncated.jpg", "truncated.png"] {
        let Err(err) = preview(&fixtures().join(name), &limits, 2048) else {
            panic!("{name} decoded");
        };
        assert!(matches!(err, Error::Decode(_)), "{name}: {err}");
    }
    let small = Limits {
        max_file_bytes: 100,
        ..Limits::default()
    };
    let Err(err) = preview(&fixtures().join("p3.jpg"), &small, 2048) else {
        panic!("file size limit ignored");
    };
    assert!(matches!(err, Error::FileTooLarge { .. }), "{err}");
}

#[test]
fn decode_timeout_fails_closed() {
    let limits = Limits {
        decode_timeout: Duration::from_millis(1),
        ..Limits::default()
    };
    let Err(err) = preview(&fixtures().join("exif-gps-thumb.jpg"), &limits, 2048) else {
        panic!("decoded within 1 ms");
    };
    assert!(
        matches!(err, Error::Decode(_)) && err.to_string().contains("decoding took too long"),
        "{err}"
    );
}

#[test]
fn heif_is_refused_before_the_decoder_runs() {
    struct Panics;
    impl Decoder for Panics {
        fn decode(&self, _: &Path, _: &Limits) -> Result<Decoded, Error> {
            panic!("the decoder must not see a HEIF file");
        }
    }
    let err = preview_with(
        &fixtures().join("heif-header.heic"),
        &Limits::default(),
        2048,
        &Panics,
    )
    .unwrap_err();
    assert!(
        matches!(err, Error::UnsupportedFormat(Format::Heif)),
        "{err}"
    );
}

const CHILD: &str = "SAFE_UPLOAD_PREVIEW_CHILD";

/// Runs only as the child of [`preview_writes_no_file`], with a fake
/// runtime dir, temp dir and home, so it sees nothing the other tests do.
#[test]
fn preview_child() {
    let Some(input) = std::env::var_os(CHILD) else {
        return;
    };
    let p = preview(Path::new(&input), &Limits::default(), 32).unwrap();
    assert_eq!(p.pixels.dimensions(), (32, 24));
}

fn tree(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).unwrap() {
        let path = e.unwrap().path();
        if path.is_dir() {
            out.extend(tree(&path));
        }
        out.push(path);
    }
    out.sort();
    out
}

#[test]
fn preview_writes_no_file() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("preview-no-file");
    let _ = fs::remove_dir_all(&root);
    let dirs = ["runtime", "tmp", "home", "input"].map(|d| root.join(d));
    for d in &dirs {
        fs::create_dir_all(d).unwrap();
        fs::set_permissions(d, Permissions::from_mode(0o700)).unwrap();
    }
    let input = dirs[3].join("photo.jpg");
    fs::write(&input, common::fixture("exif-gps-thumb.jpg")).unwrap();
    let before = tree(&root);
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "preview_child", "--test-threads=1"])
        .env(CHILD, &input)
        .env("XDG_RUNTIME_DIR", &dirs[0])
        .env("TMPDIR", &dirs[1])
        .env("HOME", &dirs[2])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(tree(&root), before);
}
