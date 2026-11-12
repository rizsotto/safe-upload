# 0001: HEIC decoding and fixtures in the devcontainer and CI

- Status: accepted
- Blocks: M2 (HEIC acceptance items in SPEC 12). No longer blocks M1: M1
  refuses HEIC (M1 task T18, test `heic_input_is_refused_with_exit_code_3`).
- SPEC refs: section 8 (Decoding), 12 M1, 13 (fixtures), 15 (glycin HEIC
  fact), UC-4, NFR-2, S-4

## Question

Stock Fedora has no HEVC codec. HEIC images are HEVC inside a HEIF box. So:

1. What can decode HEIC out of the box in Fedora 45 and in the GNOME Flatpak
   runtime?
2. How do the devcontainer and CI get HEIC decode?
3. How do we get the HEIC fixture from SPEC 13?
4. Does glycin's bubblewrap sandbox work in our rootless podman devcontainer?
5. What does this mean for shipping on Fedora and on Flathub?

## Context

All tests below ran on 2026-11-12 on a Fedora 45 host with rootless podman
6.1.3, in throwaway `fedora:45` containers. The test HEIC was made with
`heif-enc -e x265` in a container with RPM Fusion.

| Setup | HEIC decode | HEIC encode |
|---|---|---|
| Fedora 45 stock: `libheif` 1.23.7, `libheif-ffmpeg`, `ffmpeg-free` 9.0.2, `glycin-loaders` 2.2.6 | No | No |
| Fedora 45 + RPM Fusion free: `libheif-freeworld` 1.23.7 (`libde265` 1.1.3, `x265` 4.3) | Yes. `heif-dec` and glycin (in bwrap) | Yes. `heif-enc -e x265` |
| Freedesktop Platform 26.08 + `org.freedesktop.Platform.codecs-extra` (Flathub) | Yes. `glycin-thumbnailer` | Not tested |
| GNOME 51 Platform | Expected yes (same libheif and extension). Not tested | Not tested |

### 1. What decodes HEIC out of the box

**Fedora 45: nothing.**

- Fedora's `libheif.spec` never enables `WITH_LIBDE265` or `WITH_X265`. It
  ships AOM, dav1d, JPEG, OpenJPEG, OpenH264 and the FFmpeg plugin.
  `heif-dec --list-decoders` shows an empty "HEIC decoders" list.
- `libheif-ffmpeg` does not help. Fedora's `ffmpeg.spec` builds
  `ffmpeg-free` with `--disable-decoder="h264,hevc,vc1,vvc"`.
  `ffmpeg -decoders` lists no HEVC decoder at all. It lists only hardware
  HEVC *encoders* (VAAPI, NVENC and so on).
- Decoding fails with: `No decoding plugin installed for this compression
  format: HEVC (a suitable decoder plugin is libde265)`.
- Fedora's glycin heif loader (`/usr/libexec/glycin-loaders/2+/glycin-heif`)
  links the system `libheif.so.1`. It fails with the same error
  (`PluginLoadingError(NoMatchingDecoderInstalled)`).
- RPM Fusion free has `libheif-freeworld`. It adds the `libde265`, `x265`,
  `vvdec`, `vvenc` and `x264` plugins to `/usr/lib64/libheif/`. With it, both
  `heif-dec` and glycin decode our test HEIC, 8 bit and 10 bit.

**GNOME Flatpak runtime: yes, through an extension.**

- GNOME 51 (`gnome-build-meta` branch `gnome-51`) is built on Freedesktop SDK
  26.08. The GNOME Platform ships glycin and its loaders (`sdk/glycin.bst`,
  listed in `sdk-platform.bst`). The heif loader uses the Freedesktop
  `components/libheif.bst`.
- That base libheif is built with `-DWITH_X265=OFF -DWITH_LIBDE265=OFF`. Its
  plugin dir is fixed to `/usr/lib/x86_64-linux-gnu/codecs-extra/lib/libheif`.
- The `org.freedesktop.Platform.codecs-extra` extension puts
  `libheif-libde265.so` and `libheif-x265.so` there. The Platform declares
  this extension without `no-autodownload`, so Flatpak installs it with the
  runtime from Flathub.
