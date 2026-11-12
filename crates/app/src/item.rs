//! One image in the list. Rows bind to its properties.

use std::cell::{Cell, OnceCell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gettextrs::gettext;
use gtk::glib;
use gtk::glib::prelude::*;
use gtk::glib::subclass::prelude::*;
use safe_upload_core::edit::{Rect, Redaction};
use safe_upload_core::summary::Summary;
use safe_upload_editor_widget::EditModel;

/// A verified output, kept in memory. Its private folder is already gone.
#[derive(Debug)]
pub struct Output {
    pub bytes: Vec<u8>,
    pub extension: String,
    pub summary: Summary,
}

mod imp {
    use super::*;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::ImageItem)]
    pub struct ImageItem {
        pub path: OnceCell<PathBuf>,
        /// The output file name.
        #[property(get)]
        pub title: RefCell<String>,
        /// "Preparing…", the summary, or the error.
        #[property(get)]
        pub status: RefCell<String>,
        #[property(get)]
        pub busy: Cell<bool>,
        #[property(get)]
        pub failed: Cell<bool>,
        /// The editor preview is being loaded.
        #[property(get)]
        pub loading: Cell<bool>,
        /// The redaction edits, with their undo history.
        pub edits: RefCell<Option<EditModel>>,
        pub output: RefCell<Option<Rc<Output>>>,
        /// The generation the current settings need, and the one the
        /// current state was made with. Equal when nothing is left to do.
        pub wanted: Cell<u64>,
        pub done: Cell<u64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ImageItem {
        const NAME: &'static str = "SafeUploadImageItem";
        type Type = super::ImageItem;
    }

    #[glib::derived_properties]
    impl ObjectImpl for ImageItem {}
}

glib::wrapper! {
    pub struct ImageItem(ObjectSubclass<imp::ImageItem>);
}

impl ImageItem {
    pub fn new(path: PathBuf) -> Self {
        let item: Self = glib::Object::new();
        item.imp().path.set(path).expect("new item");
        item
    }

    pub fn path(&self) -> &Path {
        self.imp().path.get().expect("set in new")
    }

    pub fn output(&self) -> Option<Rc<Output>> {
        self.imp().output.borrow().clone()
    }

    pub fn edits(&self) -> Option<EditModel> {
        self.imp().edits.borrow().clone()
    }

    /// The crop and redactions for `core::Options`.
    pub fn edit_options(&self) -> (Option<Rect>, Vec<Redaction>) {
        self.imp()
            .edits
            .borrow()
            .as_ref()
            .map(EditModel::to_options)
            .unwrap_or_default()
    }

    pub(crate) fn set_edits(&self, edits: Option<EditModel>) {
        self.imp().edits.replace(edits);
    }

    pub(crate) fn set_loading(&self, loading: bool) {
        if self.imp().loading.replace(loading) != loading {
            self.notify_loading();
        }
    }

    pub(crate) fn set_title(&self, title: String) {
        if *self.imp().title.borrow() != title {
            self.imp().title.replace(title);
            self.notify_title();
        }
    }

    pub(crate) fn wanted(&self) -> u64 {
        self.imp().wanted.get()
    }

    pub(crate) fn needs_work(&self) -> bool {
        self.imp().wanted.get() != self.imp().done.get()
    }

    /// Asks for a new prepare with generation `generation`. The old output
    /// is dropped at once, so it can never be used with new settings.
    pub(crate) fn invalidate(&self, generation: u64) {
        let imp = self.imp();
        imp.wanted.set(generation);
        imp.output.replace(None);
        self.set_state(true, false, gettext("Preparing…"));
    }

    /// Stores the result of the prepare for `generation`.
    pub(crate) fn finish(&self, generation: u64, result: Result<Output, String>) {
        let imp = self.imp();
        imp.done.set(generation);
        match result {
            Ok(out) => {
                let status = out.summary.to_string();
                imp.output.replace(Some(Rc::new(out)));
                self.set_state(false, false, status);
            }
            Err(message) => {
                imp.output.replace(None);
                self.set_state(false, true, message);
            }
        }
    }

    fn set_state(&self, busy: bool, failed: bool, status: String) {
        let imp = self.imp();
        let _freeze = self.freeze_notify();
        if imp.status.replace(status.clone()) != status {
            self.notify_status();
        }
        if imp.failed.replace(failed) != failed {
            self.notify_failed();
        }
        if imp.busy.replace(busy) != busy {
            self.notify_busy();
        }
    }
}
