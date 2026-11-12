//! Private temporary output folders (S-1, S-2) and neutral names (FR-6).
//!
//! Layout: `$XDG_RUNTIME_DIR/safe-upload/<random-32-hex>/`, folders 0700,
//! files 0600. Originals are never touched (FR-7).

use std::ffi::OsString;
use std::fs::{self, DirBuilder, File, OpenOptions, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::Error;

const BASE_NAME: &str = "safe-upload";

/// Folders older than this are deleted when a new one is created (S-2).
pub const MAX_AGE: Duration = Duration::from_secs(60 * 60);

/// One private output folder.
#[derive(Debug)]
pub struct PrivateDir {
    path: PathBuf,
}

impl PrivateDir {
    /// Creates a new folder under `$XDG_RUNTIME_DIR`.
    pub fn create() -> Result<Self, Error> {
        Self::create_in(&runtime_dir(std::env::var_os("XDG_RUNTIME_DIR"))?)
    }

    /// Creates a new folder under `runtime_dir/safe-upload/`, after deleting
    /// stale folders there.
    pub fn create_in(runtime_dir: &Path) -> Result<Self, Error> {
        let base = ensure_base(runtime_dir)?;
        remove_stale(&base, MAX_AGE);

        let path = base.join(random_hex()?);
        DirBuilder::new().mode(0o700).create(&path)?;
        fs::set_permissions(&path, Permissions::from_mode(0o700))?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes a new file with mode 0600. Fails if the file exists. If
    /// writing fails, the partly written file is removed.
    pub fn write(&self, name: &str, bytes: &[u8]) -> Result<PathBuf, Error> {
        if name.is_empty() || name == "." || name == ".." || name.contains('/') {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "bad output file name").into());
        }
        let path = self.path.join(name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        let written = file
            .set_permissions(Permissions::from_mode(0o600))
            .and_then(|()| file.write_all(bytes))
            .and_then(|()| file.sync_all());
        if let Err(e) = written {
            let _ = fs::remove_file(&path);
            return Err(e.into());
        }
        Ok(path)
    }

    /// Deletes the folder and everything in it.
    pub fn remove(self) -> Result<(), Error> {
        fs::remove_dir_all(&self.path)?;
        Ok(())
    }
}

/// The neutral output name: `image.jpg` for a single file, `image-1.jpg`,
/// `image-2.jpg` and so on in a batch. `position` starts at 1.
pub fn neutral_name(position: usize, total: usize, extension: &str) -> String {
    if total <= 1 {
        format!("image.{extension}")
    } else {
        format!("image-{position}.{extension}")
    }
}

/// Deletes folders under `base` whose name is 32 hex digits and that were
/// last changed more than `max_age` ago. Best effort: errors are ignored.
/// Returns how many folders were deleted.
pub fn remove_stale(base: &Path, max_age: Duration) -> usize {
    let Ok(entries) = fs::read_dir(base) else {
        return 0;
    };
    let now = SystemTime::now();
    let mut removed = 0;
    for entry in entries.flatten() {
        let is_ours = entry.file_name().to_str().is_some_and(is_hex_name);
        // DirEntry::metadata does not follow symlinks.
        let Ok(meta) = entry.metadata() else { continue };
        let old = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age > max_age);
        if is_ours && meta.is_dir() && old && fs::remove_dir_all(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

fn runtime_dir(var: Option<OsString>) -> Result<PathBuf, Error> {
    match var.map(PathBuf::from) {
        Some(p) if p.is_absolute() => Ok(p),
        _ => Err(Error::NoRuntimeDir),
    }
}

/// Creates or checks `runtime_dir/safe-upload`: a real folder (not a
/// symlink), owned by the owner of `runtime_dir`, mode 0700.
fn ensure_base(runtime_dir: &Path) -> Result<PathBuf, Error> {
    let owner = fs::metadata(runtime_dir)
        .map_err(|_| Error::NoRuntimeDir)?
        .uid();
    let base = runtime_dir.join(BASE_NAME);
    match DirBuilder::new().mode(0o700).create(&base) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    let meta = fs::symlink_metadata(&base)?;
    if !meta.is_dir() || meta.uid() != owner {
        return Err(Error::UnsafeTempDir);
    }
    // Open the folder, then make sure it is the one checked above (same
    // device and inode), so a symlink swapped in between is not followed.
    let dir = File::open(&base)?;
    let opened = dir.metadata()?;
    if opened.dev() != meta.dev() || opened.ino() != meta.ino() {
        return Err(Error::UnsafeTempDir);
    }
    dir.set_permissions(Permissions::from_mode(0o700))?;
    Ok(base)
}

fn random_hex() -> Result<String, Error> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn is_hex_name(name: &str) -> bool {
    name.len() == 32 && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_names() {
        assert_eq!(neutral_name(1, 1, "jpg"), "image.jpg");
        assert_eq!(neutral_name(1, 3, "jpg"), "image-1.jpg");
        assert_eq!(neutral_name(3, 3, "png"), "image-3.png");
    }

    #[test]
    fn runtime_dir_must_be_set_and_absolute() {
        assert!(matches!(runtime_dir(None), Err(Error::NoRuntimeDir)));
        assert!(matches!(
            runtime_dir(Some("".into())),
            Err(Error::NoRuntimeDir)
        ));
        assert!(matches!(
            runtime_dir(Some("run".into())),
            Err(Error::NoRuntimeDir)
        ));
        assert_eq!(
            runtime_dir(Some("/run/user/1000".into())).unwrap(),
            PathBuf::from("/run/user/1000")
        );
    }

    #[test]
    fn hex_names() {
        assert!(is_hex_name("0123456789abcdef0123456789abcdef"));
        assert!(!is_hex_name("0123456789ABCDEF0123456789ABCDEF"));
        assert!(!is_hex_name("0123456789abcdef"));
        assert!(!is_hex_name("0123456789abcdef0123456789abcdeg"));
    }
}
