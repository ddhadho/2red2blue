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
use kernel::desired_state_store::DesiredStateStore;
use kernel::reconciler::{boot_reconcile, continuous_reconcile};
use kernel::types::{
    AdapterCommand, Command, Event, EventKind, EventSource, Value,
};

use adapters::mock::MockAdapter;
use adapters::ha::HaAdapter;
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
        .with_context(|| format!("failed to load config from '{}'", config_path))?;

    info!(
        adapter  = %config.adapter.kind,
        platform = %config.platform.kind,
        ui_port  = config.ui.port,
        "configuration loaded"
    );

    let registry = DeviceRegistry::load(&config.storage.devices_path)
        .context("failed to load device registry")?;

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
        .context("failed to open WAL")?;

    info!(sequence = wal.latest_sequence(), "WAL opened");

    let mut state_engine = StateEngine::new(
        &registry,
        config.reconciler.confidence_degraded_threshold,
        config.reconciler.confidence_unknown_threshold,
    );

    // ── Boot sequence ─────────────────────────────────────────
    //
    // 1. Load desired state snapshot → state_engine.set_desired
    // 2. Replay WAL → state_engine.apply_event for DeviceStateChanged
    // 3. Boot window — devices report actual state
    // 4. boot_reconcile → correction commands for mismatches

    // Step 1 — desired state
    let mut desired_store = DesiredStateStore::load(&config.storage.desired_state_path)
        .context("failed to load desired state snapshot")?;

    for (device_id, attrs) in desired_store.get_all() {
        for (attr, value) in attrs {
            state_engine.set_desired(
                device_id,
                attr.clone(),
                value.clone(),
                EventSource::System,
            );
        }
    }

    info!(devices = desired_store.get_all().len(), "desired state loaded");

    // Step 2 — WAL replay
    // NOTE: replays from sequence 0 — full history on every boot.
    // This is correct but slow as the WAL grows. The fix is to write a
    // state snapshot alongside maybe_snapshot() and replay only the delta.
    // That fix requires the snapshot write and sequence advance to be atomic.
    // Tracked for post-pilot implementation. Do not change replay_from(0)
    // until the state snapshot store is implemented and tested together.
    let replay_events = wal.replay_from(0)
        .context("WAL replay failed")?;

    let replay_count = replay_events.len();
    for event in replay_events {
        if let EventKind::DeviceStateChanged = &event.kind {
            state_engine.apply_event(&event);
        }
    }

    info!(events_replayed = replay_count, "WAL replay complete");

    let registry = Arc::new(registry);

    let mut ingestor = EventIngestor::new(
        registry.clone(),
        config.adapter.event_dedup_window_ms,
    );

    let loaded_rules = load_rules(&config.storage.rules_path, &registry)
        .context("failed to load rules")?;

    let mut rule_engine = RuleEngine::new();
    rule_engine.load_rules(loaded_rules);

    let resolver   = ConflictResolver::new();
    let mut dispatcher = CommandDispatcher::new(
        config.dispatcher.max_retries,
        config.dispatcher.timeout_ms,
    );

    let shared = new_shared();

    shared.lock().unwrap().registry = registry.all().cloned().collect();

    let ui_shared = shared.clone();
    tokio::spawn(async move {
        ui::start(config.ui.port, ui_shared).await;
    });

    // Populate rule summaries — stable until next hot-reload
    {
        let mut s = shared.lock().unwrap();
        s.rule_summaries = rule_engine.rule_summaries();
    }

    // ── Channels ──────────────────────────────────────────────
    //
    // event_tx   — raw device events from adapter to main
    // cmd_tx     — adapter commands from main to adapter
    // confirm_tx — command IDs confirmed by adapter, bypass ingestor
    //
    // Confirmations are system events, not device observations.
    // Sending them through event_tx → ingestor would cause them to be
    // dropped (ingestor only resolves known device external_ids).

    let (event_tx,   mut event_rx)   = tokio::sync::mpsc::channel::<kernel::types::RawDeviceEvent>(256);
    let (cmd_tx,     mut cmd_rx)     = tokio::sync::mpsc::channel::<AdapterCommand>(32);
    let (confirm_tx, mut confirm_rx) = tokio::sync::mpsc::channel::<String>(32);

    let mut adapter: Box<dyn DeviceAdapter + Send> = match config.adapter.kind.as_str() {
        "homeassistant" => {
            let ha_cfg = config.home_assistant.clone()
                .context("home_assistant config required")?;
            let mut a = HaAdapter::new(ha_cfg, confirm_tx.clone());
            a.connect().await.context("HA adapter connect failed")?;
            a.start_poll_loop(event_tx.clone());
            Box::new(a)
        }
        _ => {
            let mut a = MockAdapter::new(confirm_tx.clone());
            a.connect().await.context("mock adapter connect failed")?;
            Box::new(a)
        }
    };

    tokio::spawn(async move {
        loop {
            tokio::select! {
                result = adapter.next_event() => {
                    match result {
                        Ok(event) => {
                            if event_tx.send(event).await.is_err() { return; }
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "adapter error — reconnecting in 5s");
                            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                            if let Err(e) = adapter.connect().await {
                                tracing::error!(error = %e, "reconnect failed — retrying");
                            }
                        }
                    }
                }
                Some(cmd) = cmd_rx.recv() => {
                    if let Err(e) = adapter.send_command(cmd).await {
                        tracing::error!(error = %e, "send_command failed");
                    }
                }
            }
        }
    });

    // Step 3 — boot window
    // Accept device events so actual state is fresh before reconciliation.
    // Rule evaluation and command dispatch are suppressed during this window
    // — we don't want rules acting on stale replayed state before devices
    // have had a chance to report their current actual state.
    let boot_deadline = SystemTime::now()
        + std::time::Duration::from_secs(config.reconciler.boot_window_secs);

    info!(
        boot_window_secs = config.reconciler.boot_window_secs,
        "boot window open"
    );

    loop {
        let remaining = boot_deadline
            .duration_since(SystemTime::now())
            .unwrap_or_default();

        if remaining.is_zero() { break; }

        let timeout = tokio::time::sleep(remaining);
        tokio::pin!(timeout);

        tokio::select! {
            Some(raw) = event_rx.recv() => {
                if let Some(event) = ingestor.ingest(raw) {
                    wal.append(event.clone(), EventPriority::for_kind(&event.kind))
                        .context("WAL append failed during boot window")?;
                    state_engine.apply_event(&event);
                    // No rule evaluation, no commands during boot window
                }
            }
            // Drain any stale confirmations that arrive during boot window —
            // there should be none, but drain so the channel stays clear
            Some(_command_id) = confirm_rx.recv() => {}
            _ = &mut timeout => { break; }
        }
    }

    info!("boot window closed — running boot reconciliation");

    // Step 4 — boot reconciliation
    let (reconcile_commands, report) = boot_reconcile(
        &state_engine,
        config.reconciler.confidence_degraded_threshold,
    );

    if !reconcile_commands.is_empty() {
        dispatch_resolved(
            reconcile_commands,
            &resolver,
            &mut dispatcher,
            &mut wal,
            &shared,
        ).await?;
    }

    {
        let mut s = shared.lock().unwrap();
        s.devices = state_engine.get_all().clone();
        s.last_reconciliation = Some(report);
    }

    // ── Event loop ────────────────────────────────────────────

    let mut tick = tokio::time::interval(tokio::time::Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut sigusr1 = tokio::signal::unix::signal(
        tokio::signal::unix::SignalKind::user_defined1()
    ).context("failed to register SIGUSR1 handler")?;

    let rules_path = config.storage.rules_path.clone();

    info!("entering event loop");

    loop {
        tokio::select! {
            // ── Confirmation — highest priority arm ───────────
            // Drain before processing new events so retries don't fire
            // on commands that are already confirmed.
            Some(command_id) = confirm_rx.recv() => {
                if let Some(confirmed) = dispatcher.confirm(&command_id) {
                    info!(
                        command_id = %confirmed.id,
                        device_id  = %confirmed.device_id,
                        "command confirmed"
                    );
                    let mut payload = HashMap::new();
                    payload.insert(
                        "command_id".to_string(),
                        Value::Text(confirmed.id.clone()),
                    );
                    let event = Event::new(
                        EventSource::System,
                        EventKind::CommandConfirmed,
                        payload,
                    );
                    wal.append(event, EventPriority::Critical)
                        .context("WAL append failed for CommandConfirmed")?;

                    let mut s = shared.lock().unwrap();
                    s.pending_commands.retain(|c| c.id != confirmed.id);
                }
            }

            // ── Device event ──────────────────────────────────
            Some(raw) = event_rx.recv() => {
                if let Some(event) = ingestor.ingest(raw) {
                    let now = SystemTime::now();

                    wal.append(event.clone(), EventPriority::for_kind(&event.kind))
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
                        &mut wal,
                        &shared,
                    ).await?;

                    let mut s = shared.lock().unwrap();
                    s.devices          = state_engine.get_all().clone();
                    s.pending_commands = dispatcher.all_commands()
                        .into_iter().cloned().collect();
                }
            }

            // ── Tick ──────────────────────────────────────────
            _ = tick.tick() => {
                let now    = SystemTime::now();
                let now_ms = to_ms(now);

                // Confidence decay
                let degraded = state_engine.tick(now);
                if !degraded.is_empty() {
                    tracing::warn!(devices = ?degraded, "devices confidence degraded");
                }

                // Rule timers
                let timer_cmds = rule_engine.tick(now, state_engine.get_all());
                if !timer_cmds.is_empty() {
                    dispatch_resolved(
                        timer_cmds,
                        &resolver,
                        &mut dispatcher,
                        &mut wal,
                        &shared,
                    ).await?;
                }

                // Continuous reconciliation
                let recon_cmds = continuous_reconcile(
                    &state_engine,
                    config.reconciler.confidence_degraded_threshold,
                );
                if !recon_cmds.is_empty() {
                    dispatch_resolved(
                        recon_cmds,
                        &resolver,
                        &mut dispatcher,
                        &mut wal,
                        &shared,
                    ).await?;
                }

                // Command timeouts + retries
                dispatcher.tick(now_ms);

                for cmd in dispatcher.drain_pending() {
                    send_to_adapter(&cmd, &cmd_tx, &mut dispatcher);
                }

                for failed in dispatcher.drain_failed() {
                    tracing::error!(
                        command_id = %failed.id,
                        device_id  = %failed.device_id,
                        "command permanently failed after max retries"
                    );
                    let mut payload = HashMap::new();
                    payload.insert("command_id".to_string(), Value::Text(failed.id.clone()));
                    payload.insert("device_id".to_string(),  Value::Text(failed.device_id.0.clone()));
                    let event = Event::new(
                        EventSource::System,
                        EventKind::CommandFailed,
                        payload,
                    );
                    wal.append(event, EventPriority::Critical)
                        .context("WAL append failed for CommandFailed")?;
                }

                // Periodic desired state flush
                desired_store.flush_if_needed(now)
                    .context("desired state flush failed")?;

                let mut s = shared.lock().unwrap();
                s.devices          = state_engine.get_all().clone();
                s.pending_commands = dispatcher.all_commands()
                    .into_iter().cloned().collect();
                s.in_flight        = rule_engine.in_flight_summaries();
            }

            // ── SIGUSR1 — hot reload rules ─────────────────────
            _ = sigusr1.recv() => {
                info!("SIGUSR1 — reloading rules");
                match load_rules(&rules_path, &registry) {
                    Ok(rules) => {
                        let r = rule_engine.hot_reload(rules);
                        info!(
                            loaded            = r.rules_loaded,
                            added             = r.rules_added,
                            removed           = r.rules_removed,
                            in_flight_cancelled = r.in_flight_cancelled,
                            "rules reloaded"
                        );
                        // Refresh summaries — rule set has changed
                        shared.lock().unwrap().rule_summaries =
                            rule_engine.rule_summaries();
                    }
                    Err(e) => tracing::error!(
                        error = %e,
                        "rule reload failed — keeping existing rules"
                    ),
                }
            }

            // ── Shutdown ──────────────────────────────────────
            _ = tokio::signal::ctrl_c() => {
                info!("shutdown signal received");
                break;
            }
        }
    }

    // ── Clean shutdown ────────────────────────────────────────

    info!("flushing desired state");
    desired_store.flush().context("desired state flush failed")?;

    info!("flushing WAL buffer");
    wal.flush().context("WAL flush failed")?;

    info!("smarthome daemon stopped cleanly");
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────

