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
use image::RgbaImage;
use safe_upload_app::item::ImageItem;
use safe_upload_app::save::SaveError;
use safe_upload_app::window::Window;
use safe_upload_app::{config, window};
use safe_upload_core::edit::{self, Rect};
use safe_upload_core::{Limits, Options, pipeline};
use safe_upload_editor_widget::{RedactionEditor, Tool, preview};

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

const KEYS: &[&str] = &["max-size", "max-long-edge", "output-format", "keep-names"];

/// A window with default settings. The memory backend is shared by the
/// whole process, so each test starts from the defaults.
fn new_window() -> Window {
    let settings = gio::Settings::new(config::APP_ID);
    for key in KEYS {
        settings.reset(key);
    }
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
    assert!(out.bytes.len() <= 5_000_000);

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

fn only_regular_files_are_added() {
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("pipe.jpg");
    let c = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: plain syscall with a valid C string.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    let win = new_window();
    win.add_files(&[
        gio::File::for_path(dir.path()),
        gio::File::for_path(&fifo),
        gio::File::for_path(dir.path().join("missing.jpg")),
    ]);
    assert!(win.model().items().is_empty());
    win.destroy();
}

fn find<T: IsA<gtk::Widget>>(root: &gtk::Widget, pred: &dyn Fn(&T) -> bool) -> Option<T> {
    if let Some(w) = root.downcast_ref::<T>().filter(|w| pred(w)) {
        return Some(w.clone());
    }
    let mut child = root.first_child();
    while let Some(c) = child {
        if let Some(found) = find(&c, pred) {
            return Some(found);
        }
        child = c.next_sibling();
    }
    None
}

fn spin_row(win: &Window, title: &str) -> adw::SpinRow {
    find::<adw::SpinRow>(win.upcast_ref(), &|r| r.title() == title)
        .unwrap_or_else(|| panic!("no spin row {title}"))
}

/// Collects the output sizes each time `item` becomes ready.
fn record_ready(item: &ImageItem) -> Rc<RefCell<Vec<usize>>> {
    let ready = Rc::new(RefCell::new(Vec::new()));
    let r = ready.clone();
    item.connect_busy_notify(move |i| {
        if let Some(out) = i.output() {
            r.borrow_mut().push(out.bytes.len());
        }
    });
    ready
}

fn default_max_size_shown() {
    let win = new_window();
    let row = spin_row(&win, "Max _size (MB)");
    assert_eq!(row.value(), 5.0);
    assert_eq!(win.model().options().target_bytes, 5_000_000);
    row.set_value(2.5);
    assert_eq!(win.settings().uint64("max-size"), 2_500_000);
    assert_eq!(win.model().options().target_bytes, 2_500_000);
    win.settings().set_uint64("max-size", 1_200_000).unwrap();
    assert_eq!(row.value(), 1.2);
    let edge = spin_row(&win, "Max _long edge (px)");
    edge.set_value(800.0);
    assert_eq!(win.model().options().max_long_edge, Some(800));
    edge.set_value(0.0);
    assert_eq!(win.model().options().max_long_edge, None);
    win.destroy();
}

/// Random noise, which compresses badly: `w`x`h` pixels as `name` (the
/// extension picks the format).
fn noise(dir: &Path, w: u32, h: u32, name: &str) -> PathBuf {
    let out = dir.join(name);
    let status = Command::new("magick")
        .args([
            "-size",
            &format!("{w}x{h}"),
            "xc:",
            "+noise",
            "Random",
            "-strip",
        ])
        .arg(&out)
        .status()
        .expect("run magick");
    assert!(status.success(), "magick failed");
    out
}

fn changing_max_size_reprepares() {
    let dir = tempfile::tempdir().unwrap();
    let input = noise(dir.path(), 1600, 1200, "noise.jpg");
    let win = new_window();
    win.add_files(&[gio::File::for_path(input)]);
    let item = win.model().items()[0].clone();
    let ready = record_ready(&item);
    wait_idle(&win);
    let first = ready.borrow()[0];
    let target = first / 2;
    assert!(target >= 100_000, "{first}");
    win.settings()
        .set_uint64("max-size", target as u64)
        .unwrap();
    assert!(item.busy());
    wait_idle(&win);
    let sizes = ready.borrow().clone();
    assert_eq!(sizes.len(), 2, "{sizes:?}");
    assert!(sizes[1] <= target, "{sizes:?} target {target}");
    win.destroy();
}

fn impossible_target_shows_core_message() {
    let dir = tempfile::tempdir().unwrap();
    let noise = noise(dir.path(), 640, 480, "noise.png");
    let win = new_window();
    // The schema allows 0.1 MB to 100 MB; the row shows the whole range.
    assert!(win.settings().set_uint64("max-size", 1000).is_err());
    let row = spin_row(&win, "Max _size (MB)");
    win.settings().set_uint64("max-size", 100_000).unwrap();
    assert_eq!(row.value(), 0.1);
    win.settings().set_uint64("max-size", 100_000_000).unwrap();
    assert_eq!(row.value(), 100.0);
    // Random noise cannot be made smaller than 0.1 MB as a PNG.
    win.settings().set_uint64("max-size", 100_000).unwrap();
    win.settings().set_string("output-format", "png").unwrap();
    win.add_files(&[gio::File::for_path(&noise)]);
    wait_idle(&win);
    let item = &win.model().items()[0];
    assert!(item.failed());
    assert!(item.output().is_none());
    assert!(
        item.status().contains("cannot be made small enough"),
        "{}",
        item.status()
    );
    win.destroy();
}

fn keep_names_and_format_settings() {
    let (_dir, files) = inputs(&["exif-gps-thumb.jpg", "png-text.png"]);
    let win = new_window();
    win.add_files(&files);
    wait_idle(&win);
    let items = win.model().items();
    let out = items[0].output().unwrap();
    assert_eq!(items[0].title(), "image-1.jpg");

    // Names are the app's job: no new prepare.
    win.settings().set_boolean("keep-names", true).unwrap();
    assert!(win.model().is_idle());
    assert!(Rc::ptr_eq(&out, &items[0].output().unwrap()));
    assert_eq!(items[0].title(), "exif-gps-thumb.jpg");
    assert_eq!(items[1].title(), "png-text.jpg");

    win.settings().set_string("output-format", "png").unwrap();
    wait_idle(&win);
    assert_eq!(items[1].title(), "png-text.png");
    assert!(items[1].output().unwrap().bytes.starts_with(b"\x89PNG"));
    win.destroy();
}

fn action_enabled(win: &Window, name: &str) -> bool {
    win.lookup_action(name).unwrap().is_enabled()
}

/// `exiftool` groups that hold metadata. None may show up in a saved file.
fn assert_no_metadata(path: &Path) {
    let out = Command::new("exiftool")
        .args(["-a", "-u", "-G1", "-s"])
        .arg(path)
        .output()
        .expect("exiftool is in the devcontainer");
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    for line in text.lines() {
        let group = line
            .strip_prefix('[')
            .and_then(|l| l.split_once(']'))
            .map(|(g, _)| g)
            .unwrap_or_else(|| panic!("odd exiftool line: {line}"));
        assert!(
            matches!(group, "ExifTool" | "System" | "File" | "JFIF" | "Composite"),
            "saved file shows {line}"
        );
    }
}

fn save_one_writes_the_verified_bytes() {
    let (dir, files) = inputs(&["exif-gps-thumb.jpg"]);
    let before = snapshot(dir.path());
    let win = new_window();
    assert!(!action_enabled(&win, "save"));
    win.add_files(&files);
    wait_idle(&win);
    assert!(action_enabled(&win, "save"));
    assert!(action_enabled(&win, "copy"));
    let item = win.model().items()[0].clone();
    let out = item.output().unwrap();
    assert_eq!(item.title(), "image.jpg");

    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join(item.title());
    let ctx = glib::MainContext::default();
    ctx.block_on(win.save_to_file(path.clone())).unwrap();
    assert_eq!(fs::read(&path).unwrap(), out.bytes);
    assert_no_metadata(&path);

    // Choosing the original in the dialog never replaces it.
    let original = files[0].path().unwrap();
    let err = ctx
        .block_on(win.save_to_file(original.clone()))
        .unwrap_err();
    assert!(matches!(err, SaveError::Original), "{err}");
    assert_eq!(win.last_toast().as_deref(), Some(err.to_string().as_str()));
    let hard = folder.path().join("hard.jpg");
    fs::hard_link(&original, &hard).unwrap();
    let err = ctx.block_on(win.save_to_file(hard)).unwrap_err();
    assert!(matches!(err, SaveError::Original), "{err}");

    // A link is never followed, and the file behind it is left alone.
    let theirs = folder.path().join("theirs.jpg");
    fs::write(&theirs, "theirs").unwrap();
    let link = folder.path().join("link.jpg");
    std::os::unix::fs::symlink(&theirs, &link).unwrap();
    let err = ctx.block_on(win.save_to_file(link.clone())).unwrap_err();
    assert!(matches!(err, SaveError::Link), "{err}");
    assert_eq!(fs::read(&theirs).unwrap(), b"theirs");
    assert!(link.is_symlink());

    // Replacing a file the user chose in the dialog is allowed.
    ctx.block_on(win.save_to_file(theirs.clone())).unwrap();
    assert_eq!(fs::read(&theirs).unwrap(), out.bytes);
    assert_eq!(snapshot(dir.path()), before);
    win.destroy();
}

fn save_batch_to_folder_never_overwrites() {
    let (dir, files) = inputs(&["exif-gps-thumb.jpg", "png-text.png"]);
    let before = snapshot(dir.path());
    let win = new_window();
    win.add_files(&files);
    wait_idle(&win);
    assert!(action_enabled(&win, "save"));
    assert!(!action_enabled(&win, "copy"));
    let outs: Vec<_> = win
        .model()
        .items()
        .iter()
        .map(|i| i.output().unwrap())
        .collect();
    let ctx = glib::MainContext::default();

    let folder = tempfile::tempdir().unwrap();
    assert_eq!(
        ctx.block_on(win.save_to_folder(folder.path().into()))
            .unwrap(),
        2
    );
    let saved = snapshot(folder.path());
    assert_eq!(saved.len(), 2);
    for (i, (path, bytes)) in saved.iter().enumerate() {
        assert_eq!(
            path.file_name().unwrap(),
            format!("image-{}.jpg", i + 1).as_str()
        );
        assert_eq!(*bytes, outs[i].bytes);
        assert_no_metadata(path);
    }
    // Again into the same folder: nothing is written or replaced.
    let err = ctx
        .block_on(win.save_to_folder(folder.path().into()))
        .unwrap_err();
    assert_eq!(err.to_string(), "image-1.jpg already exists in that folder");
    assert_eq!(snapshot(folder.path()), saved);

    // One name taken: the other file is not written either.
    let other = tempfile::tempdir().unwrap();
    fs::write(other.path().join("image-2.jpg"), "mine").unwrap();
    let err = ctx
        .block_on(win.save_to_folder(other.path().into()))
        .unwrap_err();
    assert!(
        matches!(err, SaveError::Exists(ref n) if n == "image-2.jpg"),
        "{err}"
    );
    let left = snapshot(other.path());
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].1, b"mine");

    // A failed image blocks Save.
    let (_d, txt) = inputs(&["notes.txt"]);
    win.add_files(&txt);
    wait_idle(&win);
    assert!(!action_enabled(&win, "save"));
    let err = ctx
        .block_on(win.save_to_folder(other.path().into()))
        .unwrap_err();
    assert!(matches!(err, SaveError::NotReady), "{err}");
    assert_eq!(
        win.last_toast().as_deref(),
        Some("Not every image is ready")
    );
    let err = ctx
        .block_on(win.save_to_file(other.path().join("x.jpg")))
        .unwrap_err();
    assert!(matches!(err, SaveError::NotReady), "{err}");
    assert_eq!(
        win.last_toast().as_deref(),
        Some("Not every image is ready")
    );
    assert_eq!(snapshot(other.path()).len(), 1);
    assert_eq!(snapshot(dir.path()), before);
    win.destroy();
}

