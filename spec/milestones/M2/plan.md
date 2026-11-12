# M2 plan: Standalone app

Status: draft. Review before tasks start. Criteria: [spec.md](spec.md).
Tasks: [tasks.md](tasks.md).

## Shape

```
safe-upload-app  ──►  safe-upload-editor-widget  ──►  safe-upload-core
      │                       (GTK4)                     (no GTK, no bus)
      └──────────────────────────────────────────────►  safe-upload-core
safe-upload-cli  ──►  safe-upload-core
```

- `safe-upload-core`: unchanged in spirit. M2 adds HEIC decode, the
  Flatpak sandbox selector, a managed glycin main context, a preview entry
  point, HEIC in the summary, and blur (if 16.4 keeps it). No fit changes:
  lane `m2/fit-predict` owns `fit.rs`. M2 tasks only call `prepare`,
  `prepare_with` and the public types.
- `safe-upload-editor-widget`: a reusable GTK4 widget plus a pure Rust edit
  model. Depends on `gtk4` and on core for `Rect`, `Redaction` and
  `edit::apply` (used for the preview). No libadwaita, so other apps can use
  it.
- `safe-upload-app` (binary `safe-upload-app` for now; renamed with 16.1):
  libadwaita UI, settings, threading, save, copy.
- `safe-upload-portal`: stays a stub (M3).

The app calls core as a library, in process. No bus, no helper process.
glycin's own peer-to-peer channel to its loaders is the only IPC (SPEC 14,
16.8).

## Versions (checked 2026-11-12, see "Facts verified")

| Thing | Fedora 45 (devcontainer) | GNOME 51 runtime (Flatpak) |
|---|---|---|
| GTK | 4.24.1 | 4.24.1 |
| libadwaita | 1.10.0 | 1.10.0 |
| glycin loaders | 2.2.6 | glycin-heif 2.2.0 |
| glib | 2.90.1 | (runtime) |

Crates: `gtk4` 0.11.5 (features up to `v4_24`), `libadwaita` 0.9.2 (up to
`v1_10`), both on `glib` 0.22, the same `glib` the `glycin` 4.0.5 crate
uses, so there is one glib in the tree. Enable only the features we use, and
never above the runtime (4.24 / 1.10).

## Core changes in M2 (small, each its own task)

1. **HEIC decode** (replaces the M1 refusal). `sniff` no longer refuses
   HEIF brands. `decode::check_complete` accepts HEIF (`ftyp` with a HEIF
   brand) with the existing top-level box check (`isobmff_complete`).
   Detection stays by content. The header-only 24 byte fixture now gives
   `Error::Decode` (exit 1), not exit 3.
2. **Decoder missing.** glycin has no typed error for this. A missing HEVC
   plugin comes back from `next_frame()` (not `load()`) as a remote
   `LoadingError` whose text contains `NoMatchingDecoderInstalled`. Core
   maps that text to a new variant `Error::MissingDecoder(Format::Heif)`,
   message: "This HEIC image needs an HEVC decoder, which is not installed.
   On Fedora, install libheif-freeworld from RPM Fusion. In the Flatpak,
   install the org.freedesktop.Platform.codecs-extra extension." The CLI
   maps it to exit 3, the app shows it on the row. The text match is
   fragile, so an end-to-end test (below) guards it. `Format` keeps
   `Heif`; `UnsupportedFormat` stays for future refusals.
3. **Sandbox selector.** `GlycinDecoder` picks the mechanism at runtime:
   `FlatpakSpawn` if `/.flatpak-info` exists, else `Bwrap`. Never `Auto`,
   never `NotSandboxed`. After `load()` it checks
   `active_sandbox_mechanism()` equals the chosen one, else
   `Error::Decode("the decoder did not run in the sandbox")` (fail closed,
   as in M1). Detection is a function of a path so a unit test can feed a
   fake `/.flatpak-info`. This is feature detection (D-2), not desktop
   detection.
4. **Main context.** The loader sets `MainContextSelector::Managed`. With
   the default `Auto`, glycin running in a worker thread uses the global
   default main context when it has a running loop, which in the app is
   the GTK main loop (glycin-core 4.0.5 `src/main_context.rs`). That would
   tie decoding to UI responsiveness and could deadlock if the main thread
   ever waited on a worker. `Managed` gives glycin its own loop thread. The
   CLI is unaffected.
