//! `summary::scan` on real HEIC files from `make.sh`.

mod common;

use safe_upload_core::summary::{Removed, Summary, scan};

#[test]
fn heic_reports_location_and_camera() {
    for name in ["heic-gps.heic", "heic-rot90.heic"] {
        let found = scan(&common::fixture(name)).unwrap_or_else(|| panic!("{name}: not scanned"));
        assert!(found.contains(&Removed::Location), "{name}: {found:?}");
        assert!(found.contains(&Removed::Camera), "{name}: {found:?}");
    }
}

#[test]
fn heic_summary_shows_no_values() {
    let heic = common::fixture("heic-gps.heic");
    let s = Summary {
        input_bytes: heic.len() as u64,
        output_bytes: 1000,
        input_size: (640, 480),
        output_size: (640, 480),
        removed: scan(&heic),
    };
    let text = s.to_string();
    for value in ["FixtureCam", "FixtureModel", "47", "19"] {
        assert!(!text.contains(value), "{text}");
    }
}

#[test]
fn cut_heic_does_not_panic() {
    let heic = common::fixture("heic-gps.heic");
    for n in 0..heic.len() {
        scan(&heic[..n]);
    }
}
