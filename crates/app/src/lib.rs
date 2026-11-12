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

pub fn run() -> glib::ExitCode {
    application().run()
}
