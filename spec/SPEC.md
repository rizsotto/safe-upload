# Safe Upload: prepare images for upload, right in the file chooser

> Working name: **Safe Upload**. Check for name clashes on Flathub before release.
> Target desktops: GNOME first, and any Linux desktop that uses xdg-desktop-portal (tested: Niri). See section 7.3.
> This document is the spec for AI coding agents and human reviewers. Read all of it before writing code.

---

## 1. Pitch

People upload photos to social media, dating sites, marketplaces, forums and support tickets every day. Each upload can leak private data: GPS location, phone model, the time a photo was taken, or a face or a house number in the background. Sites also reject files that are too big, and most people do not know how to make a file smaller.

On Linux this takes 2 to 4 separate apps today. Safe Upload puts it where the upload starts: **the file chooser**. When you pick an image, you can switch on "Prepare for upload". The app then gets a cleaned copy:

- all metadata removed (EXIF, XMP, IPTC, GPS, embedded thumbnails),
- re-encoded to fit **a file size you choose** ("under 5 MB"), not a resolution,
- optionally with parts painted over or pixelated,
- with a neutral file name.

The original file is never changed.

There is precedent for this design. Android's system Photo Picker removes location data from photos by default ([Android Authority](https://www.androidauthority.com/android-photo-picker-location-3683074/)).

---

## 2. Users and use cases

| ID | User story | Must work in v1 |
|---|---|---|
| UC-1 | As a user uploading a phone photo to a dating site, I want location and camera data removed so strangers cannot find where I live. | Yes |
| UC-2 | As a user, I want to type "max 5 MB" and get a file that fits, without knowing what resolution or JPEG quality means. | Yes |
| UC-3 | As a user, I want to paint over a car plate, a face or a document number before uploading. | Yes |
| UC-4 | As an iPhone user, I want HEIC photos turned into JPEG because many sites reject HEIC. | Yes |
| UC-5 | As a user, I want the uploaded file name to be neutral (not `IMG_20260412_183301.jpg`), because that name leaks the date. | Yes |
| UC-6 | As a user, I want the app (for example Firefox) to remember my choice, so I do not set it up every time. | Yes |
| UC-7 | As a user, I want to see what was removed (for example "Location, camera model, date") so I can trust the tool. | Yes |
| UC-8 | As a user, I want to prepare several images at once with the same settings. | Yes |
| UC-9 | As a user, I want the same function outside the file chooser (drag a file onto an app, or "Open With" in Files). | Yes (standalone app) |
| UC-10 | As a user, I want faces found and blurred for me. | No (later) |

---

## 3. Existing solutions and their gaps

