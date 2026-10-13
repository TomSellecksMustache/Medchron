mod app;
mod chronology;
mod models;
mod redaction;
mod storage;

use axum::routing::get;
use std::{env, net::SocketAddr, path::PathBuf};
use tokio::net::TcpListener;

const DEFAULT_BIND: &str = "127.0.0.1:56753";

fn default_data_dir() -> PathBuf {
    // Installed Windows applications cannot write to Program Files. Case data
    // belongs to the current user, outside the installation directory.
    #[cfg(windows)]
    {
        if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local_app_data).join("Medchron").join("data");
        }
    }
    PathBuf::from("data")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = env::var_os("MEDCHRON_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(default_data_dir);
    std::fs::create_dir_all(&data_dir)?;
    let app = app::router(data_dir).route("/healthz", get(|| async { "medchron-local-ok" }));

    let addr: SocketAddr = env::var("MEDCHRON_BIND")
        .unwrap_or_else(|_| DEFAULT_BIND.into())
        .parse()?;
    if !addr.ip().is_loopback() {
        return Err(
            "Medchron refuses non-loopback addresses: no network authentication is implemented"
                .into(),
        );
    }

    let listener = TcpListener::bind(addr).await?;
    println!("Medchron is listening at http://{addr}");
    println!("Keep this service on loopback unless you have added and reviewed access controls.");
    axum::serve(listener, app).await?;
    Ok(())
}
