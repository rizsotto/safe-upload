//! The image list and its prepare queue (M2 plan, "Model and threading").
//!
//! Every change schedules a prepare, debounced. Work runs one item at a
//! time on a blocking thread, so the main loop never waits. Each item has a
//! generation; a result for an old generation is dropped, and so is one for
//! an item that was removed meanwhile.

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::Duration;

use gettextrs::gettext;
use gtk::gio;
use gtk::gio::prelude::*;
use gtk::glib;
use safe_upload_core::output::{PrivateDir, file_name};
use safe_upload_core::{Error, Format, Options, Target, prepare};
use safe_upload_editor_widget::EditModel;

use crate::item::{ImageItem, Output};

pub const DEBOUNCE: Duration = Duration::from_millis(300);

#[derive(Clone)]
pub struct Model(Rc<Inner>);

/// A model reference that does not keep it alive (for row closures).
#[derive(Clone)]
pub struct WeakModel(Weak<Inner>);

impl WeakModel {
    pub fn upgrade(&self) -> Option<Model> {
        upgrade(&self.0)
    }
}

struct Inner {
    store: gio::ListStore,
    options: RefCell<Options>,
    debounce: RefCell<Option<glib::SourceId>>,
    working: Cell<bool>,
    generation: Cell<u64>,
}

impl Model {
    pub fn new(options: Options) -> Self {
        Model(Rc::new(Inner {
            store: gio::ListStore::new::<ImageItem>(),
            options: RefCell::new(options),
            debounce: RefCell::new(None),
            working: Cell::new(false),
            generation: Cell::new(0),
        }))
    }

    pub fn downgrade(&self) -> WeakModel {
        WeakModel(Rc::downgrade(&self.0))
    }

    pub fn store(&self) -> &gio::ListStore {
        &self.0.store
    }

    pub fn items(&self) -> Vec<ImageItem> {
        self.0.store.iter::<ImageItem>().flatten().collect()
    }

    pub fn options(&self) -> Options {
        self.0.options.borrow().clone()
    }

    /// True while a prepare runs on the worker thread.
    pub fn is_working(&self) -> bool {
        self.0.working.get()
    }

    /// True when no prepare is running or waiting.
    pub fn is_idle(&self) -> bool {
        !self.is_working()
            && self.0.debounce.borrow().is_none()
            && !self.items().iter().any(ImageItem::needs_work)
    }

    pub fn add(&self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let generation = self.next_generation();
        for path in paths {
            let item = ImageItem::new(path);
            item.invalidate(generation);
            self.0.store.append(&item);
        }
        self.rename();
        self.schedule();
    }

    pub fn remove(&self, item: &ImageItem) {
        if let Some(pos) = self.0.store.find(item) {
            self.0.store.remove(pos);
            self.rename();
        }
    }

    /// New settings. Every item is prepared again, unless only the name
    /// setting changed: names are the app's job, not core's.
    pub fn set_options(&self, options: Options) {
        let old = self.0.options.replace(options.clone());
        let same_output = Options {
            keep_name: options.keep_name,
            ..old
        } == options;
        if !same_output {
            let generation = self.next_generation();
            for item in self.items() {
                item.invalidate(generation);
            }
            self.schedule();
        }
        self.rename();
    }

    /// Stores the editor's edits on `item`. If its crop or redactions
    /// changed, the item is prepared again.
    pub fn set_edits(&self, item: &ImageItem, edits: EditModel) {
        self.replace_edits(item, Some(edits));
    }

    /// Drops the edits of `item`, as when they no longer fit the image.
    pub fn clear_edits(&self, item: &ImageItem) {
        self.replace_edits(item, None);
    }

    fn replace_edits(&self, item: &ImageItem, edits: Option<EditModel>) {
        if self.0.store.find(item).is_none() {
            return;
        }
        let before = item.edit_options();
        item.set_edits(edits);
        if item.edit_options() != before {
            item.invalidate(self.next_generation());
            self.schedule();
        }
    }

    fn next_generation(&self) -> u64 {
        let g = self.0.generation.get() + 1;
        self.0.generation.set(g);
        g
    }

    fn rename(&self) {
        let items = self.items();
        let keep = self.0.options.borrow().keep_name;
        for (i, item) in items.iter().enumerate() {
            let ext = item.output().map(|o| o.extension.clone());
            item.set_title(output_name(
                item.path(),
                keep,
                i + 1,
                items.len(),
                ext.as_deref(),
            ));
        }
    }

    fn schedule(&self) {
        if let Some(old) = self.0.debounce.take() {
            old.remove();
        }
        let weak = Rc::downgrade(&self.0);
        let id = glib::timeout_add_local_once(DEBOUNCE, move || {
            if let Some(model) = upgrade(&weak) {
                model.0.debounce.replace(None);
                model.pump();
            }
        });
        self.0.debounce.replace(Some(id));
    }

