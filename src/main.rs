mod app;
mod chronology;
mod models;
mod redaction;
mod storage;

use std::{env, net::SocketAddr, path::PathBuf};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = PathBuf::from(env::var("MEDCHRON_DATA").unwrap_or_else(|_| "data".into()));
    std::fs::create_dir_all(&data_dir)?;
    let app = app::router(data_dir);
    // Bind to loopback by default: this is a local desktop/development service.
    let addr: SocketAddr = env::var("MEDCHRON_BIND")
        .unwrap_or_else(|_| "127.0.0.1:8000".into())
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
