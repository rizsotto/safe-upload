# M1 tasks

Each task is one branch (`m1/tNN-short-name`) and one PR under about 400
lines. Write the test first. "Needs" lists tasks that must merge first. Tasks
with no open needs can run in parallel.

| ID | Task | Needs | Test |
|---|---|---|---|
| T01 (done) | CI check: fail if `reqwest`, `hyper`, `ureq`, `curl`, `isahc`, `surf`, `tokio-native-tls`, `rustls` etc. appear in `Cargo.lock` (S-3) | none | Script run against a sample lock file with `reqwest` exits non-zero; clean lock passes |
| T02 (done) | `tests/fixtures/make.sh`: all M1 fixtures in SPEC 13. HEIF: only the 24 byte `ftyp` header, written with `printf` (plan, test strategy). The real HEIC fixture waits for M2 | none | `tests/fixtures/check.sh`: exiftool confirms GPS, MakerNotes, XMP, IPTC, thumbnail, orientations 1-8, PNG chunks, P3 profile are present; the HEIF header file starts with `ftypheic` at byte 4 |
| T03 (done) | Core skeleton: `Error`, `Options`, `OutputFormat`, size parsing, `Limits` | none | Unit: `2MB`, `500KB`, `1.5MiB`, bad input |
| T05 (done) | Spike: glycin ICC and orientation (no HEIC, decision 0001); bwrap sandbox in the devcontainer and on the CI runner. Tick SPEC 15 or open a decision | none | Spike notes in PR; a test that reads ICC and orientation from a JPEG fixture through glycin and fails if decoding ran without the bwrap sandbox (how to detect that is part of the spike). Runs in CI, not `#[ignore]` |
| T18 (done) | `sniff`: detect HEIC/HEIF by content (plan, "HEIC/HEIF is refused in M1"), return `Error::UnsupportedFormat(Format::Heif)` | T03 | Unit `heif_brands_are_refused`: `ftyp` with `heic`, `heix`, `mif1`, `msf1` refused; `avif` passes; JPEG and PNG bytes pass; HEIF header with a `.jpg` name refused; JPEG with a `.heic` name passes; header cut off after `ftyp` refused, no panic |
| T04 (done) | `output`: private temp dir, 0700/0600, 32-hex name, neutral names, delete dirs older than 1 hour | T03 | Unit with a fake `XDG_RUNTIME_DIR`: modes, name pattern, `image-1.jpg` batch names, old dir removed, new dir kept |
| T07 (done) | `orient`: apply orientation 1 to 8 | T03 | Unit: 2x3 synthetic image, expected pixel layout for each value |
| T08 (done) | `color`: ICC or CICP to sRGB with moxcms (pure Rust, our own call, not glycin's `color_convert_icc_srgb(true)`), drop the profile | T03 | Unit: P3 red converts to expected sRGB values within tolerance, from an ICC profile and from CICP; sRGB input unchanged; bad profile or CICP is an error |
| T09 (done) | `encode`: `Encoder` trait, JPEG (mozjpeg), PNG (+imagequant) | T03 | Unit: encode then decode round trip; output has only allowed markers/chunks |
| T10 (done) | `verify`: JPEG marker and PNG chunk allow list | T03 | Unit: hand-built bytes with APP1 Exif, APP13, XMP, `tEXt`, `eXIf`, second SOI each fail; clean files pass |
| T12 (done) | `edit`: crop, box, pixelate (SPEC 9.4). No blur yet (SPEC 16.4) | T03 | Unit: box pixels are solid; pixelate block size and noise bounds; crop size |
| T06 (done) | `decode`: glycin `Decoder` (T05), 200 MP limit, timeout, truncated file error. From the T05 findings (plan.md): force `SandboxSelector::Bwrap`, never `Auto`; check width and height against `Limits` after `load()` and before `next_frame()` (glycin allocates the full buffer for a 20000x20000 header); add our own truncation check (glycin decodes truncated JPEGs without an error); pass `Limits::decode_timeout` to glycin's `Limits::timeout` | T02, T03, T05 | Unit: decode each fixture except the HEIF header; huge image refused before any pixel buffer; truncated file gives `Error::Decode`, no panic; a test through the core decoder (not a hand-built `Loader`) asserts `active_sandbox_mechanism() == Bwrap` |
| T11 (done) | `fit`: fit-to-size search (SPEC 9.3) | T09 | Unit with fake encoder: under target, at least 85% when possible, scale down loop, 640 px floor error. Proptest with real JPEG, 100 KB to 10 MB |
| T13 (done) | `pipeline::prepare`: `sniff` first, then exact 9.1 order, format choice, verify, fail closed | T04, T06, T07, T08, T10, T11, T12, T18 | Integration in `crates/core/tests/`: every fixture prepares; injected leaky encoder returns error and leaves no file |
| T14 | CLI: `safe-upload <IN>... --max-size <SIZE> [-o OUT] [--max-edge N] [--format F]`. Exit codes from plan; check all inputs before processing | T13 | assert_cmd: `safe-upload in.jpg --max-size 2MB -o out.jpg` exits 0, out.jpg under 2,000,000 bytes |
| T15 | Leak and acceptance tests: exiftool allow list on all outputs, thumbnail, orientation-6 visual, HEIC refusal | T02, T14 | The tests themselves, run in CI. Includes `heic_input_is_refused_with_exit_code_3` (assert_cmd): HEIF header fixture gives exit 3, the HEIC message on stderr, no file at `-o`, nothing in the private temp dir; same with a batch of one JPEG and the HEIF file |
| T16 | `summary` (FR-8): sizes, resolutions, removed metadata kinds. Optional for M1 | T13 | Unit: GPS fixture reports "Location"; no metadata values in `Display` output |
| T17 | Accept M1: tick `spec.md` boxes with test names, update SPEC | T15 | Full `cargo test --workspace` in CI |

## Parallel waves

1. T01, T02, T03, T05
2. T04, T07, T08, T09, T10, T12, T18 (after T03); T06 (after T02, T03, T05)
3. T11 (after T09)
4. T13
5. T14, T16
6. T15
7. T17
