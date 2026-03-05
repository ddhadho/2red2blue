# 2red2blue

A deterministic, crash-resilient home automation daemon written in Rust.

Built for Linux and OpenWrt. No cloud dependency.

## What it does

- Tracks device state with confidence decay — stale state is flagged, not trusted
- Evaluates rules loaded from TOML at runtime, hot-reloadable via SIGUSR1
- Persists every event to a write-ahead log before acting on it
- Applies safe defaults when a device goes dark

## Architecture
```
adapters → ingestor → WAL → state engine → rule engine → dispatcher
```

- `adapters` — device protocol drivers (mock, Zigbee, Z-Wave)
- `kernel` — ingestor, WAL, state engine, rule engine, resolver, dispatcher
- `daemon` — main loop, wires everything together
- `ui` — lightweight HTTP server for state inspection

## Running
```bash
cargo run --bin daemon -- config.toml
```

## Status

Active development. Durable WAL. Rule engine working. Conflict resolver working.Real adapter next.

* ✓ Durable WAL
* ✓ State engine with confidence decay
* ✓ Rule engine with power recovery
* ✓ Conflict resolver and command dispatcher
* → Home Assistant adapter
* → Boot reconciliation
* → UI

[View Documentation](./docs)
