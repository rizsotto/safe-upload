//! Shared helpers for the CLI tests.

use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;

use assert_cmd::Command;
use tempfile::TempDir;

/// A fresh fake `$XDG_RUNTIME_DIR` and a work folder.
pub struct Env {
    pub runtime: TempDir,
    pub work: TempDir,
}

impl Env {
    pub fn new() -> Self {
        let runtime = tempfile::tempdir().unwrap();
        fs::set_permissions(runtime.path(), Permissions::from_mode(0o700)).unwrap();
        Self {
            runtime,
            work: tempfile::tempdir().unwrap(),
        }
    }

    pub fn cmd(&self) -> Command {
        let mut c = Command::cargo_bin("safe-upload").unwrap();
        c.env("XDG_RUNTIME_DIR", self.runtime.path())
            .current_dir(self.work.path());
        c
    }

    /// Files in all private folders. A refused input creates no folder.
    pub fn private_files(&self) -> usize {
        let base = self.runtime.path().join("safe-upload");
        let Ok(dirs) = fs::read_dir(base) else {
            return 0;
        };
        dirs.map(|d| fs::read_dir(d.unwrap().path()).unwrap().count())
            .sum()
    }
}