async fn dispatch_resolved(
    candidates: Vec<Command>,
    resolver: &ConflictResolver,
    dispatcher: &mut CommandDispatcher,
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
                conflict.winning_rule
                    .as_ref()
                    .map(|r| r.0.clone())
                    .unwrap_or_default()
            ),
        );
        payload.insert(
            "losing_rule".to_string(),
            Value::Text(
                conflict.losing_rule
                    .as_ref()
                    .map(|r| r.0.clone())
                    .unwrap_or_default()
            ),
        );
        payload.insert("device_id".to_string(),  Value::Text(conflict.device_id.0.clone()));
        payload.insert("attribute".to_string(),   Value::Text(conflict.attribute.0.clone()));

        let event = Event::new(EventSource::System, EventKind::RuleConflict, payload);
        wal.append(event, EventPriority::Normal)
            .context("WAL append failed for RuleConflict")?;

        shared.lock().unwrap().conflicts.push(conflict.clone());
    }

    for command in resolved.winners {
        dispatcher.enqueue(command.clone());
    }

    Ok(())
}

fn send_to_adapter(
    command: &Command,
    cmd_tx: &tokio::sync::mpsc::Sender<AdapterCommand>,
    dispatcher: &mut CommandDispatcher,
) {
    let adapter_cmd = AdapterCommand {
        external_id:  command.device_id.0.clone(),
        attribute:    command.attribute.0.clone(),
        value:        command.value.clone(),
        command_id:   command.id.clone(),
    };

    match cmd_tx.try_send(adapter_cmd) {
        Ok(_)  => dispatcher.mark_sent(&command.id),
        Err(e) => tracing::error!(
            command_id = %command.id,
            device_id  = %command.device_id,
            error      = %e,
            "failed to send command to adapter"
        ),
    }
}

fn to_ms(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}