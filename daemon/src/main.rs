use anyhow::Context;
use tracing::{info, error};
use kernel::config::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("info".parse().unwrap())
        )
        .json()
        .init();

    info!(
        version = env!("CARGO_PKG_VERSION"),
        "smarthome daemon starting"
    );

    // Load config
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".to_string());

    let config = Config::load(&config_path)
        .with_context(|| format!("Failed to load config from '{}'", config_path))?;

    info!(
        adapter = %config.adapter.kind,
        platform = %config.platform.kind,
        log_level = %config.daemon.log_level,
        ui_port = config.ui.port,
        "configuration loaded"
    );

    // Setup signal handling
    let ctrl_c = tokio::signal::ctrl_c();

    info!("smarthome daemon ready — waiting for events");

    // Wait for shutdown signal
    tokio::select! {
        _ = ctrl_c => {
            info!("received shutdown signal");
        }
    }

    info!("smarthome daemon stopped cleanly");
    Ok(())
}