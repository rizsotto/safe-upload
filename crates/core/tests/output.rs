use std::fs::{self, File, Permissions};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use safe_upload_core::Error;
use safe_upload_core::output::{MAX_AGE, PrivateDir, clean_stale, remove_stale};

/// A fresh fake `$XDG_RUNTIME_DIR` for one test.
fn fake_runtime_dir(test: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("output")
        .join(test);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, Permissions::from_mode(0o700)).unwrap();
    dir
}

fn mode(p: &Path) -> u32 {
    fs::symlink_metadata(p).unwrap().permissions().mode() & 0o777
}

fn set_age(p: &Path, age: Duration) {
    File::open(p)
        .unwrap()
        .set_modified(SystemTime::now() - age)
        .unwrap();
}

#[test]
fn folder_and_file_modes() {
    let rt = fake_runtime_dir("modes");
    let dir = PrivateDir::create_in(&rt).unwrap();
    let base = rt.join("safe-upload");

    assert_eq!(dir.path().parent().unwrap(), base);
    assert_eq!(mode(&base), 0o700);
    assert_eq!(mode(dir.path()), 0o700);

    let file = dir.write("image.jpg", b"data").unwrap();
    assert_eq!(file, dir.path().join("image.jpg"));
    assert_eq!(mode(&file), 0o600);
    assert_eq!(fs::read(&file).unwrap(), b"data");
}

#[test]
fn folder_name_is_32_hex_and_random() {
    let rt = fake_runtime_dir("names");
    let a = PrivateDir::create_in(&rt).unwrap();
    let b = PrivateDir::create_in(&rt).unwrap();
    for d in [&a, &b] {
        let name = d.path().file_name().unwrap().to_str().unwrap();
        assert_eq!(name.len(), 32, "{name}");
        assert!(
            name.bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }
    assert_ne!(a.path(), b.path());
}

#[test]
fn existing_base_with_loose_mode_is_tightened() {
    let rt = fake_runtime_dir("loose");
    let base = rt.join("safe-upload");
    fs::create_dir(&base).unwrap();
    fs::set_permissions(&base, Permissions::from_mode(0o755)).unwrap();
    PrivateDir::create_in(&rt).unwrap();
    assert_eq!(mode(&base), 0o700);
}

#[test]
fn symlinked_base_is_refused() {
    let rt = fake_runtime_dir("symlink");
    let elsewhere = rt.join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    symlink(&elsewhere, rt.join("safe-upload")).unwrap();
    assert!(matches!(
        PrivateDir::create_in(&rt),
        Err(Error::UnsafeTempDir)
    ));
    assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0);
}

#[test]
fn clean_stale_checks_the_base_first() {
    let rt = fake_runtime_dir("clean-symlink");
    let elsewhere = rt.join("elsewhere");
    let old = elsewhere.join("0123456789abcdef0123456789abcdef");
    fs::create_dir_all(&old).unwrap();
    set_age(&old, MAX_AGE * 2);
    symlink(&elsewhere, rt.join("safe-upload")).unwrap();
    assert!(matches!(clean_stale(&rt), Err(Error::UnsafeTempDir)));
    assert!(old.exists(), "followed the symlinked base");
}

#[test]
fn clean_stale_removes_old_folders() {
    let rt = fake_runtime_dir("clean");
    let base = rt.join("safe-upload");
    let old = base.join("0123456789abcdef0123456789abcdef");
    let young = base.join("fedcba9876543210fedcba9876543210");
    fs::create_dir_all(&old).unwrap();
    fs::create_dir_all(&young).unwrap();
    set_age(&old, MAX_AGE * 2);
    fs::set_permissions(&base, Permissions::from_mode(0o755)).unwrap();
    assert_eq!(clean_stale(&rt).unwrap(), 1);
    assert!(!old.exists());
    assert!(young.exists());
    assert_eq!(mode(&base), 0o700);
    assert!(matches!(
        clean_stale(&rt.join("nope")),
        Err(Error::NoRuntimeDir)
    ));
}

#[test]
fn missing_runtime_dir_is_an_error() {
    let rt = fake_runtime_dir("missing").join("nope");
    assert!(matches!(
        PrivateDir::create_in(&rt),
        Err(Error::NoRuntimeDir)
    ));
}

#[test]
fn write_never_overwrites_or_escapes() {
    let rt = fake_runtime_dir("write");
    let dir = PrivateDir::create_in(&rt).unwrap();
    dir.write("image.jpg", b"one").unwrap();
    assert!(dir.write("image.jpg", b"two").is_err());
    assert_eq!(fs::read(dir.path().join("image.jpg")).unwrap(), b"one");
    for bad in ["", ".", "..", "../x.jpg", "a/b.jpg"] {
        assert!(dir.write(bad, b"x").is_err(), "{bad:?}");
    }
}

#[test]
fn old_folders_are_removed_and_new_ones_kept() {
    let rt = fake_runtime_dir("stale");
    let first = PrivateDir::create_in(&rt).unwrap();
    first.write("image.jpg", b"x").unwrap();
    let base = rt.join("safe-upload");

    let old = base.join("0123456789abcdef0123456789abcdef");
    fs::create_dir(&old).unwrap();
    fs::write(old.join("image.jpg"), b"old").unwrap();
    set_age(&old, MAX_AGE + Duration::from_secs(60));

    let young = base.join("fedcba9876543210fedcba9876543210");
    fs::create_dir(&young).unwrap();
    set_age(&young, MAX_AGE - Duration::from_secs(60));

    let foreign = base.join("not-ours");
    fs::create_dir(&foreign).unwrap();
    set_age(&foreign, MAX_AGE * 2);

    let second = PrivateDir::create_in(&rt).unwrap();

    assert!(!old.exists(), "old folder kept");
    assert!(young.exists(), "young folder removed");
    assert!(foreign.exists(), "foreign folder removed");
    assert!(first.path().join("image.jpg").exists());
    assert!(second.path().exists());
}

#[test]
fn stale_symlink_is_not_followed() {
    let rt = fake_runtime_dir("stale-link");
    let target = rt.join("target");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("keep"), b"x").unwrap();
    let base = rt.join("base");
    fs::create_dir(&base).unwrap();
    symlink(&target, base.join("0123456789abcdef0123456789abcdef")).unwrap();

    assert_eq!(remove_stale(&base, Duration::ZERO), 0);
    assert!(target.join("keep").exists());
}

#[test]
fn remove_deletes_folder() {
    let rt = fake_runtime_dir("remove");
    let dir = PrivateDir::create_in(&rt).unwrap();
    dir.write("image.jpg", b"x").unwrap();
    let path = dir.path().to_path_buf();
    dir.remove().unwrap();
    assert!(!path.exists());
}
