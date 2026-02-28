use anyhow::Context;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tracing::info;
use kernel::config::Config;
use kernel::ingestor::EventIngestor;
use kernel::registry::DeviceRegistry;
use kernel::wal::{Wal, WalConfig, EventPriority};
use kernel::state_engine::StateEngine;
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

    // Load device registry — not retained as Arc after pipeline init.
    // StateEngine owns its DeviceState directly; registry is only needed
    // at construction time.
    let registry = DeviceRegistry::load(&config.storage.devices_path)
        .context("Failed to load device registry")?;

    // Open WAL
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

    info!(
        sequence = wal.latest_sequence(),
        "WAL opened — sequence continues from here"
    );

    // StateEngine is initialized from the registry — takes &DeviceRegistry,
    // not Arc. After new() returns, the engine is self-contained.
    let mut state_engine = StateEngine::new(
        &registry,
        config.reconciler.confidence_degraded_threshold,
        config.reconciler.confidence_unknown_threshold,
    );

    // Ingestor needs the registry for external_id → DeviceId resolution.
    // Wrap in Arc so it can be shared without copying.
    let registry = Arc::new(registry);

    let mut ingestor = EventIngestor::new(
        registry,
        config.adapter.event_dedup_window_ms,
    );

    let mut rule_engine = RuleEngine::new();
    let resolver = ConflictResolver::new();
    let dispatcher = CommandDispatcher::new();

    // Shared state for UI
    let shared_state = Arc::new(Mutex::new(HashMap::new()));

    // Start UI server
    let ui_state = shared_state.clone();
    let ui_port = config.ui.port;
    tokio::spawn(async move {
        ui::start(ui_port, ui_state).await;
    });

    // Connect adapter and move it into its own task.
    // It owns itself entirely — no borrow crosses the task boundary.
    // Events are sent through the channel; the main loop receives them.
    let mut adapter = MockAdapter::new();
    adapter.connect().await?;

    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(32);

    tokio::spawn(async move {
        loop {
            match adapter.next_event().await {
                Ok(event) => {
                    if event_tx.send(event).await.is_err() {
                        // Receiver dropped — main loop shut down
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "adapter error");
                    break;
                }
            }
        }

        // Channel closed or error — disconnect cleanly
        if let Err(e) = adapter.disconnect().await {
            tracing::error!(error = %e, "adapter disconnect failed");
        }
    });

    // Tick timer — fires every second for confidence decay
    let mut tick_interval = tokio::time::interval(
        tokio::time::Duration::from_secs(1)
    );
    tick_interval.set_missed_tick_behavior(
        tokio::time::MissedTickBehavior::Skip
    );

    info!("smarthome daemon ready — entering event loop");

    loop {
        tokio::select! {
            Some(raw_event) = event_rx.recv() => {
                if let Some(event) = ingestor.ingest(raw_event) {
                    let priority = EventPriority::for_kind(&event.kind);
                    wal.append(event.clone(), priority)
                        .context("WAL append failed")?;

                    let update = state_engine.apply_event(&event);

                    let candidates = rule_engine.evaluate(
                        &update,
                        state_engine.get_all(),
                    );

                    let resolved = resolver.resolve(candidates);

                    for command in &resolved {
                        dispatcher.dispatch(command);
                    }

                    let mut state_map = shared_state.lock().unwrap();
                    *state_map = state_engine.get_all().clone();

                    info!(
                        wal_sequence = wal.len(),
                        devices = state_map.len(),
                        "event processed"
                    );
                }
            }

            _ = tick_interval.tick() => {
                let now = SystemTime::now();
                let degraded = state_engine.tick(now);

                if !degraded.is_empty() {
                    tracing::warn!(
                        devices = ?degraded,
                        "devices confidence degraded"
                    );
                }

                let mut state_map = shared_state.lock().unwrap();
                *state_map = state_engine.get_all().clone();
            }

            _ = tokio::signal::ctrl_c() => {
                info!("shutdown signal received");
                break;
            }
        }
    }

    info!("flushing WAL buffer before shutdown");
    wal.flush().context("WAL flush failed")?;
    info!("smarthome daemon stopped cleanly");
    Ok(())

}