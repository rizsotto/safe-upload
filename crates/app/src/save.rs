//! Save and copy (M2 plan, "Save" and "Copy to clipboard"). Only the
//! verified output bytes leave the app, and core checks them again just
//! before (S-6). Messages never contain paths (S-5).

use std::collections::HashSet;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use gettextrs::gettext;
use safe_upload_core::Error;
use safe_upload_core::verify::verify;

/// One output to save: its file name and its verified bytes.
#[derive(Debug, Clone)]
pub struct Named {
    pub name: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub enum SaveError {
    /// Not every image is ready, or there is nothing to save.
    NotReady,
    /// A file with this name is already in the chosen folder.
    Exists(String),
    /// Two outputs would get the same name.
    SameName(String),
    /// The chosen file is one of the originals.
    Original,
    /// The bytes failed the check, so nothing was written.
    Verify(Error),
    Io(io::Error),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::NotReady => f.write_str(&gettext("Not every image is ready")),
            SaveError::Exists(name) => {
                f.write_str(&gettext("{} already exists in that folder").replace("{}", name))
            }
            SaveError::SameName(name) => f.write_str(
                &gettext("Two images would be saved as {}. Turn off Keep names")
                    .replace("{}", name),
            ),
            SaveError::Original => f.write_str(&gettext(
                "That file is an original image. Choose another name",
            )),
            SaveError::Verify(_) => f.write_str(&gettext(
                "The output failed the metadata check. Nothing was written",
            )),
            SaveError::Io(e) => write!(f, "{}: {e}", gettext("Cannot write the file")),
        }
    }
}

impl From<io::Error> for SaveError {
    fn from(e: io::Error) -> Self {
        SaveError::Io(e)
    }
}

/// The MIME type of verified output bytes.
pub fn mime_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xff, 0xd8]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else {
        None
    }
}

/// Verifies the bytes again (no ICC profile is ever written) and returns
/// their MIME type.
pub fn check(bytes: &[u8]) -> Result<&'static str, SaveError> {
    verify(bytes, None).map_err(SaveError::Verify)?;
    mime_type(bytes).ok_or(SaveError::Verify(Error::Verify("unknown format".into())))
}

/// Writes one output to the file the user chose in the save dialog. The
/// dialog already asked before replacing a file; an original is never
/// replaced.
pub fn save_file(path: &Path, bytes: &[u8], originals: &[PathBuf]) -> Result<(), SaveError> {
    check(bytes)?;
    if is_original(path, originals) {
        return Err(SaveError::Original);
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    if let Err(e) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(path);
        return Err(e.into());
    }
    Ok(())
}

/// Writes every output into `folder`, all or nothing. Never replaces a
/// file: if one name is taken, nothing is written. Returns the count.
pub fn save_batch(folder: &Path, outputs: &[Named]) -> Result<usize, SaveError> {
    if outputs.is_empty() {
        return Err(SaveError::NotReady);
    }
    let mut names = HashSet::new();
    for out in outputs {
        check(&out.bytes)?;
        if !names.insert(out.name.as_str()) {
            return Err(SaveError::SameName(out.name.clone()));
        }
        if fs::symlink_metadata(folder.join(&out.name)).is_ok() {
            return Err(SaveError::Exists(out.name.clone()));
        }
    }
    let mut written = Vec::new();
    for out in outputs {
        let path = folder.join(&out.name);
        if let Err(e) = write_new(&path, &out.bytes) {
            for p in written {
                let _ = fs::remove_file(p);
            }
            return Err(match e.kind() {
                io::ErrorKind::AlreadyExists => SaveError::Exists(out.name.clone()),
                _ => e.into(),
            });
        }
        written.push(path);
    }
    Ok(written.len())
}

fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    if written.is_err() {
        let _ = fs::remove_file(path);
    }
    written
}

/// True if `path` is the same file as one of the originals (also through a
/// symlink or a hard link).
fn is_original(path: &Path, originals: &[PathBuf]) -> bool {
    let Ok(target) = fs::metadata(path) else {
        return false;
    };
    originals
        .iter()
        .any(|o| fs::metadata(o).is_ok_and(|m| m.dev() == target.dev() && m.ino() == target.ino()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_types() {
        assert_eq!(mime_type(&[0xff, 0xd8, 0xff]), Some("image/jpeg"));
        assert_eq!(mime_type(b"\x89PNG\r\n\x1a\n...."), Some("image/png"));
        assert_eq!(mime_type(b"GIF89a"), None);
    }

    #[test]
    fn unverified_bytes_are_never_written() {
        let dir = tempfile::tempdir().unwrap();
        let bad = vec![Named {
            name: "image.jpg".into(),
            bytes: vec![0xff, 0xd8, 0xff, 0xe1, 0x00, 0x04, b'E', b'x'],
        }];
        assert!(matches!(
            save_batch(dir.path(), &bad),
            Err(SaveError::Verify(_))
        ));
        let file = dir.path().join("one.jpg");
        assert!(matches!(
            save_file(&file, &bad[0].bytes, &[]),
            Err(SaveError::Verify(_))
        ));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn messages_have_no_paths() {
        let e = SaveError::Exists("image-1.jpg".into());
        assert_eq!(e.to_string(), "image-1.jpg already exists in that folder");
    }
}
