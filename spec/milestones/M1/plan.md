# M1 plan: Core + CLI (first draft)

Status: draft. Review before tasks start.

## Shape

- `safe-upload-core`: the whole pipeline (SPEC 9.1). No GTK, no D-Bus.
- `safe-upload-cli` (binary `safe-upload`): parses args, calls core, copies
  the verified result from the private temp dir to `-o`.
- Other crates stay empty stubs in M1.

## Core modules

| Module | Job | SPEC |
|---|---|---|
| `options` | `Options`, `OutputFormat`, size parsing (`2MB`) | FR-3 |
| `limits` | Max 200 MP, max file size 200 MB, decode timeout 30 s (T03 defaults, not from SPEC; change if needed) | S-4 |
| `sniff` | Detect HEIC/HEIF by content and refuse it, before decode | UC-4, S-6, decision 0001 |
| `decode` | Untrusted file to `Decoded` | 9.1.1, NFR-2 |
| `orient` | Apply EXIF orientation 1 to 8 to pixels | FR-2 |
| `edit` | Crop, box, pixelate (blur waits on SPEC 16.4) | FR-5, 9.4 |
| `color` | ICC to sRGB, then drop the profile | 9.1.5, 9.2 |
| `encode` | `Encoder` trait. JPEG (mozjpeg), PNG (with imagequant) | 8 |
| `fit` | Fit-to-size search, generic over `Encoder` | 9.3, FR-3, FR-4 |
| `output` | Private temp dir, neutral names, old dir cleanup | S-1, S-2, FR-6, FR-7 |
| `verify` | Re-read output, allow only image data and optional ICC | 9.1.8, S-6 |
| `summary` | Old/new size and resolution, removed metadata kinds | FR-8 (optional in M1) |
| `pipeline` | `prepare()` wires the steps in the exact 9.1 order | 9.1 |

## Key types (sketch)

```rust
pub struct Decoded {
    pub pixels: RgbaImage,        // straight alpha, 8 bit
    pub icc: Option<Vec<u8>>,
    pub orientation: Orientation, // 1..=8
}

pub trait Decoder {
    fn decode(&self, path: &Path, limits: &Limits) -> Result<Decoded, Error>;
}

pub struct Pixels { pub image: RgbaImage, pub has_alpha: bool } // sRGB only

pub trait Encoder {
    fn encode(&self, px: &Pixels, quality: u8) -> Result<Vec<u8>, Error>;
}

pub struct Options {
    pub target_bytes: u64,
    pub max_long_edge: Option<u32>,
    pub format: OutputFormat,     // Auto | Jpeg | Png | Webp
    pub keep_name: bool,
    pub edits: Vec<Edit>,         // crop + redactions, post-orientation coords
}

pub fn prepare(input: &Path, opts: &Options, out: &PrivateDir)
    -> Result<Prepared, Error>;
```

`Encoder` takes `Pixels` only. It cannot see source metadata (SPEC 8, key rule).
`Error` is one enum (thiserror). Messages never include metadata values (S-5).

## Choices made in this plan

- **Size units.** `MB` means 1,000,000 bytes, `KB` 1,000. `MiB`/`KiB` also
  accepted. Decimal is smaller, so the file fits sites that mean either.
- **CLI output.** Core always writes to the private temp dir and verifies
  there. The CLI then copies to `-o`. Without `-o`, it prints the temp path.
  `--max-size` is required in M1, so SPEC 16.3 does not block M1.
- **Format.** Auto: PNG with real alpha stays PNG. Everything else becomes
  JPEG. WebP output is not in M1 acceptance; add it only if time allows.
