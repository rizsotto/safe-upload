# M1 tasks

Each task is one branch (`m1/tNN-short-name`) and one PR under about 400
lines. Write the test first. "Needs" lists tasks that must merge first. Tasks
with no open needs can run in parallel.

| ID | Task | Needs | Test |
|---|---|---|---|
| T01 | CI check: fail if `reqwest`, `hyper`, `ureq`, `curl`, `isahc`, `surf`, `tokio-native-tls`, `rustls` etc. appear in `Cargo.lock` (S-3) | none | Script run against a sample lock file with `reqwest` exits non-zero; clean lock passes |
| T02 | `tests/fixtures/make.sh`: all SPEC 13 fixtures (HEIC: generate, or commit a sample) | none | `tests/fixtures/check.sh`: exiftool confirms GPS, MakerNotes, XMP, IPTC, thumbnail, orientations 1-8, PNG chunks, P3 profile are present |
| T03 | Core skeleton: `Error`, `Options`, `OutputFormat`, size parsing, `Limits` | none | Unit: `2MB`, `500KB`, `1.5MiB`, bad input |
| T05 | Spike: glycin ICC, orientation, HEIC decode in the devcontainer; bwrap in podman. Tick SPEC 15 or open a decision | none | Spike notes in PR; an `#[ignore]` test that decodes the HEIC fixture |
| T04 | `output`: private temp dir, 0700/0600, 32-hex name, neutral names, delete dirs older than 1 hour | T03 | Unit with a fake `XDG_RUNTIME_DIR`: modes, name pattern, `image-1.jpg` batch names, old dir removed, new dir kept |
| T07 | `orient`: apply orientation 1 to 8 | T03 | Unit: 2x3 synthetic image, expected pixel layout for each value |
| T08 | `color`: ICC to sRGB with lcms2, drop profile | T03 | Unit: P3 red converts to expected sRGB values within tolerance; sRGB input unchanged |
| T09 | `encode`: `Encoder` trait, JPEG (mozjpeg), PNG (+imagequant) | T03 | Unit: encode then decode round trip; output has only allowed markers/chunks |
| T10 | `verify`: JPEG marker and PNG chunk allow list | T03 | Unit: hand-built bytes with APP1 Exif, APP13, XMP, `tEXt`, `eXIf`, second SOI each fail; clean files pass |
| T12 | `edit`: crop, box, pixelate (SPEC 9.4). No blur yet (SPEC 16.4) | T03 | Unit: box pixels are solid; pixelate block size and noise bounds; crop size |
| T06 | `decode`: `Decoder` impl chosen in T05, 200 MP limit, timeout, truncated file error | T02, T03, T05 | Unit: decode each fixture; huge image refused; truncated file gives `Error::Decode`, no panic |
| T11 | `fit`: fit-to-size search (SPEC 9.3) | T09 | Unit with fake encoder: under target, at least 85% when possible, scale down loop, 640 px floor error. Proptest with real JPEG, 100 KB to 10 MB |
| T13 | `pipeline::prepare`: exact 9.1 order, format choice, verify, fail closed | T04, T06, T07, T08, T10, T11, T12 | Integration in `crates/core/tests/`: every fixture prepares; injected leaky encoder returns error and leaves no file |
| T14 | CLI: `safe-upload <IN>... --max-size <SIZE> [-o OUT] [--max-edge N] [--format F]` | T13 | assert_cmd: `safe-upload in.jpg --max-size 2MB -o out.jpg` exits 0, out.jpg under 2,000,000 bytes |
| T15 | Leak and acceptance tests: exiftool allow list on all outputs, thumbnail, orientation-6 visual, HEIC to JPEG | T02, T14 | The tests themselves, run in CI |
| T16 | `summary` (FR-8): sizes, resolutions, removed metadata kinds. Optional for M1 | T13 | Unit: GPS fixture reports "Location"; no metadata values in `Display` output |
| T17 | Accept M1: tick `spec.md` boxes with test names, update SPEC | T15 | Full `cargo test --workspace` in CI |

## Parallel waves

1. T01, T02, T03, T05
2. T04, T07, T08, T09, T10, T12 (after T03); T06 (after T02, T03, T05)
3. T11 (after T09)
4. T13
5. T14, T16
6. T15
7. T17
