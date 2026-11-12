//! Headless UI tests. Run under a private weston, D-Bus and home (see
//! `headless.rs`); `cargo test` needs no extra setup.

#[path = "../../editor-widget/tests/common/headless.rs"]
mod headless;

use std::cell::OnceCell;
use std::ffi::OsStr;
use std::process::ExitCode;
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

fn main() -> ExitCode {
    let env = headless::Headless::start();
    thread::spawn(|| {
        thread::sleep(WATCHDOG);
        eprintln!("UI tests did not finish in {WATCHDOG:?}");
        std::process::exit(1);
    });

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
    ]);
    drop(env);
    code
}
