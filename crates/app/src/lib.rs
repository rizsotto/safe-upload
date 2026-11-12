//! Safe Upload desktop app (libadwaita).

pub mod config;
pub mod window;

use adw::prelude::*;
use gtk::glib;

pub fn application() -> adw::Application {
    let app = adw::Application::builder()
        .application_id(config::APP_ID)
        .build();
    app.connect_activate(|app| window::build(app).present());
    app
}

pub fn run() -> glib::ExitCode {
    application().run()
}