5. **Preview.** `pipeline::preview(input, limits, max_edge) ->
   Result<Preview, Error>` runs steps 1, 2 and 5 of SPEC 9.1 (decode in
   the sandbox, orient, convert to sRGB) and returns the oriented sRGB
   pixels downscaled to `max_edge` plus the full oriented size. The editor
   shows this. It never encodes and never writes a file. Coordinates for
   edits are in full oriented pixels (SPEC 9.4); the widget scales.
   Decoding happens twice (preview, then `prepare`). That costs one extra
   sandboxed decode per image; acceptable for M2, revisit with NFR-1.
6. **HEIC in the summary.** `summary::scan` learns HEIF: walk `meta` /
   `iinf` for `Exif` items and `mime` items with XMP, report Location
   (GPS IFD present), Camera, Date, XMP. Otherwise HEIC rows say "removed:
   all metadata" (FR-8 still holds: sizes and resolutions are shown).
7. **Blur** (task T15, waits on 16.4). `Redaction::Blur { rect }`, Gaussian,
   radius at least 1/20 of the region's short edge (SPEC 9.4), with
   `image::imageops::blur` on the clipped region.

All of these touch `decode.rs`, `sniff.rs`, `error.rs`, `summary.rs`,
`edit.rs` and a new function in `pipeline.rs`. None touch `fit.rs`. If the
fit lane merges first and also touched `pipeline.rs`, rebase; the preview is
a new function, so conflicts stay small.

## App

### Window (libadwaita, SPEC 6.2)

- `adw::ApplicationWindow` with an `adw::ToolbarView`: header bar with
  "Open…" (`Ctrl+O`), a primary menu (Preferences is not needed in M2;
  settings live in the side panel), and the main content.
