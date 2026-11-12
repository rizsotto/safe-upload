//! The main window (SPEC 6.2, M2 plan "Window").

use std::cell::OnceCell;
use std::path::PathBuf;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gdk, gio, glib};

use crate::edit_dialog;
use crate::item::ImageItem;
use crate::model::{Model, WeakModel};
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

        let content = adw::ToolbarView::new();
        content.add_top_bar(&header);
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

        let action = gio::ActionEntry::builder("open")
            .activate(|win: &Self, _, _| win.open_dialog())
            .build();
        self.add_action_entries([action]);
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
        .tooltip_text(gettext("Edit"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    edit.update_property(&[gtk::accessible::Property::Label(&gettext("Edit"))]);
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
        .tooltip_text(gettext("Remove"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    remove.update_property(&[gtk::accessible::Property::Label(&gettext("Remove"))]);
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
