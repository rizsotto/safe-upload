# M1: Core + CLI

Source: SPEC section 12, M1. IDs point to SPEC sections 5 (FR, NFR) and 10 (S).

## Acceptance criteria (SPEC section 12)

Accepted 2026-11-12 (T17). Evidence: the full CI sequence of
`.github/workflows/ci.yml` passed in the devcontainer (no-network self test
and check, `make.sh`, `check.sh` "all checks passed", fmt, clippy `--locked
-D warnings`, `cargo test --locked --workspace`: 149 tests, 0 failed, 0
ignored). Test names below are `file::test`.

- [x] `safe-upload in.jpg --max-size 2MB -o out.jpg` works.
  ([FR-3](../../SPEC.md#functional), [FR-4](../../SPEC.md#functional), [FR-7](../../SPEC.md#functional))
  Evidence: `cli.rs::m1_command_line_works`. By hand (release build) on
  `exif-gps-thumb.jpg` copied to `in.jpg`: exit 0, 15,060 to 6,939 bytes,
  640x480, mode 0600, summary "removed: Location, Camera, Date, Maker
  notes, Thumbnail, XMP, IPTC". The private folder was removed afterwards.
- [x] All fixture tests pass (SPEC section 13).
  ([FR-1](../../SPEC.md#functional), [NFR-2](../../SPEC.md#non-functional), [S-4](../../SPEC.md#10-security-and-privacy-rules-must))
  Evidence: `tests/fixtures/check.sh` passes on every M1 fixture
  (the HEIC made with `heif-enc` is M2, decision 0001).
  `decode.rs::decodes_every_fixture`, `decodes_real_webp_and_avif_in_the_sandbox`,
  `pipeline.rs::every_fixture_prepares`, `bad_inputs_leave_no_file`, and
  `leak.rs` runs the CLI on all 18 accepted fixtures. Decoding runs in bwrap:
  `decode_sandbox.rs::bwrap_is_used_even_when_auto_would_drop_the_sandbox`,
  `decode.rs` unit `core_decoder_runs_in_bwrap`.
- [x] `exiftool out.jpg` shows no fields except basic file and image properties.
  ([FR-1](../../SPEC.md#functional), [S-6](../../SPEC.md#10-security-and-privacy-rules-must))
  Evidence: `leak.rs::exiftool_shows_only_allowed_keys_on_every_output`
  (`exiftool -a -u -G1 -s`, allow list per group, all outputs). By hand,
  `exiftool out.jpg` printed only: ExifTool version, file name, directory,
  size, dates, permissions, type, extension, MIME type, JFIF version,
  resolution unit and X/Y resolution, width, height, encoding process, bits
  per sample, color components, YCbCr subsampling, image size, megapixels.
  The input had 58 tags.
- [x] An embedded EXIF thumbnail is never present in the output.
  ([FR-1](../../SPEC.md#functional), [S-7](../../SPEC.md#10-security-and-privacy-rules-must))
  Evidence: `leak.rs::no_embedded_thumbnail_in_any_output` (one SOI only, no
  Thumbnail or Preview tag, all outputs; `check.sh` proves the input has a
  thumbnail that shows a different image). `verify.rs` units
  `second_soi_fails`, `jfif_with_thumbnail_fails`, `jpeg_metadata_segments_fail`.
- [x] An orientation-6 input comes out upright.
  ([FR-2](../../SPEC.md#functional))
  Evidence: `leak.rs::orientation_6_comes_out_upright` (pixel quadrants
  match the reference, no Orientation tag), `every_orientation_comes_out_upright`,
  `pipeline.rs::orientation_6_swaps_the_size`, `orient.rs` unit
  `each_orientation_gives_the_expected_layout`. By hand: 64x96 in, 96x64 out.
- [x] A HEIC/HEIF input is detected by content, not file name, and refused.
  The CLI prints a clear message that says what to do and exits with code 3.
  No crash, no garbled output, no output file, never the original passed
  through. Test: `heic_input_is_refused_with_exit_code_3` (T18, T15).
  HEIC decoding moves to M2 ([decision 0001](../../decisions/0001-heic-decoding-and-fixtures.md)).
  ([S-6](../../SPEC.md#10-security-and-privacy-rules-must), UC-4)
  Evidence: `leak.rs::heic_input_is_refused_with_exit_code_3` (also the same
  bytes named `.jpg`, and a batch with a JPEG: exit 3, message, empty stdout,
  no file at `-o`, nothing in the private folder). `sniff.rs` unit
  `heif_brands_are_refused`, `sniff.rs::heif_header_named_jpg_is_refused`,
  `pipeline.rs::heif_is_refused_before_the_decoder_runs`. By hand: exit 3,
  the plan's message, no `h.jpg`.

## Also required by the spec for core work

These are not listed under M1 in section 12, but they are MUST rules for code
written in M1.

- [x] Output is written to `$XDG_RUNTIME_DIR/safe-upload/<random-32-hex>/`,
  dir 0700, file 0600, before it is copied to `-o`.
  ([S-1](../../SPEC.md#10-security-and-privacy-rules-must), [FR-7](../../SPEC.md#functional))
  Evidence: `output.rs::folder_and_file_modes`, `folder_name_is_32_hex_and_random`,
  `pipeline.rs::every_fixture_prepares` (path in the private folder, mode
  0600), `cli.rs::without_o_prints_the_private_path`,
  `cli.rs::output_file_has_mode_0600`, `never_overwrites_an_input`.
- [x] Temp folders older than 1 hour are deleted on start.
  ([S-2](../../SPEC.md#10-security-and-privacy-rules-must))
  Evidence: `output.rs::old_folders_are_removed_and_new_ones_kept`,
  `stale_symlink_is_not_followed`, and through the CLI
  `cli.rs::stale_private_folders_are_removed_on_start` (added in T17).
  Note: the CLI cleans up when it creates its folder, after the input
  check, so a run that refuses every input cleans nothing. The cleanup
  timer in S-2 is for the long-running app and portal (M2, M3).
- [x] CI fails if a network crate appears in `Cargo.lock`.
  ([S-3](../../SPEC.md#10-security-and-privacy-rules-must), [NFR-3](../../SPEC.md#non-functional))
  Evidence: `build-aux/ci/test-check-no-network-crates.sh` ("no-network self
  test: ok") and `check-no-network-crates.sh Cargo.lock crates` ("clean"),
  both steps in `ci.yml`.
- [x] Images above 200 MP are refused. A truncated file gives a clean error.
  ([S-4](../../SPEC.md#10-security-and-privacy-rules-must))
  Evidence: `decode.rs::huge_image_is_refused_before_decoding` (400 MP header,
  refused before the pixel buffer), `limits.rs` unit
  `default_allows_200_megapixels`, `decode.rs::truncated_files_give_a_decode_error`,
  `decode.rs` unit `every_cut_is_found`, `cli.rs::other_errors_exit_1_and_write_nothing`.
- [x] Output never exceeds the target. It is at least 85% of the target when
  possible. Impossible targets give a clear error. Property test covers
  100 KB to 10 MB.
  ([FR-3](../../SPEC.md#functional), [FR-4](../../SPEC.md#functional))
  Evidence: `fit.rs::output_is_under_target_or_a_clear_error` (proptest, real
  JPEG encoder, 100 KB to 10 MB; in T17 it got a 1600x1200 image and an
  85% check with the real encoder, 64 cases passed once by hand),
  `fit.rs` units `under_target_and_at_least_85_percent`,
  `floor_of_640_px_gives_an_error`, `cli.rs::impossible_target_gives_a_clear_error`
  (added in T17). By hand on a 12 MP JPEG: 300 KB gave 286 KB, 1 MB gave
  932 KB, 2 MB gave 1.9 MB.
- [x] Non-sRGB input (Display P3) is converted to sRGB.
  (SPEC 9.1 step 5, 9.2)
  Evidence: `pipeline.rs::p3_output_is_converted_to_srgb` (ICC and CICP),
  `color.rs::fixture_p3_profile_converts_to_srgb`, `color.rs` units,
  `decode.rs::avif_with_cicp_only_converts_to_srgb`.
- [x] Verification fails closed when metadata is found in the output.
  ([S-6](../../SPEC.md#10-security-and-privacy-rules-must))
  Evidence: `pipeline.rs::leaky_encoder_fails_closed_and_leaves_no_file`
  (APP1 added by the encoder: `Error::Verify`, no file ever created), the
  `verify.rs` units.
- [x] No file paths or metadata values in info logs.
  ([S-5](../../SPEC.md#10-security-and-privacy-rules-must))
  Evidence: core and CLI have no logging at all (no `log` or `tracing`
  calls). glycin uses `tracing` and `log`, but nothing installs a
  subscriber: by hand with `RUST_LOG=info`, `debug` and `trace` and
  `G_MESSAGES_DEBUG=all`, stderr had only the summary line. Error and
  summary text: `cli.rs::errors_do_not_show_paths`,
  `prints_a_summary_on_stderr`, `summary.rs` unit
  `display_has_sizes_and_kinds_only`, `pipeline.rs::summary_names_removed_kinds_without_values`.
- [x] `cargo test -p safe-upload-core` passes alone (SPEC section 14).
  Evidence: `cargo test --locked -p safe-upload-core` in the devcontainer,
  all passed.

## NFR-1 timing (measured, not an acceptance box)

NFR-1: a 12 MP JPEG to 2 MB in under 2 s on a mid-range laptop (target).
Release build, devcontainer, AMD Ryzen 7 PRO 7840HS (8 cores, 16 threads),
`safe-upload big.jpg --max-size 2MB -o out.jpg`, 4000x3000 synthetic photos
from ImageMagick (`plasma:fractal` plus noise, no real camera file):

| Input | Output | Wall time |
|---|---|---|
| 6.9 MB, mild noise | 1.9 MB at 4000x3000 | 2.68 to 2.75 s (3 runs), 2.83 s on 4 cores |
| 7.1 MB, other random plasma | 1.9 MB | 1.91 s |
| 18.3 MB, heavy noise | 2.0 MB at 4000x3000 | 4.23 to 4.37 s (3 runs), 4.63 s on 4 cores |

The same input with a 100 MB target (decode, one encode round, verify,
write) took 1.24 s, so the fixed part is large. How it splits between the
sandboxed decode and the first encode round was not measured.
NFR-1 is not met on this laptop, which is above mid-range. The SPEC calls
it a target, so this does not block M1. Real phone photos may behave
differently from these synthetic ones.

## Out of scope for M1

GTK app, redaction editor UI, portal backend, blur (waits on SPEC 16.4), FR-9, HEIC
decoding (M2, decision 0001).
