//! The main window (SPEC 6.2, M2 plan "Window").

use std::cell::{Cell, OnceCell, RefCell};
use std::path::PathBuf;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::{gdk, gio, glib};

use crate::edit_dialog;
use crate::item::ImageItem;
use crate::model::{Model, WeakModel};
use crate::save::{self, Named, SaveError};
use crate::settings;

const IMAGE_TYPES: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/webp",
    "image/avif",
    "image/heic",
    "image/heif",
];

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Window {
        pub model: OnceCell<Model>,
        pub settings: OnceCell<gio::Settings>,
        pub toasts: OnceCell<adw::ToastOverlay>,
        pub saving: Cell<bool>,
        pub last_toast: RefCell<Option<String>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Window {
        const NAME: &'static str = "SafeUploadWindow";
        type Type = super::Window;
        type ParentType = adw::ApplicationWindow;
    }

    impl ObjectImpl for Window {}
    impl WidgetImpl for Window {}
    impl WindowImpl for Window {}
    impl ApplicationWindowImpl for Window {}
    impl AdwApplicationWindowImpl for Window {}
}

glib::wrapper! {
    pub struct Window(ObjectSubclass<imp::Window>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
            gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

pub fn build(app: &adw::Application) -> Window {
    Window::new(app)
}

impl Window {
    pub fn new(app: &adw::Application) -> Self {
        let win: Self = glib::Object::builder()
            .property("application", app)
            .property("title", gettext("Safe Upload"))
            .property("default-width", 800)
            .property("default-height", 600)
            .build();
        let settings = settings::load();
        let model = Model::new(settings::options(&settings));
        let weak = model.downgrade();
        settings::on_change(&settings, move |options| {
            if let Some(model) = weak.upgrade() {
                model.set_options(options);
            }
        });
        if win.imp().settings.set(settings).is_err() {
            unreachable!("settings set once");
        }
        if win.imp().model.set(model).is_err() {
            unreachable!("model set once");
        }
        win.setup();
        win
    }

    pub fn settings(&self) -> &gio::Settings {
        self.imp().settings.get().expect("set in new")
    }

    pub fn model(&self) -> &Model {
        self.imp().model.get().expect("set in new")
    }

    /// Adds files, as a drop or `Open…` does. Only regular files are kept;
    /// folders, FIFOs, devices and files without a local path are skipped
    /// with a toast (a FIFO would block the worker forever). Core checks
    /// the content of the rest.
    pub fn add_files(&self, files: &[gio::File]) {
        let (paths, skipped): (Vec<Option<PathBuf>>, Vec<_>) = files
            .iter()
            .map(|f| f.path().filter(|p| p.metadata().is_ok_and(|m| m.is_file())))
            .partition(Option::is_some);
        self.model().add(paths.into_iter().flatten().collect());
        if !skipped.is_empty() {
            self.toast(&gettext(
                "Only files can be added. Folders and other items were skipped",
            ));
        }
    }

    pub fn toast(&self, text: &str) {
        self.imp().last_toast.replace(Some(text.to_owned()));
        if let Some(t) = self.imp().toasts.get() {
            t.add_toast(adw::Toast::new(text));
        }
    }

    fn setup(&self) {
        let open = gtk::Button::builder()
            .label(gettext("_Open…"))
            .use_underline(true)
            .action_name("win.open")
            .tooltip_text(gettext("Open images"))
            .build();
        let header = adw::HeaderBar::new();
        header.pack_end(&open);

        let empty_open = gtk::Button::builder()
            .label(gettext("_Open…"))
            .use_underline(true)
            .action_name("win.open")
            .halign(gtk::Align::Center)
            .css_classes(["pill", "suggested-action"])
            .build();
        let empty = adw::StatusPage::builder()
            .icon_name("image-x-generic-symbolic")
            .title(gettext("Drop Images Here"))
            .description(gettext(
                "Metadata is removed and the size is reduced. The originals are not changed.",
            ))
            .child(&empty_open)
            .build();

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .valign(gtk::Align::Start)
            .build();
        list.update_property(&[gtk::accessible::Property::Label(&gettext("Images"))]);
        let weak_model = self.model().downgrade();
        let weak_win = self.downgrade();
        list.bind_model(Some(self.model().store()), move |obj| {
            let item = obj.downcast_ref::<ImageItem>().expect("ImageItem");
            row(item, weak_model.clone(), weak_win.clone()).upcast()
        });
        let clamp = adw::Clamp::builder()
            .child(&list)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&clamp)
            .build();

        let stack = gtk::Stack::new();
        stack.add_named(&empty, Some("empty"));
        stack.add_named(&scrolled, Some("list"));
        let store = self.model().store().clone();
        let update = glib::clone!(
            #[weak]
            stack,
            move |store: &gio::ListStore| {
                stack.set_visible_child_name(if store.n_items() == 0 {
                    "empty"
                } else {
                    "list"
                });
            }
        );
        update(&store);
        store.connect_items_changed(move |s, _, _, _| update(s));

        let sidebar = settings::panel(self.settings());
        let split = adw::OverlaySplitView::builder()
            .sidebar(&sidebar)
            .content(&stack)
            .build();
        let show_settings = gtk::ToggleButton::builder()
            .icon_name("sidebar-show-symbolic")
            .active(true)
            .tooltip_text(gettext("Show settings"))
            .build();
        show_settings
            .update_property(&[gtk::accessible::Property::Label(&gettext("Show settings"))]);
        show_settings
            .bind_property("active", &split, "show-sidebar")
            .bidirectional()
            .sync_create()
            .build();
        header.pack_start(&show_settings);

        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&split));
        self.imp().toasts.set(toasts.clone()).expect("once");

        let copy = gtk::Button::builder()
            .label(gettext("_Copy to Clipboard"))
            .use_underline(true)
            .action_name("win.copy")
            .build();
        let save = gtk::Button::builder()
            .label(gettext("_Save to…"))
            .use_underline(true)
            .action_name("win.save")
            .css_classes(["suggested-action"])
            .build();
        let bottom = gtk::ActionBar::new();
        bottom.pack_end(&save);
        bottom.pack_end(&copy);

        let content = adw::ToolbarView::new();
        content.add_top_bar(&header);
        content.add_bottom_bar(&bottom);
        content.set_content(Some(&toasts));
        self.set_content(Some(&content));

        let drop = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
        drop.connect_drop(glib::clone!(
            #[weak(rename_to = win)]
            self,
            #[upgrade_or]
            false,
            move |_, value, _, _| {
                let Ok(list) = value.get::<gdk::FileList>() else {
                    return false;
                };
                win.add_files(&list.files());
                true
            }
        ));
        self.add_controller(drop);

        let open = gio::ActionEntry::builder("open")
            .activate(|win: &Self, _, _| win.open_dialog())
            .build();
        let save = gio::ActionEntry::builder("save")
            .activate(|win: &Self, _, _| win.save_dialog())
            .build();
        let copy = gio::ActionEntry::builder("copy")
            .activate(|win: &Self, _, _| {
                if let Err(e) = win.copy() {
                    win.toast(&e.to_string());
                }
            })
            .build();
        self.add_action_entries([open, save, copy]);

        store.connect_items_changed(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |store, pos, _, added| {
                for i in pos..pos + added {
                    if let Some(item) = store.item(i).and_downcast::<ImageItem>() {
                        item.connect_busy_notify(glib::clone!(
                            #[weak]
                            win,
                            move |_| win.update_actions()
                        ));
                    }
                }
                win.update_actions();
            }
        ));
        self.update_actions();
    }

    /// Every output with its file name, or `None` if any image is not
    /// ready (a failed image blocks Save, M2 plan "Window").
    fn outputs(&self) -> Option<Vec<Named>> {
        let items = self.model().items();
        if items.is_empty() {
            return None;
        }
        items
            .iter()
            .map(|i| {
                i.output().map(|o| Named {
                    name: i.title(),
                    bytes: o.bytes.clone(),
                })
            })
            .collect()
    }

    fn originals(&self) -> Vec<PathBuf> {
        self.model()
            .items()
            .iter()
            .map(|i| i.path().to_owned())
            .collect()
    }

    /// The text of the last toast, for tests.
    pub fn last_toast(&self) -> Option<String> {
        self.imp().last_toast.borrow().clone()
    }

    fn update_actions(&self) {
        let items = self.model().items();
        let ready = items.iter().all(|i| i.output().is_some());
        let set = |name: &str, enabled: bool| {
            if let Some(a) = self.lookup_action(name).and_downcast::<gio::SimpleAction>() {
                a.set_enabled(enabled);
            }
        };
        set(
            "save",
            ready && !items.is_empty() && !self.imp().saving.get(),
        );
        set("copy", ready && items.len() == 1);
    }

    /// Puts the single output on the clipboard as its own bytes and MIME
    /// type, checked again first. Not a texture: GTK would turn that into
    /// a PNG that is neither the verified file nor within the size.
    pub fn copy(&self) -> Result<(), SaveError> {
        let outputs = self.outputs().ok_or(SaveError::NotReady)?;
        let [out] = outputs.as_slice() else {
            return Err(SaveError::NotReady);
        };
        let mime = save::check(&out.bytes)?;
        let provider = gdk::ContentProvider::for_bytes(mime, &glib::Bytes::from(&out.bytes));
        self.clipboard()
            .set_content(Some(&provider))
            .map_err(|e| SaveError::Io(std::io::Error::other(e.message.to_string())))?;
        self.toast(&gettext("Copied. Keep Safe Upload open until you paste."));
        Ok(())
    }

    /// One image: a save dialog with the output name. A batch: a folder
    /// dialog. No initial folder, so it does not open next to the
    /// originals.
    fn save_dialog(&self) {
        let Some(outputs) = self.outputs() else {
            return;
        };
        let dialog = gtk::FileDialog::builder().modal(true).build();
        if let [one] = outputs.as_slice() {
            dialog.set_title(&gettext("Save Image"));
            dialog.set_initial_name(Some(&one.name));
            dialog.save(
                Some(self),
                None::<&gio::Cancellable>,
                glib::clone!(
                    #[weak(rename_to = win)]
                    self,
                    move |result| {
                        if let Some(path) = result.ok().and_then(|f| f.path()) {
                            glib::spawn_future_local(async move {
                                let _ = win.save_to_file(path).await;
                            });
                        }
                    }
                ),
            );
        } else {
            dialog.set_title(&gettext("Save Images to Folder"));
            dialog.select_folder(
                Some(self),
                None::<&gio::Cancellable>,
                glib::clone!(
                    #[weak(rename_to = win)]
                    self,
                    move |result| {
                        if let Some(path) = result.ok().and_then(|f| f.path()) {
                            glib::spawn_future_local(async move {
                                let _ = win.save_to_folder(path).await;
                            });
                        }
                    }
                ),
            );
        }
    }

    /// Saves the single output to `path` (the file chosen in the dialog).
    /// Every outcome is shown as a toast.
    pub async fn save_to_file(&self, path: PathBuf) -> Result<(), SaveError> {
        let result = match self.outputs().as_deref() {
            Some([out]) => {
                let bytes = out.bytes.clone();
                let originals = self.originals();
                self.run_save(move || save::save_file(&path, &bytes, &originals))
                    .await
            }
            _ => Err(SaveError::NotReady),
        };
        self.toast_result(&result, || gettext("Saved 1 image"));
        result
    }

    /// Saves every output into `folder`, all or nothing. Every outcome is
    /// shown as a toast.
    pub async fn save_to_folder(&self, folder: PathBuf) -> Result<usize, SaveError> {
        let result = match self.outputs() {
            Some(outputs) => {
                self.run_save(move || save::save_batch(&folder, &outputs))
                    .await
            }
            None => Err(SaveError::NotReady),
        };
        self.toast_result(&result, || {
            let n = *result.as_ref().unwrap_or(&0);
            ngettext("Saved {} image", "Saved {} images", n as u32).replace("{}", &n.to_string())
        });
        result
    }

    fn toast_result<T>(&self, result: &Result<T, SaveError>, ok: impl FnOnce() -> String) {
        match result {
            Ok(_) => self.toast(&ok()),
            Err(e) => self.toast(&e.to_string()),
        }
    }

    /// Runs a write on a worker thread, so the window stays responsive.
    async fn run_save<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> Result<T, SaveError> + Send + 'static,
    ) -> Result<T, SaveError> {
        self.imp().saving.set(true);
        self.update_actions();
        let result = gio::spawn_blocking(work).await.unwrap_or_else(|_| {
            Err(SaveError::Io(std::io::Error::other(gettext(
                "internal error while saving",
            ))))
        });
        self.imp().saving.set(false);
        self.update_actions();
        result
    }

    /// Opens the redaction editor for `item` once its preview is loaded.
    /// Focus goes to `return_focus` when the editor closes.
    pub fn open_editor(&self, item: &ImageItem, return_focus: Option<&gtk::Widget>) {
        edit_dialog::open(self, item, return_focus.cloned());
    }

    fn open_dialog(&self) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&gettext("Images")));
        for t in IMAGE_TYPES {
            filter.add_mime_type(t);
        }
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title(gettext("Open Images"))
            .modal(true)
            .filters(&filters)
            .default_filter(&filter)
            .build();
        dialog.open_multiple(
            Some(self),
            None::<&gio::Cancellable>,
            glib::clone!(
                #[weak(rename_to = win)]
                self,
                move |result| {
                    if let Ok(list) = result {
                        let files: Vec<gio::File> = list.iter().flatten().collect();
                        win.add_files(&files);
                    }
                }
            ),
        );
    }
}

