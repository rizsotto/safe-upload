//! T15: leak and acceptance tests on real CLI outputs (SPEC 12 M1, 13).

#[path = "../../core/tests/common/mod.rs"]
mod common;
mod env;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::fixtures;
use env::Env;
use safe_upload_core::Limits;
use safe_upload_core::decode::{Decoder, GlycinDecoder};
use safe_upload_core::verify::verify;

/// Every fixture the pipeline accepts.
const INPUTS: &[&str] = &[
    "exif-gps-thumb.jpg",
    "orient-1.jpg",
    "orient-2.jpg",
    "orient-3.jpg",
    "orient-4.jpg",
    "orient-5.jpg",
    "orient-6.jpg",
    "orient-7.jpg",
    "orient-8.jpg",
    "orient-reference.png",
    "png-text.png",
    "alpha.png",
    "p3.jpg",
    "rgb.webp",
    "alpha.webp",
    "rgb.avif",
    "alpha.avif",
    "p3-nclx.avif",
    "heic-gps.heic",
    "heic-rot90.heic",
    "heic-mirror.heic",
];

/// The only keys `exiftool -a -u -G1 -s` may show, by group. Basic file
/// and image properties only (SPEC 12 M1, plan "Leak test").
const ALLOWED: &[(&str, &[&str])] = &[
    ("ExifTool", &["ExifToolVersion"]),
    (
        "System",
        &[
            "FileName",
            "Directory",
            "FileSize",
            "FileModifyDate",
            "FileAccessDate",
            "FileInodeChangeDate",
            "FilePermissions",
        ],
    ),
    (
        "File",
        &[
            "FileType",
            "FileTypeExtension",
            "MIMEType",
            "ImageWidth",
            "ImageHeight",
            "EncodingProcess",
            "BitsPerSample",
            "ColorComponents",
            "YCbCrSubSampling",
        ],
    ),
    (
        "JFIF",
        &[
            "JFIFVersion",
            "ResolutionUnit",
            "XResolution",
            "YResolution",
        ],
    ),
    (
        "PNG",
        &[
            "ImageWidth",
            "ImageHeight",
            "BitDepth",
            "ColorType",
            "Compression",
            "Filter",
            "Interlace",
        ],
    ),
    ("PNG-PLTE", &["Palette"]),
    ("PNG-tRNS", &["Transparency"]),
    ("Composite", &["ImageSize", "Megapixels"]),
];

/// Runs the CLI on all inputs as one batch into a fresh folder.
fn prepare_all(env: &Env) -> Vec<(&'static str, PathBuf)> {
    let dest = env.work.path().join("out");
    fs::create_dir(&dest).unwrap();
    env.cmd()
        .args(INPUTS.iter().map(|n| fixtures().join(n)))
        .args(["--max-size", "2MB", "-o"])
        .arg(&dest)
        .assert()
        .success();
    let mut outputs: Vec<PathBuf> = fs::read_dir(&dest)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(outputs.len(), INPUTS.len());
    // image-N.ext, N from 1.
    outputs.sort_by_key(|p| {
        let stem = p.file_stem().unwrap().to_str().unwrap();
        stem.trim_start_matches("image-").parse::<usize>().unwrap()
    });
    INPUTS.iter().copied().zip(outputs).collect()
}

/// `(group, key)` pairs from `exiftool -a -u -G1 -s`.
fn exiftool(path: &Path) -> Vec<(String, String)> {
    let out = Command::new("exiftool")
        .args(["-a", "-u", "-G1", "-s"])
        .arg(path)
        .output()
        .expect("exiftool is in the devcontainer");
    assert!(out.status.success());
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|line| {
            let (group, rest) = line
                .strip_prefix('[')
                .and_then(|l| l.split_once(']'))
                .unwrap_or_else(|| panic!("odd exiftool line: {line}"));
            let key = rest.split_whitespace().next().unwrap_or_default();
            (group.to_string(), key.to_string())
        })
        .collect()
}

#[test]
fn exiftool_shows_only_allowed_keys_on_every_output() {
    let env = Env::new();
    for (input, output) in prepare_all(&env) {
        let keys = exiftool(&output);
        assert!(keys.iter().any(|(_, k)| k == "ImageWidth"), "{input}");
        for (group, key) in keys {
            let ok = ALLOWED
                .iter()
                .any(|(g, ks)| *g == group && ks.contains(&key.as_str()));
            assert!(ok, "{input}: exiftool shows [{group}] {key}");
        }
    }
}

