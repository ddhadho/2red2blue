# 2red2blue

[![Language](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20OpenWrt-blue.svg)]()
[![Status](https://img.shields.io/badge/status-active%20development-yellow.svg)]()

A deterministic, crash-resilient smart home automation daemon written in Rust.

Designed for environments where **power outages, reboots, and unreliable internet
are normal**. 2red2blue runs entirely locally and recovers its full state after
crashes or power loss — no cloud dependency, no manual intervention required.

Runs on low-power hardware including OpenWrt routers. Configured via a
declarative TOML rule DSL.

---

## The Problem

Most smart home systems stop working when the internet goes down or power cuts.
Automations halt. Devices are left in unknown states. The homeowner has to
intervene manually.

In environments with frequent power outages and unreliable connectivity, this
is not an edge case — it is the normal operating condition.

---

## What 2red2blue Does Differently

**Continues operating without internet**
All rules, device state, and event processing happen locally. Internet
connectivity is never required.

**Recovers correctly after a power cut**
Every event is written to a WAL before the rule engine acts on it. On boot,
the daemon replays its log, reconciles desired vs actual device state, and
issues correction commands — automatically, within seconds.

**Knows what it doesn't know**
Device state carries a confidence score that decays when a device goes silent.
Below the confidence threshold, the daemon applies safe defaults rather than
acting on stale state.

**Hot-reloadable rules**
Automation rules are defined in TOML and reloaded at runtime via `SIGUSR1`
without restarting the daemon.

---

## Example Rule
```toml
[[rules]]
name        = "power-recovery-gate"
trigger     = { type = "state_change", device = "mains_power", field = "source" }
condition   = { field = "source", operator = "eq", value = "kplc" }
actions     = [
  { type = "set_state", device = "main_gate",      field = "state", value = "locked" },
  { type = "set_state", device = "security_lights", field = "state", value = "on"    },
]
```

Additional examples: [`docs/technical-design/rule-dsl/examples`](./docs/technical-design/rule-dsl/examples)

---

## Architecture
```
adapters → ingestor → WAL → state engine → rule engine → resolver → dispatcher
                                ↓
                          reconciler (boot + continuous)
```

| Layer | Responsibility |
|-------|----------------|
| `adapters` | Device protocol drivers (Home Assistant, mock) |
| `kernel` | Ingestor, WAL, state engine, rule engine, conflict resolver, dispatcher, reconciler |
| `daemon` | Main loop — wires the pipeline, manages lifecycle |
| `ui` | HTTP server for live state inspection |

---

## Status

| Component | Status |
|-----------|--------|
| WAL with durability tiers | ✓ complete |
| State engine + confidence decay | ✓ complete |
| Rule engine + TOML DSL | ✓ complete |
| Conflict resolver + dispatcher | ✓ complete |
| Boot + continuous reconciliation | ✓ complete |
| Local web dashboard | ✓ complete |
| Mock adapter | ✓ complete |
| Home Assistant adapter | in progress |
| Pilot hardening | planned |

---

## Running
```bash
cargo build --release
cargo run --bin daemon -- config.toml

# Reload rules without restart
kill -USR1 $(pidof daemon)
```

---

## Design Documentation

48 technical design documents covering architecture decisions, component
specifications, and the rule DSL:

- WAL design and durability model
- State engine and confidence decay
- Rule DSL specification and examples
- Conflict resolution strategy
- Boot reconciliation sequence
- Adapter interface contract

[`docs/technical-design/`](./docs/technical-design)
