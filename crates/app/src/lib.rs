//! Safe Upload desktop app (libadwaita).

pub mod cleanup;
pub mod config;
pub mod item;
pub mod model;
pub mod save;
pub mod settings;
pub mod window;

use adw::prelude::*;
use gtk::glib;

pub fn application() -> adw::Application {
    let app = adw::Application::builder()
        .application_id(config::APP_ID)
        .build();
    app.connect_startup(|app| {
        cleanup::start();
        app.set_accels_for_action("win.open", &["<Control>o"]);
        app.set_accels_for_action("win.save", &["<Control>s"]);
        app.set_accels_for_action("win.copy", &["<Control><Shift>c"]);
    });
    app.connect_activate(|app| window::build(app).present());
    app
}

/// Checks for the settings schema before the app starts: a panic inside a
/// GLib callback aborts the process.
pub fn run() -> glib::ExitCode {
    if settings::schema().is_none() {
        eprintln!("safe-upload-app: {}: {}", settings::MISSING, config::APP_ID);
        return glib::ExitCode::FAILURE;
    }
    application().run()
}