- On this host, `glycin-thumbnailer` inside `org.freedesktop.Platform//26.08`
  decoded the test HEIC. I did not have the GNOME 51 runtime installed, so
  GNOME itself is inferred from the build files, not tested.

### 2. Pure Rust HEVC decoders

Two exist. Neither is mature enough to pick for M1.

- `heifer` 0.5.0 (MIT OR Apache-2.0). Decode and encode, `forbid(unsafe)`,
  fuzzed, claims 62/62 conformance streams match ffmpeg. Released 2026-11-07,
  five days before this record. 0.x API.
- `heic` 0.1.6 (Imazen). AGPL-3.0 or commercial. 0.2.0 was yanked. The README
  says much of the code was written by an AI and not all reviewed.

A memory-safe decoder would meet NFR-2 without a sandbox. It is worth a new
look after M1, as the `image` fallback path.

### 3. Fixture

- Stock Fedora `heif-enc` has no HEVC encoder, and ImageMagick writes HEIC
  through libheif, so it has none either.
- With RPM Fusion, `heif-enc -e x265` works. `exiftool` can add GPS EXIF to
  the HEIC afterwards. Tested recipe: a 640x480 8 bit gradient gives a
  1.6 KB file with Main profile, 8 bit, 200 bytes of EXIF with GPS. glycin
  decodes it in the sandbox.
- Note: `heif-enc -b 8` is wrong. `-b` only applies to 16 bit PNG input. An 8
  bit PNG already gives an 8 bit HEIC.
- Real iPhone files use a grid of 512x512 tiles. The generated file has no
  grid. A grid fixture would test more, but it is not needed for M1.

### 4. glycin and bubblewrap in rootless podman

**Works with the current `runArgs`. No change needed.**

- With `--userns=keep-id --security-opt label=disable` and podman's default
  seccomp profile, glycin logged `bwrap sandboxing available: true` and
  started each loader as `bwrap --unshare-all --die-with-parent ...`. PNG and
  HEIC both loaded (HEIC only with RPM Fusion).
- A plain `bwrap --proc /proc ...` fails in the same container with `Can't
  mount proc on /proc`. It works with `--security-opt unmask=/proc/*`. glycin
  does not mount `/proc`, so it does not need this.
- glycin has two ways to turn the sandbox off:
  - Env var `GLYCIN_DISABLE_SANDBOX=i-know-the-risks` (only for the `Auto`
    selector).
  - API `Loader::sandbox_selector(SandboxSelector::NotSandboxed)`.
- Watch out: with the default `Auto` selector, glycin *silently* falls back to
  no sandbox if bwrap is blocked (it only prints a warning to stderr). This is
  `RunEnvironment::HostBwrapSyscallsBlocked` in `glycin-core/src/util.rs`.
  NFR-2 and S-4 want sandboxed decoding. T05/T06 should decide if core forces
  `SandboxSelector::Bwrap` on the host, so it fails closed. If it does, tests
  need a way to pick the selector.
- `glycin-thumbnailer` always uses `NotSandboxed`, because desktop
  thumbnailers run in their own sandbox. Do not use it to test the sandbox.

### 5. Legal and packaging

Not legal advice.

- HEVC is covered by patents. This is why Fedora leaves out HEVC decoders
  and RPM Fusion carries them.
- **Fedora package:** it can depend on `glycin-loaders` only. It cannot
  require `libheif-freeworld`. HEIC works only if the user has RPM Fusion.
  The phase 3 portal backend is a host package, so it has the same limit.
  The app should show a clear error ("HEIC needs an HEVC decoder") when glycin
  returns `NoMatchingDecoderInstalled`. It must not bundle HEVC code.
- **Flathub:** use the GNOME runtime's glycin loaders. HEVC comes from the
  `codecs-extra` extension, which Flathub hosts. Do not bundle `libde265` or
  `x265` in our manifest. Inside Flatpak, glycin uses `flatpak-spawn
  --sandbox`. It turns the sandbox off under `flatpak-builder --run` for app
  IDs that end in `Devel`.
- **Licenses:** `libde265` is LGPL-3.0-or-later, `x265` is GPL-2.0-or-later,
  `libheif-freeworld` is LGPL-3.0-or-later and MIT. All fit GPL-3.0-or-later.
  We do not link them. glycin runs loaders as separate processes. `heifer` is
  MIT/Apache. `heic` is AGPL, so avoid it.
