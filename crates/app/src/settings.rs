//! Settings panel bound to GSettings (SPEC 6.2 step 2, M2 plan "Settings").

use std::path::{Path, PathBuf};

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

/// Test hook: points the dev schema lookup at another folder, so a test
/// can hide the dev schema. Not for users.
pub const DEV_SCHEMA_DIR_ENV: &str = "SAFE_UPLOAD_DEV_SCHEMA_DIR";

/// The message `run` prints when `schema` finds nothing.
pub const MISSING: &str = "the GSettings schema is not installed";

/// The app's settings schema. Looks in the default places first (an
/// installed or Flatpak build always finds it there). Then, for a dev
/// build, in the folder `build.rs` compiled `data/` into. That path is in
/// the build tree, so it does not exist on an installed system.
pub fn schema() -> Option<gio::SettingsSchema> {
    let dev = std::env::var_os(DEV_SCHEMA_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("SAFE_UPLOAD_DEV_SCHEMA_DIR")));
    find(gio::SettingsSchemaSource::default(), &dev)
}

fn find(default: Option<gio::SettingsSchemaSource>, dev: &Path) -> Option<gio::SettingsSchema> {
    if let Some(s) = default.as_ref().and_then(|s| s.lookup(APP_ID, true)) {
        return Some(s);
    }
    gio::SettingsSchemaSource::from_directory(dev, None, false)
        .ok()?
        .lookup(APP_ID, false)
}

/// The app's settings. `run` checks `schema` before the app starts, so
/// this does not fail there.
pub fn load() -> gio::Settings {
    let schema = schema().unwrap_or_else(|| panic!("{MISSING}: {APP_ID}"));
    gio::Settings::new_full(&schema, None::<&gio::SettingsBackend>, None)
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

/// The settings panel. Every row is bound to its key. The max size row
/// covers the schema's whole range (0.1 to 100 MB).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_build_finds_compiled_schema_without_default_source() {
        let dev = Path::new(env!("SAFE_UPLOAD_DEV_SCHEMA_DIR"));
        let schema = find(None, dev).expect("dev schema");
        assert_eq!(schema.id(), APP_ID);
        assert!(schema.has_key("max-size"));
    }

    #[test]
    fn empty_dev_dir_finds_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find(None, dir.path()).is_none());
    }
}
