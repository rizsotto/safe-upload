//! T05 spike test: glycin gives us the ICC profile and the EXIF orientation,
//! and decoding runs inside the bwrap sandbox. With `SandboxSelector::Auto`
//! glycin silently drops the sandbox when bwrap is blocked, so we force
//! `Bwrap`: loading then fails instead (S-4, NFR-2).

mod common;

use common::fixture;
use futures_lite::future::block_on;
use glycin::{ColorState, Image, Loader, SandboxMechanism, SandboxSelector};

fn load(bytes: Vec<u8>, apply_orientation: bool) -> Image {
    let mut loader = Loader::new_vec(bytes);
    loader
        .sandbox_selector(SandboxSelector::Bwrap)
        .apply_transformations(apply_orientation)
        .color_convert_icc_srgb(false);
    let image = block_on(loader.load()).expect("glycin failed: is bwrap blocked?");
    assert_eq!(image.active_sandbox_mechanism(), SandboxMechanism::Bwrap);
    image
}

#[test]
fn reads_icc_profile_in_sandbox() {
    let mut image = load(fixture("p3.jpg"), false);
    let frame = block_on(image.next_frame()).unwrap();
    let want = std::fs::read(common::fixtures().join("display-p3.icc")).unwrap();
    assert_eq!(frame.details().color_icc_profile(), Some(want.as_slice()));
    assert!(matches!(frame.color_state(), ColorState::IccProfile(p) if *p == want));
}

#[test]
fn reads_orientation_in_sandbox() {
    // orient-6.jpg is stored 64x96. EXIF 6 means "rotate 90 degrees
    // clockwise to display", which glycin calls Rotation270 (counter-clockwise).
    let mut image = load(fixture("orient-6.jpg"), false);
    // The type is gufo_common's Orientation, which glycin does not re-export.
    // Its discriminants are the EXIF values.
    let orientation = image.transformation_orientation();
    assert_eq!(format!("{orientation:?}"), "Rotation270");
    assert_eq!(orientation as u16, 6);
    let frame = block_on(image.next_frame()).unwrap();
    assert_eq!((frame.width(), frame.height()), (64, 96));
}

#[test]
fn applies_orientation_when_asked() {
    // Upright, the reference image is 96x64 with red at the top left.
    let mut image = load(fixture("orient-6.jpg"), true);
    let frame = block_on(image.next_frame()).unwrap();
    assert_eq!((frame.width(), frame.height()), (96, 64));
    let px = &frame.buf_slice()[..3];
    assert!(
        px[0] > 200 && px[1] < 50 && px[2] < 50,
        "top left is {px:?}"
    );
}

#[test]
fn image_without_profile_is_srgb() {
    let mut image = load(fixture("orient-1.jpg"), false);
    let frame = block_on(image.next_frame()).unwrap();
    assert!(frame.details().color_icc_profile().is_none());
    assert!(matches!(frame.color_state(), ColorState::Srgb));
}
