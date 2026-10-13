# Medchron: offline OCR, batch intake, and encryption boundary

**Development prototype; synthetic documents only until authentication, key management, encryption, auditing and operational safeguards have been reviewed.**

## Batch intake

Open the **Batch intake · multiple people** area in the New Chronology view.

1. Select up to 30 PDFs (25 MB maximum per file; the HTTP request also has a 100 MB cap).
2. Click **Analyze names locally**. This extracts searchable PDF text and uses OCR on image-only or nearly textless pages.
3. **Review every filename** and proposed identity. Only explicitly labeled `Patient:`, `Patient Name:`, `Client:`, `Client Name:`, or `Name:` fields are considered suggestions; conflicting or missing names remain blank. OCR can misread names.
4. Choose **New person** or explicitly select an existing saved person. New documents with the same confirmed name are grouped into one *new* case; existing cases are never merged by name alone.
5. Confirm review and click **Import confirmed records**. Only then are documents saved, chronology candidates generated, and the People sidebar updated.

Cases created before this update remain readable. Existing People Workspace views, document previews and exports remain available.

**Identity safety:** two people can share a name. If there is any possibility of a collision, select an existing person's precise case entry only after verifying the document, or import separate new cases in separate batches. No person is created from an unreviewed OCR guess.

## OCR engines

Medchron uses [Poppler](https://poppler.freedesktop.org/) `pdftoppm` to render pages to temporary PNG files and [Tesseract](https://tesseract-ocr.github.io/) to extract text. Both run on the local machine as subprocesses—no outbound API or LLM calls. Page order is preserved and events retain source PDF and page numbers. When OCR fails, a warning is saved and the original source PDF remains reviewable.

The normal path extracts existing searchable text without OCR. OCR is attempted when a page has fewer than 35 alphanumeric characters. CPU OCR is the default; a GPU is **not automatically used**.

**Ubuntu prerequisites:**

```bash
sudo apt update
sudo apt install -y tesseract-ocr tesseract-ocr-eng poppler-utils
```

**Windows installer:** The updated release workflow prepares and bundles `tesseract.exe`, English language data, `pdftoppm.exe` and their supporting files under `Program Files\\Medchron\\ocr`. The app looks there first on Windows, then on `PATH`. Packaging is configured but must pass a GitHub Windows build and an actual Windows installation smoke test before being considered verified. Review bundled third-party license/distribution obligations before shipping.

**Environment variables:** `MEDCHRON_OCR_LANGUAGE` (default `eng`), `MEDCHRON_TESSERACT_EXECUTABLE`, `MEDCHRON_POPPLER_EXECUTABLE` for manually managed local binaries.

### Optional local CUDA engine

`MEDCHRON_OCR_EXECUTABLE` may point to a **trusted local program** that accepts a PNG path and prints recognized UTF-8 text to stdout. The included `tools/medchron_paddle_cuda.py` is an **experimental** PaddleOCR 3.x adapter, not the default backend. Install the exact GPU-compatible PaddlePaddle/PaddleOCR versions and download model files using **non-client data** in advance. Set `MEDCHRON_PADDLE_DETECTION_DIR`, `MEDCHRON_PADDLE_RECOGNITION_DIR` and optionally `MEDCHRON_PADDLE_DEVICE=gpu:0`. Verify truly offline operation on an isolated system before use. Model initialization overhead may be high because the adapter currently starts anew for each page.

Do **not** configure an OCR wrapper that connects to any external service or auto-downloads models when processing client files. The application does not enforce an operating-system firewall; the operator must secure the environment.

## Encryption groundwork — intentionally NOT enabled

`src/crypto.rs` is a centralized read/write boundary used by case JSON and uploaded PDFs. It defines a versioned future authenticated-encryption envelope with an algorithm and key identifier rather than any embedded key material.

- Unset `MEDCHRON_STORAGE_MODE` or set it to `plaintext-v1` for **current unencrypted storage** (default).
- Set `MEDCHRON_STORAGE_MODE=encrypted-v1` to **fail startup closed**. This is a reserved mode and *does not encrypt data yet*.
- The single storage boundary permits future migration to AES-256-GCM (or a reviewed alternative) with authenticated metadata, key rotation, and controlled migration from existing plaintext files.

Encryption-at-rest is **not implemented**. Future work must include OS keychain/DPAPI or another secure key-management strategy, per-record nonces, tamper detection, crash-safe writes, migration with backups, encrypted OCR temporary workspaces, memory/swap/log exposure review, access control and threat modeling. Current temporary rendered pages and source-PDF staging are **plaintext local files**, deleted on normal completion; hard crashes can leave artifacts in the machine's temp folder.

## Security limits

- The server must stay bound to `127.0.0.1`; **do not bind it on a shared network without authentication, TLS, authorization and CSRF protection**.
- Localhost endpoints are unauthenticated; a loopback listener is **not** a general protection against local users or malicious websites.
- PDF/image parsers and OCR engines process untrusted data; run as an unprivileged user and keep dependencies patched. Add job timeouts, quotas, cancellation, sandboxing and malware scanning before accepting real-world records.
- Do not commit real patient documents, upload them to public GitHub Actions, or transmit identifying information to cloud services.
- OCR and identity grouping are proposals only: people must verify both records and generated chronology candidates against the source PDFs.

## Smoke tests

```bash
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build
# Local OCR prerequisites:
tesseract --version
pdftoppm -v
```

Test one image-only PDF and a batch with two different fictional patient names, then another batch assigned explicitly to one existing fictional person. Confirm that the right-side document list shows only that person's files.
