# 2red2blue

[![Language](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20OpenWrt-blue.svg)]()
[![Status](https://img.shields.io/badge/status-active%20development-yellow.svg)]()
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

A deterministic, crash-resilient smart home automation daemon written in Rust.

Designed for environments where **power outages, router reboots, and unreliable
internet connectivity are normal**. Unlike cloud-dependent systems, 2red2blue
continues operating entirely locally and recovers its full state after crashes
or power loss.

Runs on inexpensive hardware like a Raspberry Pi or OpenWrt router and is
configured using a declarative automation rule DSL.

---

## Motivation

Most consumer smart home systems depend on cloud connectivity and
opaque vendor ecosystems. When internet connectivity drops or a
vendor service shuts down, automation stops working.

2red2blue explores an alternative design:

- fully local execution
- deterministic crash recovery
- hardware-agnostic device adapters
- declarative automation rules

The goal is a resilient automation system suitable for environments
with unreliable infrastructure.

---

## Key Design Principles

**Local-first automation**

The system never depends on cloud connectivity. All rules, device state,
and event processing happen locally.

**WAL before action**

Every event is persisted to a write-ahead log before the rule engine
executes automation actions. This guarantees deterministic recovery
after crashes or power failures.

**State confidence decay**

Device state becomes less trustworthy over time. If a sensor stops
reporting, the daemon marks its state as *unknown* rather than
continuing to trust stale values.

**Safe fallback states**

When devices become unreachable the daemon applies configured
safe states instead of leaving actuators in an unknown condition.

**Hot-reloadable automation rules**

Rules are defined in TOML and can be reloaded at runtime using
`SIGUSR1` without restarting the daemon.

---

## Example Automation

Turn off hallway lights 2 minutes after motion stops:

```toml
[[rules]]
name = "motion-timeout"
trigger = { type = "state_change", device = "motion_sensor_hallway", field = "occupancy" }
condition = { field = "occupancy", operator = "eq", value = false }
delay_secs = 120
actions = [
  { type = "set_state", device = "light_hallway", field = "state", value = "off" }
]
```

Additional rule examples can be found in:

[`docs/technical-design/rule-dsl/examles`](./docs/technical-design/rule-dsl/examples)

---

## Architecture

```
adapters → ingestor → WAL → state engine → rule engine → dispatcher
                                ↓
                         conflict resolver
```

| Layer | Responsibility |
|-------|----------------|
| `adapters` | Device protocol drivers (Zigbee2MQTT, Home Assistant, mock) |
| `kernel` | Ingestor, WAL, state engine, rule engine, conflict resolver, dispatcher |
| `daemon` | Main loop — wires the pipeline, manages lifecycle |
| `platform` | Linux / OpenWrt abstraction, signal handling, watchdog |
| `ui` | HTTP server for live state inspection |

---

## Running

```bash
cargo build --release
cargo run --bin daemon -- config.toml

# Reload rules without restart
kill -USR1 $(pidof daemon)
```

---

## Status

| Component | Status |
|-----------|--------|
| WAL | ✓ complete |
| State engine + confidence decay | ✓ complete |
| Rule engine | ✓ complete |
| Conflict resolver + dispatcher | ✓ complete |
| Mock adapter | ✓ complete |
| Zigbee2MQTT adapter | planned |
| Home Assistant adapter | in progress |
| Boot reconciliation | ✓ complete |
| UI | planned |

---

## Design Documentation

The project includes **48 technical design documents** covering the
internal architecture and design decisions:

- rule DSL specification
- state engine data model
- adapter interface design
- conflict resolution strategy
- platform abstraction layer

See: [`docs/technical-design/`](./docs/technical-design)

---

## License

MIT