fn copy_puts_the_verified_bytes_on_the_clipboard() {
    let (_dir, files) = inputs(&["exif-gps-thumb.jpg", "png-text.png"]);
    let win = new_window();
    win.settings().set_string("output-format", "png").unwrap();
    win.add_files(&files);
    wait_idle(&win);
    assert!(!action_enabled(&win, "copy"));
    assert!(win.copy().is_err());

    win.model().remove(&win.model().items()[0]);
    assert!(action_enabled(&win, "copy"));
    let out = win.model().items()[0].output().unwrap();
    win.copy().unwrap();

    let clipboard = win.clipboard();
    let formats = clipboard.formats();
    assert_eq!(formats.mime_types().as_slice(), ["image/png"]);
    // GDK adds texture types for readers in this process; what we offer
    // (and what other apps see) is only the bytes under their MIME type.
    let offered = clipboard.content().unwrap().formats();
    assert!(offered.types().is_empty(), "{}", offered.to_str());
    assert_eq!(offered.mime_types(), formats.mime_types());
    let (bytes, mime) =
        glib::MainContext::default().block_on(read_clipboard(&clipboard, "image/png"));
    assert_eq!(mime, "image/png");
    assert_eq!(bytes, out.bytes);
    win.destroy();
}

fn copy_jpeg_keeps_its_mime_type() {
    let (_dir, files) = inputs(&["exif-gps-thumb.jpg"]);
    let win = new_window();
    win.add_files(&files);
    wait_idle(&win);
    let out = win.model().items()[0].output().unwrap();
    win.copy().unwrap();
    assert_eq!(
        win.last_toast().as_deref(),
        Some("Copied. Keep Safe Upload open until you paste.")
    );
    let clipboard = win.clipboard();
    let formats = clipboard.formats();
    assert_eq!(formats.mime_types().as_slice(), ["image/jpeg"]);
    // GDK adds texture types for readers in this process; what we offer
    // (and what other apps see) is only the bytes under their MIME type.
    let offered = clipboard.content().unwrap().formats();
    assert!(offered.types().is_empty(), "{}", offered.to_str());
    assert_eq!(offered.mime_types(), formats.mime_types());
    let (bytes, mime) =
        glib::MainContext::default().block_on(read_clipboard(&clipboard, "image/jpeg"));
    assert_eq!(mime, "image/jpeg");
    assert_eq!(bytes, out.bytes);
    win.destroy();
}

