//! Shared test helpers. Each test binary builds the fixtures once.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The fixture folder, built by `tests/fixtures/make.sh` on first use.
pub fn fixtures() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("fixtures-{}", env!("CARGO_CRATE_NAME")));
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/make.sh");
        let status = Command::new(script)
            .arg(&out)
            .status()
            .expect("cannot run tests/fixtures/make.sh");
        assert!(status.success(), "tests/fixtures/make.sh failed");
        out
    })
}

pub fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(name)).unwrap()
}
