# Medchron (Rust)

**Medchron is a local-first prototype for turning medical and insurer PDFs into a source-linked, human-reviewed chronology.**

This is a coherent Rust implementation of the prototype: a single Rust web service handles PDF text extraction, basic rules-based candidate generation, identifier redaction, local JSON storage, review actions, and CSV/Markdown export. The browser UI is served by the same process. There is no LLM integration and no application feature that sends record text to a third-party API.

> **Privacy first:** Use synthetic records for demos. This prototype has no authentication, encryption at rest, audit trail, or multi-user controls. It is not a validated legal or clinical system. Do not process real client records until your organization has reviewed and approved the deployment.

## Features

- Rust backend using Axum.
- Local PDF text extraction, page by page.
- Candidate entries include date, category, summary, quotation, source file, page, verification flag, and review status.
- Pattern-based redaction for names supplied by the user and selected common identifiers.
- Quote-presence check against the redacted source page.
- Human review workflow: edit, accept, reject, or leave pending.
- Local JSON case storage and retained original PDFs; file permissions restricted on Unix-like systems.
- CSV and Markdown export; CSV values are guarded against common spreadsheet-formula injection.
- Local-only rules-based extraction. No cloud AI mode or API key is needed.
- Binds to `127.0.0.1:8000` by default.

## Requirements

- Rust stable toolchain (install with [rustup](https://rustup.rs/)).
- Cargo, installed with Rust.
- A modern browser.

## Run locally

From the project directory:

```bash
cargo test
cargo run
```

Open [http://127.0.0.1:8000](http://127.0.0.1:8000).

By default, case files are stored in `./data`. To choose another location:

```bash
MEDCHRON_DATA=/path/to/private/medchron-data cargo run
```

You can set `MEDCHRON_BIND` to another **loopback** address/port. The server rejects non-loopback addresses because this prototype has no authentication. Do not expose it through a reverse proxy or tunnel.

## Architecture

```text
Browser UI (served locally by Axum)
              |
              v
       Axum HTTP routes
              |
              v
 PDF page text extraction (lopdf)
              |
              v
 Pattern-based redaction
              |
              v
 Rules-based candidate chronology
              |
              v
 Quote-presence verification
              |
              v
 Local JSON storage + original PDFs
              |
              v
 Human review and CSV/Markdown export
```

### Project structure

```text
src/
├── main.rs        # App startup and loopback binding
├── app.rs         # HTTP routes, upload handling, review and exports
├── models.rs      # Case and chronology entry data types
├── chronology.rs  # PDF extraction and basic rules-based event detection
├── redaction.rs   # Pattern-based identifier redaction
└── storage.rs     # Local JSON case storage
static/
└── index.html     # Browser review interface
tests/
└── (integration tests can be added here)
```

## Privacy and security boundaries

- **No cloud model calls:** this version does not integrate with an LLM or send document text to an external AI provider.
- **Local files:** uploaded PDFs and case JSON are written under `MEDCHRON_DATA` (default `./data`).
- **No encryption at rest:** files may be readable by other users or processes with access to the directory. Use OS-level permissions and encrypted storage where appropriate.
- **Redaction is not de-identification:** name replacement and regex patterns cannot catch every identifier or contextual clue. Redaction here is mainly useful for reducing exposure in generated excerpts; it should not be treated as a guarantee of anonymity.
- **No authentication:** the app is designed for one local user and binds to loopback by default. A local service is not a security boundary against malware or other processes running as the same user.
- **No telemetry is implemented by the application:** this does not make claims about the operating system, browser, dependencies, or the host environment.
- **Deletion is not secure erasure:** deleting the case directory does not guarantee removal from backups, snapshots, or the storage device.
- **Source verification is limited:** a matching quotation shows only that the excerpt appears in extracted page text; it does not prove the event summary is accurate, complete, or properly interpreted.

Recommended practice: use synthetic records; keep `data/` out of version control; restrict directory permissions; use full-disk encryption; and perform a security/privacy review before any real-world use.

## Extraction limitations

- Scanned/image-only PDFs need OCR; this app reports pages without selectable text and does not perform OCR.
- The rules engine is intentionally simple. It recognizes a limited set of date formats and uses keyword heuristics to suggest categories.
- The rules extractor splits text pages on recognizable dated event headings; it can still miss events or group unrelated information together.
- Date interpretation and categorization may be wrong. Every entry starts as pending and should be compared with the original PDF.
- PDF text extraction quality depends on the source document and PDF structure.
- This is a prototype, not a validated legal chronology or medical decision-support product.

## Tests

```bash
cargo test
```

The unit tests cover date/category detection, multi-event segmentation, redaction, CSV escaping, and duplicate filename handling. Expand tests before relying on the application for any consequential workflow.

## License

MIT. See [`LICENSE`](LICENSE).

## Build verification

Run `cargo test` and `cargo clippy` on your machine. The project was generated in an environment without the Rust toolchain, so compilation and runtime functionality have **not** been verified here. Treat this as a complete source-code migration requiring a local build/test pass, not a production-ready deliverable.