async fn read_clipboard(clipboard: &gtk::gdk::Clipboard, mime: &str) -> (Vec<u8>, String) {
    let (stream, mime) = clipboard
        .read_future(&[mime], glib::Priority::DEFAULT)
        .await
        .unwrap();
    let mut all = Vec::new();
    loop {
        let chunk = stream
            .read_bytes_future(64 * 1024, glib::Priority::DEFAULT)
            .await
            .unwrap();
        if chunk.is_empty() {
            break;
        }
        all.extend_from_slice(&chunk);
    }
    (all, mime.to_string())
}

/// Decodes output bytes the way core reads any input, on a thread.
fn decode(bytes: &[u8], ext: &str) -> RgbaImage {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(format!("out.{ext}"));
    fs::write(&path, bytes).unwrap();
    decode_path(path)
}

fn decode_path(path: PathBuf) -> RgbaImage {
    let worker = thread::spawn(move || pipeline::preview(&path, &Limits::default(), u32::MAX));
    wait_until("decode", PREPARE_TIMEOUT, || worker.is_finished());
    worker.join().unwrap().unwrap().pixels
}

fn focus(win: &Window) -> gtk::Widget {
    GtkWindowExt::focus(win).expect("a focused widget")
}

fn edit_button(win: &Window) -> gtk::Button {
    find::<gtk::Button>(win.upcast_ref(), &|b| {
        b.tooltip_text().as_deref() == Some("Edit")
    })
    .expect("edit button")
}

