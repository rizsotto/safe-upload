//! T14: the `safe-upload` command line.

#[path = "../../core/tests/common/mod.rs"]
mod common;
mod env;

use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use common::fixtures;
use env::Env;

fn fixture(name: &str) -> std::path::PathBuf {
    fixtures().join(name)
}

#[test]
fn m1_command_line_works() {
    let env = Env::new();
    let input = common::fixture("exif-gps-thumb.jpg");
    fs::write(env.work.path().join("in.jpg"), &input).unwrap();
    env.cmd()
        .args(["in.jpg", "--max-size", "2MB", "-o", "out.jpg"])
        .assert()
        .success();
    let out = fs::read(env.work.path().join("out.jpg")).unwrap();
    assert!(out.len() < 2_000_000);
    assert!(
        out.len() < input.len(),
        "metadata and the thumbnail are gone"
    );
    assert!(out.starts_with(&[0xff, 0xd8]));
    assert_eq!(
        env.private_files(),
        0,
        "the private folder is removed after -o"
    );
}

#[test]
fn without_o_prints_the_private_path() {
    let env = Env::new();
    let out = env
        .cmd()
        .arg(fixture("p3.jpg"))
        .args(["--max-size", "500KB"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let path = String::from_utf8(out.stdout).unwrap();
    let path = Path::new(path.trim());
    assert!(path.starts_with(env.runtime.path().join("safe-upload")));
    assert_eq!(path.file_name().unwrap(), "image.jpg");
    assert!(fs::metadata(path).unwrap().len() <= 500_000);
}

#[test]
fn batch_goes_to_a_folder_with_neutral_names() {
    let env = Env::new();
    let dest = env.work.path().join("dest");
    fs::create_dir(&dest).unwrap();
    env.cmd()
        .arg(fixture("exif-gps-thumb.jpg"))
        .arg(fixture("alpha.png"))
        .args(["--max-size", "1MB", "--max-edge", "320", "-o"])
        .arg(&dest)
        .assert()
        .success();
    assert!(dest.join("image-1.jpg").is_file());
    assert!(dest.join("image-2.png").is_file());
}

#[test]
fn format_flag_is_used() {
    let env = Env::new();
    env.cmd()
        .arg(fixture("p3.jpg"))
        .args(["--max-size", "1MB", "--format", "png", "-o", "out.png"])
        .assert()
        .success();
    let out = fs::read(env.work.path().join("out.png")).unwrap();
    assert!(out.starts_with(b"\x89PNG"));
}

#[test]
fn usage_errors_exit_2() {
    let env = Env::new();
    let input = fixture("p3.jpg");
    for args in [
        vec!["--max-size", "2MB"],
        vec!["IN", "--max-size", "2XB"],
        vec!["IN"],
        vec!["IN", "--max-size", "2MB", "--format", "gif"],
        vec!["IN", "--max-size", "2MB", "--max-edge", "0"],
        vec!["IN", "IN", "--max-size", "2MB", "-o", "not-a-folder.jpg"],
    ] {
        let args: Vec<_> = args
            .iter()
            .map(|a| {
                if *a == "IN" {
                    input.as_os_str()
                } else {
                    a.as_ref()
                }
            })
            .collect();
        env.cmd().args(&args).assert().code(2);
    }
    assert!(!env.work.path().join("not-a-folder.jpg").exists());
}

#[test]
fn other_errors_exit_1_and_write_nothing() {
    let env = Env::new();
    env.cmd()
        .arg(fixture("p3.jpg"))
        .arg(env.work.path().join("missing.jpg"))
        .args(["--max-size", "1MB", "-o", "."])
        .assert()
        .code(1);
    env.cmd()
        .arg(fixture("exif-gps-thumb.jpg"))
        .arg(fixture("truncated.jpg"))
        .args(["--max-size", "1MB", "-o", "."])
        .assert()
        .code(1);
    env.cmd()
        .arg(fixture("exif-gps-thumb.jpg"))
        .args(["--max-size", "1KB", "-o", "small.jpg"])
        .assert()
        .code(1);
    assert_eq!(fs::read_dir(env.work.path()).unwrap().count(), 0);
    assert_eq!(env.private_files(), 0);
}

#[test]
fn never_overwrites_an_input() {
    let env = Env::new();
    let input = env.work.path().join("image.jpg");
    fs::copy(fixture("exif-gps-thumb.jpg"), &input).unwrap();
    let original = fs::read(&input).unwrap();
    env.cmd()
        .args(["image.jpg", "--max-size", "2MB", "-o", "image.jpg"])
        .assert()
        .code(1);
    env.cmd()
        .args(["image.jpg", "--max-size", "2MB", "-o", "."])
        .assert()
        .code(1);
    assert_eq!(fs::read(&input).unwrap(), original);
}

#[test]
fn errors_do_not_show_paths() {
    let env = Env::new();
    let out = env
        .cmd()
        .arg(env.work.path().join("secret-name.jpg"))
        .args(["--max-size", "1MB"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("input 1"), "{stderr}");
    assert!(!stderr.contains("secret-name"), "{stderr}");
}

#[test]
fn prints_a_summary_on_stderr() {
    let env = Env::new();
    let out = env
        .cmd()
        .arg(fixture("exif-gps-thumb.jpg"))
        .args(["--max-size", "2MB", "-o", "out.jpg"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.starts_with("Image 1: "), "{stderr}");
    assert!(stderr.contains("640x480 → 640x480"), "{stderr}");
    assert!(stderr.contains("Location"), "{stderr}");
    assert!(
        !stderr.contains("out.jpg") && !stderr.contains("Budapest"),
        "{stderr}"
    );
}

/// Batch with a destination that cannot be written: nothing is left at
/// `-o`, not even the outputs that came before.
#[test]
fn failed_batch_leaves_nothing_at_o() {
    let batch = |env: &Env, dest: &Path| {
        env.cmd()
            .arg(fixture("exif-gps-thumb.jpg"))
            .arg(fixture("alpha.png"))
            .args(["--max-size", "1MB", "-o"])
            .arg(dest)
            .assert()
            .code(1);
    };
    let names = |dest: &Path| -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dest)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        v.sort();
        v
    };

    // The second output's name is taken by a folder.
    let env = Env::new();
    let dest = env.work.path().join("dest");
    fs::create_dir_all(dest.join("image-2.png")).unwrap();
    batch(&env, &dest);
    assert_eq!(names(&dest), ["image-2.png"]);
    assert_eq!(env.private_files(), 0);

    // The destination folder is read-only.
    let env = Env::new();
    let dest = env.work.path().join("dest");
    fs::create_dir(&dest).unwrap();
    fs::set_permissions(&dest, Permissions::from_mode(0o500)).unwrap();
    batch(&env, &dest);
    fs::set_permissions(&dest, Permissions::from_mode(0o700)).unwrap();
    assert!(names(&dest).is_empty());
}

#[test]
fn output_file_has_mode_0600() {
    let env = Env::new();
    env.cmd()
        .arg(fixture("p3.jpg"))
        .args(["--max-size", "1MB", "-o", "out.jpg"])
        .assert()
        .success();
    let mode = fs::metadata(env.work.path().join("out.jpg"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}