#[test]
fn no_embedded_thumbnail_in_any_output() {
    let env = Env::new();
    for (input, output) in prepare_all(&env) {
        let bytes = fs::read(&output).unwrap();
        if bytes.starts_with(&[0xff, 0xd8]) {
            // In JPEG data 0xff is always followed by 0x00 or a marker, so a
            // second SOI can only be an embedded image.
            let soi = bytes.windows(2).filter(|w| w == &[0xff, 0xd8]).count();
            assert_eq!(soi, 1, "{input}: embedded JPEG");
        }
        let thumb = exiftool(&output)
            .into_iter()
            .find(|(_, k)| k.contains("Thumbnail") || k.contains("Preview"));
        assert_eq!(thumb, None, "{input}");
    }
}

/// The upright reference: red, lime / blue, white quadrants, 96x64.
fn assert_upright(name: &str, path: &Path) {
    let d = GlycinDecoder.decode(path, &Limits::default()).unwrap();
    assert_eq!(d.pixels.dimensions(), (96, 64), "{name}");
    for ((x, y), want) in [
        ((24, 16), [255, 0, 0]),
        ((72, 16), [0, 255, 0]),
        ((24, 48), [0, 0, 255]),
        ((72, 48), [255, 255, 255]),
    ] {
        let got = d.pixels.get_pixel(x, y).0;
        for c in 0..3 {
            assert!(
                got[c].abs_diff(want[c]) <= 40,
                "{name}: pixel ({x},{y}) is {got:?}, want {want:?}"
            );
        }
    }
}

#[test]
fn orientation_6_comes_out_upright() {
    let env = Env::new();
    env.cmd()
        .arg(fixtures().join("orient-6.jpg"))
        .args(["--max-size", "2MB", "-o", "out.jpg"])
        .assert()
        .success();
    let out = env.work.path().join("out.jpg");
    assert_upright("orient-6.jpg", &out);
    let keys = exiftool(&out);
    assert!(!keys.iter().any(|(_, k)| k == "Orientation"));
}

#[test]
fn every_orientation_comes_out_upright() {
    let env = Env::new();
    for (input, output) in prepare_all(&env) {
        if input.starts_with("orient-") {
            assert_upright(input, &output);
        }
    }
}

#[test]
fn heic_becomes_a_clean_upright_jpeg() {
    let env = Env::new();
    for (input, size) in [
        ("heic-gps.heic", (640, 480)),
        ("heic-rot90.heic", (480, 640)),
        ("heic-mirror.heic", (640, 480)),
    ] {
        env.cmd()
            .arg(fixtures().join(input))
            .args(["--max-size", "2MB", "-o", "out.jpg"])
            .assert()
            .success();
        let out = env.work.path().join("out.jpg");
        let bytes = fs::read(&out).unwrap();
        assert!(bytes.starts_with(&[0xff, 0xd8]), "{input}");
        verify(&bytes, None).unwrap_or_else(|e| panic!("{input}: {e}"));
        let keys = exiftool(&out);
        for (group, key) in &keys {
            for bad in ["GPS", "Make", "Model", "XMP", "Orientation"] {
                assert!(
                    !group.contains(bad) && !key.contains(bad),
                    "{input}: [{group}] {key}"
                );
            }
        }
        let d = GlycinDecoder.decode(&out, &Limits::default()).unwrap();
        assert_eq!(d.pixels.dimensions(), size, "{input}");
        // Where the red quadrant is. rot90: top-right (irot applied once,
        // EXIF 6 ignored). mirror: bottom-left as libheif shows it (imir
        // applied once, EXIF 2 ignored).
        let quadrants: &[((u32, u32), bool)] = match input {
            "heic-rot90.heic" => &[((360, 160), true), ((120, 160), false), ((360, 480), false)],
            "heic-mirror.heic" => &[
                ((160, 120), false),
                ((480, 120), false),
                ((160, 360), true),
                ((480, 360), false),
            ],
            _ => &[],
        };
        for &((x, y), red) in quadrants {
            let p = d.pixels.get_pixel(x, y);
            assert_eq!(p[0] > 200 && p[2] < 60, red, "{input}: ({x},{y}) is {p:?}");
        }
        fs::remove_file(&out).unwrap();
    }
}

#[test]
fn header_only_heif_exits_1_and_writes_nothing() {
    let env = Env::new();
    let heif = fixtures().join("heif-header.heic");
    // Detected by content: the same bytes named .jpg behave the same.
    let disguised = env.work.path().join("secret-name.jpg");
    fs::write(&disguised, common::fixture("heif-header.heic")).unwrap();

    for input in [&heif, &disguised] {
        let out = env
            .cmd()
            .arg(input)
            .args(["--max-size", "2MB", "-o", "out.jpg"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(stderr.contains("cannot decode the image"), "{stderr}");
        assert!(!stderr.contains("secret-name") && !stderr.contains("heif-header"));
        assert!(out.stdout.is_empty());
        assert!(!env.work.path().join("out.jpg").exists());
        assert_eq!(env.private_files(), 0);
    }
}
