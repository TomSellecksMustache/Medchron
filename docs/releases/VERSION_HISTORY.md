# Medchron version numbering

Medchron follows semantic versioning below `1.0.0` while it remains a prototype.

## v0.2.7

- Offline OCR for scanned PDFs using local Tesseract and Poppler.
- Reviewed batch intake for up to 30 PDFs, with explicit existing-person selection.
- OCR quotations remain unverified; original PDFs and source page links are preserved.
- Central storage codec boundary; plaintext remains the current mode and the reserved encrypted mode refuses startup.
- Windows OCR packaging, real OCR/browser release checks, and updated release notes.

## Historical version labels

| Original label | Intended canonical version |
| --- | --- |
| `2.5` / `2.5.0` | `0.2.5` |
| `2.6` / `2.6.0` | `0.2.6` |
| `2.6.0-rc.1` | `0.2.6-rc.1` |
| `2.6.0-rc.2` | `0.2.6-rc.2` |
| `2.6.0-rc.3` | `0.2.6-rc.3` |

**Historical GitHub tags are preserved**. Previously published binaries may still
carry the old version internally. The stable `v0.2.6` installer is compiled from
source labeled `0.2.6`, after testing and integrity checks.

Use the Latest stable Windows installer:
https://github.com/TomSellecksMustache/Medchron/releases/latest/download/Medchron-Setup-Windows-x64.exe
