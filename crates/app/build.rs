//! Compiles `data/*.gschema.xml` into `$OUT_DIR/schemas`, so a dev build
//! finds its settings schema without an install (see `settings::schema`).
//! An installed build (Flatpak) uses the installed schema instead.

use std::path::PathBuf;
use std::process::Command;
use std::{env, fs, io};

fn main() {
    let data = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../../data");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("schemas");
    println!("cargo:rerun-if-changed={}", data.display());

    fs::create_dir_all(&out).expect("create schema dir");
    for entry in fs::read_dir(&data).expect("read data/") {
        let path = entry.expect("data/ entry").path();
        if path.to_string_lossy().ends_with(".gschema.xml") {
            fs::copy(&path, out.join(path.file_name().unwrap())).expect("copy schema");
        }
    }
    match Command::new("glib-compile-schemas")
        .arg("--strict")
        .arg(&out)
        .status()
    {
        Ok(s) if s.success() => {}
        Ok(s) => panic!("glib-compile-schemas failed ({s}) on data/*.gschema.xml"),
        Err(e) if e.kind() == io::ErrorKind::NotFound => panic!(
            "glib-compile-schemas not found; install it (Fedora: glib2-devel, \
             Debian: libglib2.0-bin) or build in the devcontainer"
        ),
        Err(e) => panic!("cannot run glib-compile-schemas: {e}"),
    }
    println!(
        "cargo:rustc-env=SAFE_UPLOAD_DEV_SCHEMA_DIR={}",
        out.display()
    );
}
