//! Headless UI tests. Run under a private weston, D-Bus and home (see
//! `headless.rs`); `cargo test` needs no extra setup.

#[path = "../../editor-widget/tests/common/headless.rs"]
mod headless;

use std::cell::{Cell, OnceCell, RefCell};
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::rc::Rc;
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use adw::prelude::*;
use gtk::{gio, glib};
use safe_upload_app::item::ImageItem;
use safe_upload_app::window::Window;
use safe_upload_app::{config, window};
use safe_upload_core::Options;

const WATCHDOG: Duration = Duration::from_secs(300);
const PREPARE_TIMEOUT: Duration = Duration::from_secs(60);

/// S-2 test folders, made in `main` before the app starts.
const STALE: &str = "0123456789abcdef0123456789abcdef";
const FRESH: &str = "fedcba9876543210fedcba9876543210";
const NOT_OURS: &str = "not-ours";

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

fn fixtures() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fixtures-app-ui");
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/make.sh");
        let status = Command::new(script)
            .arg(&out)
            .status()
            .expect("run tests/fixtures/make.sh");
        assert!(status.success(), "tests/fixtures/make.sh failed");
        out
    })
}

/// Copies fixtures into a fresh folder, so a test can check that nothing
/// is written next to the originals (S-1).
fn inputs(names: &[&str]) -> (tempfile::TempDir, Vec<gio::File>) {
    let dir = tempfile::tempdir().unwrap();
    let files = names
        .iter()
        .map(|n| {
            let dst = dir.path().join(n);
            if n.ends_with(".txt") {
                fs::write(&dst, "just some text, not an image\n").unwrap();
            } else {
                fs::copy(fixtures().join(n), &dst).unwrap();
            }
            gio::File::for_path(dst)
        })
        .collect();
    (dir, files)
}

fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut v: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let p = e.unwrap().path();
            let bytes = fs::read(&p).unwrap();
            (p, bytes)
        })
        .collect();
    v.sort();
    v
}

/// Runs the main loop until `done` is true.
fn wait_until(what: &str, timeout: Duration, mut done: impl FnMut() -> bool) {
    let ctx = glib::MainContext::default();
    let wake = glib::timeout_add_local(Duration::from_millis(20), || glib::ControlFlow::Continue);
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < timeout, "timed out waiting for {what}");
        ctx.iteration(true);
    }
    wake.remove();
}

fn new_window() -> Window {
    let win = window::build(&app());
    win.present();
    win
}

fn wait_idle(win: &Window) {
    let model = win.model().clone();
    wait_until("prepare to finish", PREPARE_TIMEOUT, || model.is_idle());
}

fn private_folders() -> usize {
    let base = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap()).join("safe-upload");
    fs::read_dir(base).map_or(0, |d| {
        d.flatten()
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|n| n.len() == 32 && n != FRESH)
            })
            .count()
    })
}

fn add_files_gives_ready_and_error_rows() {
    let (dir, files) = inputs(&["exif-gps-thumb.jpg", "png-text.png", "notes.txt"]);
    let before = snapshot(dir.path());
    let win = new_window();
    win.add_files(&files);
    let items = win.model().items();
    assert_eq!(items.len(), 3);
    assert!(items.iter().all(ImageItem::busy));
    wait_idle(&win);

    let jpg = &items[0];
    assert!(!jpg.failed(), "{}", jpg.status());
    assert_eq!(jpg.title(), "image-1.jpg");
    assert!(jpg.status().contains("Location"), "{}", jpg.status());
    let out = jpg.output().unwrap();
    assert_eq!(out.extension, "jpg");
    assert!(out.bytes.starts_with(&[0xff, 0xd8]));
    assert!(out.bytes.len() as u64 <= window::DEFAULT_MAX_SIZE);

    let png = &items[1];
    assert!(!png.failed(), "{}", png.status());
    assert_eq!(png.title(), "image-2.jpg");
    assert!(png.output().is_some());

    let txt = &items[2];
    assert!(txt.failed());
    assert!(txt.output().is_none());
    assert_eq!(txt.title(), "image-3");
    assert_eq!(txt.status(), "Not a supported image");

    // S-1: the originals are unchanged and nothing was written next to
    // them; the private folders are gone after each prepare.
    assert_eq!(snapshot(dir.path()), before);
    assert_eq!(private_folders(), 0);

    win.model().remove(txt);
    assert_eq!(win.model().items().len(), 2);
    assert_eq!(items[1].title(), "image-2.jpg");
    win.model().remove(&items[1]);
    assert_eq!(items[0].title(), "image.jpg");
    win.destroy();
}