| Tool | What it does | Gap |
|---|---|---|
| [Switcheroo](https://gitlab.com/adhami3310/Switcheroo) (GNOME Circle, Rust) | Converts and resizes images. Has an option to remove EXIF ([Flathub changelog](https://flathub.org/apps/io.gitlab.adhami3310.Converter)) | Resizes by resolution, not by file size. No redaction. It is a separate app, not part of the upload flow. |
| [Obfuscate](https://gitlab.gnome.org/World/obfuscate) (GNOME Circle, Rust) | Blurs or paints over parts of an image | Only does redaction. Its blur can be reversed ([LinuxLinks](https://linuxlinks.com/obfuscate-utility-censor-private-information/2)). Separate app. |
| [Curtail](https://apps.gnome.org/Curtail/) (GNOME Circle, Python) | Compresses images | No target file size, no redaction, separate app. |
| [Metadata Cleaner](https://apps.gnome.org/MetadataCleaner/) (Python, uses [mat2](https://0xacab.org/jvoisin/mat2)) | Removes metadata from many file types | Original project discontinued ([AlternativeTo](https://alternativeto.net/software/metadata-cleaner/about)). No resize, no redaction. |
| [Loupe](https://gitlab.gnome.org/GNOME/loupe) (GNOME core image viewer) | Crop, rotate, flip | No resize, no redaction, no metadata control. |
| GNOME file chooser ([xdg-desktop-portal-gnome](https://gitlab.gnome.org/GNOME/xdg-desktop-portal-gnome); since version 47 the dialog is Nautilus, per the [Niri docs](https://niri-wm.github.io/niri/Important-Software.html)) | Picks files for apps | Hands over the original file unchanged. |
| GTK portal file chooser (xdg-desktop-portal-gtk; common on Niri, Sway and others) | Picks files for apps | Hands over the original file unchanged. |
| Windows Photos/Paint, macOS Preview | Resize and markup in one preinstalled app | Not Linux. Also not part of the upload flow. |

**Main gap:** no single tool does strip + fit-to-size + redact, and no tool sits in the upload path itself.

---

## 4. Scope

### In scope (v1)
- Image input: JPEG, PNG, HEIC/HEIF, WebP, AVIF (decoding).
- Image output: JPEG (default), PNG (when transparency must be kept), WebP (only if the user picks it).
- Remove all metadata. Keep color correct (see 9.2).
- Fit to a target file size. Optional maximum long edge in pixels.
- Redaction tools: solid box (default), pixelate, blur (with warning). Plus crop.
- Neutral output file names.
- Batch: the same settings for all selected images.
- Per-app presets (for example "Firefox: on, 5 MB").
- A "what was removed" summary.
- **Three ways to use it:** a CLI (for tests and power users), a standalone GTK app, and a file chooser portal integration.
- Works on any desktop that uses xdg-desktop-portal. Tier 1: GNOME and Niri (section 7.3).

### Out of scope (v1)
- Video, audio, PDF and office documents.
- Automatic face or text detection.
- Uploading anything. Safe Upload never talks to the network.
- Changing the original file.
- Knowing which website the file is for. The portal cannot know this.
- Platforms other than Linux.
- Changing the file chooser dialogs themselves (Nautilus, GTK, KDE) in v1. Our backend wraps them instead (section 7).

### Later (not v1)
- Face detection with a local model.
- Site presets shared by the community ("Tinder", "eBay").
- PDF metadata stripping.
- Upstream proposals: an in-dialog "Prepare for upload" toggle in Nautilus (GNOME's file chooser) and in the GTK portal's chooser.
- Hand off "Edit…" to Loupe's edit mode, once Loupe has redaction (see the separate Screenshot Annotator project spec).
- Privacy gatekeeper for every file type: always inspect, flag hidden metadata and visible personal data (also text in screenshots), then strip, edit or send on purpose. After the MVP. See `spec/vision/privacy-gatekeeper.md`.

---

## 5. Requirements

### Functional
- **FR-1** Remove EXIF, XMP, IPTC, MakerNotes, GPS, embedded thumbnails, PNG text chunks (`tEXt`, `iTXt`, `zTXt`), PNG `eXIf`, and HEIC metadata items.
- **FR-2** Apply the EXIF orientation to the pixels **before** removing metadata, so the image does not end up sideways.
- **FR-3** Fit-to-size: the output file must be ≤ the target size. It should be ≥ 85% of the target when that is possible (so quality is not thrown away for nothing).
- **FR-4** If the target cannot be met at the minimum quality and minimum size (see 9.3), fail with a clear message. Never return a file above the target.
- **FR-5** Redactions are burned into the pixels before encoding. The output has no layers and no undo history.
- **FR-6** The output name is `image.jpg`, or `image-1.jpg`, `image-2.jpg` and so on for a batch. A setting can keep original names.
- **FR-7** The output goes to a private temporary folder (see 10). Originals are never touched.
- **FR-8** Show a summary for each file: removed fields, old size → new size, old resolution → new resolution.
- **FR-9** The portal remembers the toggle state and target size per app ID.

### Non-functional
- **NFR-1** Processing a 12 MP JPEG to 5 MB takes under 2 seconds on a mid-range laptop (target, not a hard limit). Fit-to-size should predict the encoder settings from the image, so it needs one encode in the common case (section 9.3).
- **NFR-2** Image decoding must be sandboxed or use memory-safe decoders. Input files are untrusted.
- **NFR-3** No network access at all. The Flatpak manifest must not request `--share=network`.
- **NFR-4** Follow the GNOME HIG, support keyboard-only use, and work with screen readers.
- **NFR-5** All user-visible strings are translatable (gettext).

---

## 6. UX outline

### 6.1 Through the file chooser (phase 3, wrapper backend)
The user's normal file chooser opens (Nautilus on GNOME, the GTK chooser on Niri, and so on). After the user picks images, our small sheet appears **only if** this app has Safe Upload turned on, or the first time an app asks:

```
┌ Prepare 2 images for upload? ─────────────────────────────┐
│ Firefox wants to open: beach.jpg, kids.heic                │
│                                                            │
│  Remove location and camera data      [always on]          │
│  Max file size                        [ 5 MB ▾ ]           │
│  [Edit…]  (paint over parts)                               │
│                                                            │
│  [ ] Remember for Firefox                                  │
│               [Send originals]   [Prepare and send]        │
└────────────────────────────────────────────────────────────┘
```
- Per-app choice: "Always prepare", "Always send originals" or "Ask". Changeable in the standalone app's settings.
- "Edit…" opens the redaction editor.
- Non-image files skip the sheet.
- A notification shows "2 images prepared: location removed, 4.1 MB → 1.9 MB".

### 6.2 Standalone app (phase 2)
1. Drop images onto the window, or open them.
2. Settings panel: max size, max long edge (optional), output format, keep names (off).
3. Image strip with per-image redaction.
4. Buttons: "Copy to clipboard" (single image) and "Save to…".
5. A summary row for each image.

### 6.3 Redaction editor
- Tools: Box (solid, default), Pixelate, Blur, Crop.
- Blur and Pixelate show an inline warning: "Not safe for text. Use Box for numbers and names."
- Undo/redo. Keyboard: `B` box, `P` pixelate, `U` blur, `C` crop, `Ctrl+Z` undo.

---

## 7. Architecture

```
                 ┌────────────────────────────┐
                 │  safe-upload-core (Rust)   │  pure library, no UI
                 │  decode → orient → redact  │
                 │  → strip → fit-to-size     │
                 └─────────────┬──────────────┘
          ┌────────────────────┼────────────────────────┐
          ▼                    ▼                        ▼
 ┌────────────────┐  ┌───────────────────┐  ┌──────────────────────────┐
 │ safe-upload CLI│  │ Standalone app    │  │ Portal backend           │
 │ (tests, power  │  │ (GTK4+libadwaita) │  │ org.freedesktop.impl.    │
 │  users)        │  │ + redaction editor│  │ portal.FileChooser       │
 └────────────────┘  └───────────────────┘  │ WRAPPER: delegates the   │
                                            │ picking to the user's    │
                                            │ chooser, then prepares   │
                                            └────────────┬─────────────┘
                                                         ▼
                                    existing backend: gnome (Nautilus), gtk, kde, …
```

### 7.1 Phases
1. **Core + CLI.** All the logic lives here and is fully tested.
2. **Standalone app.** It reuses the core and contains the redaction editor widget (a reusable crate).
3. **Wrapper portal backend.** A separate backend that implements `org.freedesktop.impl.portal.FileChooser`. Users opt in by routing FileChooser to it in `portals.conf`. It **does not draw its own file chooser**. It forwards the request to the backend the user had before (gnome, gtk, kde, …), gets the picked URIs back, and then shows the "Prepare" sheet (6.1) for images. This works on every desktop and keeps the user's familiar dialog. The same trick is already used by [xdg-desktop-portal-shana](https://wiki.archlinux.org/title/XDG_Desktop_Portal), which redirects requests to GNOME, GTK, KDE or LXQt backends.
4. **Upstream (later, human-led).** Propose an in-dialog toggle to Nautilus (GNOME's chooser since xdg-desktop-portal-gnome 47) and to xdg-desktop-portal-gtk. This needs design agreement first.

Why not draw our own chooser: GTK4's `GtkFileChooserWidget` is deprecated (since GTK 4.10; check), and a custom chooser would feel foreign on every desktop.

### 7.2 Portal flow (phase 3)
1. The app calls `org.freedesktop.portal.FileChooser.OpenFile`.
2. xdg-desktop-portal forwards the call to our backend (selected in `portals.conf`).
3. Our backend calls `OpenFile` on the **delegate** backend's `org.freedesktop.impl.portal.FileChooser` (its D-Bus name is read from our config, for example `org.freedesktop.impl.portal.desktop.gnome`), passing on the parent window, title and options. It also implements `org.freedesktop.impl.portal.Request.Close` and forwards it.
4. If any picked file is an image and the app's preset is "prepare" or "ask", show the sheet. Process the images into the private temporary folder.
5. Return the processed URIs (and the unchanged URIs of non-images) to the frontend, which exports them to sandboxed apps through the document portal.
6. `SaveFile` and `SaveFiles` are forwarded unchanged.
7. If the delegate is missing or fails: return an error to the frontend. Never show a broken dialog.

### 7.3 Desktop compatibility

**Rules (MUST)**
- **D-1** Use only public, cross-desktop interfaces: GTK, portals, freedesktop D-Bus specs. Never call `org.gnome.Shell.*` private APIs. No Shell extensions.
- **D-2** Detect features at runtime and fall back cleanly. Do not branch on `XDG_CURRENT_DESKTOP` for behavior (logging it is fine).
- **D-3** Never hard-depend on Nautilus. The delegate backend is configurable.

**Tiers**
| Tier | Desktop | Delegate backend | Expectation |
|---|---|---|---|
| 1 | GNOME (current stable) | gnome (Nautilus dialog) | Fully tested. |
| 1 | Niri, recommended portals ([Niri docs](https://niri-wm.github.io/niri/Important-Software.html)) | gnome (needs Nautilus) or gtk (`FileChooser=gtk`) | Fully tested with both delegates. |
| 2 | Sway/Hyprland | gtk | Best effort. |
| 2 | KDE Plasma | kde | Best effort. The sheet is GTK, so it looks foreign there. |

**Setup per desktop** (the standalone app should offer to write this for the user, showing the diff first):
```
# ~/.config/xdg-desktop-portal/<desktop>-portals.conf   (e.g. niri-portals.conf, gnome-portals.conf)
[preferred]
org.freedesktop.impl.portal.FileChooser=safeupload
```
and our own config names the delegate (for example `delegate=gnome` or `delegate=gtk`).

---

## 8. Tech stack and decisions

| Decision | Choice | Why |
|---|---|---|
| Language | Rust (stable) | Memory safety for untrusted images. The compiler gives agents fast feedback. Same language as many GNOME Circle apps. |
| UI | gtk4-rs + libadwaita-rs | Native GNOME. |
| Decoding | [glycin](https://gitlab.gnome.org/GNOME/glycin) first. Fall back to `image` + `libheif-rs` if glycin lacks something | glycin is what Loupe uses and runs its loaders in a sandbox. |
| JPEG encoding | `mozjpeg` crate | Better quality per byte, which matters for fit-to-size. |
| WebP encoding | `webp` crate (libwebp) | Standard. |
| Metadata | Write a fresh file from pixels only. Never copy metadata blocks | Stripping by rebuilding is safer than deleting known tags. |
| D-Bus (portal) | `zbus` | Pure Rust, used widely in GNOME Rust apps. |
| Phase 3 design | Wrapper backend that delegates to the existing FileChooser backend | Works on every desktop, keeps the user's dialog, avoids the deprecated GTK4 chooser widget. |
| Packaging | Flatpak (app), distro package (portal backend) | A portal backend must run on the host, not in a Flatpak. |
| License | GPL-3.0-or-later (proposed) | GNOME Circle norm. Confirm with the human. |

**Key rule:** the encoder only ever receives a pixel buffer plus an optional ICC profile. It never receives the source file's metadata. This makes leaks impossible by construction.

---

## 9. Core algorithms

### 9.1 Pipeline order (must be exactly this)
1. Decode (sandboxed) → RGBA pixel buffer + ICC profile + EXIF orientation.
2. Apply the orientation to the pixels.
3. Apply crop.
4. Apply redactions (burn into pixels).
5. Convert color: if the ICC profile is not sRGB, convert the pixels to sRGB and drop the profile. If it is sRGB, keep a minimal sRGB profile or none.
6. Fit-to-size encode (9.3).
7. Write to the private temporary folder with a neutral name.
8. **Verify:** re-open the output and check that it has no metadata except an optional ICC profile. Fail closed if anything is found.

### 9.2 Why convert to sRGB
Dropping a Display P3 profile (common on phones) without converting makes colors look washed out. Converting to sRGB and dropping the profile is both safe and correct.

### 9.3 Fit-to-size
Predict the settings from a small sample, encode the image once, and correct only if that encode misses (decision 0002).
```
input: pixels, target_bytes, max_long_edge (optional), format
# quality range [Q_MIN, Q_MAX]: JPEG 40..92, PNG 40..100 (100 = lossless)
scale = min(1.0, max_long_edge / long_edge)
floor = min(scale, MIN_LONG_EDGE (640) / long_edge)
overhead = len(encode(8x8 corner, Q_MAX))      # headers and profile, not per pixel
ratio = 1.0                                     # actual / predicted, learned from full encodes
cap = Q_MAX                                     # none: even Q_MIN is too big at this scale
loop:
    sample = 8x8 grid of tiles, 64 px each (less if the short edge is under 512),
             one from the middle of each grid cell, at the output scale (Lanczos3);
             the whole scaled image if it has at most 512x512 pixels
    predict(q) = overhead + (len(encode(sample, q)) - overhead) * pixels(image) / pixels(sample)
    q = highest quality in [Q_MIN, cap] with predict(q) * ratio <= 0.95 * target   # binary search on the sample
    if no q:
        if cap and (scale <= floor or predict(Q_MIN) * ratio <= target): q = Q_MIN   # try it for real
        elif scale <= floor: error "cannot fit"
        else:
            scale = max(floor, scale * min(0.95, (0.95 * target / (predict(Q_MIN) * ratio)) ^ (1 / 1.7)))
            cap = Q_MAX; continue
    out = encode(resize(pixels, scale), q)
    ratio = len(out) / predict(q)
    if len(out) > target:
        cap = q - 1, or none if q == Q_MIN; continue
    if len(out) >= 0.85 * target or q == cap: return out
    q2 = highest quality in (q, cap] with predict(q2) * ratio <= 0.95 * target   # one try up
    return encode(q2) if it fits, else out
```
The search never returns a file above the target. Most photos need one full encode, a few need two (decision 0002 has the data).
- PNG input with transparency → output PNG. Fit by quantizing (`imagequant`) and then scaling. If that fails, tell the user, or offer WebP if they allow it.
- PNG input without transparency → JPEG by default (much smaller for photos).
- Cache encodes so the UI preview does not encode twice.

### 9.4 Redaction
- **Box:** fill with opaque black (or a user-chosen solid color). Safe for text.
- **Pixelate:** block size ≥ 1/40 of the region's short edge and at least 16 px. Then add noise of ±8 per channel to each block. Show the warning (pixelation of text can sometimes be reversed).
- **Blur:** Gaussian, radius ≥ 1/20 of the region's short edge. Show the warning.
- Redaction regions are rectangles in v1. Store them in original-image coordinates **after** orientation.

---

## 10. Security and privacy rules (MUST)

- **S-1** Never write processed files next to the original. Use `$XDG_RUNTIME_DIR/safe-upload/<random-32-hex>/`, with mode `0700` for the folder and `0600` for files.
- **S-2** Delete temporary folders older than 1 hour on each start, and register a cleanup timer.
- **S-3** No network code. Add a CI check that fails if network crates (`reqwest`, `hyper`, and so on) appear in `Cargo.lock`.
- **S-4** Treat all input files as hostile: sandboxed decoding, size limits (refuse images above 200 MP), timeouts.
- **S-5** Never log file paths or metadata values at info level. Debug logs must be opt-in.
- **S-6** Fail closed: if the verification in step 9.1.8 fails, return an error. Never fall back to the original file.
- **S-7** Embedded thumbnails are the classic leak (the thumbnail keeps the un-redacted picture). Tests must cover this.

---

## 11. Repository layout

```
safe-upload/
├── Cargo.toml                # workspace
├── crates/
│   ├── core/                 # safe-upload-core: pipeline, no GTK
│   ├── cli/                  # `safe-upload` binary
│   ├── editor-widget/        # reusable GTK4 redaction editor
│   ├── app/                  # standalone libadwaita app
│   └── portal/               # xdg-desktop-portal FileChooser backend
├── data/                     # .desktop, metainfo, icons, gschema, portal files
├── po/                       # translations
├── tests/fixtures/           # see 13
├── build-aux/flatpak/        # Flatpak manifest for the app
├── spec/                 # SPEC.md, decisions, milestones (for developers)
└── docs/                 # user documentation
```

---

## 12. Milestones and acceptance criteria

### M1: Core + CLI
Status: accepted 2026-11-12. Evidence for each item: [milestones/M1/spec.md](milestones/M1/spec.md).

- `safe-upload in.jpg --max-size 2MB -o out.jpg` works.
- All fixture tests pass (13).
- `exiftool out.jpg` shows no fields except basic file and image properties.
- An embedded EXIF thumbnail is never present in the output.
- An orientation-6 input comes out upright.
- A HEIC/HEIF input is detected by its content, not its file name, and refused with a clear message and exit code 3. No crash, no garbled output, no output file, and never the original passed through. HEIC decoding comes in M2 (see [decision 0001](decisions/0001-heic-decoding-and-fixtures.md)).

### M2: Standalone app
Status: planned 2026-11-12. Criteria, plan and tasks: [milestones/M2/](milestones/M2/spec.md).

- Drag-and-drop, batch, settings, summary, save and copy all work.
- The redaction editor supports box, pixelate, blur and crop, with undo/redo.
- Passes the Flatpak build with no network permission.
- Keyboard-only walkthrough is possible. Orca reads all controls.
- HEIC (UC-4): in the shipped Flatpak build, a HEIC input becomes an upright JPEG with no metadata. This replaces the M1 refusal.
- The devcontainer and CI decode HEIC. They use RPM Fusion's `libheif-freeworld` ([decision 0001](decisions/0001-heic-decoding-and-fixtures.md), option A). Packages to add in M2: `rpmfusion-free-release`, `libheif-freeworld`, `libheif-tools` (for `heif-enc -e x265`, which makes the HEIC fixture).
- If no HEVC decoder is present at runtime (for example a host install without RPM Fusion, or a Flatpak without the `codecs-extra` extension), the user gets a clear message and the CLI exits with code 3. Never a crash or a raw library error.
- Every test runs in CI. None is skipped, ignored or silently degraded.
- **Open items (resolved at the start of M2, see [milestones/M2/plan.md](milestones/M2/plan.md), "Facts verified"):**
  - glycin's bwrap sandbox on GitHub runners: settled in M1 (T05, T06). Core forces `SandboxSelector::Bwrap`; CI runs 43158776772 through 43170714939 passed with the sandbox tests. CI turns off seccomp and AppArmor confinement for the job container and allows unprivileged user namespaces on the runner (`.github/ci/devcontainer.json`, `.github/workflows/ci.yml`). In Flatpak core selects `FlatpakSpawn` instead (verified: `Bwrap` cannot run inside a Flatpak).
  - Verified 2026-11-12: the GNOME 51 runtime decodes HEIC through glycin's `flatpak-spawn --sandbox` subsandbox with `codecs-extra` (M2 plan, "Facts verified").
  - "Decoder missing" end to end: hide the libheif plugin folder with `unshare -rm` and a tmpfs mount; glycin's bwrap still runs and HEIC fails with `NoMatchingDecoderInstalled` (verified). A fake `Decoder` is not needed.
  - Rotated HEIC fixture: `heif-enc -e x265 --rotate-cw 90` writes an `irot` box. glycin applies `irot` in the loader and ignores EXIF orientation for HEIF (verified), so the fixture also gets EXIF `Orientation=6` to catch double rotation.
  - Still open: the Flatpak CI job on a GitHub runner (M2 task T13).

### M3: Wrapper portal backend
- With `portals.conf` routing FileChooser to our backend, Firefox and a GTK4 test app receive processed files when the preset is on, and originals when it is off.
- The user sees their normal chooser (Nautilus on GNOME; the GTK chooser on Niri with `delegate=gtk`).
- Per-app memory works. Non-image files pass through unchanged. `SaveFile` is untouched.
- Cancel and Close are forwarded correctly. Temporary files are cleaned up.
- Passes the tier 1 desktop matrix (13).

### M4: Polish for GNOME Circle
- HIG review, translations set up, AppStream metainfo, screenshots, Flathub submission (app only).

---

## 13. Test plan

### Fixtures to create (put a script in `tests/fixtures/make.sh` that generates them)
- JPEG with GPS EXIF, MakerNotes, XMP, IPTC and an **embedded thumbnail that shows a different image**.
- JPEG with each orientation value 1 to 8.
- PNG with `tEXt`, `iTXt`, `eXIf` chunks.
- PNG with transparency.
- HEIF header only: a hand-built `ftyp` box with brand `heic` (M1 refusal test, no HEVC encoder needed).
- HEIC generated with `heif-enc -e x265` with GPS (M2, see decision 0001).
- Display P3 JPEG.
- Huge image (refused) and truncated file (clean error).

### Test types
- Unit tests for each pipeline step.
- Property test: for random targets between 100 KB and 10 MB, the output is ≤ target, or the result is a clear error.
- Leak test: run `exiftool -a -G1 -s` on every output and allow only a short list of keys.
- Visual test: an orientation-corrected output matches a reference within a small tolerance.
- Portal: an integration test under `dbus-run-session` with a fake delegate backend (python-dbusmock or a small test service) and a fake frontend call.
- Desktop matrix (manual, record results in the merge request): GNOME + gnome delegate; Niri + gnome delegate; Niri + gtk delegate; Sway + gtk delegate. Apps: Firefox, Chromium, a Flatpak GTK4 app, an Electron app.

---

## 14. Instructions for agents

- Work milestone by milestone. Do not start M3 before M1 passes.
- Keep `core` free of GTK and D-Bus. It must build and test with `cargo test -p safe-upload-core` alone.
  - Clarified (human OK 2026-11-12, see 16.8): core makes no bus connection and has no direct `zbus` dependency. glycin's private peer-to-peer channel to its sandboxed loaders (zbus over a socket pair, no bus daemon) is allowed.
- Write a test that fails first for every leak or bug you fix.
- Do not add network dependencies (S-3).
- Prefer small pull requests (under about 400 changed lines) with a clear description.
- Run `cargo fmt`, `cargo clippy -- -D warnings`, and the tests before every commit.
- If a fact in section 15 turns out wrong, update this document in the same pull request.
- Follow the desktop rules D-1 to D-3 (section 7.3). No GNOME-only shortcuts in code.
- When you need a design decision that is not in this document, add it to section 16 and stop. Do not guess.

---

## 15. Facts to verify before relying on them

- [ ] `portals.conf` can route only `org.freedesktop.impl.portal.FileChooser` to a custom backend while everything else stays on the GNOME backend (expected in xdg-desktop-portal ≥ 1.18).
- [ ] Which options the backend `OpenFile` call receives (`app_id` for non-Flatpak apps may be empty) and how the frontend exports returned URIs to sandboxed apps.
- [ ] Calling another backend's `org.freedesktop.impl.portal.FileChooser` directly works with the parent window handle passed through (study xdg-desktop-portal-shana).
- [ ] Whether the frontend accepts returned URIs that point to files the user did not pick in the dialog (our temporary copies), for both sandboxed and host apps.
- [ ] That xdg-desktop-portal-gnome ≥ 47 uses Nautilus as the dialog, and how its backend behaves when called by another backend.
- [ ] Whether `GtkFileChooserWidget` is deprecated since GTK 4.10 (it only affects the fallback idea, not the main design).
- [ ] Whether Firefox and Chrome on GNOME use the portal file chooser by default outside Flatpak or Snap. Firefox has the setting `widget.use-xdg-desktop-portal.file-picker`.
- [x] glycin's public API exposes the ICC profile and the orientation (M1). Checked in task M1 T05 with the `glycin` crate 4.0.5 and Fedora `glycin-loaders` 2.2.6: `Loader::color_convert_icc_srgb(false)` keeps the profile, which `FrameDetails::color_icc_profile()` returns; `Image::transformation_orientation()` gives the EXIF orientation, and `Loader::apply_transformations(true)` applies it to the pixels. Test: `crates/core/tests/glycin_sandbox.rs`. Sources: glycin-core 4.0.5 `src/api/loader.rs`, `src/api/common.rs`.
- [x] Whether glycin can decode HEIC in the user's runtime (M2). Flatpak: yes. Checked 2026-11-12 with `org.gnome.Platform//51` (commit `2ec125d2`) and `org.freedesktop.Platform.codecs-extra//26.08-extra` from Flathub: a minimal installed app with only `--socket=wayland` loaded HEIC with `SandboxSelector::FlatpakSpawn`; glycin's log showed `flatpak-spawn --sandbox ... glycin-heif` (v2.2.0). `Bwrap` fails inside the Flatpak. Host: only with RPM Fusion's `libheif-freeworld` ([decision 0001](decisions/0001-heic-decoding-and-fixtures.md)); without it glycin returns `NoMatchingDecoderInstalled` from `next_frame()`. glycin applies the HEIF `irot` box itself and ignores EXIF orientation for HEIF (Fedora loaders 2.2.6 and the GNOME 51 loader). Details: [milestones/M2/plan.md](milestones/M2/plan.md), "Facts verified".
- [x] GTK4 tests run headless (M2). gtk4-rs 0.11.5 `#[gtk::test]` works under `weston --backend=headless`, `mutter --headless`, broadway and Xvfb in the devcontainer image (GTK 4.24.1, libadwaita 1.10.0). On the headless Wayland compositors the window is not active, so tests check `GtkWindow::focus()`, not `has_focus()`. The AT-SPI tree is readable with pyatspi under weston and `dbus-run-session`. Details: M2 plan.
- [x] The GNOME 51 runtime ships the same GTK (4.24.1) and libadwaita (1.10.0) as Fedora 45, and its glycin loaders (`glycin-heif`, `glycin-image-rs`) (M2).
- [ ] The Flathub `gnome-51` builder container can install and run the built Flatpak on a GitHub runner, for the HEIC test (M2 T13). Works in rootless podman with `seccomp=unconfined` and `unmask=/proc/*`.
- [ ] Which apps accept an `image/jpeg` clipboard paste (Firefox, Chromium). M2 copies the verified bytes with their own MIME type.
- [ ] Whether `ashpd` supports implementing portal backends, or only plain `zbus` does.

---

## 16. Open questions for the human

1. Final name and app ID (reverse DNS). M2 uses the placeholder `org.example.SafeUpload` until then (M2 task T19 renames it).
2. License confirmation (GPL-3.0-or-later proposed).
3. ~~Default target size when the toggle is first turned on?~~ Answered 2026-11-12: 5 MB, a common upload limit today.
4. Keep blur at all, or only offer box and pixelate? Blocks only M2 task T15. If blur is dropped, the M2 criterion and 6.3 change instead.
5. Should the standalone app also register as a "Share" target in Files? Not in the M2 criteria; deferred.
6. Should the standalone app edit `portals.conf` for the user (with a diff and an undo), or only show instructions?
7. First-time behavior per app: ask (proposed), or off until the user enables it?
8. ~~Accept the "no D-Bus" clarification in section 14?~~ Answered 2026-11-12: yes. glycin's peer-to-peer socket to its loaders is allowed; core itself makes no bus connection.

Decided: HEIC decoding moves to M2; M1 refuses HEIC. See [decision 0001](decisions/0001-heic-decoding-and-fixtures.md).

---

## 17. Glossary

- **EXIF / XMP / IPTC:** metadata formats inside image files. They can hold GPS location, camera model, dates and names.
- **Orientation:** an EXIF field that says how to rotate the image for display.
- **ICC profile:** describes the image's color space.
- **Portal:** a D-Bus service that sandboxed apps use to reach the system (for example to pick files). Frontend: `xdg-desktop-portal`. Backend: `xdg-desktop-portal-gnome` or ours.
- **Document portal:** gives a sandboxed app access to a single file the user picked.

## 18. References

- xdg-desktop-portal FileChooser API: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.FileChooser.html
- Backend API: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.impl.portal.FileChooser.html
- xdg-desktop-portal-gnome: https://gitlab.gnome.org/GNOME/xdg-desktop-portal-gnome
- glycin: https://gitlab.gnome.org/GNOME/glycin
- GNOME HIG: https://developer.gnome.org/hig/
- GNOME Circle criteria: https://circle.gnome.org/
- gtk4-rs book: https://gtk-rs.org/gtk4-rs/stable/latest/book/
- Android Photo Picker location stripping: https://www.androidauthority.com/android-photo-picker-location-3683074/