/// Clicks the row's Edit button and waits for the editor to be laid out.
fn open_editor(win: &Window) -> (adw::Dialog, RedactionEditor) {
    edit_button(win).emit_clicked();
    let w = win.clone();
    wait_until("editor dialog", PREPARE_TIMEOUT, || {
        w.visible_dialog().is_some()
    });
    let dialog = win.visible_dialog().unwrap();
    let ed = find::<RedactionEditor>(dialog.upcast_ref(), &|_| true).expect("editor");
    let canvas = ed.canvas();
    wait_until("editor layout", PREPARE_TIMEOUT, || canvas.width() > 0);
    (dialog, ed)
}

fn drag(ed: &RedactionEditor, a: (f64, f64), b: (f64, f64)) {
    let v = ed.view();
    let (a, b) = (v.to_widget(a), v.to_widget(b));
    ed.drag_begin(a.0, a.1);
    ed.drag_update(b.0 - a.0, b.1 - a.1);
    ed.drag_end(b.0 - a.0, b.1 - a.1);
}

fn finish(win: &Window, dialog: &adw::Dialog, action: &str) {
    dialog.activate_action(action, None).unwrap();
    let w = win.clone();
    wait_until("dialog to close", PREPARE_TIMEOUT, || {
        w.visible_dialog().is_none()
    });
    wait_idle(win);
}

