# Decisions

One file per decision. Copy `0000-template.md` to the next free number. See
"Blocked on a decision" in `../workflow.md`.

## Records

| # | Decision | Status |
|---|---|---|
| [0001](0001-heic-decoding-and-fixtures.md) | HEIC decoding and fixtures in the devcontainer and CI | accepted. HEIC decoding moves to M2; M1 refuses HEIC (T18) |

No open question in SPEC section 16 blocks M1. M2 needs no new decision record: every M2 question has a safe default or waits on the human (below).

## Pending (SPEC section 16, not yet a record)

Open a record when the milestone that needs the answer starts.

| # | Question | Status |
|---|---|---|
| 16.1 | Final name and app ID (reverse DNS) | pending (human). M2 uses the placeholder `org.example.SafeUpload`; only M2 T19 (rename) and the Flathub submission (M4) wait on it |
| 16.2 | License confirmation (GPL-3.0-or-later proposed) | pending (human). Not in the M2 criteria. Confirm before the first public push, since `LICENSE` is already GPL-3.0. |
| 16.3 | Default target size when the toggle is first turned on | answered 2026-11-12: 5 MB (SPEC 16.3). M2 gschema default `5000000` |
| 16.4 | Keep blur, or only box and pixelate? | pending (human). Blocks only M2 T15 (blur). Other M2 tasks go on |
| 16.5 | Register the standalone app as a "Share" target in Files? | pending (human). Not in the M2 criteria; deferred, blocks nothing in M2 |
| 16.6 | Edit `portals.conf` for the user (diff and undo), or only show instructions? | pending, blocks M3 |
| 16.7 | First-time behavior per app: ask (proposed), or off? | pending, blocks M3 |
| 16.8 | "No D-Bus" in core allows glycin's peer-to-peer zbus channel to its loaders (SPEC 14 proposal) | answered 2026-11-12: yes (SPEC 14, 16.8) |

## Deferred (found while planning, not in SPEC 16)

Safe defaults are in use. Open a record only if a later milestone needs
more.

| Question | Default in use | Milestone |
|---|---|---|
| Clipboard: also offer `image/png` or a file URI when a target app does not accept `image/jpeg` paste? A PNG can be far above the target size; a file URI from inside the Flatpak may point to a path the host cannot see (not verified) | Copy only the verified bytes with their own MIME type (M2 plan, "Copy to clipboard") | M2 acceptance records what Firefox and Chromium accept; revisit in M4 |
| Save a batch when some images failed | Save is disabled until every image is ready or the failed ones are removed | revisit in M4 (HIG review) |
| Batch folder save when a name already exists | Write nothing and say which name exists (same as the CLI) | revisit in M4 |
