//! The editor dialog (M2 plan, "Editor dialog in the app"). The preview is
//! made off the main thread; "Done" stores the edits on the item, and the
//! model prepares it again. The dialog never writes files.

use adw::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use safe_upload_core::Limits;
use safe_upload_core::pipeline::{self, Preview};
use safe_upload_editor_widget::{EditModel, RedactionEditor};

use crate::item::ImageItem;
use crate::model::row_error;
use crate::window::Window;

/// Long edge of the editor preview, in pixels.
pub const PREVIEW_EDGE: u32 = 2048;

/// Loads the preview of `item` on a worker thread, then shows the editor.
/// The row shows a spinner meanwhile. Focus goes to `return_focus` when
/// the dialog closes.
pub fn open(win: &Window, item: &ImageItem, return_focus: Option<gtk::Widget>) {
    if item.loading() {
        return;
    }
    item.set_loading(true);
    let path = item.path().to_owned();
    let win = win.downgrade();
    let item = item.clone();
    glib::spawn_future_local(async move {
        let result =
            gio::spawn_blocking(move || pipeline::preview(&path, &Limits::default(), PREVIEW_EDGE))
                .await;
        item.set_loading(false);
        let Some(win) = win.upgrade() else {
            return;
        };
        if win.model().store().find(&item).is_none() {
            return;
        }
        match result {
            Ok(Ok(preview)) => present(&win, &item, preview, return_focus),
            Ok(Err(e)) => win.toast(&row_error(&e)),
            Err(_) => win.toast(&gettext("Internal error while loading the image")),
        }
    });
}

fn present(win: &Window, item: &ImageItem, preview: Preview, return_focus: Option<gtk::Widget>) {
    let (w, h) = preview.full_size;
    let model = item
        .edits()
        .filter(|m| m.size() == (w, h))
        .unwrap_or_else(|| EditModel::new(w, h));
    let editor = RedactionEditor::new();
    if editor.load(preview.pixels, model).is_err() {
        win.toast(&gettext("The image could not be shown in the editor"));
        return;
    }
    editor.set_margin_top(12);
    editor.set_margin_bottom(12);
    editor.set_margin_start(12);
    editor.set_margin_end(12);

    let cancel = gtk::Button::builder()
        .label(gettext("_Cancel"))
        .use_underline(true)
        .action_name("dialog.cancel")
        .build();
    let done = gtk::Button::builder()
        .label(gettext("_Done"))
        .use_underline(true)
        .action_name("dialog.done")
        .css_classes(["suggested-action"])
        .build();
    let header = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    header.pack_start(&cancel);
    header.pack_end(&done);
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&editor));

    // Escape and other closes ask first, so a press or a held arrow key
    // is never cut off.
    let dialog = adw::Dialog::builder()
        .title(gettext("Edit Image"))
        .content_width(900)
        .content_height(700)
        .can_close(false)
        .child(&view)
        .build();
    dialog.connect_close_attempt(glib::clone!(
        #[weak]
        editor,
        move |d| {
            if !editor.in_gesture() {
                d.force_close();
            }
        }
    ));
    dialog.connect_closed(move |_| {
        if let Some(w) = &return_focus {
            w.grab_focus();
        }
    });

    let model = win.model().downgrade();
    let done = gio::ActionEntry::builder("done")
        .activate(glib::clone!(
            #[weak]
            editor,
            #[weak]
            dialog,
            #[weak]
            item,
            move |_: &gio::SimpleActionGroup, _, _| {
                if let (Some(model), Some(edits)) = (model.upgrade(), editor.model()) {
                    model.set_edits(&item, edits);
                }
                dialog.force_close();
            }
        ))
        .build();
    let cancel = gio::ActionEntry::builder("cancel")
        .activate(glib::clone!(
            #[weak]
            dialog,
            move |_: &gio::SimpleActionGroup, _, _| dialog.force_close()
        ))
        .build();
    let actions = gio::SimpleActionGroup::new();
    actions.add_action_entries([done, cancel]);
    dialog.insert_action_group("dialog", Some(&actions));

    dialog.set_focus(Some(&editor.canvas()));
    dialog.present(Some(win));
}