fn near_black(p: &image::Rgba<u8>) -> bool {
    p.0[..3].iter().all(|&c| c < 40)
}

fn editor_box_is_solid_where_drawn_on_rotated_input() {
    let (dir, files) = inputs(&["orient-6.jpg"]);
    let before = snapshot(dir.path());
    let win = new_window();
    win.add_files(&files);
    wait_idle(&win);
    let item = win.model().items()[0].clone();
    let first = item.output().unwrap();

    let (dialog, ed) = open_editor(&win);
    // The editor works on the upright image, 96x64; red is top left.
    assert_eq!(ed.model().unwrap().size(), (96, 64));
    let (w, canvas) = (win.clone(), ed.canvas());
    wait_until("canvas focus", Duration::from_secs(5), || {
        GtkWindowExt::focus(&w).is_some_and(|f| f == canvas)
    });
    drag(&ed, (2.0, 2.0), (30.0, 20.0));
    assert_eq!(ed.to_options().1.len(), 1);
    // Nothing changes until Done.
    assert!(Rc::ptr_eq(&first, &item.output().unwrap()));
    finish(&win, &dialog, "dialog.done");
    assert_eq!(focus(&win), edit_button(&win).upcast::<gtk::Widget>());

    assert!(!item.failed(), "{}", item.status());
    let out = item.output().unwrap();
    let img = decode(&out.bytes, &out.extension);
    assert_eq!(img.dimensions(), (96, 64));
    for (x, y) in [(6, 6), (16, 10), (26, 16)] {
        assert!(
            near_black(img.get_pixel(x, y)),
            "({x}, {y}) {:?}",
            img.get_pixel(x, y)
        );
    }
    for (x, y) in [(40, 10), (14, 27)] {
        let red = img.get_pixel(x, y);
        assert!(red.0[0] > 150 && red.0[1] < 100, "({x}, {y}) {red:?}");
    }
    for (x, y) in [(72, 16), (20, 48), (72, 48)] {
        assert!(!near_black(img.get_pixel(x, y)), "({x}, {y})");
    }

    // Undo in a reopened editor, then Done: the box is gone.
    let (dialog, ed) = open_editor(&win);
    assert_eq!(ed.to_options().1.len(), 1);
    ed.activate_action("editor.undo", None).unwrap();
    finish(&win, &dialog, "dialog.done");
    assert!(item.edit_options().1.is_empty());
    let out = item.output().unwrap();
    let img = decode(&out.bytes, &out.extension);
    let red = img.get_pixel(16, 10);
    assert!(red.0[0] > 150 && red.0[1] < 100, "{red:?}");

    // The editor wrote nothing next to the input or in the private folder.
    assert_eq!(snapshot(dir.path()), before);
    assert_eq!(private_folders(), 0);
    win.destroy();
}

fn editor_closes_only_after_the_gesture_and_cancel_discards_edits() {
    let (_dir, files) = inputs(&["orient-6.jpg"]);
    let win = new_window();
    win.add_files(&files);
    wait_idle(&win);
    let item = win.model().items()[0].clone();
    let out = item.output().unwrap();

    let (dialog, ed) = open_editor(&win);
    let v = ed.view();
    let a = v.to_widget((10.0, 10.0));
    ed.drag_begin(a.0, a.1);
    ed.drag_update(40.0, 40.0);
    // What Escape does: close asks first, and a press is in progress.
    dialog.close();
    assert!(win.visible_dialog().is_some());
    ed.drag_end(40.0, 40.0);
    assert_eq!(ed.to_options().1.len(), 1);
    dialog.close();
    let w = win.clone();
    wait_until("dialog to close", PREPARE_TIMEOUT, || {
        w.visible_dialog().is_none()
    });
    // Closing is Cancel: no edits stored, no new prepare.
    assert!(item.edits().is_none());
    assert!(win.model().is_idle());
    assert!(Rc::ptr_eq(&out, &item.output().unwrap()));
    assert_eq!(focus(&win), edit_button(&win).upcast::<gtk::Widget>());

    let (dialog, _) = open_editor(&win);
    finish(&win, &dialog, "dialog.cancel");
    assert!(item.edits().is_none());
    win.destroy();
}

