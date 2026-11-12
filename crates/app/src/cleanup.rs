//! S-2: delete stale private folders on start and every 10 minutes. Our
//! own folders live only for one prepare, far less than `MAX_AGE`, so this
//! never removes a folder in use.

use std::path::PathBuf;
use std::time::Duration;

use gtk::glib;
use safe_upload_core::output::clean_stale;

pub const INTERVAL: Duration = Duration::from_secs(10 * 60);

/// Removes stale folders under `$XDG_RUNTIME_DIR/safe-upload`, after core
/// checked that folder. Returns how many were removed.
pub fn remove_stale_now() -> usize {
    match std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) {
        Some(dir) if dir.is_absolute() => clean_stale(&dir).unwrap_or(0),
        _ => 0,
    }
}

/// Cleans up now and starts the timer.
pub fn start() -> glib::SourceId {
    remove_stale_now();
    glib::timeout_add_seconds_local(INTERVAL.as_secs() as u32, || {
        remove_stale_now();
        glib::ControlFlow::Continue
    })
}
