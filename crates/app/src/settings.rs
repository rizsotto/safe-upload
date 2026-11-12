//! Settings panel bound to GSettings (SPEC 6.2 step 2, M2 plan "Settings").

use adw::prelude::*;
use gettextrs::gettext;
use gtk::gio;
use safe_upload_core::{Options, OutputFormat};

use crate::config::APP_ID;

/// Format choices, in the order of the combo row. No WebP: core refuses
/// WebP output in this version.
const FORMATS: &[(&str, OutputFormat)] = &[
    ("auto", OutputFormat::Auto),
    ("jpeg", OutputFormat::Jpeg),
    ("png", OutputFormat::Png),
];

/// The app's settings. Panics with a clear message if the schema is not
/// installed, because GLib would abort without one.
pub fn load() -> gio::Settings {
    let found = gio::SettingsSchemaSource::default().and_then(|s| s.lookup(APP_ID, true));
    assert!(
        found.is_some(),
        "the GSettings schema {APP_ID} is not installed; compile data/ with \
         glib-compile-schemas and set GSETTINGS_SCHEMA_DIR"
    );
    gio::Settings::new(APP_ID)
}

/// The prepare options the settings ask for.
pub fn options(settings: &gio::Settings) -> Options {
    let nick = settings.string("output-format");
    let format = FORMATS
        .iter()
        .find(|(n, _)| *n == nick.as_str())
        .map_or(OutputFormat::Auto, |(_, f)| *f);
    let edge = settings.uint("max-long-edge");
    Options {
        max_long_edge: (edge > 0).then_some(edge),
        format,
        keep_name: settings.boolean("keep-names"),
        ..Options::new(settings.uint64("max-size"))
    }
}

/// The settings panel. Every row is bound to its key.
pub fn panel(settings: &gio::Settings) -> adw::PreferencesPage {
    let max_size = adw::SpinRow::builder()
        .title(gettext("Max _size (MB)"))
        .use_underline(true)
        .digits(1)
        .adjustment(&gtk::Adjustment::new(5.0, 0.1, 100.0, 0.1, 1.0, 0.0))
        .build();
    settings
        .bind("max-size", &max_size, "value")
        .mapping(|v, _| Some((v.get::<u64>()? as f64 / 1e6).to_value()))
        .set_mapping(|v, _| {
            Some(((v.get::<f64>().ok()? * 10.0).round() as u64 * 100_000).to_variant())
        })
        .build();

    let long_edge = adw::SpinRow::builder()
        .title(gettext("Max _long edge (px)"))
        .subtitle(gettext("0 means no limit"))
        .use_underline(true)
        .digits(0)
        .adjustment(&gtk::Adjustment::new(0.0, 0.0, 20000.0, 100.0, 1000.0, 0.0))
        .build();
    settings
        .bind("max-long-edge", &long_edge, "value")
        .mapping(|v, _| Some(f64::from(v.get::<u32>()?).to_value()))
        .set_mapping(|v, _| Some((v.get::<f64>().ok()?.round() as u32).to_variant()))
        .build();

    let names = gtk::StringList::new(&[&gettext("Automatic"), "JPEG", "PNG"]);
    let format = adw::ComboRow::builder()
        .title(gettext("_Format"))
        .subtitle(gettext("Automatic keeps transparency as PNG, else JPEG"))
        .use_underline(true)
        .model(&names)
        .build();
    settings
        .bind("output-format", &format, "selected")
        .mapping(|v, _| {
            let nick = v.str()?;
            let i = FORMATS.iter().position(|(n, _)| *n == nick)?;
            Some((i as u32).to_value())
        })
        .set_mapping(|v, _| {
            let (nick, _) = FORMATS.get(v.get::<u32>().ok()? as usize)?;
            Some(nick.to_variant())
        })
        .build();

    let keep = adw::SwitchRow::builder()
        .title(gettext("_Keep names"))
        .subtitle(gettext("Off: files are named image-1, image-2…"))
        .use_underline(true)
        .build();
    settings.bind("keep-names", &keep, "active").build();

    let group = adw::PreferencesGroup::builder()
        .title(gettext("Settings"))
        .build();
    group.add(&max_size);
    group.add(&long_edge);
    group.add(&format);
    group.add(&keep);
    let page = adw::PreferencesPage::new();
    page.add(&group);
    page
}

/// Gives every change to the settings to `apply`.
pub fn on_change(settings: &gio::Settings, apply: impl Fn(Options) + 'static) {
    settings.connect_changed(None, move |s, _| apply(options(s)));
}