- **Fixture:** a generated gradient has no third-party content. If we ever
  commit it, mark it as our own work under the repo license.

### Not verified

- GNOME 51 runtime decoding HEIC. Inferred from `gnome-build-meta` and a test
  on Freedesktop 26.08, not run in the GNOME runtime.
- That `codecs-extra` is reachable from glycin's `flatpak-spawn --sandbox`
  subsandbox. My Flatpak test used `glycin-thumbnailer`, which runs with no
  sandbox.
- CI. `ci.yml` uses `devcontainers/ci` on `ubuntu-latest`, which runs Docker.
  `--userns=keep-id` is a podman flag. Docker probably rejects it. Docker's
  default seccomp and Ubuntu's AppArmor userns limit probably block bwrap,
  and glycin would then fall back to no sandbox. HEIC decode would still work
  with this decision. Check this on the first CI run.
- How often RPM Fusion lags Fedora. `libheif-freeworld` requires the exact
  same `libheif` version (`libheif(x86-64) = 1.23.7`). If Fedora updates
  `libheif` first, an image build could fail until RPM Fusion catches up.

A full build of the project Containerfile with the change below was tested.
It built, the `dev` user has the x265 encoder, and glycin decoded the HEIC
fixture with `bwrap sandboxing available: true`. It was not opened through
VS Code or the devcontainer CLI.

## Options

### A. Enable RPM Fusion free in the Containerfile and generate the fixture

Install `rpmfusion-free-release`, then `libheif-freeworld`. glycin decodes
HEIC in its sandbox. `make.sh` makes the HEIC fixture with `heif-enc -e x265`.

Pros:
- Tested. Decode and encode both work, glycin sandbox included.
- Same path as a real Fedora user with RPM Fusion. Same glycin code as on
  Flathub.
- No binary in git. All fixtures come from `make.sh`.
- CI uses the same image, so it gets the same codecs.

Cons:
- Dev and CI images pull from a third-party repo.
- Version lock between `libheif` and `libheif-freeworld` can break a build
  for a few days.
- `make.sh` does not work on a stock Fedora host.

### B. Rely on `ffmpeg-free` through `libheif-ffmpeg`

Pros:
- Stock Fedora only.

Cons:
- Does not work. `ffmpeg-free` is built without an HEVC decoder.

### C. Pure Rust HEVC decoder (`heifer` or `heic`)

Pros:
- Memory safe, so NFR-2 holds without a sandbox. No RPM Fusion.
- `heifer` can also encode, so it could make the fixture.

Cons:
- `heifer` is five days old (0.5.0). `heic` is AGPL and partly unreviewed.
- Moves away from the glycin-first plan in SPEC 8.
- Patent question stays the same, and now the code is inside our binary.

### D. Test HEIC only in a Flatpak runtime

Pros:
- Matches the Flathub app. No RPM Fusion.

Cons:
- M1 is a CLI tested with `cargo test`. There is no manifest until M2.
- Running Flatpak inside podman is extra work and was not tested.
- Still needs an HEVC encoder for the fixture.

### E. Commit a HEIC fixture and skip HEIC decode in CI

Pros:
- No encoder needed.

Cons:
- Does not meet "A HEIC input becomes a JPEG". Decode still needs HEVC.

## Preferred option and why

Note after acceptance: the steps below now apply to M2, not M1. Read "T05"
as "the first M2 HEIC task". The `#[ignore]` test in step 4 is replaced by a
normal test that runs in CI. See "Human decision".

**Option A.** It is the only tested path that gives HEIC decode through glycin
in the devcontainer and CI. The same packages also give the encoder for the
fixture. No `runArgs` change is needed.

Fallback for the fixture only: if RPM Fusion is ever dropped from the image,
generate `heic-gps.heic` once with the recipe below and commit it as
`tests/fixtures/heic-gps.heic`, with a note on how it was made.

### Steps

