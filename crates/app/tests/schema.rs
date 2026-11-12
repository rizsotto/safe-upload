//! The app exits cleanly when its settings schema is missing.

use std::os::unix::process::ExitStatusExt;
use std::process::Command;

use safe_upload_app::config::APP_ID;
use safe_upload_app::settings::{DEV_SCHEMA_DIR_ENV, MISSING};

#[test]
fn missing_schema_exits_1_with_message() {
    let empty = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_safe-upload-app"))
        .env_remove("GSETTINGS_SCHEMA_DIR")
        .env("XDG_DATA_DIRS", empty.path())
        .env("XDG_DATA_HOME", empty.path())
        .env(DEV_SCHEMA_DIR_ENV, empty.path())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.signal(), None, "stderr: {stderr}");
    assert_eq!(out.status.code(), Some(1), "stderr: {stderr}");
    assert_eq!(stderr, format!("safe-upload-app: {MISSING}: {APP_ID}\n"));
}