fn row(item: &ImageItem, model: WeakModel, win: glib::WeakRef<Window>) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title_lines(1);
    item.bind_property("title", &row, "title")
        .sync_create()
        .build();
    item.bind_property("status", &row, "subtitle")
        .sync_create()
        .build();

    let spinner = adw::Spinner::new();
    let show_spinner = glib::clone!(
        #[weak]
        spinner,
        move |i: &ImageItem| spinner.set_visible(i.busy() || i.loading())
    );
    show_spinner(item);
    item.connect_busy_notify(show_spinner.clone());
    item.connect_loading_notify(show_spinner);
    row.add_prefix(&spinner);

    let error = gtk::Image::from_icon_name("dialog-error-symbolic");
    error.add_css_class("error");
    error.set_tooltip_text(Some(&gettext("Failed")));
    item.bind_property("failed", &error, "visible")
        .sync_create()
        .build();
    row.add_prefix(&error);

    let edit = gtk::Button::builder()
        .icon_name("document-edit-symbolic")
        .tooltip_text(gettext("Edit image"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    edit.update_property(&[gtk::accessible::Property::Label(&gettext("Edit image"))]);
    edit.connect_clicked(glib::clone!(
        #[weak]
        item,
        move |b| {
            if let Some(win) = win.upgrade() {
                win.open_editor(&item, Some(b.upcast_ref()));
            }
        }
    ));
    row.add_suffix(&edit);

    let remove = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text(gettext("Remove image"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    remove.update_property(&[gtk::accessible::Property::Label(&gettext("Remove image"))]);
    remove.connect_clicked(glib::clone!(
        #[weak]
        item,
        move |_| {
            if let Some(model) = model.upgrade() {
                model.remove(&item);
            }
        }
    ));
    row.add_suffix(&remove);
    row
}
