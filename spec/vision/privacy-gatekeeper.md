# Vision: a privacy gatekeeper for every upload

Status: vision, after the MVP. Not part of M1 to M4. No decision records yet.
They open when a milestone for this starts (see `spec/workflow.md`, scope
first).

## Problem

People upload files that carry private data they cannot see or forget about.
A photo carries GPS. A PDF carries the author's full name and the original
file path. A Word file carries tracked changes and comments. A screenshot
shows an email address, a phone number or a home address. Safe Upload v1
handles hidden image metadata only.

## Idea

Safe Upload sits in the file chooser portal, so every upload passes through
it. Grow it from "clean images" into a gatekeeper for any file:

1. **Always look.** Every picked file is inspected by default.
2. **Flag risks.** Show what was found, where, and how sure we are.
3. **Let the user choose.** Clean it, edit it, or send it as it is, on
   purpose.

Example of an intended bypass: a signed job contract that has to carry your
name, address and phone number. The user sends it unchanged, and sees a short
summary of what leaves the machine.

## Two kinds of checks

| | Hidden metadata | Visible content |
|---|---|---|
| Examples | EXIF/GPS, PDF author and XMP, Office author, revision history, comments, tracked changes | Email, phone number, IP address, postal address, names, text in screenshots (OCR) |
| Precision | High: the data is in known places | Fuzzy: patterns and OCR give false positives and misses |
| Fix | Automatic: rebuild the file without it (as images do in v1) | Needs the user: review, redact, or bypass |
| Default | Strip | Flag for review |

## Shape

```
sniff type by content ──► handler.inspect ──► findings
                                              (kind: metadata | content,
                                               location, confidence)
        ──► policy (strip metadata, flag content)
        ──► review: send as is / edit / cancel ──► output
```

- One handler per file type, behind one trait: detect, inspect, sanitize,
  preview for redaction. The v1 image pipeline becomes the first handler.
- Handlers are built into the repo. No loadable third-party plugins at first:
  they add attack surface and support load. Revisit only with a real need.
- Handlers parse hostile files, so each runs sandboxed (the glycin model).
- File type comes from content, never from the extension (as the v1 HEIF
  sniffing does).
- Everything runs locally. No network (S-3 stays). OCR and pattern matching
  are local too.

## Rules to keep

- **Never say "safe".** Detection misses things. The UI says "we found
  these", never "this file is clean".
- **Few false alarms.** Flag only findings we are sure about. Too many flags
  teach people to click "send as is" every time, which protects nobody.
  Always show what was found, not just "risk".
- **Real redaction.** A black box drawn over PDF text leaves the text under
  it. Redaction must remove the content. Images are easy (we re-encode the
  pixels). PDF and Office redaction is a large project of its own.
- **Bypass is per file and explicit**, with a summary of what is sent. Per
  app memory (v1) still applies to "inspect or not", not to silent bypass.
- **Fast enough for a file chooser.** Inspection that takes long (OCR on big
  files) must not freeze the dialog.

## Open questions

1. Which file types first? Likely order: PDF, ODT/DOCX, screenshots (OCR).
2. Which local OCR engine (for example Tesseract) and which pattern rules?
   How do we measure false positives before shipping?
3. Which content kinds to flag in the first version (email, phone, IP,
   address, names)? Names are the hardest.
4. How is a bypass remembered, if at all?
5. What does the review UI look like inside or after the file chooser?
6. PDF and Office: strip and rebuild, or refuse what we cannot clean?

## Not in scope

- Cloud or online checks of any kind.
- Guarantees. This is a strong default, not a promise.