- **Verify in Rust, not exiftool.** `verify` walks JPEG markers and PNG
  chunks with an allow list (JPEG: SOI, DQT, SOF, DHT, SOS, DRI, EOI, APP0
  JFIF, APP2 ICC; PNG: IHDR, PLTE, tRNS, IDAT, IEND, iCCP, sRGB, gAMA, cHRM).
  `verify(bytes, expected_icc)`: an ICC profile must equal `expected_icc`
  byte for byte (our minimal sRGB profile, SPEC 9.1.5) or be absent. JPEG
  ICC chunks must come before SOF and join by sequence number; PNG iCCP
  must come before PLTE and IDAT. DQT, DHT, SOF, SOS and DRI must have no
  padding. PNG chunks have exact sizes, a fixed order and a checked CRC.
  JFIF, DRI, each quantization table id and every PNG chunk except IDAT
  appear at most once; SOS before SOF fails. The PNG iCCP name must be
  `verify::ICC_PROFILE_NAME` (`icc`); the T09 PNG encoder must write it.
  Not checked: the entropy-coded JPEG data and the IDAT content are not
  decoded, so data hidden inside them would pass. They come from our own
  encoder, which only gets pixels (SPEC 8).
  exiftool is used only in tests.
- **HEIC/HEIF is refused in M1** (decision 0001). `sniff` reads the first
  box of the file, never the name. It is HEIF if bytes 4..8 are `ftyp`. Then
  it reads the major brand and the compatible brands. Refuse if any brand is
  `heic`, `heix`, `heim`, `heis`, `hevc`, `hevx`, `mif1`, `mif2` or `msf1`,
  unless a brand is `avif` or `avis` (AVIF is HEIF with AV1, which stock
  Fedora decodes). A file cut off after the `ftyp` box is still refused. The
  check runs before glycin sees the file, so the result is the same on hosts
  that have an HEVC decoder. Error: `Error::UnsupportedFormat(Format::Heif)`.
- **Decode input formats.** `decode` accepts only JPEG, PNG, WebP and
  AVIF (SPEC 4), detected by content. Anything else (GIF, SVG, TIFF...)
  gives `Error::Decode` before glycin sees it. Each accepted format has a
  cut-off check: JPEG markers up to EOI, PNG chunks up to IEND, the RIFF
  size for WebP, top-level box sizes for AVIF.
- **CLI exit codes.** 0 success. 1 any other error. 2 usage error (clap
  default). 3 input format not supported in this build; in M1 only HEIC/HEIF.
  The CLI checks all inputs before it processes any. If one is HEIC/HEIF, it
  writes nothing and exits 3.
- **HEIC message** (no file path, S-5): "Input 1 is a HEIC/HEIF image. This
  version of safe-upload cannot read HEIC yet. Convert it to JPEG or PNG
  first, then try again. On an iPhone, Settings > Camera > Formats > Most
  Compatible saves new photos as JPEG."

## Dependencies to add (proposal)

clap, thiserror, image, glycin (or libheif-rs as fallback, see facts),
mozjpeg, imagequant, lcms2, getrandom. Dev: proptest, assert_cmd, tempfile.
None of them may pull in network crates (S-3). Check licenses fit
GPL-3.0-or-later.

## Facts from SPEC section 15 that M1 relies on

- [x] glycin exposes the ICC profile and orientation. This picks the decoder:
  glycin (crate 4.0.5). Checked in T05, see SPEC 15. HEIC decode is not
  needed in M1 (decision 0001).

### T05 findings (glycin spike)

- The `glycin` crate needs glib, gio, fontconfig and libseccomp (C) and
  talks to its loader processes with zbus in peer-to-peer mode over a
  socket pair. No session or system bus is used, so core still has no
  D-Bus service dependency and tests run without `dbus-daemon`. No GTK
  (`gdk4` is an optional feature we do not enable). No network crates.
- ICC: `color_convert_icc_srgb(false)` keeps the profile for our own
  conversion (T08). With `true` (the default) glycin converts to sRGB
  inside the sandbox; T08 may use that instead of lcms2.
- Orientation: `transformation_orientation()` returns gufo-common's
  `Orientation`, whose values are the EXIF numbers (glycin does not
  re-export the type). `apply_transformations(true)` applies it.
- Sandbox: core must use `SandboxSelector::Bwrap`, never `Auto`. Under a
  Docker-like seccomp profile `Auto` printed "Glycin running without
  sandbox" and decoded anyway; `Bwrap` failed with "Loader process exited
  early". `Image::active_sandbox_mechanism()` reports what ran.
