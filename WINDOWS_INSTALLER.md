# Medchron for Windows — Installer and Desktop Shortcut

<!-- MEDCHRON WINDOWS RELEASE INFO START -->
## Recommended Windows download

**[Download the latest Windows installer](https://github.com/TomSellecksMustache/Medchron/releases/latest/download/Medchron-Setup-Windows-x64.exe)**

Release version **v0.2.7** (`Cargo.toml`: `0.2.7`). Future release tags use `v0.*`.
Only a successful Windows installer build may publish a stable release as Latest.
Historical `v2.*` tags retain their original commits and binary metadata.

<!-- MEDCHRON WINDOWS RELEASE INFO END -->

**Prototype only. Use synthetic data for demonstrations. Do not process confidential client documents without a formal security review.**

## Windows 64-bit installation

1. Visit https://github.com/TomSellecksMustache/Medchron/releases and download the latest `Medchron-Setup-Windows-x64.exe`.
2. Run the installer and approve the standard Windows administrator prompt. The executable is **not code-signed**, so Windows SmartScreen may show an unknown-publisher warning. Only run a file downloaded from the project's own release page and verify its SHA256 checksum.
3. Keep the **Create a Medchron desktop shortcut** checkbox checked.
4. Double-click the **Medchron** desktop icon. It starts the local server with no visible console and opens the default browser once the server is ready.

## Installation layout

| Component | Location |
|---|---|
| Rust web server | `C:\Program Files\Medchron\bin\medchron-server.exe` |
| Desktop shortcut target | `C:\Program Files\Medchron\bin\medchron-launcher.exe` |
| Branded shortcut icon | `C:\Program Files\Medchron\medchron.ico` |
| Local OCR tools and English language data | `C:\Program Files\Medchron\ocr\` |
| Application data, per Windows user | `%LOCALAPPDATA%\Medchron\data` |
| Default local web address | `http://127.0.0.1:56753/` |

**Port correction:** `86753` cannot be a TCP port (the maximum is `65535`). This installer uses `56753`. To change it, edit the `DEFAULT_BIND` constant in `src/main.rs` plus the `BIND` and `URL` constants in `src/bin/medchron-launcher.rs`, then rebuild and publish a new release.

## Behavior

- Clicking the desktop icon starts `medchron-server.exe` invisibly in the background if Medchron isn't already running.
- If the application is already running and responds to its local health-check endpoint, a second click simply opens another browser tab.
- The server listens **only on the local loopback interface** (`127.0.0.1`) — never a network-facing address.
- The browser receives the app over local HTTP. There is no cloud model/API integration in the included server.
- Tesseract and Poppler run locally to extract scanned pages. Batch intake proposes names for review and saves records only after confirmation; OCR entries remain unverified until checked against the source.
- Close the browser to leave the server running. To stop it for this prototype, use Task Manager → **Details** → `medchron-server.exe` → **End task**, or run `Stop-Process -Name medchron-server` in PowerShell under the same Windows account. Do not stop processes unless you are sure they belong to your own Medchron session.
- The installer does not register a Windows Service, run at startup, or add firewall rules.

## Uninstall and case data

Use Windows **Settings → Apps → Installed apps → Medchron → Uninstall**. Your case data lives separately in `%LOCALAPPDATA%\Medchron\data` and should **not** be automatically deleted by the uninstaller. Back up or remove this folder yourself when appropriate under your organization's retention and secure-erasure policies.

## Developers: compile the Windows installer

In the Rust repository (not the older Python ZIP):

1. Add `src/main.rs`, `src/bin/medchron-launcher.rs`, `packaging/windows/*`, `.github/workflows/windows-release.yml` and this guide to the corresponding locations.
2. On an x64 Windows development machine with Rust and [Inno Setup 6](https://jrsoftware.org/isinfo.php):

   ```powershell
   cargo test --release --locked
   cargo build --release --locked --bins
   # Prepare dist\ocr-runtime\tesseract and dist\ocr-runtime\poppler,
   # including English tessdata and the runtime dependencies, as in the release workflow.
   & "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" '/DMyAppVersion=0.2.7' 'packaging\windows\medchron.iss'
   ```

3. Find `dist\Medchron-Setup-v0.2.7-windows-x64.exe`.

Alternatively, push the updated source to `main`, then push a new version tag. The Windows GitHub Actions workflow compiles the installer and portable ZIP and creates the full Release automatically after successful packaging. Do not manually publish a Release first.

The release workflow also checks real OCR and reviewed batch intake against four fictional scanned PDFs, using the executables from the packaged portable app. It captures People Workspace and Batch Intake screenshots for the release. OCR or screenshot failures prevent publication.

## Security notes

Local-only binding is not a guarantee of confidentiality. There is no user authentication on the HTTP server, no built-in encryption at rest, and no full audit trail. Local malware or other local processes may access the service or stored data. The app must not be exposed via tunnels, reverse proxies, or firewall exceptions. Use only synthetic demonstration records until security/privacy review and hardening are completed.

## Bundled synthetic records

The Windows installer and portable ZIP contain **30 additional synthetic medical and insurer PDF records**, plus the original sample PDFs. All new records are clearly marked *SYNTHETIC RECORD — DEMONSTRATION ONLY* and describe a fictional person.

- **Installed copy:** `C:\Program Files\Medchron\samples\`
- **Portable ZIP:** `samples\` inside the extracted package
- **Source checkout:** `samples/`

Open Medchron and use **Choose files** to select these PDFs. The browser controls the file picker's starting location; no automatic sample-folder chooser is involved. See [`SYNTHETIC_RECORDS.md`](SYNTHETIC_RECORDS.md) for details.
