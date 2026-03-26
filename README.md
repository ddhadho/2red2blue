# 2red2blue

[![Language](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20OpenWrt-blue.svg)]()

A deterministic, crash-resilient smart home automation daemon written in Rust.


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

## Running
```bash
cargo build --release
cargo run --bin daemon -- config.toml

# Reload rules without restart
kill -USR1 $(pidof daemon)
```

---

[`docs/technical-design/`](./docs/technical-design)

---

## Why each component exists

| Component | Why it exists |
|-----------|---------------------|
| WAL | State survives a crash or power cut |
| Boot reconciliation | Home is in the right state after reboot |
| Confidence decay | System doesn't act on stale state |
| HA adapter | Connects the daemon to real devices |
| Rules engine | Encodes the recovery sequence declaratively |
| Conflict resolver | Prevents contradictory commands to the same device |