- T06 must handle these itself, glycin does not:
  - `truncated.jpg` decodes without an error (the missing part is filled
    in). T06 needs its own check, for example a missing JPEG EOI.
  - `huge-dims.jpg` (header says 20000x20000) passes `load()` and
    `next_frame()` allocates the full buffer. Check `details()` width and
    height against `Limits` after `load()` and before `next_frame()`.
    glycin's own `Limits` are per side (default 65535 x 65535) with a 60 s
    timeout.

No other section 15 item is needed for M1. They are all portal or GTK facts.

## Risks found while bootstrapping (verify in task T05)

- HEIC: stock Fedora has no HEVC decoder or encoder. Settled by decision
  0001: M1 refuses HEIC, M2 adds decoding with RPM Fusion in dev and CI.
- glycin's bwrap works in our rootless podman devcontainer with the current
  `runArgs` (decision 0001, section 4, confirmed in T05). It may be blocked
  on GitHub runners. The T05 test forces `SandboxSelector::Bwrap`, and T06
  must do the same in the core decoder, so CI fails rather than passing
  with the sandbox off. The CI job runs its container
  with `seccomp=unconfined` and `apparmor=unconfined` and sets
  `kernel.apparmor_restrict_unprivileged_userns=0` on the runner. Not yet
  seen on a GitHub runner: check the first CI run. If bwrap still fails
  there, open a decision record (see SPEC 12, M2 open items).

## Test strategy

- **Fixtures.** `tests/fixtures/make.sh` builds every M1 file in SPEC 13 with
  ImageMagick and exiftool. Generated files are not committed. The HEIF
  refusal fixture is a 24 byte `ftyp` box written with `printf` (size
  `0x18`, `ftyp`, major brand `heic`, minor version 0, compatible brands
  `mif1` and `heic`). No HEVC encoder is needed. Tests call the script once,
  or CI runs it first.
- **No skipped tests.** Every test runs in CI. No `#[ignore]`, no "skip if
  tool missing".
- **Unit tests** per module, on small synthetic buffers. `fit` uses a fake
  encoder whose size is a known function of quality and scale.
- **Property test** (proptest): target from 100 KB to 10 MB. Output is under
  the target, or the result is a clear error.
- **Leak test**: run `exiftool -a -G1 -s` on every output. Allow only groups
  `ExifTool`, `System`, `File`, `JFIF`, `PNG` (IHDR fields), `Composite`, and
  `ICC_Profile` when kept. Any other key fails.
- **Thumbnail test** (S-7): the fixture thumbnail shows a different image.
  Assert no second SOI in the output and no thumbnail tag in exiftool output.
- **Visual test**: orientation 1 to 8 outputs match a reference within a small
  per-pixel tolerance.
- **Fail closed test**: inject an encoder that adds an APP1 segment. `prepare`
  must return an error and leave no file behind.
- **CLI test** (assert_cmd): the exact M1 command line.

## Devcontainer packages to add

The devcontainer starts minimal (Rust only). Add these Fedora packages to
`.devcontainer/Containerfile` in the task that first needs them:

`libheif-devel libwebp-devel nasm cmake lcms2-devel libseccomp-devel glib2-devel fontconfig-devel pkgconf-pkg-config glycin-loaders bubblewrap perl-Image-ExifTool perl-IO-Compress ImageMagick xdg-desktop-portal dbus-daemon python3-dbusmock`

(T05: the `glycin` crate needs `glib2-devel` and `fontconfig-devel`, not
`glycin-devel`, which is the C library. T02: exiftool needs
`perl-IO-Compress` to write `zTXt`.)

`libheif-tools`, `libheif-ffmpeg` and RPM Fusion's `libheif-freeworld` are
for HEIC only. They wait for M2 (decision 0001).

Also add `gtk4-devel libadwaita-devel` and `xvfb-run` for GTK tests when the
first GTK crate gets real code.
