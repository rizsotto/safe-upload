# M1: Core + CLI

Source: SPEC section 12, M1. IDs point to SPEC sections 5 (FR, NFR) and 10 (S).

## Acceptance criteria (SPEC section 12)

- [ ] `safe-upload in.jpg --max-size 2MB -o out.jpg` works.
  ([FR-3](../../SPEC.md#functional), [FR-4](../../SPEC.md#functional), [FR-7](../../SPEC.md#functional))
- [ ] All fixture tests pass (SPEC section 13).
  ([FR-1](../../SPEC.md#functional), [NFR-2](../../SPEC.md#non-functional), [S-4](../../SPEC.md#10-security-and-privacy-rules-must))
- [ ] `exiftool out.jpg` shows no fields except basic file and image properties.
  ([FR-1](../../SPEC.md#functional), [S-6](../../SPEC.md#10-security-and-privacy-rules-must))
- [ ] An embedded EXIF thumbnail is never present in the output.
  ([FR-1](../../SPEC.md#functional), [S-7](../../SPEC.md#10-security-and-privacy-rules-must))
- [ ] An orientation-6 input comes out upright.
  ([FR-2](../../SPEC.md#functional))
- [ ] A HEIC input becomes a JPEG.
  (UC-4, [FR-1](../../SPEC.md#functional))

## Also required by the spec for core work

These are not listed under M1 in section 12, but they are MUST rules for code
written in M1.

- [ ] Output is written to `$XDG_RUNTIME_DIR/safe-upload/<random-32-hex>/`,
  dir 0700, file 0600, before it is copied to `-o`.
  ([S-1](../../SPEC.md#10-security-and-privacy-rules-must), [FR-7](../../SPEC.md#functional))
- [ ] Temp folders older than 1 hour are deleted on start.
  ([S-2](../../SPEC.md#10-security-and-privacy-rules-must))
- [ ] CI fails if a network crate appears in `Cargo.lock`.
  ([S-3](../../SPEC.md#10-security-and-privacy-rules-must), [NFR-3](../../SPEC.md#non-functional))
- [ ] Images above 200 MP are refused. A truncated file gives a clean error.
  ([S-4](../../SPEC.md#10-security-and-privacy-rules-must))
- [ ] Output never exceeds the target. It is at least 85% of the target when
  possible. Impossible targets give a clear error. Property test covers
  100 KB to 10 MB.
  ([FR-3](../../SPEC.md#functional), [FR-4](../../SPEC.md#functional))
- [ ] Non-sRGB input (Display P3) is converted to sRGB.
  (SPEC 9.1 step 5, 9.2)
- [ ] Verification fails closed when metadata is found in the output.
  ([S-6](../../SPEC.md#10-security-and-privacy-rules-must))
- [ ] No file paths or metadata values in info logs.
  ([S-5](../../SPEC.md#10-security-and-privacy-rules-must))
- [ ] `cargo test -p safe-upload-core` passes alone (SPEC section 14).

## Out of scope for M1

GTK app, redaction editor UI, portal backend, blur (waits on SPEC 16.4), FR-9.
