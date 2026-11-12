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
| `limits` | Max 200 MP, max file size, decode timeout | S-4 |
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
  exiftool is used only in tests.

## Dependencies to add (proposal)

clap, thiserror, image, glycin (or libheif-rs as fallback, see facts),
mozjpeg, imagequant, lcms2, getrandom. Dev: proptest, assert_cmd, tempfile.
None of them may pull in network crates (S-3). Check licenses fit
GPL-3.0-or-later.

## Facts from SPEC section 15 that M1 relies on

- [ ] glycin exposes the ICC profile and orientation, and decodes HEIC in our
  runtime. This picks the decoder. If it fails, use `image` + `libheif-rs`
  and run them in a sandbox or accept memory-safe decoders only (NFR-2). If
  neither path works, open a decision record.

No other section 15 item is needed for M1. They are all portal or GTK facts.

## Risks found while bootstrapping (verify in task T05)

- Fedora's `libheif` has no HEVC decoder. `libde265` and `x265` are only in
  RPM Fusion. The devcontainer installs `libheif-ffmpeg`, which may decode
  HEVC through Fedora's ffmpeg. Not verified. glycin's HEIF loader has the
  same dependency. If HEIC decode fails, M1's "HEIC becomes JPEG" is blocked.
  Add this as a section 15 fact.
- `heif-enc` needs an HEVC encoder to make the HEIC fixture. Stock Fedora
  likely has none. Fallback: commit a small HEIC file with a known free
  license.
- glycin runs loaders under bubblewrap. bwrap may not work inside a rootless
  podman container. Check, and document any extra `runArgs`.

## Test strategy

- **Fixtures.** `tests/fixtures/make.sh` builds every file in SPEC 13 with
  ImageMagick and exiftool. Generated files are not committed (except the HEIC
  file if needed). Tests call the script once, or CI runs it first.
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

`libheif-devel libheif-tools libheif-ffmpeg libwebp-devel nasm cmake lcms2-devel libseccomp-devel glycin-devel glycin-loaders bubblewrap perl-Image-ExifTool ImageMagick xdg-desktop-portal dbus-daemon python3-dbusmock`

Also add `gtk4-devel libadwaita-devel` and `xvfb-run` for GTK tests when the
first GTK crate gets real code.
