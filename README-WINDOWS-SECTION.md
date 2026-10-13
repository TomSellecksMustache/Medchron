## Windows installer (64-bit)

![Medchron logo](packaging/windows/medchron-logo.png)

Download the branded `Medchron-Setup-v*-windows-x64.exe` from the [GitHub Releases page](https://github.com/TomSellecksMustache/Medchron/releases).

The installer places the Rust server at `C:\Program Files\Medchron\bin\medchron-server.exe`, creates a desktop shortcut with the Medchron logo, and installs a small launcher. Opening the shortcut starts the server in the background and opens the default browser at **http://127.0.0.1:56753/**. Clicking again simply reopens the browser when the server is already running.

Case files remain in the current Windows user's `%LOCALAPPDATA%\Medchron\data` folder, **not** under Program Files. The server binds to localhost only, and this prototype does not integrate with cloud AI APIs. The installer is not code-signed and is intended for **synthetic demo data only**. For a detailed installation guide, troubleshooting, and security limitations, read [WINDOWS_INSTALLER.md](WINDOWS_INSTALLER.md).
