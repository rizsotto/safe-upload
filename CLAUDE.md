# safe-upload: notes for coding agents

Read `spec/SPEC.md` fully before writing code. Then read `spec/workflow.md`.
SPEC.md section 14 ("Instructions for agents") and section 10/11 (security
rules) are binding.

## Commands (run inside the devcontainer)

- Format: `cargo fmt --all`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Test: `xvfb-run -a cargo test --workspace`

## Rules

- Work on the current milestone only. Its files are in `spec/milestones/MN/`.
- One task per branch and pull request. Test first.
- Never use real keys, accounts, servers or the user's real home directory in
  tests.
- Missing decision: follow "Blocked on a decision" in `spec/workflow.md`.
  Do not guess.
- If a fact in SPEC.md section 15 turns out wrong, fix SPEC.md in the same PR.

## Project rules (from SPEC.md sections 7.3, 8, 10, 14)

- `safe-upload-core` has no GTK and no D-Bus. `cargo test -p safe-upload-core`
  must pass on its own.
- The encoder gets only pixels plus an optional ICC profile. Never copy
  metadata blocks from the source (SPEC 8, "Key rule").
- Keep the pipeline order in SPEC 9.1 exactly. Orientation before strip.
  Redact before encode.
- Fail closed (S-6). If output verification finds metadata, return an error.
  Never fall back to the original file.
- Never write next to the original. Temp files go in
  `$XDG_RUNTIME_DIR/safe-upload/<random-32-hex>/`, dir 0700, files 0600 (S-1).
- Input files are hostile: sandboxed or memory-safe decoding, 200 MP limit,
  timeouts (S-4, NFR-2).
- No network crates, ever (S-3, NFR-3).
- No file paths or metadata values in info logs (S-5).
- Every leak or bug fix starts with a failing test. Thumbnail leaks need a
  test (S-7).
- Desktop rules D-1 to D-3: public cross-desktop APIs only, detect features at
  runtime, no hard dependency on Nautilus. Do not branch on
  `XDG_CURRENT_DESKTOP`.
- Do not start M3 before M1 passes.
