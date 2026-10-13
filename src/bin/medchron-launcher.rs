#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod windows_launcher {
    use std::{
        env, fs,
        io::{Read, Write},
        net::{SocketAddr, TcpStream},
        os::windows::process::CommandExt,
        path::PathBuf,
        process::{Command, Stdio},
        thread,
        time::Duration,
    };

    const BIND: &str = "127.0.0.1:56753";
    const URL: &str = "http://127.0.0.1:56753/";
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // Keep the launcher GUI-only. Display errors as Windows dialogs, not an
    // invisible console message that users would never see.
    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(
            hwnd: *mut std::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            flags: u32,
        ) -> i32;
    }

    fn error_dialog(message: &str) {
        let text: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
        let title: Vec<u16> = "Medchron"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // MB_OK | MB_ICONERROR
        unsafe {
            MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), 0x10);
        }
    }

    fn is_our_server_ready() -> bool {
        let addr: SocketAddr = BIND.parse().expect("valid loopback socket address");
        let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(250)) else {
            return false;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_millis(600)));
        let _ = stream.set_write_timeout(Some(Duration::from_millis(600)));
        if stream
            .write_all(b"GET /healthz HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
            .is_err()
        {
            return false;
        }
        let mut response = String::new();
        match stream.read_to_string(&mut response) {
            Ok(_) => response.contains("200 OK") && response.contains("medchron-local-ok"),
            Err(_) => false,
        }
    }

    fn launch_server() -> Result<(), String> {
        let exe = env::current_exe().map_err(|e| format!("Cannot locate Medchron: {e}"))?;
        let server_exe = exe.with_file_name("medchron-server.exe");
        if !server_exe.is_file() {
            return Err(format!(
                "Server executable not found:\n{}",
                server_exe.display()
            ));
        }
        let local_app_data =
            env::var_os("LOCALAPPDATA").ok_or("Windows LOCALAPPDATA directory is not available")?;
        let data_dir: PathBuf = PathBuf::from(local_app_data).join("Medchron").join("data");
        fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Cannot create your local data folder: {e}"))?;

        Command::new(server_exe)
            .env("MEDCHRON_DATA", &data_dir)
            .env("MEDCHRON_BIND", BIND)
            .current_dir(&data_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Cannot start the Medchron server: {e}"))?;
        Ok(())
    }

    fn open_browser() -> Result<(), String> {
        let status = Command::new("cmd.exe")
            .args(["/C", "start", "", URL])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| format!("Cannot open a browser: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "Windows could not open the browser (exit code: {status})."
            ))
        }
    }

    pub fn run() -> Result<(), String> {
        if !is_our_server_ready() {
            launch_server()?;
            // Wait for the local server, not for a guessed fixed sleep interval.
            for _ in 0..80 {
                if is_our_server_ready() {
                    return open_browser();
                }
                thread::sleep(Duration::from_millis(150));
            }
            return Err(format!(
                "Medchron could not start at {URL}\n\nAnother application may be using port 56753, or the server may have failed."
            ));
        }
        open_browser()
    }

    pub fn main() {
        if let Err(error) = run() {
            error_dialog(&error);
        }
    }
}

#[cfg(windows)]
fn main() {
    windows_launcher::main();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("medchron-launcher is intended for Windows builds only.");
}