    /// Starts the next prepare, if nothing runs or waits for the debounce.
    fn pump(&self) {
        if self.is_working() || self.0.debounce.borrow().is_some() {
            return;
        }
        let Some(item) = self.items().into_iter().find(ImageItem::needs_work) else {
            return;
        };
        let generation = item.wanted();
        let path = item.path().to_owned();
        let (crop, redactions) = item.edit_options();
        let options = Options {
            keep_name: false,
            crop,
            redactions,
            ..self.options()
        };
        self.0.working.set(true);
        let weak = Rc::downgrade(&self.0);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || work(&path, &options))
                .await
                .unwrap_or_else(|_| Err(gettext("Internal error while preparing the image")));
            let Some(model) = upgrade(&weak) else {
                return;
            };
            model.0.working.set(false);
            if model.0.store.find(&item).is_some() && item.wanted() == generation {
                item.finish(generation, result);
                model.rename();
            }
            model.pump();
        });
    }
}

fn upgrade(weak: &Weak<Inner>) -> Option<Model> {
    weak.upgrade().map(Model)
}

/// Runs on the worker thread. The private folder lives only for this
/// prepare (S-1); the verified bytes stay in memory.
fn work(path: &Path, options: &Options) -> Result<Output, String> {
    let run = || -> Result<Output, Error> {
        let dir = PrivateDir::create()?;
        let target = Target {
            dir: &dir,
            position: 1,
            total: 1,
        };
        let result = prepare(path, options, target).and_then(|p| {
            let bytes = fs::read(&p.path)?;
            let extension = p
                .path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default()
                .to_owned();
            Ok(Output {
                bytes,
                extension,
                summary: p.summary,
            })
        });
        // A folder left behind is removed by the S-2 cleanup.
        let _ = dir.remove();
        result
    };
    run().map_err(|e| row_error(&e))
}

/// The text shown on a failed row. Core messages have no paths or metadata
/// values (S-5).
pub fn row_error(e: &Error) -> String {
    match e {
        Error::Decode(_) => gettext("Not a supported image"),
        Error::MissingDecoder(Format::Heic) => gettext(
            "This HEIC image needs an HEVC decoder, which is not installed. \
             On Fedora, install libheif-freeworld from RPM Fusion. In the Flatpak, \
             install the org.freedesktop.Platform.codecs-extra extension.",
        ),
        Error::CannotFit { .. } => format!(
            "{}. {}",
            capitalize(&e.to_string()),
            gettext("Raise the max size or lower the max long edge.")
        ),
        e => capitalize(&e.to_string()),
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(first) => first.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// The name the output gets when saved. The app names outputs (core's
/// rules), so adding, removing or renaming needs no new prepare. Before the
/// format is known, the name has no extension.
pub fn output_name(
    input: &Path,
    keep: bool,
    position: usize,
    total: usize,
    ext: Option<&str>,
) -> String {
    match ext {
        Some(ext) => file_name(input, keep, position, total, ext),
        None => file_name(input, keep, position, total, "")
            .trim_end_matches('.')
            .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        let p = Path::new("/x/holiday.heic");
        assert_eq!(output_name(p, false, 1, 1, Some("jpg")), "image.jpg");
        assert_eq!(output_name(p, false, 2, 3, Some("png")), "image-2.png");
        assert_eq!(output_name(p, true, 2, 3, Some("jpg")), "holiday.jpg");
        assert_eq!(output_name(p, false, 2, 3, None), "image-2");
        assert_eq!(output_name(p, true, 1, 1, None), "holiday");
    }

    #[test]
    fn row_errors() {
        assert_eq!(
            row_error(&Error::Decode("cannot read the image")),
            "Not a supported image"
        );
        assert_eq!(
            row_error(&Error::Sandbox("the decoder did not run in the sandbox")),
            "The image decoder could not run in its sandbox: \
             the decoder did not run in the sandbox"
        );
        assert!(
            row_error(&Error::MissingDecoder(Format::Heic))
                .starts_with("This HEIC image needs an HEVC decoder, which is not installed.")
        );
        assert_eq!(
            row_error(&Error::MissingDecoder(Format::Heif)),
            "This HEIC/HEIF image needs a codec that is not installed"
        );
        assert_eq!(
            row_error(&Error::MissingDecoder(Format::Avif)),
            "This AVIF image needs a codec that is not installed"
        );
        assert_eq!(
            row_error(&Error::CannotFit { target_bytes: 1000 }),
            "The image cannot be made small enough for 1000 bytes. \
             Raise the max size or lower the max long edge."
        );
    }
}