1. `.devcontainer/Containerfile`: swap `libheif-ffmpeg` for
   `libheif-freeworld` in `EXTRA_PACKAGES`, and add the RPM Fusion step before
   the main install:

   ```diff
   -ARG EXTRA_PACKAGES="libheif-devel libheif-tools libheif-ffmpeg libwebp-devel nasm cmake lcms2-devel libseccomp-devel glycin-devel glycin-loaders bubblewrap perl-Image-ExifTool ImageMagick xdg-desktop-portal dbus-daemon python3-dbusmock"
   +ARG EXTRA_PACKAGES="libheif-devel libheif-tools libheif-freeworld libwebp-devel nasm cmake lcms2-devel libseccomp-devel glycin-devel glycin-loaders bubblewrap perl-Image-ExifTool ImageMagick xdg-desktop-portal dbus-daemon python3-dbusmock"
   +# HEVC for HEIC decode and the HEIC fixture (decision 0001).
   +RUN dnf -y install "https://mirrors.rpmfusion.org/free/fedora/rpmfusion-free-release-$(rpm -E %fedora).noarch.rpm"
   +
    RUN dnf -y install \
   ```

   `libheif` still pulls in `libheif-ffmpeg` as a weak dependency. That is
   fine.

2. `.devcontainer/devcontainer.json`: no change. The current `runArgs`
   (`--userns=keep-id`, `label=disable`) are enough for glycin's bwrap.

3. `tests/fixtures/make.sh` (task T02), HEIC part:

   ```sh
   if ! heif-enc --list-encoders | grep -q '^- x265 '; then
       echo "make.sh: heif-enc has no x265 encoder. See spec/decisions/0001." >&2
       exit 1
   fi
   magick -size 640x480 gradient:red-blue -depth 8 PNG24:"$tmp/heic-src.png"
   heif-enc -e x265 -q 60 -o "$out/heic-gps.heic" "$tmp/heic-src.png"
   exiftool -q -overwrite_original \
       -GPSLatitude=47.5 -GPSLatitudeRef=N \
       -GPSLongitude=19.04 -GPSLongitudeRef=E \
       -Make=Test -Model=Fixture "$out/heic-gps.heic"
   ```

   `check.sh` confirms GPS with `exiftool -G1 -s -GPSLatitude`.

4. T05: add the `#[ignore]` test that decodes `heic-gps.heic` with glycin.
   Run it once with `RUST_LOG=glycin_core=debug` and check that the log shows
   `bwrap sandboxing available: true`. Record the result in the PR.

5. T05: decide the sandbox selector (see section 4 above). Map
   `NoMatchingDecoderInstalled` to a clear `Error` variant.

6. After this is accepted: tick the HEIC half of the glycin fact in SPEC 15
   with a link here, and update the "Risks" list in
   `spec/milestones/M1/plan.md`.

7. First CI run: check the Docker and `--userns=keep-id` question from "Not
   verified". Fixing CI is out of scope for this record.

## Sources

Fedora (branch `f45`, fetched 2026-11-12):
- https://src.fedoraproject.org/rpms/libheif/blob/f45/f/libheif.spec (cmake
  flags: no `WITH_LIBDE265`, no `WITH_X265`; `WITH_FFMPEG_DECODER_PLUGIN=ON`)
- https://src.fedoraproject.org/rpms/ffmpeg/blob/f45/f/ffmpeg.spec (line
  ~902: `--disable-decoder="h264,hevc,vc1,vvc"`)
- `dnf repoquery` on the host: `libheif-freeworld`, `libde265`, `x265-libs`
  are in `rpmfusion-free` for fc45. `libheif-freeworld` requires
  `libheif(x86-64) = 1.23.7`.
- https://github.com/rpmfusion/libheif-freeworld
- https://rpmfusion.org/Configuration

glycin (commit `9614918995b11ed99e9fb03b9829fd2055acb80f`, 2026-11-06):
- https://gitlab.gnome.org/GNOME/glycin/-/blob/9614918995b11ed99e9fb03b9829fd2055acb80f/glycin-core/src/util.rs
  (`RunEnvironment`, `GLYCIN_DISABLE_SANDBOX`, silent fallback)
- https://gitlab.gnome.org/GNOME/glycin/-/blob/9614918995b11ed99e9fb03b9829fd2055acb80f/glycin-core/src/sandbox.rs
  (bwrap arguments, `check_bwrap_syscalls_blocked`)
