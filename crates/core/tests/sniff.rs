use std::path::PathBuf;

use safe_upload_core::{Error, Format, sniff};

fn write(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sniff");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

const HEIF_HEADER: &[u8] = b"\0\0\0\x18ftypheic\0\0\0\0mif1heic";
const JPEG_START: &[u8] = &[0xff, 0xd8, 0xff, 0xe0, 0, 16, b'J', b'F', b'I', b'F', 0];

#[test]
fn heif_header_named_jpg_is_refused() {
    let path = write("photo.jpg", HEIF_HEADER);
    assert!(matches!(
        sniff::check_file(&path),
        Err(Error::UnsupportedFormat(Format::Heif))
    ));
}

#[test]
fn jpeg_named_heic_passes() {
    let path = write("photo.heic", JPEG_START);
    assert!(sniff::check_file(&path).is_ok());
}

#[test]
fn missing_file_is_an_io_error() {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sniff/does-not-exist");
    assert!(matches!(sniff::check_file(&path), Err(Error::Io(_))));
}

#[test]
fn message_says_what_format() {
    let msg = Error::UnsupportedFormat(Format::Heif).to_string();
    assert!(msg.contains("HEIC/HEIF"), "{msg}");
}
