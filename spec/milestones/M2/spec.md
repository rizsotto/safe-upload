# M2: Standalone app

Source: SPEC section 12, M2. IDs point to SPEC sections 5 (FR, NFR), 6 (UX),
9 (algorithms) and 10 (S). Plan: [plan.md](plan.md). Tasks: [tasks.md](tasks.md).

Open human questions that touch M2: SPEC 16.1 (name and app ID), 16.4 (keep
blur), 16.5 (share target). The plan works around them; see "Waiting on the
human" below.

## Acceptance criteria (SPEC section 12)

### App

- [ ] Drag-and-drop works: images dropped on the window are added, also from
  the host into the Flatpak (file transfer portal). Non-images get a clear
  row error, never a crash.
  (UC-1, UC-2, [NFR-2](../../SPEC.md#non-functional), [S-4](../../SPEC.md#10-security-and-privacy-rules-must), SPEC 6.2 step 1)
- [ ] Batch works: several images, the same settings for all, neutral names
  `image-1.jpg`, `image-2.jpg` ... (or original names if "Keep names" is on).
  ([FR-6](../../SPEC.md#functional), SPEC 4)
- [ ] Settings work and are remembered between runs: max size (default
  5 MB, SPEC 16.3), max long edge (optional), output format (Auto, JPEG,
  PNG), keep names (off).
  ([FR-3](../../SPEC.md#functional), [FR-6](../../SPEC.md#functional), SPEC 6.2 step 2)
- [ ] Summary works: a row per image with removed metadata kinds, old size
  to new size, old resolution to new resolution. No metadata values, no
  paths.
  ([FR-8](../../SPEC.md#functional), [S-5](../../SPEC.md#10-security-and-privacy-rules-must))
- [ ] Save works: "Save to…" through the portal file dialog. One image: a file
  dialog. A batch: a folder dialog. Never overwrites an existing file
  without the dialog's own confirmation. The saved bytes are the verified
  core output.
  ([FR-7](../../SPEC.md#functional), [S-6](../../SPEC.md#10-security-and-privacy-rules-must), SPEC 6.2 step 4)
- [ ] Copy to clipboard works for a single image: the verified output bytes
  with their own MIME type (`image/jpeg` or `image/png`), not re-encoded.
  ([FR-3](../../SPEC.md#functional), [S-6](../../SPEC.md#10-security-and-privacy-rules-must), SPEC 6.2 step 4)
- [ ] Output never exceeds the target; an impossible target shows the core's
  clear message on that row and nothing is saved for it.
  ([FR-3](../../SPEC.md#functional), [FR-4](../../SPEC.md#functional))
- [ ] The UI stays responsive while images are decoded and encoded (work
  runs off the main thread).
  ([NFR-1](../../SPEC.md#non-functional), [NFR-4](../../SPEC.md#non-functional))
- [ ] Temp folders: the app deletes folders older than 1 hour on start and
  on a timer, and removes its own folder when done with it.
  ([S-1](../../SPEC.md#10-security-and-privacy-rules-must), [S-2](../../SPEC.md#10-security-and-privacy-rules-must), [FR-7](../../SPEC.md#functional))

### Redaction editor

- [ ] Box (solid, default), pixelate, blur and crop work. Blur depends on
  SPEC 16.4; see below.
  ([FR-5](../../SPEC.md#functional), UC-3, SPEC 6.3, 9.4)
- [ ] Undo and redo work for every edit (add, move, resize, delete, crop).
  (SPEC 6.3)
- [ ] Keys: `B` box, `P` pixelate, `U` blur, `C` crop, `Ctrl+Z` undo,
  `Ctrl+Shift+Z` redo. Areas can be drawn, moved, resized and deleted with
  the keyboard alone.
  (SPEC 6.3, [NFR-4](../../SPEC.md#non-functional))
- [ ] Pixelate and blur show the inline warning "Not safe for text. Use Box
  for numbers and names."
  (SPEC 6.3, 9.4)
- [ ] Redactions are burned in by core before encoding, in coordinates after
  orientation. The output has no layers or history. An output test proves a
  boxed area is solid in the saved file.
  ([FR-5](../../SPEC.md#functional), [FR-2](../../SPEC.md#functional), SPEC 9.1)

### Flatpak

- [ ] The Flatpak builds from `build-aux/flatpak/` with the GNOME 51 runtime,
  and its permissions have no network: no `--share=network` in the manifest
  and none in the built app's metadata (checked in CI).
  ([NFR-3](../../SPEC.md#non-functional), [S-3](../../SPEC.md#10-security-and-privacy-rules-must))
- [ ] Inside the Flatpak, core decodes with glycin's `FlatpakSpawn` sandbox
  and fails closed if any other mechanism ran.
  ([NFR-2](../../SPEC.md#non-functional), [S-4](../../SPEC.md#10-security-and-privacy-rules-must))

### Accessibility

- [ ] Keyboard-only walkthrough is possible: add images, change every
  setting, open the editor, draw and delete an area, undo, save, copy.
  ([NFR-4](../../SPEC.md#non-functional))
- [ ] Orca reads all controls: every interactive control has an accessible
  name and role (automated AT-SPI check), plus one manual Orca run recorded
  at acceptance.
  ([NFR-4](../../SPEC.md#non-functional))

### HEIC (UC-4, decision 0001)

- [ ] In the shipped Flatpak build, a HEIC input becomes an upright JPEG with
  no metadata. This replaces the M1 refusal. Tested in CI with the installed
  Flatpak bundle, a plain HEIC with GPS and a rotated HEIC (`irot`).
  (UC-4, [FR-1](../../SPEC.md#functional), [FR-2](../../SPEC.md#functional), [NFR-2](../../SPEC.md#non-functional))
- [ ] The devcontainer and CI decode HEIC with RPM Fusion's
  `libheif-freeworld`; `libheif-tools` makes the HEIC fixtures.
  ([decision 0001](../../decisions/0001-heic-decoding-and-fixtures.md), option A)
- [ ] With no HEVC decoder at runtime, the user gets a clear message ("HEIC
  needs an HEVC decoder...") and the CLI exits with code 3. Never a crash or
  a raw library error. Tested end to end with the plugin folder hidden.
  ([S-6](../../SPEC.md#10-security-and-privacy-rules-must), UC-4)

### Process

- [ ] Every test runs in CI. None is skipped, ignored or silently degraded.
  GTK tests run under a headless Wayland compositor started by the test
  itself, so `cargo test --workspace` needs no extra setup.
- [ ] `cargo test -p safe-upload-core` still passes alone; core still has no
  GTK and makes no bus connection (SPEC 14).

## Waiting on the human

- **16.1 name and app ID.** M2 uses the placeholder `org.example.SafeUpload`
  everywhere an ID is needed. One task renames it once 16.1 is answered. The
  placeholder is fine for CI and local builds, not for Flathub (M4).
- **16.4 blur.** The criterion above lists blur. The blur task (T15) is the
  only task blocked on 16.4. If the human drops blur, the criterion and
  SPEC 6.3 change instead, and `U` is not bound.
- **16.5 share target.** Not in the M2 criteria. Deferred; nothing in M2
  depends on it.

## Out of scope for M2

Portal backend and FR-9 (M3). Translations set up, AppStream screenshots,
Flathub submission (M4). Strings are marked for gettext in M2 (NFR-5) but no
`po/` yet. WebP output (core refuses it; the format menu does not offer it).
Fit-to-size changes (lane `m2/fit-predict`, not part of this plan).