- https://gitlab.gnome.org/GNOME/glycin/-/blob/9614918995b11ed99e9fb03b9829fd2055acb80f/glycin-core/src/api/common.rs
  (`SandboxSelector`)
- https://gitlab.gnome.org/GNOME/glycin/-/blob/9614918995b11ed99e9fb03b9829fd2055acb80f/glycin-thumbnailer/src/lib.rs
  (thumbnailer forces `NotSandboxed`)
- https://gitlab.gnome.org/GNOME/glycin/-/blob/9614918995b11ed99e9fb03b9829fd2055acb80f/README.md
  ("Sandboxing and Inner Workings")

GNOME and Freedesktop runtimes:
- https://gitlab.gnome.org/GNOME/gnome-build-meta/-/blob/gnome-51/elements/freedesktop-sdk.bst
  (tracks `freedesktop-sdk-26.08*`)
- https://gitlab.gnome.org/GNOME/gnome-build-meta/-/blob/main/elements/sdk/glycin.bst
  and `elements/sdk-platform.bst` (main at `5e5b6ddc`)
- https://gitlab.com/freedesktop-sdk/freedesktop-sdk/-/blob/release/26.08/elements/components/libheif.bst
  (`-DWITH_X265=OFF -DWITH_LIBDE265=OFF`)
- https://gitlab.com/freedesktop-sdk/freedesktop-sdk/-/blob/release/26.08/elements/include/libheif.yml
  (plugin dir in `codecs-extra`)
- https://gitlab.com/freedesktop-sdk/freedesktop-sdk/-/blob/release/26.08/elements/extensions/codecs-extra/libheif.bst
- https://gitlab.com/freedesktop-sdk/freedesktop-sdk/-/blob/release/26.08/elements/flatpak-images/platform.bst
  (`Extension org.freedesktop.Platform.codecs-extra`)
  (release/26.08 at `27a19148`)

Pure Rust decoders:
- https://docs.rs/crate/heifer/latest
- https://docs.rs/crate/heic/latest

Local tests (2026-11-12): container probes for stock Fedora 45, Fedora 45 with
RPM Fusion, and `flatpak run org.freedesktop.Platform//26.08`. glycin was
driven through `Gly-2` GObject introspection from Python, with
`RUST_LOG=glycin_core=debug` to see the sandbox.

## Human decision

Decision (2026-11-12, intent as given): "Go for a solution that ensures the
usability of the tool. Broken functionality is not good. You can opt for
later implementation if that enables progress short term."

How it was applied:

- **HEIC decoding moves from M1 to M2.** M2 ships the Flatpak. The GNOME
  runtime gets HEVC from the `codecs-extra` extension on Flathub (section 1),
  so HEIC can really work for users there. M1 is a host CLI. On stock Fedora
  it could never decode HEIC, so "HEIC becomes JPEG" in M1 would pass in CI
  (with RPM Fusion) and fail for most users. M3 is a host package too, so it
  is no better. M2 is the first milestone where HEIC works for real users.
- **M1 refuses HEIC/HEIF.** Detected by content (`ftyp` brands), not by file
  name. Clear message, exit code 3, no output, never the original. The test
  fixture is a 24 byte `ftyp` header written with `printf`, so no HEVC
  encoder is needed. Details in `spec/milestones/M1/plan.md`.
- **Option A is accepted for M2.** The devcontainer and CI add RPM Fusion's
  `libheif-freeworld` in M2, not in M1. M1 drops `libheif-tools` and
  `libheif-ffmpeg` from its package list.
- **M2 acceptance (SPEC 12):** HEIC becomes an upright JPEG with no metadata
  in the shipped Flatpak; dev and CI decode HEIC; a missing decoder at
  runtime gives a clear message, not a failure.
- **No skipped tests.** Every test runs in CI. glycin's silent fallback to no
  sandbox on GitHub runners (section 4, "Not verified") is an open item in
  SPEC 12 M2. It also matters for M1 decoding of JPEG and PNG, so M1 task T05
  checks it first.
- Still not verified and listed as M2 open items: HEIC through glycin's
  `flatpak-spawn --sandbox` in the GNOME runtime, an end-to-end "decoder
  missing" test, and a rotated HEIC fixture.
