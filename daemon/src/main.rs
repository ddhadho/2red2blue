use anyhow::Context;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::info;
use kernel::config::Config;
use kernel::ingestor::EventIngestor;
use kernel::wal::{Wal, WalConfig, EventPriority};
use kernel::state::StateEngine;
use kernel::rules::RuleEngine;
use kernel::resolver::ConflictResolver;
use kernel::dispatcher::CommandDispatcher;
use adapters::mock::MockAdapter;
use adapters::traits::DeviceAdapter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("info".parse().unwrap())
        )
        .init();

    info!(version = env!("CARGO_PKG_VERSION"), "smarthome daemon starting");

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".to_string());

    let config = Config::load(&config_path)
        .with_context(|| format!("Failed to load config from '{}'", config_path))?;

    info!(
        adapter = %config.adapter.kind,
        platform = %config.platform.kind,
        ui_port = config.ui.port,
        "configuration loaded"
    );

    // Shared state between event loop and UI server
    let shared_state = Arc::new(Mutex::new(HashMap::new()));

    // Start UI server in background
    let ui_state = shared_state.clone();
    let ui_port = config.ui.port;
    tokio::spawn(async move {
        ui::start(ui_port, ui_state).await;
    });

    // Initialize pipeline components
    let mut adapter = MockAdapter::new();
    let mut ingestor = EventIngestor::new();
    let mut state_engine = StateEngine::new();
    let mut rule_engine = RuleEngine::new();
    let resolver = ConflictResolver::new();
    let dispatcher = CommandDispatcher::new();

    let wal_config = match config.platform.kind.as_str() {
        "openwrt" => WalConfig::for_openwrt(
            &config.storage.wal_path,
            &config.storage.snapshot_path,
        ),
        _ => WalConfig::for_linux(
            &config.storage.wal_path,
            &config.storage.snapshot_path,
        ),
    };

    let mut wal = Wal::open(wal_config)
        .context("Failed to open WAL")?;

    // Log boot sequence number — this is what proves durability
    info!(
        sequence = wal.latest_sequence(),
        "WAL opened — sequence continues from here"
    );

    // Connect adapter
    adapter.connect().await?;

    info!("smarthome daemon ready — entering event loop");

    // Main event loop
    loop {
        tokio::select! {
            result = adapter.next_event() => {
                match result {
                    Ok(raw_event) => {
                        // Ingest and normalize
                        if let Some(event) = ingestor.ingest(raw_event) {
                            // Append to WAL
                            let priority = EventPriority::for_kind(&event.kind);
                            wal.append(event.clone(), priority)
                               .context("WAL append failed")?;

                            // Update state
                            let update = state_engine.apply_event(&event);

                            // Evaluate rules
                            let candidates = rule_engine.evaluate(
                                &update,
                                state_engine.get_all()
                            );

                            // Resolve conflicts
                            let resolved = resolver.resolve(candidates);

                            // Dispatch commands
                            for command in &resolved {
                                dispatcher.dispatch(command);
                            }

                            // Update shared state for UI
                            let mut state_map = shared_state.lock().unwrap();
                            *state_map = state_engine.get_all().clone();

                            info!(
                                wal_sequence = wal.len(),
                                devices = state_map.len(),
                                "event loop cycle complete"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "adapter error");
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                info!("shutdown signal received");
                break;
            }
        }
    }

    // Before adapter.disconnect():
    info!("flushing WAL buffer before shutdown");
    wal.flush().context("WAL flush failed")?;

    adapter.disconnect().await?;
    info!("smarthome daemon stopped cleanly");
    Ok(())
}