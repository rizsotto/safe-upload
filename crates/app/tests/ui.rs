//! Headless UI tests. Run under a private weston, D-Bus and home (see
//! `headless.rs`); `cargo test` needs no extra setup.

#[path = "../../editor-widget/tests/common/headless.rs"]
mod headless;

use std::cell::OnceCell;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, ExitCode};
use std::thread;
use std::time::Duration;

use adw::prelude::*;
use gtk::gio;
use safe_upload_app::{config, window};

const WATCHDOG: Duration = Duration::from_secs(120);

thread_local! {
    static APP: OnceCell<adw::Application> = const { OnceCell::new() };
}

fn app() -> adw::Application {
    APP.with(|a| a.get().expect("app registered in main").clone())
}

fn window_presents_and_draws() {
    let win = window::build(&app());
    win.present();
    gtk::test_widget_wait_for_draw(&win);
    assert!(win.is_mapped());
    assert!(win.width() > 0 && win.height() > 0);
    assert_eq!(
        win.application().unwrap().application_id().unwrap(),
        config::APP_ID
    );
    win.destroy();
}

fn missing_weston_fails_with_clear_message() {
    let empty = std::env::temp_dir().join("safe-upload-no-such-dir");
    let err = headless::require_program("weston", Some(empty.as_os_str())).unwrap_err();
    assert!(err.contains("`weston`"), "{err}");
    assert!(err.contains("Containerfile"), "{err}");
    assert!(headless::require_program("weston", None).is_err());
    assert!(headless::require_program("weston", Some(OsStr::new(""))).is_err());
}

fn require_program_needs_the_exec_bit() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("weston");
    fs::write(&file, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(headless::require_program("weston", Some(dir.path().as_os_str())).is_err());
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        headless::require_program("weston", Some(dir.path().as_os_str())).unwrap(),
        file
    );
}

fn abort_cleanup_kills_children_and_removes_root() {
    let root = tempfile::tempdir().unwrap().keep();
    fs::write(root.join("weston.log"), "").unwrap();
    let mut child = Command::new("sleep").arg("60").spawn().unwrap();
    headless::abort_cleanup(&[child.id()], &root);
    assert_eq!(child.wait().unwrap().signal(), Some(libc::SIGKILL));
    assert!(!root.exists());
}

fn children_die_with_their_parent() {
    let mut child = thread::spawn(|| {
        let mut cmd = Command::new("sleep");
        cmd.arg("60");
        headless::die_with_parent(&mut cmd);
        cmd.spawn().unwrap()
    })
    .join()
    .unwrap();
    assert_eq!(child.wait().unwrap().signal(), Some(libc::SIGKILL));
}

fn main() -> ExitCode {
    let env = headless::Headless::start();
    env.watchdog(WATCHDOG);

    adw::init().expect("GTK init under headless weston");
    let app = safe_upload_app::application();
    app.register(None::<&gio::Cancellable>)
        .expect("register app on the private bus");
    APP.with(|a| a.set(app)).expect("set app once");

    let code = headless::run(&[
        ("window_presents_and_draws", window_presents_and_draws),
        (
            "missing_weston_fails_with_clear_message",
            missing_weston_fails_with_clear_message,
        ),
        (
            "require_program_needs_the_exec_bit",
            require_program_needs_the_exec_bit,
        ),
        (
            "abort_cleanup_kills_children_and_removes_root",
            abort_cleanup_kills_children_and_removes_root,
        ),
        (
            "children_die_with_their_parent",
            children_die_with_their_parent,
        ),
    ]);
    drop(env);
    code
}