- `adw::OverlaySplitView`: sidebar = settings; content = image list.
- Empty state: `adw::StatusPage` "Drop images here" with an "Open…" button.
- Image list: `gtk::ListBox` (boxed list style) of `adw::ActionRow`s, one
  per image: thumbnail, neutral output name, summary subtitle ("Location,
  Camera removed · 4.1 MB → 1.9 MB · 4032×3024 → 4032×3024"), a spinner
  while working, an error label on failure, buttons "Edit…" and "Remove".
- Bottom bar: "Copy to clipboard" (enabled for exactly one ready image,
  `Ctrl+Shift+C`) and "Save to…" (`Ctrl+S`, enabled when every image is
  ready). A failed image blocks Save until it is removed or fixed; this is
  simpler and safer than saving part of a batch silently.
- Toasts (`adw::ToastOverlay`) for "Saved 3 images", "Copied".

### Model and threading

- `ImageItem` (GObject subclass, so rows bind to properties): input path,
  position, `Edits` (crop + redactions), `state` = Loading | Ready(bytes,
  summary, ext) | Failed(message).
- Every change (images added, setting changed, edits changed) schedules a
  prepare per affected item, debounced 300 ms. Each item has a generation
  counter; a result whose generation is stale is dropped.
- Work runs with `gio::spawn_blocking(move || work())` and is awaited in
  `glib::spawn_future_local`, so the main thread never blocks. One item at
  a time per batch (a small queue): fit already runs encodes in parallel,
  and two 12 MP images at once would double peak memory.
- `work()` = `PrivateDir::create()`, `core::prepare(path, opts, target)`,
  read the output bytes back, `PrivateDir::remove()`. The item keeps the
  verified bytes in memory. The private folder exists only during one
  prepare (FR-7, S-1 hold: core writes and verifies there).
- Core is not cancellable. Removing an item or closing the window drops the
  result when it arrives.
- S-2: on startup, `output::remove_stale(base, MAX_AGE)`; then every 10
  minutes with `glib::timeout_add_seconds_local`. Our own folders live only
  for one prepare, so the timer never removes a folder in use.

### Settings (GSettings)

- Schema `org.example.SafeUpload` (placeholder, see "App ID" below) in
  `data/org.example.SafeUpload.gschema.xml`:

  | Key | Type | Default |
  |---|---|---|
  | `max-size` | `t` (bytes) | `5000000` (SPEC 16.3: 5 MB) |
  | `max-long-edge` | `u` (0 = none) | `0` |
  | `output-format` | enum `auto`, `jpeg`, `png` | `auto` |
  | `keep-names` | `b` | `false` |
  | `window-width`, `window-height`, `is-maximized` | | GNOME norm |

- UI: `adw::SpinRow` for max size in MB (one decimal, 0.1 to 100) and long
  edge (0 = off), `adw::ComboRow` for format, `adw::SwitchRow` for keep
  names. Bound with `gio::Settings::bind`.
- No WebP in the menu: core refuses WebP output in this version.
- Tests: `GSETTINGS_BACKEND=memory` and `GSETTINGS_SCHEMA_DIR` pointing at a
  temp dir where the test harness runs `glib-compile-schemas` on `data/`.
  Never the real dconf or home (CLAUDE.md).
- In the Flatpak, GSettings uses the keyfile backend in the app's data dir.
  No extra permission.

### App ID (SPEC 16.1 open)

- Placeholder: `org.example.SafeUpload`. Clearly fake, valid reverse DNS,
  no claim on a real domain or name.
- It appears in: `crates/app/src/config.rs` (`APP_ID`, the only Rust
  place), `data/` file names (`.desktop`, `.metainfo.xml`, `.gschema.xml`,
  icon), and the Flatpak manifest file name and `id`.
- `build-aux/ci/check-app-id.sh` fails if any of those disagree, so the
  rename (task T19, after 16.1) is a search and replace plus file moves.
- Never end the ID in `Devel` for the tested build: glycin turns its sandbox
  off for `Devel` IDs under `flatpak-builder --run` (glycin `SandboxSelector`
  docs). We always test the installed bundle anyway.

### Open, drag and drop

- `gtk::DropTarget` for `gdk::FileList` on the window (copy action). GTK4
  handles the file transfer portal (`application/vnd.portal.filetransfer`)
  itself, so drops from the host into the Flatpak arrive as readable
  document portal paths. Folders and non-files are ignored with a toast.
- "Open…": `gtk::FileDialog::open_multiple` with an image filter (by MIME:
  JPEG, PNG, WebP, AVIF, HEIC/HEIF). The filter is a convenience; core still
  checks content.
- Non-images reach core and fail with `Error::Decode`; the row shows "Not a
  supported image".

### Save

- One image: `gtk::FileDialog::save` with the neutral name (or kept name)
  as initial name. The dialog asks before overwriting, which is the user's
  choice.
- A batch: `gtk::FileDialog::select_folder`, then write each file into it.
  Never overwrite: if `image-1.jpg` exists, the app writes nothing and
  shows "image-1.jpg already exists in that folder" (same all-or-nothing
  rule as the CLI's `-o` folder). Files are created with `create_new`.
- Before writing, the app runs `core::verify::verify(bytes, None)` on the
  in-memory bytes again (cheap; guards against bugs between prepare and
  save, S-6).
- No initial folder is set, so the dialog does not default to the
  original's folder.
- In the Flatpak this needs no filesystem permission: the portal grants
  the chosen file or folder.

### Copy to clipboard

- Safe default: `gdk::ContentProvider::for_bytes(mime, bytes)` with the
  verified output bytes and their real MIME type (`image/jpeg` or
  `image/png`). Not a `gdk::Texture`: GTK would serialize a texture as PNG,
  which can be far above the target size and is not our verified file.
- Only for a single image (SPEC 6.2).
- The clipboard content lives while the app runs (Wayland). Show this in the
  toast: "Copied. Keep Safe Upload open until you paste."
- Not verified: which targets accept `image/jpeg` paste (Firefox, Chromium).
  If one only takes `image/png`, offering a PNG or a file URI is a later
  choice, deferred in `spec/decisions/README.md` (needs no answer for M2:
  copy works and keeps the size and verify guarantees).

### Strings (NFR-5)

User-visible strings go through `gettext-rs` (`gettext-system` feature, the
system libintl, no bundled C build) from the first UI task. No `po/` until
M4.

### Logging (S-5)

No `log`/`tracing` subscriber is installed by the app, as in M1. Errors on
rows use core's `Display`, which never has paths or metadata values.

## Redaction editor widget (`safe-upload-editor-widget`)

### Edit model (`model.rs`, pure Rust, no GTK)

```rust
pub enum Tool { Box, Pixelate, Blur, Crop }
pub struct Area { pub kind: AreaKind, pub rect: Rect }   // full oriented px
pub enum AreaKind { Box, Pixelate, Blur }
pub struct EditModel {
    size: (u32, u32),            // full oriented size
    areas: Vec<Area>,
    crop: Option<Rect>,
    selected: Option<usize>,
    undo: Vec<Snapshot>, redo: Vec<Snapshot>,
}
impl EditModel {
    pub fn add(&mut self, kind: AreaKind, r: Rect);
    pub fn move_by(&mut self, i: usize, dx: i32, dy: i32);
    pub fn resize_by(&mut self, i: usize, dw: i32, dh: i32);
    pub fn delete(&mut self, i: usize);
    pub fn set_crop(&mut self, r: Option<Rect>);
    pub fn undo(&mut self) -> bool;
    pub fn redo(&mut self) -> bool;
    pub fn to_options(&self) -> (Option<Rect>, Vec<Redaction>);
}
```

- Undo stores whole snapshots (a few rectangles; cheap and obviously
  right). Any new edit clears redo. A drag or a held arrow key is one undo
  step (coalesced until release).
- Rectangles are clamped to the image and have a minimum size of 4x4 px.
- Redaction rectangles are in coordinates before the crop, as
  `core::Options` expects. The crop is a separate rectangle, not an area.
- `ViewTransform { scale, offset }` maps widget points to image pixels and
  back, with rounding that never shrinks a box (floor the start, ceil the
  end), so the burned-in area always covers what the user saw.
- Unit tests cover all of this without a display.

### Widget (`RedactionEditor`, a `gtk::Widget` subclass)

- Input: the `Preview` from core (downscaled oriented sRGB pixels plus full
  size) and an `EditModel`. Output: a `changed` signal; the app reads
  `to_options()`.
- Draws in `snapshot()`: a `gdk::MemoryTexture` of the preview with box,
  pixelate and blur previews applied by `core::edit::apply` on the preview
  pixels (scaled rectangles), so what you see is what core does (at preview
  scale). Areas get an outline; the selected one gets handles. Outside the
  crop is dimmed.
- Mouse: `gtk::GestureDrag` draws a new area with the current tool, or moves
  the selected area when the drag starts inside it, or resizes from a
  handle. Click selects. A press that moves less than 4 points is a
  click: it selects the topmost area under it or clears the selection,
  and never draws (so a click on or inside a pixel behaves the same). A
  real drag always draws, at least 1 px per side before the 4 px minimum.
- On screen, each area shows the core-redacted preview clipped to the
  exact model rect (preview rects round outward, the clip cuts them back),
  so an area is never drawn smaller or larger than in the model.
- Pixelate on the preview uses core's full-size block scaled to the
  preview (`edit::apply_preview`), so it looks as coarse as the output,
  with noise seeded per area so it does not flicker. The output keeps
  fresh random noise.
- The description updates after discrete actions and when a drag or held
  arrow ends, not on every step.
- Toolbar (in the widget, so it is reusable): linked `gtk::ToggleButton`s
  Box, Pixelate, Blur, Crop with visible text labels (no icons), text
  tooltips that name the shortcut, and accessible labels; Undo, Redo,
  Delete buttons.
- Warning: when Pixelate or Blur is the tool, or a pixelate/blur area is
  selected, an inline label (role `alert` is not needed; a visible label
  with `gtk::AccessibleRelation::DescribedBy` from the tool buttons) shows
  "Not safe for text. Use Box for numbers and names." (SPEC 6.3).
- Actions on the widget's action group `editor`: `editor.tool(s)`,
  `editor.undo`, `editor.redo`, `editor.delete`, `editor.next-area`,
  `editor.new-area`, `editor.commit`, `editor.cancel`.
- Shortcuts (`gtk::ShortcutController`, scope local to the editor):
  `B`, `P`, `U`, `C` tools; `Ctrl+Z` undo; `Ctrl+Shift+Z` and `Ctrl+Y` redo;
  `Delete`/`BackSpace` delete selected.
- Keyboard-only drawing (NFR-4): `Enter` on the canvas with no selection
  adds a new area of the current tool in the center (a quarter of the
  view). Arrow keys move the selected area (10 px of the view, `Alt` for 1
  px), `Shift`+arrows resize, `Tab`/`Shift+Tab` (inside the canvas) step
  through the areas once, then leave the canvas (no wrap), `Escape` deselects, then leaves the canvas focus. For Crop, the
  same keys edit the crop rectangle.
- Accessibility: the canvas is focusable, role `Img`, label "Image to
  redact", and a description kept up to date: "3 areas: 2 boxes, 1
  pixelate. Selected: box 2, 120 by 40 pixels at 300, 210." Every tool
  button has a label.

### Editor dialog in the app

`adw::Dialog` (or a pushed `adw::NavigationPage`) holding the widget, with
"Done" and "Cancel". "Done" stores the edits on the item and re-prepares it.
The preview is fetched with `core::pipeline::preview` off the main thread
before the dialog opens (spinner on the row meanwhile), max edge 2048 px.

## Flatpak (`build-aux/flatpak/org.example.SafeUpload.json`)

```json
{
  "id": "org.example.SafeUpload",
  "runtime": "org.gnome.Platform", "runtime-version": "51",
  "sdk": "org.gnome.Sdk",
  "sdk-extensions": ["org.freedesktop.Sdk.Extension.rust-stable"],
  "command": "safe-upload-app",
  "finish-args": [
    "--socket=wayland", "--socket=fallback-x11", "--share=ipc",
    "--device=dri"
  ],
  "build-options": { "append-path": "/usr/lib/sdk/rust-stable/bin",
                     "env": { "CARGO_HOME": "/run/build/safe-upload/cargo" } },
  "modules": [{
    "name": "safe-upload", "buildsystem": "simple",
    "build-commands": [
      "cargo --offline build --release --locked -p safe-upload-app -p safe-upload-cli",
      "install -Dm755 target/release/safe-upload-app -t /app/bin",
      "install -Dm755 target/release/safe-upload -t /app/bin",
      "... data files: desktop, metainfo, gschema + glib-compile-schemas, icon"
    ],
    "sources": [{ "type": "dir", "path": "../.." }, "cargo-sources.json"]
  }]
}
```

- No `--share=network`, no `--filesystem=*`, no `--talk-name`. The glycin
  `flatpak-spawn --sandbox` call goes to `org.freedesktop.portal.Flatpak`,
  which every Flatpak may talk to (verified: it worked with only
  `--socket=wayland`).
- glycin loaders come from the GNOME runtime; HEVC from the
  `org.freedesktop.Platform.codecs-extra` extension, installed with the
  runtime from Flathub. We bundle no codec (decision 0001).
- The CLI is shipped in the Flatpak too (`flatpak run
  --command=safe-upload ...`). It costs little and it is how CI tests the
  Flatpak HEIC path without a display.
- Offline build: `cargo-sources.json` from flathub's
  `flatpak-cargo-generator.py`, regenerated by
  `build-aux/flatpak/update-cargo-sources.sh`. CI fails if it does not
  match `Cargo.lock`.
- `build-aux/ci/check-flatpak-permissions.sh`: fails if the manifest or the
  built app's `metadata` (`flatpak build-finish` output, `[Context]
  shared=`) has `network`. Self-tested like the S-3 script.

### CI job `flatpak`

A second job in `ci.yml`, in the `ghcr.io/flathub-infra/flatpak-github-actions:gnome-51`
container (`--privileged`, as the flathub action needs), with
`flatpak/flatpak-github-actions/flatpak-builder` to build a bundle. Then:

1. `check-flatpak-permissions.sh` on the manifest and the build dir.
2. Install the bundle `--user` (glycin's `FlatpakSpawn` needs an installed
   app, not `flatpak-builder --run`).
3. `build-aux/flatpak/test-heic.sh`: the HEIC fixtures come from the main
   job (it runs `make.sh` with `heif-enc -e x265` and uploads
   `target/fixtures` as an artifact; the flathub container has no x265).
   The script runs `dbus-run-session -- flatpak run --command=safe-upload
   org.example.SafeUpload heic-gps.heic --max-size 5MB -o gps.jpg` and the
   same for `heic-rot90.heic`, expects exit 0, and uploads the outputs.
   The D-Bus session autostarts `flatpak-portal` (verified in a podman
   spike).
4. A small third job `flatpak-check` in the devcontainer image (it has
   exiftool and ImageMagick) runs `build-aux/ci/check-heic-outputs.sh` on
   those outputs: JPEG, size under 5 MB, exiftool allow list (same list as
   `leak.rs`), `rot90` output is 480x640 with red top-right. A script, not
   a cargo test, so `cargo test` never depends on another job's files.

The flatpak job needs the GNOME 51 SDK and the rust extension (about 1.4 GB
download). Cache `.flatpak-builder` with the action's cache key.

Not verified: this job on a GitHub runner. Verified in rootless podman with
`--security-opt seccomp=unconfined --security-opt unmask=/proc/*`. If the
flathub container cannot run `flatpak run` on the runner, task T13 records
it and opens a decision (a skipped test is not allowed).

## HEIC in dev and CI (decision 0001, option A)

- `.devcontainer/Containerfile`: add RPM Fusion free, then
  `libheif-freeworld libheif-tools`.
- `tests/fixtures/make.sh` adds:
  - `heic-gps.heic`: 640x480 gradient, `heif-enc -e x265 -q 60`, GPS,
    Make, Model via exiftool (decision 0001 recipe).
  - `heic-rot90.heic`: a source with a red top-left quadrant on blue,
    `heif-enc -e x265 --rotate-cw 90` (writes an `irot` box; `heif-info`
    shows 480x640, exiftool `Rotation: Rotate 90 CW`), plus GPS and EXIF
    `Orientation=6`. The EXIF tag catches double rotation: glycin ignores it
    for HEIF and applies `irot` in the loader (verified), so the correct
    output is 480x640 with red top-right.
- `check.sh` confirms GPS, the `irot` rotation and the EXIF orientation.
- Tests (core, CLI, leak) treat both like other fixtures: decoded in bwrap,
  output upright, exiftool allow list.

### Decoder missing, end to end

`unshare -rm sh -c 'mount -t tmpfs none /usr/lib64/libheif && <cmd>'`
hides every libheif plugin in a private mount namespace. glycin's bwrap
still runs inside it, PNG still decodes, and HEIC fails with
`PluginLoadingError(NoMatchingDecoderInstalled)` (verified in the
devcontainer image with default podman seccomp). The CLI test runs
`safe-upload heic-gps.heic --max-size 5MB -o out.jpg` this way and expects
exit 3, the message, no output. The test finds the plugin dir with
`pkg-config --variable=libdir libheif`/`libheif` or a fixed Fedora path
checked at the start (fail, not skip, if missing).

## Testing GTK headless

Verified in the devcontainer image plus GTK packages (spike, see facts):

| Backend | `#[gtk::test]` runs | Window active / `has_focus()` | AT-SPI tree via pyatspi |
|---|---|---|---|
| `weston --backend=headless` | yes | no (no seat focus) | yes |
| `mutter --headless --wayland --virtual-monitor` | yes | no | yes |
| `gtk4-broadwayd` | yes | yes | not checked |
| Xvfb (`xvfb-run`) | yes | yes | not checked |

Choice: **weston headless**, a Wayland backend like real use, small, and it
worked for both GTK tests and AT-SPI. Focus checks use
`GtkWindow::focus()` (the focus widget), not `has_focus()`, which needs an
active window.

How tests run without extra setup (`cargo test --workspace` must just
work):

- GTK tests live in integration test binaries with `harness = false`
  (`crates/app/tests/ui.rs`, `crates/editor-widget/tests/ui.rs`). Their
  `main()` runs before any thread exists, so it can safely: make a private
  `XDG_RUNTIME_DIR` (0700 temp dir), start `dbus-daemon --session` on a
  temp address, start `weston --backend=headless --socket=<random>`, wait
  for the socket, set `WAYLAND_DISPLAY`, `GDK_BACKEND=wayland`,
  `GSETTINGS_BACKEND=memory`, `GSETTINGS_SCHEMA_DIR`, `HOME` to a temp dir,
  `GTK_A11Y` unset (AT-SPI on), then init GTK on the main thread and run
  each test function in turn, printing `ok`/`FAILED` and exiting non-zero
  on failure. Children are killed on exit (a guard struct). A small shared
  helper crate is overkill; the helper lives in
  `crates/editor-widget/tests/common/headless.rs` and the app test includes
  it with `#[path]`.
- If weston is missing, the test fails with a clear message. Never a skip.
- Unit tests of pure logic (edit model, settings mapping, name conflicts)
  stay normal `#[test]`s with no display.

What the UI tests check:

- Widget level: drawing an area via actions, undo/redo, keyboard moves,
  coordinate mapping, warning visibility, `editor.*` shortcuts registered
  (`ShortcutController` model), accessible labels on every tool button
  (`gtk::test_accessible_has_property(.., AccessibleProperty::Label)` or a
  text child).
- App level: add fixture files through the same function the drop target
  calls (`window.add_files(&[gio::File])`; a synthetic drop event is not
  possible on headless weston), wait for rows to be Ready (main loop
  iteration with a timeout), check summaries, change settings and see
  re-prepare, call the save path with a temp folder, check the files are
  the verified bytes and nothing is overwritten, copy and read the
  clipboard back with `gdk::Clipboard::read_future` (same process).
- Keyboard walkthrough: from the window, repeated
  `child_focus(TabForward)` reaches every control in the expected order
  (open, settings rows, each row's buttons, copy, save), and every action
  has an accelerator (`app.accels_for_action`). Real key injection is not
  available on headless weston; actions and focus order cover it. A manual
  keyboard run is recorded at acceptance.
- Orca: an AT-SPI test. The app test binary starts the app window, then
  runs `python3 build-aux/ci/atspi-check.py` (pyatspi) against the same
  D-Bus session. It walks the app's tree and fails on any push button,
  toggle button, check box, combo box, spin button, text entry, menu item
  or list item with an empty name (spike: an icon-only button without a
  label shows up as `button: name=''`). One manual Orca run at acceptance.

Packages this needs in the devcontainer: `gtk4-devel libadwaita-devel
weston dbus-daemon at-spi2-core python3-pyatspi glib2-devel` (for
`glib-compile-schemas`), plus `desktop-file-utils appstream` for data file
validation.

## Data files (`data/`)

- `org.example.SafeUpload.desktop` (validated with `desktop-file-validate`)
- `org.example.SafeUpload.metainfo.xml` (`appstreamcli validate
  --no-net`; `--no-net` because CI must not need network)
- `org.example.SafeUpload.gschema.xml` (`glib-compile-schemas --strict
  --dry-run`)
- `icons/hicolor/scalable/apps/org.example.SafeUpload.svg` (a simple
  placeholder; real icon in M4)

A CI step runs the three validators.

## Accessibility summary (NFR-4)

- Every control is a standard GTK/Adw widget with a visible label, or has
  `accessible::Property::Label`. Icon buttons always get a label and a
  tooltip.
- Mnemonics on main buttons; accelerators listed above; the shortcuts
  window (`Ctrl+?`) lists them.
- Row state changes (Ready, Failed) update the row subtitle, which AT-SPI
  reports; errors are text, not color only.

## Facts verified (SPEC section 15 and M2 open items)

All on 2026-11-12, host Fedora 45, rootless podman. Spike scripts were in
the planner's scratch folder and are not committed; the commands are
summarized here.

- [x] **glycin decodes HEIC in the GNOME runtime through `FlatpakSpawn`.**
  `org.gnome.Platform//51` (commit `2ec125d2`, "51.1-55-g6340644e6") with
  `codecs-extra` 26.08-extra, installed from Flathub in a podman container
  run with `--userns=keep-id --security-opt label=disable --security-opt
  seccomp=unconfined --security-opt unmask=/proc/*`. A minimal app
  (`flatpak build-init` with the Platform, Python and the runtime's
  `Gly-2` typelib, only `--socket=wayland`) loaded HEIC with
  `SandboxSelector.FLATPAK_SPAWN` under `dbus-run-session`. glycin's debug
  log showed `flatpak-spawn --sandbox --watch-bus ... glycin-heif` and
  "Loader glycin-heif v2.2.0". The runtime has
  `/usr/lib/x86_64-linux-gnu/codecs-extra/lib/libheif/libheif-libde265.so`.
- [x] **`Bwrap` cannot run inside the Flatpak** ("Loader process exited
  early"), so core must select `FlatpakSpawn` there.
- [x] **HEIC rotation.** `heif-enc --rotate-cw 90` writes an `irot` box.
  glycin (Fedora loaders 2.2.6 in bwrap, and the GNOME 51 loader in
  FlatpakSpawn) returns already rotated pixels and orientation 1, even with
  `apply_transformations(false)`. An EXIF `Orientation=6` added to a HEIC
  (with or without `irot`) is ignored. So core's orientation step is a
  no-op for HEIC and there is no double rotation.
- [x] **Decoder missing, end to end.** `unshare -rm` plus a tmpfs over
  `/usr/lib64/libheif` gives `NoMatchingDecoderInstalled` from
  `next_frame()`, while bwrap and PNG still work. `LIBHEIF_PLUGIN_PATH`
  does not work for this: bwrap clears the environment.
- [x] **glycin errors.** glycin-core 4.0.5 `src/error.rs` has no typed
  "no decoder" check; only the remote error text has
  `NoMatchingDecoderInstalled`.
- [x] **glycin main context.** glycin-core 4.0.5 `src/main_context.rs`:
  `Auto` uses the thread default, else the global default context if a
  loop runs on it, else a managed thread. `Managed` always uses glycin's
  own thread.
- [x] **GTK headless tests** (table above). gtk4-rs 0.11.5 has
  `#[gtk::test]` (`test_synced`), `test_widget_wait_for_draw`,
  `test_accessible_has_role`, `test_accessible_has_property`.
- [x] **AT-SPI tree** readable with pyatspi under weston headless and
  `dbus-run-session` (the a11y bus autostarts). Names and roles of GTK
  buttons, toggle buttons and labels are as expected.
- [x] **Versions.** Fedora 45 GTK 4.24.1, libadwaita 1.10.0; GNOME 51
  runtime GTK 4.24.1 (`libgtk-4.so.1.2400.1`), libadwaita 1.10.0; crates
  `gtk4` 0.11.5, `libadwaita` 0.9.2, `glib` 0.22.10 (same as glycin's).
- [x] **Flathub refs exist:** `org.gnome.Sdk//51` (875 MB download),
  `org.freedesktop.Sdk.Extension.rust-stable//26.08` (557 MB), and the
  container image `ghcr.io/flathub-infra/flatpak-github-actions:gnome-51`.
- [x] **GTK4 file transfer portal.** Fedora's `libgtk-4.so.1` contains
  `org.freedesktop.portal.FileTransfer` and
  `application/vnd.portal.filetransfer`.

Not verified (each has a task that checks it first):

- [ ] The flatpak CI job on a GitHub runner (T13).
- [ ] `nasm`, `libseccomp` and `fontconfig` headers in `org.gnome.Sdk//51`
  for building mozjpeg with SIMD and the glycin crate (T13; the GNOME Sdk
  builds glycin itself, so libseccomp is very likely there).
- [ ] Which apps accept an `image/jpeg` clipboard paste (manual, at
  acceptance; deferred question).
- [ ] Drops from Nautilus into the Flatpak on a real desktop (manual, at
  acceptance; the headless tests call the same `add_files` path).

## Risks

- Text match on `NoMatchingDecoderInstalled` breaks if glycin changes the
  message. The end-to-end test catches it.
- RPM Fusion lag can break the image build for a few days (decision 0001).
- Flatpak CI is slow (SDK download). Cache it; it is a separate job, so the
  main job's feedback stays fast.
- Double decode per image (preview + prepare) costs time. Measure at
  acceptance with NFR-1.
