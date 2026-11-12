mod common;

use image::{Rgba, RgbaImage};
use safe_upload_core::color::{SourceColor, to_srgb};

#[test]
fn fixture_p3_profile_converts_to_srgb() {
    let icc = common::fixture("display-p3.icc");
    let input = RgbaImage::from_fn(2, 1, |x, _| {
        Rgba(if x == 0 {
            [255, 0, 0, 255]
        } else {
            [200, 120, 80, 255]
        })
    });
    let out = to_srgb(input, Some(&SourceColor::Icc(icc))).unwrap();
    for (p, want) in out.pixels().zip([[255, 0, 0], [213, 115, 70]]) {
        for (g, w) in p.0.iter().zip(want) {
            assert!(g.abs_diff(w) <= 2, "got {:?}, want {want:?}", p.0);
        }
    }
}
