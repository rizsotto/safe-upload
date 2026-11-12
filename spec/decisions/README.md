# Decisions

One file per decision. Copy `0000-template.md` to the next free number. See
"Blocked on a decision" in `../workflow.md`.

## Records

| # | Decision | Status |
|---|---|---|
| [0001](0001-heic-decoding-and-fixtures.md) | HEIC decoding and fixtures in the devcontainer and CI | accepted. HEIC decoding moves to M2; M1 refuses HEIC (T18) |

No open question in SPEC section 16 blocks M1.

## Pending (SPEC section 16, not yet a record)

Open a record when the milestone that needs the answer starts.

| # | Question | Status |
|---|---|---|
| 16.1 | Final name and app ID (reverse DNS) | pending, blocks M2 (.desktop, gschema, Flatpak) |
| 16.2 | License confirmation (GPL-3.0-or-later proposed) | pending, blocks M2. Confirm before the first public push, since `LICENSE` is already GPL-3.0. |
| 16.3 | Default target size when the toggle is first turned on (2 MB proposed) | pending, blocks M2. The M1 CLI requires `--max-size`, so it needs no default. |
| 16.4 | Keep blur, or only box and pixelate? | pending, blocks M2. M1 builds box and pixelate in core and leaves blur out until this is decided. |
| 16.5 | Register the standalone app as a "Share" target in Files? | pending, blocks M2 |
| 16.6 | Edit `portals.conf` for the user (diff and undo), or only show instructions? | pending, blocks M3 |
| 16.7 | First-time behavior per app: ask (proposed), or off? | pending, blocks M3 |
| 16.8 | "No D-Bus" in core allows glycin's peer-to-peer zbus channel to its loaders (SPEC 14 proposal) | awaiting human OK. M1 goes on with glycin meanwhile |
