# M2 tasks

Each task is one branch (`m2/tNN-short-name`) and one PR under about 400
changed lines. Write the test first. "Needs" lists tasks that must merge
first. Tasks with no open needs can run in parallel.

Do not change `crates/core/src/fit.rs`: lane `m2/fit-predict` owns it. Tasks
that touch `crates/core/src/pipeline.rs` (T05) rebase on that lane if it
merged first.

| ID | Task | Needs | Test |
|---|---|---|---|
| T01 | Devcontainer: RPM Fusion free, `libheif-freeworld libheif-tools` (decision 0001); GTK test packages `gtk4-devel libadwaita-devel weston dbus-daemon at-spi2-core python3-pyatspi glib2-devel desktop-file-utils appstream`. `make.sh`: `heic-gps.heic` and `heic-rot90.heic` (plan, "HEIC in dev and CI"); `check.sh` checks them | none | `check.sh`: GPS in both, `heif-info` 480x640 and exiftool `Rotation` for rot90, EXIF `Orientation` 6; `make.sh` fails clearly without the x265 encoder |
| T02 | Core: sandbox selector (`FlatpakSpawn` if `/.flatpak-info`, else `Bwrap`; check `active_sandbox_mechanism()` equals it) and `MainContextSelector::Managed` | none | Unit: selector from a fake info path both ways; existing `bwrap_is_used_even_when_auto_would_drop_the_sandbox` still passes; decode from a thread while another thread runs a `glib::MainLoop` on the default context completes |
| T03 | Workspace deps and app skeleton: `gtk4` 0.11, `libadwaita` 0.9, `gettext-rs` (`gettext-system`); `adw::Application` with `APP_ID = "org.example.SafeUpload"` in `config.rs`; empty window; headless test harness (`harness = false`, weston + dbus + temp `XDG_RUNTIME_DIR`/`HOME`, plan "Testing GTK headless") in `crates/editor-widget/tests/common/headless.rs` | none | `crates/app/tests/ui.rs`: window presents and draws under headless weston; harness fails (not skips) with a clear message if weston is missing (unit test on the check function); S-3 check still clean |
| T04 | Data files and checks: `.desktop`, `.metainfo.xml`, `.gschema.xml` (plan keys, `max-size` default 5000000), placeholder icon; `build-aux/ci/check-app-id.sh`; CI step running `desktop-file-validate`, `appstreamcli validate --no-net`, `glib-compile-schemas --strict --dry-run` | none | The validators pass in CI; `check-app-id.sh` self test: a copy with one mismatched ID fails |
| T05 | Core: `pipeline::preview(input, limits, max_edge)` (decode, orient, sRGB, downscale; no encode, no file) | none (rebase on fit lane if needed) | Integration: orientation-6 fixture preview is upright and swapped; P3 fixture preview is sRGB; max edge respected; full oriented size reported; no file in the private folder |
| T06 | Core + CLI: HEIC decode. `sniff` stops refusing HEIF; `check_complete` accepts HEIF; `Error::MissingDecoder(Format::Heif)` from the `NoMatchingDecoderInstalled` text; CLI exit 3 for it; replace the M1 refusal tests | T01, T02 | `decode.rs`: `heic-gps.heic` decodes in bwrap; `pipeline.rs`/`leak.rs`: HEIC becomes JPEG, allow list, no GPS; `heic-rot90.heic` comes out 480x640 with red top-right (no double rotation); header-only HEIF gives `Error::Decode`, exit 1; `cli.rs::heic_without_decoder_exits_3`: `unshare -rm` + tmpfs over the libheif plugin dir, exit 3, message, no output (plan, "Decoder missing") |
| T07 | Core: HEIC in `summary::scan` (Exif items, XMP `mime` items) | T01 | Unit on `heic-gps.heic`: reports Location and Camera; `Display` has no values; garbage boxes do not panic |
| T08 | Editor model (pure Rust): `EditModel`, `ViewTransform`, undo/redo snapshots, coalescing, clamping, `to_options()` | none | Unit: add/move/resize/delete/crop with undo and redo; new edit clears redo; clamp to image; view mapping never shrinks a box; `to_options()` coords are pre-crop |
| T09 | Editor widget: drawing, preview via `core::edit::apply` on preview pixels, selection, handles, crop dimming, toolbar with labelled buttons, `changed` signal | T03, T08 | `crates/editor-widget/tests/ui.rs` (headless): draw through actions and a simulated `GestureDrag` sequence where possible, else via the widget's drag handlers; preview pixels in a box are black; every tool button has an accessible label |
| T10 | Editor keyboard and a11y: shortcuts `B P C`, `Ctrl+Z`, `Ctrl+Shift+Z`, `Ctrl+Y`, `Delete`; keyboard drawing (Enter, arrows, Shift/Alt, Tab cycle, Escape); live accessible description; pixelate/blur warning label | T09 | Headless: shortcut controller has each trigger mapped to its action; keyboard actions add, move, resize and delete an area; description text updates; warning visible only for pixelate (and blur after T15) |
| T11 | App window and model: drop target, `Open…`, `ImageItem`, list rows, debounced prepare off the main thread (`gio::spawn_blocking`), generation counter, one-at-a-time queue, row errors, S-2 startup cleanup and 10 minute timer | T03, T04 | Headless: `add_files` with JPEG, PNG and a text file gives two Ready rows and one "Not a supported image" row; main loop keeps running during prepare (a timeout source fires while work runs); stale results dropped; stale folder older than 1 hour removed on start (fake `XDG_RUNTIME_DIR`) |
| T12 | Settings panel bound to GSettings (memory backend in tests), re-prepare on change, impossible target message | T11 | Headless: default 5 MB shown; changing max size re-prepares and the output is under the new target; 1 KB target gives the core's "cannot be made small enough" on the row; keep names changes the output name |
| T13 | Flatpak manifest, `cargo-sources.json` and its update script, `check-flatpak-permissions.sh` (+ self test), CI jobs `flatpak` and `flatpak-check`, `test-heic.sh`, `check-heic-outputs.sh`. First verify the plan's unverified Flatpak facts (Sdk has nasm etc.; job runs on a GitHub runner) | T03, T04, T06 | CI: bundle builds; permission check passes and its self test (manifest with `--share=network`) fails as expected; installed Flatpak turns both HEIC fixtures into clean upright JPEGs under 5 MB, decoded with `FlatpakSpawn` |
| T14 | Save and copy: file dialog (one), folder dialog (batch), `create_new`, conflict message, re-verify before write; clipboard `ContentProvider::for_bytes` with the real MIME type, single image only | T11 | Headless: save path into a temp folder writes the verified bytes (byte-equal, exiftool allow list); existing `image-1.jpg` makes the batch write nothing; clipboard read back equals the output bytes and MIME; copy disabled for two images |
| T15 | Blur: core `Redaction::Blur` (SPEC 9.4 radius rule) and the editor tool, `U` shortcut, warning. **Blocked on SPEC 16.4** | T08, T10, 16.4 | Core unit: blurred region differs from input, radius is at least short edge / 20, outside untouched; widget: `U` selects blur, warning shown |
| T16 | App editor dialog: preview off the main thread, edits stored on the item, re-prepare; output test that a boxed area is solid in the saved file and an orientation-6 input keeps boxes where drawn | T05, T10, T11 | Headless: box drawn on the oriented preview; saved JPEG decoded: the area is near-black within JPEG tolerance, the rest is not; undo then Done removes it |
| T17 | Accessibility checks: `build-aux/ci/atspi-check.py` run from the app UI test against the live window (every interactive control named); keyboard walkthrough test (focus order, accelerators for every action); shortcuts window | T12, T14, T16 | The tests themselves, in CI; a deliberately unlabelled icon button in a test-only window makes `atspi-check.py` fail |
| T18 | Accept M2: tick `spec.md` boxes with evidence; manual Orca and keyboard run, drop from Nautilus into the Flatpak, clipboard paste into Firefox and Chromium (recorded); NFR-1 timing with preview + prepare; update SPEC 12, 15, 16 | all others (T15 only if 16.4 keeps blur) | Full CI green, including both Flatpak jobs |
| T19 | Rename the placeholder app ID. **Blocked on SPEC 16.1** | T04, T13, 16.1 | `check-app-id.sh` passes with the new ID; Flatpak job green |

## Start right away (no needs)

T01, T02, T03, T04, T05, T08. They touch different files:
T01 Containerfile and fixtures; T02 `decode.rs`; T03 app crate and
`Cargo.toml`/`Cargo.lock`; T04 `data/` and CI script; T05 `pipeline.rs`
(new function); T08 editor-widget `model.rs`. T03 and any other task adding
crates both change `Cargo.lock`: merge T03 first among those (T02 and T05
add none).

## Parallel lanes

| Lane | Tasks in order |
|---|---|
| A: HEIC and core | T01, T02 → T06 → T07 |
| B: editor | T08 → T09 (after T03) → T10 → T15 (after 16.4) |
| C: app shell | T03, T04 → T11 → T12, T14 |
| D: Flatpak | T13 (after T03, T04, T06) → T19 (after 16.1) |
| E: preview and editor dialog | T05 → T16 (after T10, T11) |
| Final | T17 → T18 |

## Waves

1. T01, T02, T03, T04, T05, T08
2. T06, T07 (after T01/T02); T09 (after T03, T08); T11 (after T03, T04)
3. T10, T12, T14, T13
4. T16, T15 (if 16.4 answered)
5. T17
6. T18; T19 whenever 16.1 is answered
