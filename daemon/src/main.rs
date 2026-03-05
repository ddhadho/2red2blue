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
use kernel::rule_loader::load_rules;
use kernel::resolver::ConflictResolver;
use kernel::dispatcher::CommandDispatcher;
use kernel::shared_state::{SharedState, new_shared};
use kernel::types::{AdapterCommand, Command, Event, EventKind, EventSource, RawDeviceEvent, Value};
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

    let registry = DeviceRegistry::load(&config.storage.devices_path)
        .context("Failed to load device registry")?;

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

    let mut state_engine = StateEngine::new(
        &registry,
        config.reconciler.confidence_degraded_threshold,
        config.reconciler.confidence_unknown_threshold,
    );

    let registry = Arc::new(registry);

    let mut ingestor = EventIngestor::new(
        registry.clone(),
        config.adapter.event_dedup_window_ms,
    );

    let rules_path = config.storage.rules_path.clone();
    let loaded_rules = load_rules(&rules_path, &registry)
        .context("Failed to load rules")?;

    let mut rule_engine = RuleEngine::new();
    rule_engine.load_rules(loaded_rules);

    let resolver = ConflictResolver::new();

    let mut dispatcher = CommandDispatcher::new(
        config.dispatcher.max_retries,
        config.dispatcher.command_timeout_seconds * 1000,
    );

    let shared = new_shared();

    let ui_shared = shared.clone();
    let ui_port = config.ui.port;
    tokio::spawn(async move {
        ui::start(ui_port, ui_shared).await;
    });

    // raw_tx carries RawDeviceEvent from the adapter task into the main loop.
    // send_command in the mock adapter emits a CommandConfirmed RawDeviceEvent
    // back through the same channel so the main loop can route it to the
    // dispatcher before handing anything to the ingestor.
    let (raw_tx, mut raw_rx) = tokio::sync::mpsc::channel::<RawDeviceEvent>(64);
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel::<AdapterCommand>(32);

    let mut adapter = MockAdapter::new(raw_tx.clone());
    adapter.connect().await?;

    tokio::spawn(async move {
        loop {
            tokio::select! {
                result = adapter.next_event() => {
                    match result {
                        Ok(raw) => {
                            if raw_tx.send(raw).await.is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "adapter error");
                            break;
                        }
                    }
                }
                Some(cmd) = cmd_rx.recv() => {
                    if let Err(e) = adapter.send_command(cmd).await {
                        tracing::error!(error = %e, "adapter send_command failed");
                    }
                }
            }
        }
        if let Err(e) = adapter.disconnect().await {
            tracing::error!(error = %e, "adapter disconnect failed");
        }
    });

    let mut tick_interval = tokio::time::interval(
        tokio::time::Duration::from_secs(1)
    );
    tick_interval.set_missed_tick_behavior(
        tokio::time::MissedTickBehavior::Skip
    );

    let mut sigusr1 = tokio::signal::unix::signal(
        tokio::signal::unix::SignalKind::user_defined1()
    ).context("Failed to register SIGUSR1 handler")?;

    info!("smarthome daemon ready — entering event loop");

    loop {
        tokio::select! {
            Some(raw_event) = raw_rx.recv() => {
                // CommandConfirmed events come back through the raw channel
                // from the adapter's send_command. Check external_id on the
                // raw event before passing anything to the ingestor.
                if raw_event.external_id == "system.command_confirmed" {
                    if let Value::Text(command_id) = &raw_event.value {
                        if let Some(confirmed) = dispatcher.confirm(&command_id) {
                            info!(
                                command_id = %confirmed.id,
                                device_id = %confirmed.device_id,
                                "command confirmed"
                            );
                            let mut s = shared.lock().unwrap();
                            s.pending_commands.retain(|c| c.id != confirmed.id);
                        }
                    }
                    // Don't process as a state event
                    continue;
                }

                if let Some(event) = ingestor.ingest(raw_event) {
                    let now = SystemTime::now();
                    let _now_ms = to_ms(now);
                    let priority = EventPriority::for_kind(&event.kind);

                    wal.append(event.clone(), priority)
                        .context("WAL append failed")?;

                    let update = state_engine.apply_event(&event);

                    let mut candidates = rule_engine.evaluate(
                        &update,
                        state_engine.get_all(),
                        now,
                    );

                    candidates.extend(rule_engine.tick(now, state_engine.get_all()));

                    dispatch_resolved(
                        candidates,
                        &resolver,
                        &mut dispatcher,
                        &cmd_tx,
                        &mut wal,
                        &shared,
                    ).await?;

                    let mut s = shared.lock().unwrap();
                    s.devices = state_engine.get_all().clone();
                    s.pending_commands = dispatcher.all_commands()
                        .into_iter().cloned().collect();

                    info!(
                        wal_sequence = wal.len(),
                        devices = s.devices.len(),
                        pending = s.pending_commands.len(),
                        "event processed"
                    );
                }
            }

            _ = tick_interval.tick() => {
                let now = SystemTime::now();
                let now_ms = to_ms(now);

                let degraded = state_engine.tick(now);
                if !degraded.is_empty() {
                    tracing::warn!(devices = ?degraded, "devices confidence degraded");
                }

                let timer_commands = rule_engine.tick(now, state_engine.get_all());
                if !timer_commands.is_empty() {
                    dispatch_resolved(
                        timer_commands,
                        &resolver,
                        &mut dispatcher,
                        &cmd_tx,
                        &mut wal,
                        &shared,
                    ).await?;
                }

                dispatcher.tick(now_ms);

                // Resend retried commands
                for cmd in dispatcher.drain_pending() {
                    send_to_adapter(&cmd, &cmd_tx, &mut dispatcher);
                }

                // Handle permanently failed commands — log, WAL, surface in UI.
                // Confidence is NOT zeroed — write failure ≠ stale state.
                for failed in dispatcher.drain_failed() {
                    tracing::error!(
                        command_id = %failed.id,
                        device_id = %failed.device_id,
                        attribute = %failed.attribute,
                        "command permanently failed — not zeroing confidence"
                    );

                    let mut payload = HashMap::new();
                    payload.insert(
                        "command_id".to_string(),
                        Value::Text(failed.id.clone()),
                    );
                    payload.insert(
                        "device_id".to_string(),
                        Value::Text(failed.device_id.0.clone()),
                    );
                    let event = Event::new(
                        EventSource::System,
                        EventKind::CommandFailed,
                        payload,
                    );
                    wal.append(event, EventPriority::Normal)
                        .context("WAL append failed for CommandFailed")?;
                }

                let mut s = shared.lock().unwrap();
                s.devices = state_engine.get_all().clone();
                s.pending_commands = dispatcher.all_commands()
                    .into_iter().cloned().collect();
            }

            _ = sigusr1.recv() => {
                info!("SIGUSR1 received — reloading rules");
                match load_rules(&rules_path, &registry) {
                    Ok(rules) => {
                        let report = rule_engine.hot_reload(rules);
                        info!(
                            loaded = report.rules_loaded,
                            added = report.rules_added,
                            removed = report.rules_removed,
                            in_flight_cancelled = report.in_flight_cancelled,
                            "rules hot-reloaded"
                        );
                    }
                    Err(e) => tracing::error!(
                        error = %e,
                        "rule reload failed — keeping existing rules"
                    ),
                }
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

// ── Helpers ──────────────────────────────────────────────────

async fn dispatch_resolved(
    candidates: Vec<Command>,
    resolver: &ConflictResolver,
    dispatcher: &mut CommandDispatcher,
    cmd_tx: &tokio::sync::mpsc::Sender<AdapterCommand>,
    wal: &mut Wal,
    shared: &Arc<Mutex<SharedState>>,
) -> anyhow::Result<()> {
    if candidates.is_empty() {
        return Ok(());
    }

    let resolved = resolver.resolve(candidates);

    for conflict in &resolved.conflicts {
        let mut payload = HashMap::new();
        payload.insert(
            "winning_rule".to_string(),
            Value::Text(
                conflict.winning_rule.as_ref()
                    .map(|r| r.0.clone())
                    .unwrap_or_default()
            ),
        );
        payload.insert(
            "losing_rule".to_string(),
            Value::Text(
                conflict.losing_rule.as_ref()
                    .map(|r| r.0.clone())
                    .unwrap_or_default()
            ),
        );
        payload.insert(
            "device_id".to_string(),
            Value::Text(conflict.device_id.0.clone()),
        );
        payload.insert(
            "attribute".to_string(),
            Value::Text(conflict.attribute.0.clone()),
        );

        let event = Event::new(EventSource::System, EventKind::RuleConflict, payload);
        wal.append(event, EventPriority::Normal)
            .context("WAL append failed for conflict record")?;

        shared.lock().unwrap().conflicts.push(conflict.clone());
    }

    for command in resolved.winners {
        dispatcher.enqueue(command.clone());
        send_to_adapter(&command, cmd_tx, dispatcher);
    }

    Ok(())
}

fn send_to_adapter(
    command: &Command,
    cmd_tx: &tokio::sync::mpsc::Sender<AdapterCommand>,
    dispatcher: &mut CommandDispatcher,
) {
    let adapter_cmd = AdapterCommand {
        external_id: command.device_id.0.clone(),
        attribute: command.attribute.0.clone(),
        value: command.value.clone(),
        command_id: command.id.clone(),
    };

    match cmd_tx.try_send(adapter_cmd) {
        Ok(_) => {
            dispatcher.mark_sent(&command.id);
        }
        Err(e) => {
            tracing::error!(
                command_id = %command.id,
                device_id = %command.device_id,
                error = %e,
                "failed to send command to adapter"
            );
        }
    }
}

fn to_ms(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}