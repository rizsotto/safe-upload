//! Headless GTK test harness (M2 plan, "Testing GTK headless").
//!
//! Used by test binaries with `harness = false`. `Headless::start()` must run
//! at the top of `main()`, before any other thread exists, because it sets
//! environment variables. It gives the tests a private `XDG_RUNTIME_DIR`,
//! `HOME` and XDG dirs, a private D-Bus session bus and a headless weston,
//! so tests never touch the real session, home or dconf. A missing tool is a
//! test failure, never a skip.

use std::env;
use std::ffi::OsStr;
use std::fs::{self, DirBuilder, File};
use std::io::ErrorKind;
use std::os::unix::fs::DirBuilderExt;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);

/// Finds `name` in the folders of `path` (a `PATH`-style list).
pub fn require_program(name: &str, path: Option<&OsStr>) -> Result<PathBuf, String> {
    path.into_iter()
        .flat_map(env::split_paths)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
        .ok_or_else(|| {
            format!(
                "headless UI tests need `{name}`, but it is not in PATH. \
                 Add it to .devcontainer/Containerfile; UI tests never skip."
            )
        })
}

pub struct Headless {
    root: PathBuf,
    schema_dir: PathBuf,
    children: Vec<Child>,
}

impl Headless {
    /// Sets up the private environment and starts D-Bus and weston.
    /// Panics with a clear message if a tool is missing or does not start.
    pub fn start() -> Headless {
        let path = env::var_os("PATH");
        let dbus =
            require_program("dbus-daemon", path.as_deref()).unwrap_or_else(|e| panic!("{e}"));
        let weston = require_program("weston", path.as_deref()).unwrap_or_else(|e| panic!("{e}"));

        let root = temp_root();
        let runtime = private_dir(&root, "run");
        let home = private_dir(&root, "home");
        let schema_dir = private_dir(&root, "schemas");
        let mut h = Headless {
            root,
            schema_dir,
            children: Vec::new(),
        };

        let bus = runtime.join("bus");
        let socket = "wayland-test";
        // SAFETY: called at the start of main, before any thread exists.
        unsafe {
            env::set_var("XDG_RUNTIME_DIR", &runtime);
            env::set_var("HOME", &home);
            env::set_var("XDG_CONFIG_HOME", home.join(".config"));
            env::set_var("XDG_DATA_HOME", home.join(".local/share"));
            env::set_var("XDG_CACHE_HOME", home.join(".cache"));
            env::set_var("XDG_STATE_HOME", home.join(".local/state"));
            env::set_var(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}", bus.display()),
            );
            env::set_var("WAYLAND_DISPLAY", socket);
            env::set_var("GDK_BACKEND", "wayland");
            env::set_var("GSETTINGS_BACKEND", "memory");
            // No GPU here; skip the Vulkan and GL probes.
            env::set_var("GSK_RENDERER", "cairo");
            env::set_var("GSETTINGS_SCHEMA_DIR", &h.schema_dir);
            env::remove_var("DISPLAY");
            env::remove_var("GTK_A11Y");
        }

        h.spawn(
            Command::new(dbus)
                .args(["--session", "--nofork", "--nopidfile"])
                .arg(format!("--address=unix:path={}", bus.display())),
            "dbus-daemon",
            &bus,
        );
        h.spawn(
            Command::new(weston).args([
                "--backend=headless",
                "--no-config",
                "--idle-time=0",
                &format!("--socket={socket}"),
            ]),
            "weston",
            &runtime.join(socket),
        );
        h
    }

    /// Compiles the `*.gschema.xml` files in `src` into the private
    /// `GSETTINGS_SCHEMA_DIR`.
    #[allow(dead_code)]
    pub fn compile_schemas(&self, src: &Path) {
        for entry in fs::read_dir(src).expect("read schema folder") {
            let p = entry.expect("schema folder entry").path();
            if p.to_string_lossy().ends_with(".gschema.xml") {
                fs::copy(&p, self.schema_dir.join(p.file_name().unwrap())).expect("copy schema");
            }
        }
        let status = Command::new("glib-compile-schemas")
            .arg("--strict")
            .arg(&self.schema_dir)
            .status()
            .expect("run glib-compile-schemas (install glib2-devel)");
        assert!(status.success(), "glib-compile-schemas failed");
    }

    fn spawn(&mut self, cmd: &mut Command, name: &str, ready: &Path) {
        let log = self.root.join(format!("{name}.log"));
        let out = File::create(&log).expect("create log file");
        let err = out.try_clone().expect("clone log file");
        let child = cmd
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err)
            .spawn()
            .unwrap_or_else(|e| panic!("cannot start {name}: {e}"));
        self.children.push(child);
        let start = Instant::now();
        while !ready.exists() {
            let exited = self.children.last_mut().unwrap().try_wait().ok().flatten();
            if exited.is_some() || start.elapsed() > STARTUP_TIMEOUT {
                let text = fs::read_to_string(&log).unwrap_or_default();
                panic!("{name} did not start ({exited:?}). Its output:\n{text}");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Headless {
    fn drop(&mut self) {
        for child in self.children.iter_mut().rev() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn temp_root() -> PathBuf {
    let tmp = env::temp_dir();
    for n in 0u32.. {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let dir = tmp.join(format!("safe-upload-ui-{}-{nanos}-{n}", std::process::id()));
        match DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => return dir,
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => panic!("cannot create {}: {e}", dir.display()),
        }
    }
    unreachable!()
}

fn private_dir(parent: &Path, name: &str) -> PathBuf {
    let dir = parent.join(name);
    DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .unwrap_or_else(|e| panic!("cannot create {}: {e}", dir.display()));
    dir
}

pub type Test = (&'static str, fn());

/// Runs each test whose name contains every non-flag argument, prints
/// `ok`/`FAILED` like libtest, and fails if any test failed.
pub fn run(tests: &[Test]) -> ExitCode {
    let filters: Vec<String> = env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .collect();
    let selected: Vec<&Test> = tests
        .iter()
        .filter(|(name, _)| filters.iter().all(|f| name.contains(f.as_str())))
        .collect();
    println!("\nrunning {} tests", selected.len());
    let mut failed = Vec::new();
    for (name, test) in selected.iter().copied() {
        let result = panic::catch_unwind(AssertUnwindSafe(test));
        println!(
            "test {name} ... {}",
            if result.is_ok() { "ok" } else { "FAILED" }
        );
        if result.is_err() {
            failed.push(*name);
        }
    }
    let passed = selected.len() - failed.len();
    if failed.is_empty() {
        println!("\ntest result: ok. {passed} passed; 0 failed\n");
        ExitCode::SUCCESS
    } else {
        println!("\nfailures:\n    {}", failed.join("\n    "));
        println!(
            "\ntest result: FAILED. {passed} passed; {} failed\n",
            failed.len()
        );
        ExitCode::FAILURE
    }
}