fn main_loop_runs_during_prepare() {
    let (_dir, files) = inputs(&["exif-gps-thumb.jpg", "png-text.png"]);
    let win = new_window();
    let model = win.model().clone();
    let ticks = Rc::new(Cell::new(0u32));
    let t = ticks.clone();
    let m = model.clone();
    let source = glib::timeout_add_local(Duration::from_millis(5), move || {
        if m.is_working() {
            t.set(t.get() + 1);
        }
        glib::ControlFlow::Continue
    });
    win.add_files(&files);
    wait_idle(&win);
    source.remove();
    assert!(ticks.get() > 0, "no timeout fired while preparing");
    win.destroy();
}

fn stale_results_are_dropped() {
    let (_dir, files) = inputs(&["exif-gps-thumb.jpg"]);
    let win = new_window();
    let model = win.model().clone();
    win.add_files(&files);
    let item = model.items()[0].clone();
    let ready = Rc::new(RefCell::new(Vec::new()));
    let r = ready.clone();
    item.connect_busy_notify(move |i| {
        if let Some(out) = i.output() {
            r.borrow_mut().push(out.summary.output_size);
        }
    });

    let m = model.clone();
    wait_until("prepare to start", PREPARE_TIMEOUT, || m.is_working());
    model.set_options(Options {
        max_long_edge: Some(100),
        ..model.options()
    });
    assert!(item.busy());
    wait_idle(&win);

    // Only the result for the new settings arrived on the item.
    assert_eq!(*ready.borrow(), vec![(100, 75)]);
    win.destroy();
}

fn removed_item_result_is_dropped() {
    let (_dir, files) = inputs(&["exif-gps-thumb.jpg"]);
    let win = new_window();
    let model = win.model().clone();
    win.add_files(&files);
    let item = model.items()[0].clone();
    let m = model.clone();
    wait_until("prepare to start", PREPARE_TIMEOUT, || m.is_working());
    model.remove(&item);
    wait_until("prepare to end", PREPARE_TIMEOUT, || !m.is_working());
    assert!(item.busy());
    assert!(item.output().is_none());
    assert!(model.items().is_empty());
    win.destroy();
}

fn folders_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let win = new_window();
    win.add_files(&[gio::File::for_path(dir.path())]);
    assert!(win.model().items().is_empty());
    win.destroy();
}

fn make_stale_folders() {
    let base = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap()).join("safe-upload");
    let old = SystemTime::now() - Duration::from_secs(2 * 60 * 60);
    for name in [STALE, FRESH, NOT_OURS] {
        let dir = base.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("image.jpg"), "x").unwrap();
        if name != FRESH {
            fs::File::open(&dir).unwrap().set_modified(old).unwrap();
        }
    }
}

fn stale_folder_removed_on_start() {
    let base = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap()).join("safe-upload");
    assert!(!base.join(STALE).exists());
    assert!(base.join(FRESH).exists());
    assert!(base.join(NOT_OURS).exists());
    assert_eq!(safe_upload_app::cleanup::INTERVAL, Duration::from_secs(600));
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

    make_stale_folders();
    adw::init().expect("GTK init under headless weston");
    let app = safe_upload_app::application();
    app.register(None::<&gio::Cancellable>)
        .expect("register app on the private bus");
    APP.with(|a| a.set(app)).expect("set app once");

    let code = headless::run(&[
        ("window_presents_and_draws", window_presents_and_draws),
        (
            "stale_folder_removed_on_start",
            stale_folder_removed_on_start,
        ),
        (
            "add_files_gives_ready_and_error_rows",
            add_files_gives_ready_and_error_rows,
        ),
        (
            "main_loop_runs_during_prepare",
            main_loop_runs_during_prepare,
        ),
        ("stale_results_are_dropped", stale_results_are_dropped),
        (
            "removed_item_result_is_dropped",
            removed_item_result_is_dropped,
        ),
        ("folders_are_skipped", folders_are_skipped),
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