/// Lengths of the runs of equal pixels on row `y` from `x0` to `x1`; the
/// most common one.
fn block_on_row(img: &RgbaImage, y: u32, x0: u32, x1: u32) -> u32 {
    let mut runs = std::collections::HashMap::<u32, u32>::new();
    let mut start = x0;
    for x in x0 + 1..=x1 {
        if x == x1 || img.get_pixel(x, y) != img.get_pixel(start, y) {
            if x < x1 {
                *runs.entry(x - start).or_default() += 1;
            }
            start = x;
        }
    }
    runs.into_iter().max_by_key(|&(_, n)| n).unwrap().0
}

fn pixelate_output_is_as_coarse_as_the_preview() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("cells.jpg");
    // 30 px cells of random colour, so block averages differ.
    let status = Command::new("magick")
        .args([
            "-size", "100x67", "xc:", "+noise", "Random", "-scale", "3000%",
        ])
        .args(["-strip", "-quality", "95"])
        .arg(&input)
        .status()
        .expect("run magick");
    assert!(status.success());
    let win = new_window();
    // PNG keeps the blocks exact; the limit is high enough for lossless.
    win.settings().set_uint64("max-size", 100_000_000).unwrap();
    win.settings().set_string("output-format", "png").unwrap();
    win.add_files(&[gio::File::for_path(&input)]);
    wait_idle(&win);
    let item = win.model().items()[0].clone();

    let (dialog, ed) = open_editor(&win);
    let full = ed.model().unwrap().size();
    assert_eq!(full, (3000, 2010));
    ed.set_tool(Tool::Pixelate);
    drag(&ed, (500.0, 500.0), (1500.0, 1500.0));
    let model = ed.model().unwrap();
    let rect: Rect = model.areas()[0].rect;
    finish(&win, &dialog, "dialog.done");
    assert!(!item.failed(), "{}", item.status());

    let out = item.output().unwrap();
    assert_eq!(out.extension, "png");
    let img = decode(&out.bytes, &out.extension);
    assert_eq!(img.dimensions(), full);
    let out_block = block_on_row(&img, rect.y + 3, rect.x, rect.x + rect.width);
    assert_eq!(out_block, edit::block_size(rect));

    // What the editor shows: the same preview, rendered by the widget.
    let shown = {
        let path = input.clone();
        let worker = thread::spawn(move || {
            pipeline::preview(
                &path,
                &Limits::default(),
                safe_upload_app::edit_dialog::PREVIEW_EDGE,
            )
        });
        wait_until("preview", PREPARE_TIMEOUT, || worker.is_finished());
        worker.join().unwrap().unwrap().pixels
    };
    let size = shown.dimensions();
    assert!(size.0 < full.0);
    let shown = preview::render(&shown, &model);
    let pr = preview::to_preview(rect, full, size);
    let shown_block = block_on_row(&shown, pr.y + 1, pr.x, pr.x + pr.width);
    let scaled = f64::from(shown_block) * f64::from(full.0) / f64::from(size.0);
    assert!(
        (scaled - f64::from(out_block)).abs() <= f64::from(full.0) / f64::from(size.0),
        "preview block {shown_block} ({scaled:.1} full px), output block {out_block}"
    );
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
    env.compile_schemas(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data"));
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
        ("only_regular_files_are_added", only_regular_files_are_added),
        ("default_max_size_shown", default_max_size_shown),
        ("changing_max_size_reprepares", changing_max_size_reprepares),
        (
            "impossible_target_shows_core_message",
            impossible_target_shows_core_message,
        ),
        (
            "keep_names_and_format_settings",
            keep_names_and_format_settings,
        ),
        (
            "save_one_writes_the_verified_bytes",
            save_one_writes_the_verified_bytes,
        ),
        (
            "save_batch_to_folder_never_overwrites",
            save_batch_to_folder_never_overwrites,
        ),
        (
            "copy_puts_the_verified_bytes_on_the_clipboard",
            copy_puts_the_verified_bytes_on_the_clipboard,
        ),
        (
            "copy_jpeg_keeps_its_mime_type",
            copy_jpeg_keeps_its_mime_type,
        ),
        (
            "editor_box_is_solid_where_drawn_on_rotated_input",
            editor_box_is_solid_where_drawn_on_rotated_input,
        ),
        (
            "editor_closes_only_after_the_gesture_and_cancel_discards_edits",
            editor_closes_only_after_the_gesture_and_cancel_discards_edits,
        ),
        (
            "pixelate_output_is_as_coarse_as_the_preview",
            pixelate_output_is_as_coarse_as_the_preview,
        ),
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
