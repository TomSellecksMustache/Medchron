# Apply the Medchron Windows installer kit

The supplied `Medchron.zip` in this conversation contains **the older Python prototype**. This package is instead an **overlay for the current public Rust GitHub repository** at https://github.com/TomSellecksMustache/Medchron . Do not extract it into the Python package.

## Contents

- `src/main.rs`: current Rust startup flow with new loopback port, Windows-local data folder, and health-check route.
- `src/bin/medchron-launcher.rs`: GUI-subsystem launcher with a hidden server process, wait-for-ready check, and default browser launch.
- `packaging/windows/medchron.iss`: Inno Setup script placing the server at `C:\Program Files\Medchron\bin\medchron-server.exe` and creating a branded desktop shortcut.
- `packaging/windows/medchron.ico`: multi-size Windows desktop/installer icon derived from the supplied logo.
- `packaging/windows/medchron-icon.png`: icon preview.
- `packaging/windows/medchron-logo.png`: exact original branding image.
- `.github/workflows/windows-release.yml`: builds/tests on Windows and uploads an installer plus portable ZIP to a published GitHub Release.
- `WINDOWS_INSTALLER.md`: Windows end-user and developer instructions.

## Ubuntu steps

Download and extract this kit, then copy its **contents**, not the enclosing folder, into `~/Medchron`:

```bash
cd ~/Medchron
# If extracted at ~/Downloads/Medchron-Windows-Installer-Kit:
rsync -av ~/Downloads/Medchron-Windows-Installer-Kit/ ~/Medchron/ \
  --exclude README-APPLY-THIS-KIT.md

cargo fmt
cargo test
cargo clippy --all-targets -- -D warnings

git diff --check
git status
git add src/main.rs src/bin/medchron-launcher.rs packaging/windows \
  .github/workflows/windows-release.yml WINDOWS_INSTALLER.md
git commit -m "Add Medchron Windows installer and branded desktop launcher"
git push origin main
```

The `rsync` operation **overwrites** `src/main.rs` and `.github/workflows/windows-release.yml`. Inspect `git diff` to make sure this is compatible with any unpublished local changes. The original Rust server still builds and runs on Linux; the `medchron-launcher` binary is Windows-only.

Then follow the tagging/release steps in `WINDOWS_INSTALLER.md` to build the Windows EXE via GitHub Actions.

## Important port choice

The requested `127.0.0.1:86753` is invalid because TCP ports range from 0 to 65535. This package uses **`127.0.0.1:56753`** as a provisional alternative. See `WINDOWS_INSTALLER.md` to change it before your release.

## Verification

These files were checked for layout and syntax where possible. **The Windows installer and launcher have not been compiled or tested in this environment.** The GitHub Actions Windows job must succeed before the files should be presented as downloadable working binaries. Test the installed shortcut, upload, export, data storage, and uninstall on a real Windows computer before distributing it.
