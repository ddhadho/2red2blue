# 2red2blue

[![Language](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20OpenWrt-blue.svg)]()

A deterministic, crash-resilient smart home automation daemon.

Connects to Home Assistant over WebSocket and REST. Applies declarative rules. 

---

## Architecture

```
HA WebSocket → adapter → ingestor → state engine → rule engine → resolver → dispatcher → HA REST
                                          ↓
                                    WAL (SQLite)
                                          ↓
                                 reconciler (boot + continuous)
```

| Layer | Responsibility |
|-------|----------------|
| `adapters` | Device protocol drivers — Home Assistant WebSocket + REST, mock |
| `kernel` | Ingestor, WAL, state engine, rule engine, conflict resolver, dispatcher, reconciler |
| `daemon` | Main loop — wires the pipeline, manages lifecycle |
| `ui` | HTTP + WebSocket server — dashboard, Flutter app API |

---

## How it works

**Events** flow in one direction through the pipeline. The HA adapter receives `state_changed` events over WebSocket and maps HA entity IDs to internal device IDs. The ingestor normalises and deduplicates. The state engine tracks actual, desired, and confidence for every device. The rule engine evaluates TOML-defined rules and produces commands. The conflict resolver drops lower-priority commands when rules compete. The dispatcher manages retry and confirmation.

**Confidence decay** means the system knows what it doesn't know. Every device has a decay timer. When a device goes silent, confidence drops. Below the unknown threshold, safe defaults apply instead of stale state. The HA poll loop resets confidence on devices that don't heartbeat.

**The WAL** sits underneath everything. Every event is written to SQLite before the system acts on it. On crash or power cut, the daemon replays the WAL on boot to reconstruct state exactly where it left off.

**Boot reconciliation** runs after WAL replay. It diffs desired state against actual state and issues correction commands for any mismatches. The home corrects itself after every restart.

---

## Why each component exists

| Component | Why it exists |
|-----------|---------------|
| WAL | State survives a crash or power cut |
| Boot reconciliation | Home is in the right state after every reboot |
| Confidence decay | System doesn't act on stale state |
| HA adapter | Connects the daemon to real devices over WebSocket + REST |
| Poll loop | Resets confidence on devices that don't heartbeat |
| Rule engine | Encodes the recovery sequence declaratively |
| Conflict resolver | Prevents contradictory commands to the same device |
| Dispatcher | Tracks retry and confirmation — commands are never fire-and-forget |
| Desired state store | Persists what the home should look like across reboots |

---

## Config

Three files drive everything. No database schema, no migrations.

**`config.toml`** — daemon, storage paths, adapter, reconciler, dispatcher, UI, HA connection and entity mappings.

**`devices.toml`** — device registry. One entry per physical device — ID, kind, capabilities, confidence decay, safe defaults.

**`rules.toml`** — automation rules. Declarative trigger → conditions → actions with optional delays, conflict groups, and priority.

---

## Running

```bash
cargo build --release
cargo run --bin daemon -- /etc/smarthome/config.toml

# Reload rules without restart
kill -USR1 $(pidof daemon)
```

---

## API

The daemon exposes an HTTP API on port 7000 (configurable).

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/state` | All device states with confidence |
| GET | `/devices` | Device metadata — name, kind, capabilities |
| POST | `/command` | Send a manual command to a device |
| GET | `/commands` | Pending and recent commands |
| GET | `/events` | Event history — state changes, confirmations, failures |
| GET | `/rules` | Loaded rules with full trigger/condition/action detail |
| POST | `/rules` | Create a new rule |
| PUT | `/rules/:id` | Edit an existing rule |
| POST | `/rules/:id/enable` | Enable a rule |
| POST | `/rules/:id/disable` | Disable a rule |
| GET | `/conflicts` | Rule conflict log |
| GET | `/reconciliation` | Last boot reconciliation report |
| GET | `/ha/entities` | All HA entities — for device discovery |
| POST | `/devices` | Add a new device — writes config files |
| GET | `/ws` | WebSocket — live state push |

Full API spec: [`docs/api-spec/`](./docs/api-spec/)

---

## Dashboard

Live dashboard at `http://<device>:7000` — device state with confidence scores, pending commands, rule conflicts, in-flight delayed actions, and boot reconciliation report.

Homeowner view at `http://<device>:7000/home` — clean mobile-friendly interface.

---

## Crate structure

```
2red2blue/
├── kernel/        — core types, WAL, state engine, rules, reconciler
├── adapters/      — HA adapter, mock adapter, DeviceAdapter trait
├── daemon/        — main loop, boot sequence, event loop
├── ui/            — HTTP server, API handlers, dashboard HTML
└── platform/      — platform-specific (Linux, OpenWrt)
```

---

## Production deployment

```bash
# Install binary
cp target/release/daemon /usr/local/bin/smarthome-daemon

# Config files
mkdir -p /etc/smarthome /var/lib/smarthome/wal /var/lib/smarthome/snapshots
cp config.toml devices.toml rules.toml /etc/smarthome/

# Systemd — auto-restart on crash
cp smarthome.service /etc/systemd/system/
systemctl enable smarthome
systemctl start smarthome
```

Runtime data (`/var/lib/smarthome/`) survives reboots. Config (`/etc/smarthome/`) is edited by the installer. `/tmp` is never used.
